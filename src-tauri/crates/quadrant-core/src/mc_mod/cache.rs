//! Local file cache utilities used by mod downloads.

use std::path::PathBuf;

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

/// Initializes the cache directory and removes stale cache entries.
pub async fn init_cache() -> Result<(), anyhow::Error> {
    log::info!("Initializing mod cache");
    let cache_dir = cache_dir();
    if !cache_dir.exists() {
        std::fs::create_dir_all(&cache_dir)?
    }
    let index_file = cache_dir.join("cacheIndex.json");

    if !index_file.exists() {
        std::fs::write(
            &index_file,
            serde_json::to_string_pretty(&Vec::<CacheIndex>::new())?,
        )?;
    }

    let mut config_raw = std::fs::read_to_string(&index_file)?;
    if serde_json::from_str::<Vec<CacheIndex>>(&config_raw).is_err() {
        std::fs::write(
            &index_file,
            serde_json::to_string_pretty(&Vec::<CacheIndex>::new())?,
        )?;
    }

    config_raw = std::fs::read_to_string(&index_file)?;
    let mut file_conts = serde_json::from_str::<Vec<CacheIndex>>(&config_raw)?;

    for index in file_conts.clone() {
        let is_old = Utc::now() - index.last_used_date;
        let file = PathBuf::from(&index.file_name);

        if is_old.num_days() >= 90 {
            log::info!("Removing stale cache entry: {}", index.file_name);
            if file.exists() {
                std::fs::remove_file(&file)?;
            }
            file_conts.retain(|cont| cont.file_hash != index.file_hash);
        }

        if !file.exists() {
            file_conts.retain(|cont| cont.file_hash != index.file_hash);
        }
    }

    std::fs::write(index_file, serde_json::to_string_pretty(&file_conts)?)?;
    Ok(())
}

/// Computes the SHA-1 hash of a file payload.
pub fn file_hash(file_bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(file_bytes);
    let hash = hasher.finalize();
    hex::encode(hash)
}

/// Looks up a cached file by content hash.
pub async fn get_cache_index(file_hash: String) -> Result<Option<CacheIndex>, anyhow::Error> {
    let cache_dir = cache_dir();
    init_cache().await?;
    let cache_config = cache_dir.join("cacheIndex.json");
    let config_raw = std::fs::read_to_string(cache_config)?;
    let file_conts: Vec<CacheIndex> = serde_json::from_str(&config_raw)?;
    let file_conts: Vec<CacheIndex> = file_conts
        .into_iter()
        .filter(|index| index.file_hash == file_hash)
        .collect();
    if file_conts.is_empty() {
        return Ok(None);
    }
    Ok(Some(file_conts[0].clone()))
}

/// Stores a file in the shared cache or refreshes an existing cache entry.
pub async fn add_cache_index(
    file_name: String,
    file_bytes: &[u8],
    file_hash: String,
) -> Result<PathBuf, anyhow::Error> {
    let _guard = CACHE_WRITE_LOCK.lock().await;
    let cache_dir = cache_dir();
    let cache_config = cache_dir.join("cacheIndex.json");
    if !cache_config.exists() {
        init_cache().await?;
    }
    let config_raw = std::fs::read_to_string(&cache_config)?;
    let mut file_conts: Vec<CacheIndex> = serde_json::from_str(&config_raw)?;

    let exists = file_conts
        .iter()
        .filter(|index| index.file_hash == file_hash)
        .collect::<Vec<&CacheIndex>>();
    let file_path: PathBuf;
    if !exists.is_empty() {
        log::info!("Cache entry for hash {file_hash} already exists, refreshing last-used date");
        file_path = PathBuf::from(exists[0].file_name.clone());
        let index = file_conts
            .iter()
            .position(|index| index.file_hash == file_hash)
            .unwrap();
        file_conts[index].last_used_date = Utc::now();
    } else {
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
        std::fs::write(&new_file, file_bytes)?;
        file_conts.push(CacheIndex {
            last_used_date: Utc::now(),
            file_hash,
            file_name: new_file.to_string_lossy().to_string(),
        });
        file_path = new_file;
    }

    std::fs::write(cache_config, serde_json::to_string_pretty(&file_conts)?)?;
    Ok(file_path)
}

#[cfg(test)]
mod tests {
    use super::{CacheIndex, add_cache_index, file_hash, get_cache_index, init_cache};
    use chrono::{Days, Utc};

    static CACHE_ENV_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct CacheDirGuard {
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

    fn set_cache_dir() -> CacheDirGuard {
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

        init_cache().await.unwrap();

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
    async fn init_cache_recovers_from_corrupt_index() {
        let _guard = set_cache_dir();
        init_cache().await.unwrap();
        let dir = std::path::PathBuf::from(std::env::var("QUADRANT_TEST_CACHE_DIR").unwrap());
        std::fs::write(dir.join("cacheIndex.json"), "{not json").unwrap();

        init_cache().await.unwrap();
        assert!(
            get_cache_index(file_hash(b"anything"))
                .await
                .unwrap()
                .is_none()
        );
    }
}
