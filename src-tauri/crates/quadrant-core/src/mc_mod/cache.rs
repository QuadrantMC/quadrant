//! Local file cache utilities used by mod downloads.

use std::{
    collections::HashSet,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chrono::prelude::*;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::sync::Mutex;

static CACHE_WRITE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn cache_dir() -> PathBuf {
    #[cfg(test)]
    if let Ok(dir) = std::env::var("QUADRANT_TEST_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::cache_dir()
        .unwrap_or_default()
        .join("mrquantumoff.dev")
        .join("QuadrantNextCache")
}

/// Cache index entry describing a downloaded file in the shared cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheIndex {
    /// Last time the cached file was used.
    pub last_used_date: DateTime<Utc>,
    /// SHA-1 hash of the cached file contents.
    pub file_hash: String,
    /// Absolute path to the cached file on disk.
    pub file_name: String,
}

const INDEX_FILE: &str = "cacheIndex.json";
const INDEX_LOCK_FILE: &str = "cacheIndex.json.lock";

/// The index as it is on disk. A missing or unreadable one reads as empty:
/// cached files only save downloads, and [`init_cache`] sweeps up the files an
/// unreadable index forgot.
fn read_index(cache_dir: &Path) -> Vec<CacheIndex> {
    let raw = match std::fs::read(cache_dir.join(INDEX_FILE)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            log::warn!("Can't read the mod cache index: {error}");
            return Vec::new();
        }
    };
    serde_json::from_slice(&raw).unwrap_or_else(|error| {
        log::warn!("Ignoring an unreadable mod cache index: {error}");
        Vec::new()
    })
}

fn write_index(cache_dir: &Path, entries: &[CacheIndex]) -> Result<(), anyhow::Error> {
    crate::modpacks::write_file_atomically(
        cache_dir.join(INDEX_FILE),
        serde_json::to_string_pretty(entries)?,
    )?;
    Ok(())
}

/// Held around every index read-modify-write, after [`CACHE_WRITE_LOCK`]:
/// Quadrant-Native and the CLI share the cache folder.
fn lock_index(cache_dir: &Path) -> Result<std::fs::File, anyhow::Error> {
    std::fs::create_dir_all(cache_dir)?;
    Ok(crate::modpacks::lock_file(
        &cache_dir.join(INDEX_LOCK_FILE),
    )?)
}

fn remove_cached_file(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("Failed to remove cached file {}: {error}", path.display());
    }
}

/// Prunes the cache once, at startup: entries unused for `keep_alive_days`
/// (the `cacheKeepAlive` setting) with their files, entries whose file is
/// gone, and files no entry names that are as old. Lookups never prune.
pub async fn init_cache(keep_alive_days: i64) -> Result<(), anyhow::Error> {
    let _guard = CACHE_WRITE_LOCK.lock().await;
    prune_cache(&cache_dir(), keep_alive_days)
}

fn prune_cache(cache_dir: &Path, keep_alive_days: i64) -> Result<(), anyhow::Error> {
    log::info!("Pruning the mod cache (keeping files used in the last {keep_alive_days} days)");
    let keep_alive_days = keep_alive_days.max(1);
    let _lock = lock_index(cache_dir)?;
    let now = Utc::now();
    let mut entries = read_index(cache_dir);
    entries.retain(|entry| {
        let file = Path::new(&entry.file_name);
        let keep = (now - entry.last_used_date).num_days() < keep_alive_days && file.is_file();
        if !keep {
            log::info!("Removing stale cache entry: {}", entry.file_name);
            remove_cached_file(file);
        }
        keep
    });
    write_index(cache_dir, &entries)?;

    // Files an unreadable index forgot, or a crash left between writing a
    // file and indexing it.
    let indexed: HashSet<_> = entries
        .iter()
        .filter_map(|entry| {
            Path::new(&entry.file_name)
                .file_name()
                .map(ToOwned::to_owned)
        })
        .collect();
    let keep_alive = std::time::Duration::from_secs(keep_alive_days as u64 * 24 * 60 * 60);
    for entry in std::fs::read_dir(cache_dir)?.flatten() {
        let name = entry.file_name();
        if indexed.contains(&name) || name == INDEX_FILE || name == INDEX_LOCK_FILE {
            continue;
        }
        let is_old = entry
            .metadata()
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= keep_alive);
        if is_old {
            log::info!("Removing unindexed cache file: {}", entry.path().display());
            remove_cached_file(&entry.path());
        }
    }
    Ok(())
}

/// Computes the SHA-1 hash of a file payload.
pub fn file_hash(file_bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(file_bytes);
    let hash = hasher.finalize();
    hex::encode(hash)
}

/// Lowercase hex SHA-512 of a payload, the `sha512` a modpack entry records.
pub fn sha512_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha512::digest(bytes))
}

/// Copies `reader` into `writer` and returns the lowercase hex SHA-512 of the
/// bytes copied, so a file is hashed exactly as it is written.
pub(crate) fn copy_hashing(
    reader: &mut impl Read,
    writer: &mut impl Write,
) -> std::io::Result<String> {
    let mut hasher = sha2::Sha512::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Lowercase hex SHA-512 of a file on disk, read in chunks.
pub(crate) fn file_sha512(path: &Path) -> std::io::Result<String> {
    copy_hashing(&mut std::fs::File::open(path)?, &mut std::io::sink())
}

/// Looks up a cached file by content hash. Read-only: the index is written
/// atomically, so it needs no lock.
pub async fn get_cache_index(file_hash: String) -> Result<Option<CacheIndex>, anyhow::Error> {
    Ok(read_index(&cache_dir())
        .into_iter()
        .find(|index| index.file_hash == file_hash && Path::new(&index.file_name).is_file()))
}

/// Stores a file in the shared cache or refreshes an existing cache entry,
/// writing the file again when its entry outlived it.
pub async fn add_cache_index(
    file_name: String,
    file_bytes: &[u8],
    file_hash: String,
) -> Result<PathBuf, anyhow::Error> {
    add_cache_entry(file_name, file_hash, |path| {
        crate::modpacks::write_file_atomically(path, file_bytes)
    })
    .await
}

/// [`add_cache_index`] for a file already downloaded to `download` (from
/// [`download_path`]), which is moved into the cache instead of read into
/// memory. `download` is gone afterwards either way.
pub(crate) async fn add_cache_file(
    file_name: String,
    download: &Path,
    file_hash: String,
) -> Result<PathBuf, anyhow::Error> {
    let result =
        add_cache_entry(file_name, file_hash, |path| std::fs::rename(download, path)).await;
    let _ = std::fs::remove_file(download);
    result
}

/// A fresh path in the cache folder for a download to stream into, so
/// [`add_cache_file`] can rename it into place.
pub(crate) fn download_path() -> std::io::Result<PathBuf> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cache_dir = cache_dir();
    std::fs::create_dir_all(&cache_dir)?;
    Ok(cache_dir.join(format!(
        ".download-{}-{}.part",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )))
}

/// Lowercase hex SHA-1 and SHA-512 of a file on disk, read in chunks.
pub(crate) fn file_hashes(path: &Path) -> std::io::Result<(String, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut sha1 = Sha1::new();
    let mut sha512 = sha2::Sha512::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        sha1.update(&buffer[..read]);
        sha512.update(&buffer[..read]);
    }
    Ok((hex::encode(sha1.finalize()), hex::encode(sha512.finalize())))
}

async fn add_cache_entry(
    file_name: String,
    file_hash: String,
    place: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<PathBuf, anyhow::Error> {
    let _guard = CACHE_WRITE_LOCK.lock().await;
    let cache_dir = cache_dir();
    let _lock = lock_index(&cache_dir)?;
    let mut entries = read_index(&cache_dir);

    let file_path = match entries
        .iter_mut()
        .find(|index| index.file_hash == file_hash)
    {
        Some(existing) => {
            log::info!(
                "Cache entry for hash {file_hash} already exists, refreshing last-used date"
            );
            existing.last_used_date = Utc::now();
            PathBuf::from(&existing.file_name)
        }
        None => {
            log::info!("Adding new cache entry: {file_name} (hash={file_hash})");
            let safe_name = PathBuf::from(file_name)
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Invalid cache file name"))?
                .to_string_lossy()
                .to_string();
            // Content-address the on-disk name so provider files with identical
            // names cannot overwrite one another in the shared cache.
            let new_file = cache_dir.join(format!(
                "{}-{safe_name}",
                &file_hash[..12.min(file_hash.len())]
            ));
            entries.push(CacheIndex {
                last_used_date: Utc::now(),
                file_hash,
                file_name: new_file.to_string_lossy().to_string(),
            });
            new_file
        }
    };
    if !file_path.is_file() {
        place(&file_path)?;
    }

    write_index(&cache_dir, &entries)?;
    Ok(file_path)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{CacheIndex, add_cache_index, file_hash, get_cache_index, init_cache};
    use chrono::{Days, Utc};

    static CACHE_ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(crate) struct CacheDirGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
    }

    impl Drop for CacheDirGuard {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("QUADRANT_TEST_CACHE_DIR");
            }
        }
    }

    pub(crate) fn set_cache_dir() -> CacheDirGuard {
        let lock = CACHE_ENV_TEST_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("QUADRANT_TEST_CACHE_DIR", dir.path());
        }
        CacheDirGuard {
            _lock: lock,
            _dir: dir,
        }
    }

    #[test]
    fn file_hash_computes_sha1() {
        assert_eq!(
            file_hash(b"hello"),
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        );
        assert_eq!(file_hash(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[tokio::test]
    async fn add_and_get_cache_index_round_trip() {
        let _guard = set_cache_dir();
        let bytes = b"mod jar contents";
        let hash = file_hash(bytes);

        let path = add_cache_index("my mod.jar".to_string(), bytes, hash.clone())
            .await
            .unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        let entry = get_cache_index(hash.clone()).await.unwrap().unwrap();
        assert_eq!(entry.file_hash, hash);
        assert_eq!(entry.file_name, path.to_string_lossy());

        assert!(
            get_cache_index("0000000000000000000000000000000000000000".to_string())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn add_cache_index_refreshes_existing_entry_instead_of_duplicating() {
        let _guard = set_cache_dir();
        let bytes = b"same contents";
        let hash = file_hash(bytes);

        let first = add_cache_index("a.jar".to_string(), bytes, hash.clone())
            .await
            .unwrap();
        let second = add_cache_index("b.jar".to_string(), bytes, hash.clone())
            .await
            .unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn init_cache_evicts_stale_and_missing_entries() {
        let _guard = set_cache_dir();
        let fresh = add_cache_index("fresh.jar".to_string(), b"fresh", file_hash(b"fresh"))
            .await
            .unwrap();

        let stale = add_cache_index("stale.jar".to_string(), b"stale", file_hash(b"stale"))
            .await
            .unwrap();
        let index_file = stale.parent().unwrap().join("cacheIndex.json");
        let mut entries: Vec<CacheIndex> =
            serde_json::from_str(&std::fs::read_to_string(&index_file).unwrap()).unwrap();
        for entry in &mut entries {
            if entry.file_hash == file_hash(b"stale") {
                entry.last_used_date = Utc::now().checked_sub_days(Days::new(120)).unwrap();
            }
        }
        std::fs::write(&index_file, serde_json::to_string_pretty(&entries).unwrap()).unwrap();

        init_cache(30).await.unwrap();

        assert!(fresh.exists());
        assert!(!stale.exists());
        assert!(
            get_cache_index(file_hash(b"stale"))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            get_cache_index(file_hash(b"fresh"))
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn a_corrupt_index_reads_as_empty_and_its_old_files_are_swept() {
        let _guard = set_cache_dir();
        let forgotten = add_cache_index("old.jar".to_string(), b"old", file_hash(b"old"))
            .await
            .unwrap();
        let dir = super::cache_dir();
        std::fs::write(dir.join("cacheIndex.json"), "{not json").unwrap();
        age_file(&forgotten, 40);
        let stray = dir.join("fresh-stray.jar");
        std::fs::write(&stray, b"stray").unwrap();

        assert!(get_cache_index(file_hash(b"old")).await.unwrap().is_none());
        let added = add_cache_index("new.jar".to_string(), b"new", file_hash(b"new"))
            .await
            .unwrap();
        init_cache(30).await.unwrap();

        assert!(!forgotten.exists());
        assert!(stray.exists(), "a recent file may be mid-indexing");
        assert!(added.exists());
        assert!(get_cache_index(file_hash(b"new")).await.unwrap().is_some());
    }

    fn age_file(path: &std::path::Path, days: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(
            std::time::SystemTime::now() - std::time::Duration::from_secs(days * 24 * 60 * 60),
        )
        .unwrap();
    }

    fn set_last_used(days_ago: u64) {
        let index_file = super::cache_dir().join("cacheIndex.json");
        let mut entries: Vec<CacheIndex> =
            serde_json::from_str(&std::fs::read_to_string(&index_file).unwrap()).unwrap();
        for entry in &mut entries {
            entry.last_used_date = Utc::now().checked_sub_days(Days::new(days_ago)).unwrap();
        }
        std::fs::write(&index_file, serde_json::to_string_pretty(&entries).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn init_cache_keeps_files_for_the_keep_alive_setting() {
        let _guard = set_cache_dir();
        let path = add_cache_index("mod.jar".to_string(), b"mod", file_hash(b"mod"))
            .await
            .unwrap();
        set_last_used(40);

        init_cache(90).await.unwrap();
        assert!(path.exists());
        init_cache(30).await.unwrap();
        assert!(!path.exists());
        assert!(get_cache_index(file_hash(b"mod")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn lookups_never_rewrite_or_prune_the_index() {
        let _guard = set_cache_dir();
        let path = add_cache_index("mod.jar".to_string(), b"mod", file_hash(b"mod"))
            .await
            .unwrap();
        set_last_used(400);
        let index_file = super::cache_dir().join("cacheIndex.json");
        let before = std::fs::read(&index_file).unwrap();

        assert!(get_cache_index(file_hash(b"mod")).await.unwrap().is_some());
        assert!(
            get_cache_index(file_hash(b"other"))
                .await
                .unwrap()
                .is_none()
        );

        assert_eq!(std::fs::read(&index_file).unwrap(), before);
        assert!(path.exists());
    }

    #[tokio::test]
    async fn an_entry_whose_file_is_gone_gets_the_file_again() {
        let _guard = set_cache_dir();
        let path = add_cache_index("mod.jar".to_string(), b"mod", file_hash(b"mod"))
            .await
            .unwrap();
        std::fs::remove_file(&path).unwrap();

        assert!(get_cache_index(file_hash(b"mod")).await.unwrap().is_none());
        let again = add_cache_index("mod.jar".to_string(), b"mod", file_hash(b"mod"))
            .await
            .unwrap();

        assert_eq!(again, path);
        assert_eq!(std::fs::read(&path).unwrap(), b"mod");
    }

    #[test]
    fn concurrent_cache_maintenance_and_downloads_preserve_every_entry() {
        let _guard = set_cache_dir();
        const WORKERS: usize = 6;
        const DOWNLOADS: usize = 12;
        let start = std::sync::Barrier::new(WORKERS);
        std::thread::scope(|scope| {
            for worker in 0..WORKERS {
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    futures::executor::block_on(async {
                        for download in 0..DOWNLOADS {
                            let bytes = format!("download-{worker}-{download}");
                            let hash = file_hash(bytes.as_bytes());
                            let file = add_cache_index(
                                "mod.jar".to_string(),
                                bytes.as_bytes(),
                                hash.clone(),
                            )
                            .await
                            .unwrap();
                            init_cache(30).await.unwrap();
                            let entry = get_cache_index(hash)
                                .await
                                .unwrap()
                                .expect("entry was lost");
                            assert_eq!(std::fs::read(file).unwrap(), bytes.as_bytes());
                            assert!(std::path::Path::new(&entry.file_name).is_file());
                        }
                    });
                });
            }
        });
        let entries: Vec<CacheIndex> = serde_json::from_slice(
            &std::fs::read(super::cache_dir().join("cacheIndex.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(entries.len(), WORKERS * DOWNLOADS);
    }
}
