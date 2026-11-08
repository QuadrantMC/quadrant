//! Local modpack filesystem operations and import/export flows.

use crate::mc_mod::cache::file_sha512;
use crate::{
    Result,
    events::BackendEvent,
    mc_mod::enrich_installed_mod,
    models::{
        InstalledMod, InstalledModpack, LocalModpack, ModLoader, ModSource, SyncInfo,
        is_portable_file_name, is_single_path_component, modpack_path,
    },
    ports::{EventSink, SettingsStore},
};
use anyhow::anyhow;
use chrono::Utc;
use futures::StreamExt;
use sha2::Digest;
use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
};
use zip::write::{ExtendedFileOptions, FileOptions};

/// Opens `path`, creating it, and holds an exclusive advisory lock on it until
/// the returned file is dropped. Quadrant, Quadrant-Native and the CLI share
/// the data folder, the game folder and the mod cache, and this keeps their
/// read-modify-write cycles of one file from interleaving. Take it after any
/// in-process lock for the same file, never while holding it across an await.
pub fn lock_file(path: &Path) -> std::io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock()?;
    Ok(file)
}

/// Writes `contents` through a sibling temp file and an atomic rename, so a
/// crash mid-write never leaves a truncated manifest or metadata file behind.
pub fn write_file_atomically(
    path: impl AsRef<Path>,
    contents: impl AsRef<[u8]>,
) -> std::io::Result<()> {
    write_atomically(path.as_ref(), |file| file.write_all(contents.as_ref()))
}

/// Fills a sibling temp file with `write` and renames it over `path`, so the
/// destination is either the old file or the whole new one. Streaming callers
/// use this instead of [`write_file_atomically`] to avoid holding the contents
/// in memory.
pub(crate) fn write_atomically(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = parent.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let temp_path = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let fill = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp_path)?;
        write(&mut file)?;
        file.sync_all()?;
        std::fs::rename(&temp_path, path)
    };
    let result = fill();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

/// Every modpack manifest read-modify-write runs under this, so a write never
/// starts from a copy another one has since replaced: the process-wide mutex,
/// then an advisory lock on `<mcFolder>/modpacks/.modpacks.lock` that the other
/// apps sharing the game folder take too. Re-read the manifest with
/// [`read_manifest`] once it is held.
pub(crate) struct ManifestLock {
    _file: std::fs::File,
    _guard: std::sync::MutexGuard<'static, ()>,
}

static MODPACK_MANIFEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn lock_manifests(mc_folder: &Path) -> Result<ManifestLock> {
    let guard = MODPACK_MANIFEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let modpacks_folder = mc_folder.join("modpacks");
    std::fs::create_dir_all(&modpacks_folder)?;
    Ok(ManifestLock {
        _file: lock_file(&modpacks_folder.join(".modpacks.lock"))?,
        _guard: guard,
    })
}

/// The manifest of the pack `name` in `folder` as it is on disk now:
/// `modConfigV2.json`, else a legacy `modConfig.json`, else `None`.
pub(crate) fn read_manifest(folder: &Path, name: &str) -> Result<Option<InstalledModpack>> {
    for file in ["modConfigV2.json", "modConfig.json"] {
        match std::fs::read(folder.join(file)) {
            Ok(raw) => {
                return serde_json::from_slice(&raw)
                    .map(Some)
                    .map_err(|error| crate::models::unreadable_manifest(name, error));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}

pub(crate) fn write_manifest(folder: &Path, manifest: &InstalledModpack) -> Result<()> {
    write_file_atomically(
        folder.join("modConfigV2.json"),
        serde_json::to_string_pretty(manifest)?,
    )?;
    Ok(())
}

pub(crate) fn validate_modpack_name(name: &str) -> Result<()> {
    if !is_single_path_component(name) {
        return Err(anyhow::Error::from(
            crate::error::ErrorCode::InvalidModpackName,
        ));
    }
    Ok(())
}

/// The stricter check for a name that is about to become a folder, so a new
/// or incoming pack can never alias an existing one.
pub(crate) fn validate_new_modpack_name(name: &str) -> Result<()> {
    if !is_portable_file_name(name) {
        return Err(anyhow::Error::from(
            crate::error::ErrorCode::InvalidModpackName,
        ));
    }
    Ok(())
}

/// Whether a manifest entry from `source` may download from `host`. Provider
/// entries come only from that provider's CDN, so a shared or synced pack can
/// not dress up another host as Modrinth or CurseForge.
pub fn is_trusted_download_host(source: &ModSource, host: &str) -> bool {
    match source {
        ModSource::Modrinth => host == "cdn.modrinth.com",
        ModSource::CurseForge => matches!(host, "edge.forgecdn.net" | "mediafilez.forgecdn.net"),
        // Online mods are links the user added, so any HTTPS host is theirs to
        // pick. Frontends name the host before such a pack is installed.
        ModSource::Online => true,
    }
}

pub(crate) fn safe_download_file_name(download_url: &str, source: &ModSource) -> Result<String> {
    let url = reqwest::Url::parse(download_url)?;
    let scheme_allowed = url.scheme() == "https"
        || cfg!(test) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
    if !scheme_allowed {
        return Err(anyhow::Error::from(crate::error::ErrorCode::UnsafeDownload));
    }

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let trusted = is_trusted_download_host(source, &host) || cfg!(test) && host == "127.0.0.1";
    if !trusted {
        return Err(anyhow::Error::from(crate::error::ErrorCode::UnsafeDownload));
    }

    let encoded = url
        .path_segments()
        .and_then(|segments| segments.filter(|part| !part.is_empty()).next_back())
        .ok_or_else(|| anyhow::Error::from(crate::error::ErrorCode::UnsafeDownload))?;
    let decoded = urlencoding::decode(encoded)?.into_owned();
    if !is_portable_file_name(&decoded) {
        return Err(anyhow::Error::from(crate::error::ErrorCode::UnsafeDownload));
    }
    Ok(decoded)
}

//// Discovers local modpacks under `<mcFolder>/modpacks`.
pub async fn get_modpacks(mc_folder: &Path, hide_free: bool) -> Result<Vec<LocalModpack>> {
    let mut modpacks = Vec::new();
    let modpacks_folder = mc_folder.join("modpacks");
    if !modpacks_folder.exists() {
        return Ok(modpacks);
    }

    for entry in std::fs::read_dir(modpacks_folder)? {
        let path = entry?.path();
        if hide_free && path.file_name().is_some_and(|name| name == "free") {
            continue;
        }
        if let Some(modpack) = load_modpack(mc_folder, &path).await? {
            modpacks.push(modpack);
        }
    }

    Ok(modpacks)
}

/// The one pack `name` as [`get_modpacks`] lists it, without reading every
/// other pack. Commands that act on a single pack use this.
pub async fn get_modpack(mc_folder: &Path, name: &str) -> Result<Option<LocalModpack>> {
    validate_modpack_name(name)?;
    load_modpack(mc_folder, &modpack_path(mc_folder, name)).await
}

async fn load_modpack(mc_folder: &Path, path: &Path) -> Result<Option<LocalModpack>> {
    // Skip non-directory entries before trying to read them as folders.
    // macOS drops `.DS_Store` files into directories, and calling
    // `read_dir` on a file errors out — which previously failed the whole
    // modpack listing on macOS.
    if !path.is_dir() {
        return Ok(None);
    }

    // One unreadable pack must not blank the whole listing: every
    // pre-listing operation (delete, rename, install, share) depends on it.
    let file_amount = match std::fs::read_dir(path) {
        Ok(entries) => entries.count(),
        Err(error) => {
            log::warn!(
                "Skipping unreadable modpack folder {}: {error}",
                path.display()
            );
            return Ok(None);
        }
    };

    let mut extra_files = 0;
    let modpack_config_v2 = path.join("modConfigV2.json");
    let modpack_config_v1 = path.join("modConfig.json");
    let modpack_sync = path.join("quadrantSync.json");
    let mut last_synced = 0_i64;
    let mut modpack_id = None;
    let name = path.file_name().unwrap().to_string_lossy().to_string();

    let is_applied =
        mods_link_target(&mc_folder.join("mods")).is_some_and(|mods_path| mods_path == path);

    let has_v2 = modpack_config_v2.exists();
    let has_v1 = modpack_config_v1.exists();

    if !has_v2 && !has_v1 {
        return Ok(Some(LocalModpack {
            name,
            version: "-".to_string(),
            mods: Vec::new(),
            mod_loader: ModLoader::Unknown,
            is_applied,
            last_synced,
            modpack_id: None,
            unknown_mods: true,
            manifest_error: None,
        }));
    }

    if modpack_sync.exists() {
        extra_files += 1;
        // Sync metadata is advisory. A damaged file degrades to "never
        // synced" instead of failing the listing.
        match std::fs::read_to_string(&modpack_sync)
            .map_err(anyhow::Error::from)
            .and_then(|raw| serde_json::from_str::<SyncInfo>(&raw).map_err(Into::into))
        {
            Ok(sync_info) => {
                last_synced = sync_info.last_synced.saturating_mul(1000);
                modpack_id = sync_info.modpack_id;
            }
            Err(error) => {
                log::warn!("Ignoring damaged sync metadata for modpack \"{name}\": {error}");
            }
        }
    }

    let config_path = if has_v2 {
        &modpack_config_v2
    } else {
        &modpack_config_v1
    };
    let parsed = std::fs::File::open(config_path)
        .map_err(anyhow::Error::from)
        .and_then(|config_file| {
            serde_json::from_reader::<_, InstalledModpack>(std::io::BufReader::new(config_file))
                .map_err(Into::into)
        });
    let mut modpack = match parsed {
        Ok(modpack) => modpack,
        Err(error) => {
            log::warn!("Modpack \"{name}\" has an unreadable manifest: {error}");
            return Ok(Some(LocalModpack {
                name,
                version: "-".to_string(),
                mods: Vec::new(),
                mod_loader: ModLoader::Unknown,
                is_applied,
                last_synced,
                modpack_id,
                unknown_mods: true,
                manifest_error: Some(error.to_string()),
            }));
        }
    };
    modpack.name = name.clone();

    let mut needs_rewrite = enrich_unnamed_mods(&mut modpack).await;
    if !has_v2 {
        modpack.mod_config_version = "2".to_string();
        modpack.quadrant_version = crate::models::quadrant_version();
        needs_rewrite = true;
    }

    if needs_rewrite && let Err(e) = record_listing_changes(mc_folder, path, &modpack) {
        log::error!("Failed to write modConfigV2.json for modpack \"{name}\": {e}");
    }

    let v2_file_count = if modpack_config_v2.exists() { 1 } else { 0 };
    let v1_file_count = if modpack_config_v1.exists() { 1 } else { 0 };
    extra_files += v2_file_count + v1_file_count;

    let mut modpack = LocalModpack::from((modpack, is_applied, last_synced));
    modpack.modpack_id = modpack_id;
    let expected_files = modpack.mods.len() + extra_files;
    if expected_files < file_amount {
        modpack.unknown_mods = true;
    }
    Ok(Some(modpack))
}

/// How long an entry whose details could not be fetched is left alone, so an
/// offline listing does not wait on the same failing requests every time.
const ENRICH_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const ENRICH_CONCURRENCY: usize = 8;

type EnrichKey = (String, String);

static ENRICH_FAILURES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<EnrichKey, std::time::Instant>>,
> = std::sync::LazyLock::new(Default::default);

fn enrich_key(mod_: &InstalledMod) -> EnrichKey {
    (format!("{:?}", mod_.source), mod_.id.clone())
}

/// Fetches the details of the provider entries that have no name, a few at a
/// time. Returns whether any entry learned its name.
async fn enrich_unnamed_mods(modpack: &mut InstalledModpack) -> bool {
    let now = std::time::Instant::now();
    let pending: Vec<usize> = {
        let failures = ENRICH_FAILURES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        modpack
            .mods
            .iter()
            .enumerate()
            .filter(|(_, mod_)| mod_.name.is_empty() && mod_.source != ModSource::Online)
            .filter(|(_, mod_)| {
                failures
                    .get(&enrich_key(mod_))
                    .is_none_or(|failed_at| now.duration_since(*failed_at) >= ENRICH_RETRY_AFTER)
            })
            .map(|(index, _)| index)
            .collect()
    };
    if pending.is_empty() {
        return false;
    }

    let results: Vec<(usize, Result<InstalledMod>)> =
        futures::stream::iter(pending.into_iter().map(|index| {
            let mod_ = modpack.mods[index].clone();
            async move { (index, enrich_installed_mod(mod_).await) }
        }))
        .buffer_unordered(ENRICH_CONCURRENCY)
        .collect()
        .await;

    let mut changed = false;
    let mut failures = ENRICH_FAILURES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (index, result) in results {
        let key = enrich_key(&modpack.mods[index]);
        match result {
            Ok(enriched) if !enriched.name.is_empty() => {
                failures.remove(&key);
                modpack.mods[index] = enriched;
                changed = true;
            }
            // Nothing to learn without the provider (CurseForge compiled out).
            Ok(_) => {
                failures.insert(key, now);
            }
            Err(e) => {
                log::warn!(
                    "Failed to enrich mod {} in modpack \"{}\": {}",
                    modpack.mods[index].id,
                    modpack.name,
                    e
                );
                failures.insert(key, now);
            }
        }
    }
    changed
}

// Writes what listing learned, names fetched for bare entries and the move
/// from `modConfig.json` to `modConfigV2.json`, onto the manifest as it is now:
/// the fetches took a while, and another write may have landed meanwhile.
fn record_listing_changes(
    mc_folder: &Path,
    folder: &Path,
    listed: &InstalledModpack,
) -> Result<()> {
    let _lock = lock_manifests(mc_folder)?;
    let Some(mut manifest) = read_manifest(folder, &listed.name)? else {
        return Ok(());
    };
    for mod_ in manifest.mods.iter_mut().filter(|mod_| mod_.name.is_empty()) {
        let enriched = listed.mods.iter().find(|enriched| {
            !enriched.name.is_empty()
                && enriched.id == mod_.id
                && enriched.source == mod_.source
                && enriched.download_url == mod_.download_url
        });
        if let Some(enriched) = enriched {
            *mod_ = InstalledMod {
                sha512: mod_.sha512.take(),
                ..enriched.clone()
            };
        }
    }
    let migrating = !folder.join("modConfigV2.json").exists();
    if migrating {
        manifest.mod_config_version = "2".to_string();
        manifest.quadrant_version = crate::models::quadrant_version();
    }
    manifest.name = listed.name.clone();
    write_manifest(folder, &manifest)?;
    if migrating {
        let _ = std::fs::remove_file(folder.join("modConfig.json"));
    }
    Ok(())
}

/// Name prefix of the folder a real `mods` directory is set aside under, shared
/// with whoever has to find that backup again.
pub(crate) const MODS_BACKUP_PREFIX: &str = "mods-backup-";

/// What a game directory's `mods` folder links to, or `None` when it is a real
/// directory. The single place a modpack/game-directory pairing is read from.
pub(crate) fn mods_link_target(mods_path: &Path) -> Option<PathBuf> {
    mods_path
        .is_symlink()
        .then(|| mods_path.read_link().ok())
        .flatten()
}

/// Makes `mods_path` a symlink to `target`, preserving whatever was there.
///
/// A timestamped backup of a real `mods` directory is created inside
/// `backup_parent`.
pub(crate) fn link_mods_folder(
    mods_path: &Path,
    target: &Path,
    backup_parent: &Path,
) -> Result<()> {
    let mut backup = None;
    if mods_path.is_symlink() {
        // An existing `mods` symlink. This may be dangling if the modpack it
        // pointed at was deleted as a directory outside of Quadrant, in which
        // case `exists()` is false (it follows the link) but the broken link
        // still occupies the path and would make symlink creation fail with
        // `AlreadyExists`. Remove it either way before re-linking.
        std::fs::remove_dir_all(mods_path)?;
    } else if mods_path.exists() {
        // A real, non-symlink `mods` directory — back it up rather than delete.
        let path = backup_parent.join(format!(
            "{MODS_BACKUP_PREFIX}{}",
            Utc::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::rename(mods_path, &path)?;
        backup = Some(path);
    }

    restore_backup_on_error(
        create_mods_link(target, mods_path),
        backup.as_deref(),
        mods_path,
    )
}

#[cfg(target_os = "windows")]
fn create_mods_link(target: &Path, mods_path: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, mods_path)
}

#[cfg(not(target_os = "windows"))]
fn create_mods_link(target: &Path, mods_path: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, mods_path)
}

/// Puts a backed-up `mods` directory back when creating the link failed, and
/// reports the original failure.
///
/// Creating a symlink needs a privilege Windows does not grant by default, and
/// without this the jars are left under a backup name with no `mods` folder at
/// all.
fn restore_backup_on_error(
    linked: std::io::Result<()>,
    backup: Option<&Path>,
    mods_path: &Path,
) -> Result<()> {
    let Err(error) = linked else {
        return Ok(());
    };
    if let Some(backup) = backup
        && let Err(restore_error) = std::fs::rename(backup, mods_path)
    {
        log::error!(
            "Failed to restore \"{}\" after linking failed: {restore_error}",
            backup.display()
        );
    }
    Err(error.into())
}

/// Applies the named modpack by making `<mcFolder>/mods` point at it.
pub fn apply_modpack(mc_folder: &Path, name: &str) -> Result<()> {
    validate_modpack_name(name)?;
    log::info!("Applying modpack \"{name}\"");
    let modpack_dir = modpack_path(mc_folder, name);
    if !modpack_dir.exists() {
        return Err(anyhow::Error::from(crate::error::ErrorCode::ModpackMissing));
    }
    link_mods_folder(
        &mc_folder.join("mods"),
        &modpack_dir,
        &mc_folder.join("modpacks"),
    )
}

/// Creates a new modpack folder and manifest.
pub fn create_modpack(
    mc_folder: &Path,
    existing_modpacks: &[LocalModpack],
    name: &str,
    version: &str,
    mod_loader: ModLoader,
) -> Result<()> {
    validate_new_modpack_name(name)?;
    log::info!("Creating modpack \"{name}\" (version={version}, mod_loader={mod_loader:?})");
    if existing_modpacks.iter().any(|modpack| modpack.name == name) {
        return Err(anyhow::Error::from(crate::error::ErrorCode::ModpackExists));
    }
    let modpack_folder = modpack_path(mc_folder, name);
    std::fs::create_dir_all(mc_folder.join("modpacks"))?;
    // The caller's listing can be stale. Create exclusively so a concurrent
    // creation cannot replace an existing pack's manifest.
    std::fs::create_dir(&modpack_folder).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            anyhow::Error::from(crate::error::ErrorCode::ModpackExists)
        } else {
            error.into()
        }
    })?;
    let modpack = InstalledModpack {
        mod_config_version: "2".to_string(),
        quadrant_version: crate::models::quadrant_version(),
        name: name.to_string(),
        version: version.to_string(),
        mod_loader,
        mods: Vec::new(),
    };
    write_file_atomically(
        modpack_folder.join("modConfigV2.json"),
        serde_json::to_string_pretty(&modpack)?,
    )?;
    Ok(())
}

/// Updates an existing modpack manifest and optionally renames the modpack.
pub fn update_modpack(
    mc_folder: &Path,
    existing_modpacks: &[LocalModpack],
    modpack_source: &str,
    name: Option<String>,
    version: Option<String>,
    mod_loader: Option<ModLoader>,
) -> Result<LocalModpack> {
    validate_modpack_name(modpack_source)?;
    if let Some(name) = name.as_deref() {
        validate_new_modpack_name(name)?;
    }
    log::info!(
        "Updating modpack \"{modpack_source}\" (name={name:?}, version={version:?}, mod_loader={mod_loader:?})"
    );
    let mut modpack = existing_modpacks
        .iter()
        .find(|modpack| modpack.name == modpack_source)
        .cloned()
        .ok_or_else(|| anyhow::Error::from(crate::error::ErrorCode::ModpackMissing))?;
    modpack.ensure_manifest_readable()?;
    let modpack_folder = modpack_path(mc_folder, modpack_source);
    let original_name = modpack.name.clone();

    if let Some(name) = name.as_deref()
        && name != modpack_source
    {
        let destination = modpack_path(mc_folder, name);
        // A case-only rename on a case-insensitive file system finds the pack
        // itself at the destination.
        let is_same_folder = matches!(
            (destination.canonicalize(), modpack_folder.canonicalize()),
            (Ok(destination), Ok(source)) if destination == source
        );
        if !is_same_folder && (destination.try_exists()? || destination.is_symlink()) {
            return Err(anyhow::Error::from(crate::error::ErrorCode::ModpackExists));
        }
    }

    let _lock = lock_manifests(mc_folder)?;
    let mut manifest = read_manifest(&modpack_folder, modpack_source)?
        .unwrap_or_else(|| InstalledModpack::from(modpack.clone()));
    manifest.mod_config_version = "2".to_string();
    manifest.quadrant_version = crate::models::quadrant_version();
    manifest.name = name.unwrap_or_else(|| original_name.clone());
    if let Some(version) = version {
        manifest.version = version;
    }
    if let Some(mod_loader) = mod_loader {
        manifest.mod_loader = mod_loader;
    }
    write_manifest(&modpack_folder, &manifest)?;
    modpack.name = manifest.name;
    modpack.version = manifest.version;
    modpack.mod_loader = manifest.mod_loader;
    modpack.mods = manifest.mods;

    if modpack.name != original_name {
        let was_applied = modpack.is_applied;
        let renamed_folder = modpack_path(mc_folder, &modpack.name);
        std::fs::rename(&modpack_folder, &renamed_folder)?;
        if was_applied {
            let mods_path = mc_folder.join("mods");
            if mods_path.exists() || mods_path.is_symlink() {
                std::fs::remove_dir_all(&mods_path)?;
            }
            #[cfg(target_os = "windows")]
            std::os::windows::fs::symlink_dir(&renamed_folder, &mods_path)?;
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            std::os::unix::fs::symlink(&renamed_folder, &mods_path)?;
        }
    }

    Ok(modpack)
}

/// Deletes a modpack and removes the active `mods` link if needed.
pub fn delete_modpack(
    mc_folder: &Path,
    existing_modpacks: &[LocalModpack],
    name: &str,
) -> Result<()> {
    validate_modpack_name(name)?;
    log::info!("Deleting modpack \"{name}\"");
    let is_applied = existing_modpacks
        .iter()
        .find(|modpack| modpack.name == name)
        .map(|modpack| modpack.is_applied)
        .ok_or_else(|| anyhow::Error::from(crate::error::ErrorCode::ModpackMissing))?;

    let mods_folder = mc_folder.join("mods");
    if is_applied && mods_folder.exists() {
        std::fs::remove_dir_all(mods_folder)?;
    }

    std::fs::remove_dir_all(modpack_path(mc_folder, name))?;
    Ok(())
}

/// Registers a mod entry inside the target modpack manifest.
pub async fn register_mod(
    mc_folder: &Path,
    existing_modpacks: &[LocalModpack],
    mut mod_: InstalledMod,
    modpack_name: &str,
) -> Result<()> {
    validate_modpack_name(modpack_name)?;
    log::info!("Registering mod {} in modpack \"{modpack_name}\"", mod_.id);
    let modpack = existing_modpacks
        .iter()
        .find(|modpack| modpack.name == modpack_name)
        .cloned()
        .ok_or_else(|| anyhow::Error::from(crate::error::ErrorCode::ModpackMissing))?;
    modpack.ensure_manifest_readable()?;

    if mod_.name.is_empty() && mod_.source != ModSource::Online {
        match enrich_installed_mod(mod_.clone()).await {
            Ok(enriched) => mod_ = enriched,
            Err(e) => log::warn!(
                "Failed to enrich mod {} in modpack \"{}\": {}",
                mod_.id,
                modpack_name,
                e
            ),
        }
    }

    // The file already in the pack is what this entry installs. A hash sent
    // along (from identifying a renamed file) only stands in when it is absent.
    let modpack_folder = modpack_path(mc_folder, modpack_name);
    let installed_file = crate::mc_mod::download_file_name(&mod_.download_url)
        .map(|file_name| modpack_folder.join(file_name))
        .filter(|path| path.is_file());
    if let Some(path) = installed_file {
        mod_.sha512 = Some(tokio::task::spawn_blocking(move || file_sha512(&path)).await??);
    }

    let _lock = lock_manifests(mc_folder)?;
    let mut manifest = read_manifest(&modpack_folder, modpack_name)?
        .unwrap_or_else(|| InstalledModpack::from(modpack));
    if manifest.mods.iter().any(|existing| existing.id == mod_.id) {
        return Err(anyhow::Error::from(
            crate::error::ErrorCode::ModAlreadyRegistered,
        ));
    }
    manifest.mods.push(mod_);
    write_manifest(&modpack_folder, &manifest)
}

/// Removes a mod entry and its downloaded file from a modpack.
pub fn delete_mod(
    mc_folder: &Path,
    existing_modpacks: &[LocalModpack],
    modpack_name: &str,
    mod_id: &str,
) -> Result<LocalModpack> {
    validate_modpack_name(modpack_name)?;
    log::info!("Deleting mod {mod_id} from modpack \"{modpack_name}\"");
    let mut modpack = existing_modpacks
        .iter()
        .find(|modpack| modpack.name == modpack_name)
        .cloned()
        .ok_or_else(|| anyhow::Error::from(crate::error::ErrorCode::ModpackMissing))?;
    modpack.ensure_manifest_readable()?;

    let modpack_folder = modpack_path(mc_folder, modpack_name);
    let _lock = lock_manifests(mc_folder)?;
    if let Some(manifest) = read_manifest(&modpack_folder, modpack_name)? {
        modpack.mods = manifest.mods;
    }
    let to_delete_ids: Vec<_> = modpack
        .mods
        .iter()
        .filter(|mod_| mod_.id == mod_id)
        .map(|mod_| mod_.id.clone())
        .collect();
    if to_delete_ids.is_empty() {
        return Err(anyhow!("No mods found"));
    }

    // Entries whose download URL fails the current policy (legacy manifests,
    // manually edited packs) must still be removable from the manifest; only
    // their on-disk file is left alone.
    let to_delete_paths: Vec<_> = modpack
        .mods
        .iter()
        .filter(|mod_| mod_.id == mod_id)
        .filter_map(
            |mod_| match safe_download_file_name(&mod_.download_url, &mod_.source) {
                Ok(name) => Some(modpack_folder.join(name)),
                Err(error) => {
                    log::warn!(
                        "Removing mod {} from \"{modpack_name}\" without deleting its file: {error}",
                        mod_.id
                    );
                    None
                }
            },
        )
        .collect();
    modpack
        .mods
        .retain(|mod_| !to_delete_ids.contains(&mod_.id));

    for file in to_delete_paths {
        if file.exists() {
            std::fs::remove_file(file)?;
        }
    }

    write_manifest(&modpack_folder, &InstalledModpack::from(modpack.clone()))?;
    Ok(modpack)
}

/// Fails when the named pack has a manifest that does not parse, so a caller
/// holding a listing from elsewhere cannot push it over the real one.
pub fn check_manifest_readable(mc_folder: &Path, name: &str) -> Result<()> {
    validate_modpack_name(name)?;
    read_manifest(&modpack_path(mc_folder, name), name).map(drop)
}

/// Records a `sha512` for every entry whose file is in its pack folder but
/// whose manifest predates hashes, and returns how many it recorded. It runs
/// on every launch: entries that have one, and packs with nothing to add, are
/// left alone, so after the first launch of a version it finds no work.
pub async fn backfill_mod_hashes(mc_folder: &Path) -> Result<usize> {
    let mc_folder = mc_folder.to_path_buf();
    let modpacks_folder = mc_folder.join("modpacks");
    tokio::task::spawn_blocking(move || {
        let Ok(entries) = std::fs::read_dir(&modpacks_folder) else {
            return Ok(0);
        };
        let mut recorded = 0;
        for folder in entries.flatten().map(|entry| entry.path()) {
            if !folder.is_dir() {
                continue;
            }
            match backfill_pack_hashes(&mc_folder, &folder) {
                Ok(count) => recorded += count,
                Err(error) => log::warn!(
                    "Skipped recording file hashes for {}: {error}",
                    folder.display()
                ),
            }
        }
        Ok(recorded)
    })
    .await?
}

fn backfill_pack_hashes(mc_folder: &Path, folder: &Path) -> Result<usize> {
    let manifest_path = folder.join("modConfigV2.json");
    if !manifest_path.is_file() {
        return Ok(0);
    }
    let read_manifest = || -> Result<InstalledModpack> {
        Ok(serde_json::from_slice(&std::fs::read(&manifest_path)?)?)
    };
    let hashes: Vec<(String, String)> = read_manifest()?
        .mods
        .into_iter()
        .filter(|mod_| mod_.sha512.is_none())
        .filter_map(|mod_| {
            let path = folder.join(crate::mc_mod::download_file_name(&mod_.download_url)?);
            let hash = file_sha512(&path).ok()?;
            Some((mod_.download_url, hash))
        })
        .collect();
    if hashes.is_empty() {
        return Ok(0);
    }

    // Hashing takes a while, so the manifest is read again under the lock and
    // only entries that still name the same download take a hash.
    let _lock = lock_manifests(mc_folder)?;
    let mut manifest = read_manifest()?;
    let mut recorded = 0;
    for mod_ in manifest
        .mods
        .iter_mut()
        .filter(|mod_| mod_.sha512.is_none())
    {
        if let Some((_, hash)) = hashes.iter().find(|(url, _)| url == &mod_.download_url) {
            mod_.sha512 = Some(hash.clone());
            recorded += 1;
        }
    }
    if recorded > 0 {
        write_file_atomically(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    }
    Ok(recorded)
}

/// Persists the sync metadata for a modpack.
pub fn set_modpack_sync_date(
    mc_folder: &Path,
    time: u64,
    modpack: &str,
    modpack_id: Option<&str>,
) -> Result<()> {
    validate_modpack_name(modpack)?;
    write_file_atomically(
        modpack_path(mc_folder, modpack).join("quadrantSync.json"),
        serde_json::to_string_pretty(&SyncInfo {
            last_synced: time as i64,
            modpack_id: modpack_id.map(ToOwned::to_owned),
        })?,
    )?;
    Ok(())
}

/// The entries of an incoming mod list that deserve a warning before it is
/// installed over `current`, the list it replaces.
#[derive(Debug, Clone, Default)]
pub struct IncomingModsReview {
    /// New or changed entries that download from a link someone added, which
    /// can be any HTTPS host.
    pub online: Vec<InstalledMod>,
    /// New or changed entries with no `sha512`, so their files can not be
    /// verified. Packs shared from older versions have none at all.
    pub unverified: Vec<InstalledMod>,
}

/// Sorts out the incoming entries that are new or changed against `current`
/// (same id, source and download). Pass an empty `current` to review a whole
/// shared pack.
pub fn review_incoming_mods(
    current: &[InstalledMod],
    incoming: &[InstalledMod],
) -> IncomingModsReview {
    let mut review = IncomingModsReview::default();
    let changed = incoming.iter().filter(|mod_| {
        !current.iter().any(|existing| {
            existing.id == mod_.id
                && existing.source == mod_.source
                && existing.download_url == mod_.download_url
        })
    });
    for mod_ in changed {
        if mod_.source == ModSource::Online {
            review.online.push(mod_.clone());
        }
        if mod_.sha512.is_none() {
            review.unverified.push(mod_.clone());
        }
    }
    review
}

/// Downloads the files declared by a modpack manifest into its local folder.
///
/// Progress is emitted through [`BackendEvent::ModpackDownloadProgress`].
pub async fn install_modpack(
    mc_folder: &Path,
    mut mod_config: InstalledModpack,
    settings: &impl SettingsStore,
    event_sink: &impl EventSink,
) -> Result<()> {
    validate_new_modpack_name(&mod_config.name)?;
    log::info!(
        "Installing modpack \"{}\" ({} mod(s))",
        mod_config.name,
        mod_config.mods.len()
    );
    mod_config.mod_config_version = "2".to_string();
    mod_config.quadrant_version = crate::models::quadrant_version();

    let expected_file_names = mod_config
        .mods
        .iter()
        .map(|mod_| safe_download_file_name(&mod_.download_url, &mod_.source))
        .collect::<Result<Vec<_>>>()?;
    // Case-insensitive file systems would download both into one file.
    let mut unique_file_names = HashSet::new();
    for name in &expected_file_names {
        if !unique_file_names.insert(name.to_lowercase()) {
            return Err(anyhow!("Duplicate download file name: {name}"));
        }
    }

    let modpack_folder = modpack_path(mc_folder, &mod_config.name);
    std::fs::create_dir_all(&modpack_folder)?;
    let old_manifest = modpack_folder.join("modConfigV2.json");
    let old_mod_config = match std::fs::read_to_string(&old_manifest) {
        Ok(raw) => Some(
            serde_json::from_str::<InstalledModpack>(&raw)
                .map_err(|error| crate::models::unreadable_manifest(&mod_config.name, error))?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };

    enrich_unnamed_mods(&mut mod_config).await;

    let staging = modpack_folder.join(format!(
        ".quadrant-install-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir(&staging)?;
    let staging = StagingDir(staging);
    download_missing_files(
        &mut mod_config.mods,
        &expected_file_names,
        &modpack_folder,
        &staging.0,
        old_mod_config.as_ref(),
        settings,
        event_sink,
    )
    .await?;

    let _lock = lock_manifests(mc_folder)?;
    let current_mod_config = read_manifest(&modpack_folder, &mod_config.name)?;
    // Only files the manifest tracks now go, and the plan is made before
    // anything moves. Entries from an older URL policy still name their jar.
    let expected_lowercase: HashSet<_> = expected_file_names
        .iter()
        .map(|name| name.to_lowercase())
        .collect();
    let obsolete_files: Vec<_> = current_mod_config
        .iter()
        .flat_map(|old| &old.mods)
        .filter_map(|old_mod| crate::mc_mod::download_file_name(&old_mod.download_url))
        .filter(|old_name| !expected_lowercase.contains(&old_name.to_lowercase()))
        .map(|old_name| modpack_folder.join(old_name))
        .collect();

    for entry in std::fs::read_dir(&staging.0)? {
        let entry = entry?;
        std::fs::rename(entry.path(), modpack_folder.join(entry.file_name()))?;
    }
    drop(staging);

    for old_path in obsolete_files {
        if old_path.is_file() {
            std::fs::remove_file(old_path)?;
        }
    }

    write_file_atomically(&old_manifest, serde_json::to_string_pretty(&mod_config)?)?;

    event_sink.publish(BackendEvent::ModpackDownloadProgress(1.0))?;
    log::info!("Modpack installation complete");
    Ok(())
}

/// Exports a modpack folder as a Quadrant zip archive at the provided path.
///
/// Progress is emitted through [`BackendEvent::QuadrantExportProgress`].
pub fn export_modpack_to(
    mc_folder: &Path,
    modpack: &str,
    destination: &Path,
    event_sink: &impl EventSink,
) -> Result<()> {
    validate_modpack_name(modpack)?;
    log::info!(
        "Exporting modpack \"{modpack}\" to {}",
        destination.display()
    );
    let modpack_folder = modpack_path(mc_folder, modpack);
    // Only regular files belong in the archive. Subdirectories (for example a
    // staging folder left by an interrupted install) must not abort the export.
    let mut entries = std::fs::read_dir(&modpack_folder)?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    // Build the archive beside the destination and rename it into place, so a
    // failed export never leaves a truncated zip where the user asked for one.
    let file_name = destination
        .file_name()
        .ok_or_else(|| anyhow!("Invalid export destination"))?
        .to_string_lossy()
        .to_string();
    let partial_destination = destination.with_file_name(format!("{file_name}.part"));
    let result = write_export_archive(&entries, &partial_destination, event_sink).and_then(|()| {
        std::fs::rename(&partial_destination, destination).map_err(anyhow::Error::from)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&partial_destination);
        return result;
    }

    event_sink.publish(BackendEvent::QuadrantExportProgress(1.0))?;
    log::info!("Modpack export complete");
    Ok(())
}

fn write_export_archive(
    entries: &[std::fs::DirEntry],
    archive_path: &Path,
    event_sink: &impl EventSink,
) -> Result<()> {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(archive_path)?);
    let compressed: FileOptions<ExtendedFileOptions> = FileOptions::default()
        .compression_method(zip::CompressionMethod::Bzip2)
        .unix_permissions(0o755)
        .compression_level(Some(9))
        .large_file(true);
    // Jars and zips are compressed already; recompressing them costs minutes
    // on a large pack and saves nothing.
    let stored = compressed
        .clone()
        .compression_method(zip::CompressionMethod::Stored)
        .compression_level(None);
    let total_files = entries.len();

    for (index, file) in entries.iter().enumerate() {
        let file_name = file.file_name().to_string_lossy().to_string();
        let lowercase = file_name.to_ascii_lowercase();
        let options = if lowercase.ends_with(".jar") || lowercase.ends_with(".zip") {
            stored.clone()
        } else {
            compressed.clone()
        };
        zip.start_file(file_name, options)?;
        std::io::copy(&mut std::fs::File::open(file.path())?, &mut zip)?;
        // 1.0 is sent once, after the archive is finished and in place.
        event_sink.publish(BackendEvent::QuadrantExportProgress(
            (index + 1) as f64 / (total_files + 1) as f64,
        ))?;
    }

    zip.finish()?;
    Ok(())
}

/// How many files a pack install downloads at once. More only gets Quadrant
/// rate limited or reset by the CDNs.
const DOWNLOAD_CONCURRENCY: usize = 6;

/// Fills `staging` with every entry's file that the pack folder does not
/// already hold, and records each entry's `sha512`. An entry that carries one
/// must match it, or nothing is installed.
async fn download_missing_files(
    mods: &mut [InstalledMod],
    file_names: &[String],
    modpack_folder: &Path,
    staging: &Path,
    old_mod_config: Option<&InstalledModpack>,
    settings: &impl SettingsStore,
    event_sink: &impl EventSink,
) -> Result<()> {
    let total_mods = mods.len();
    let mut downloads = Vec::new();
    for (index, (mod_, file_name)) in mods.iter_mut().zip(file_names).enumerate() {
        let final_path = modpack_folder.join(file_name);
        // A version may keep its filename while changing its download URL.
        // Reuse only a file recorded as the same download in the old manifest.
        let same_download = old_mod_config.is_some_and(|old| {
            old.mods.iter().any(|old_mod| {
                old_mod.id == mod_.id
                    && old_mod.source == mod_.source
                    && old_mod.download_url == mod_.download_url
            })
        });
        if same_download && final_path.is_file() {
            let local_hash =
                tokio::task::spawn_blocking(move || file_sha512(&final_path)).await??;
            match &mod_.sha512 {
                Some(expected) if !expected.eq_ignore_ascii_case(&local_hash) => {}
                _ => {
                    mod_.sha512 = Some(local_hash);
                    continue;
                }
            }
        }

        let download = download_mod(mod_.clone(), staging.join(file_name));
        downloads.push(async move { download.await.map(|hash| (index, hash)) });
    }

    // Dropping the stream on the first error cancels the downloads in flight.
    let mut downloads = futures::stream::iter(downloads).buffer_unordered(DOWNLOAD_CONCURRENCY);
    let mut downloaded = 0_usize;
    let (mut modrinth, mut curseforge) = (0_i64, 0_i64);
    while let Some(result) = downloads.next().await {
        let (index, hash) = result?;
        match mods[index].source {
            ModSource::Modrinth => modrinth += 1,
            ModSource::CurseForge => curseforge += 1,
            ModSource::Online => {}
        }
        mods[index].sha512 = Some(hash);
        downloaded += 1;
        // install_modpack sends 1.0 once the files are moved into the pack.
        event_sink.publish(BackendEvent::ModpackDownloadProgress(
            downloaded as f64 / (total_mods + 1) as f64,
        ))?;
    }

    for (key, count) in [("modrinthUsage", modrinth), ("curseforgeUsage", curseforge)] {
        if count > 0 {
            let current = settings.get_i64(key)?.unwrap_or_default();
            settings.set_i64(key, current + count)?;
        }
    }
    Ok(())
}

/// Streams one entry's file to `file` and returns its SHA-512, failing with
/// [`ErrorCode::Checksum`](crate::error::ErrorCode::Checksum) when the entry
/// records a different one.
async fn download_mod(mod_: InstalledMod, file: PathBuf) -> Result<String> {
    use tokio::io::AsyncWriteExt;

    safe_download_file_name(&mod_.download_url, &mod_.source)?;
    let response = crate::mc_mod::http::provider_http_client()
        .get(&mod_.download_url)
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = response.bytes_stream();
    let mut file = tokio::fs::File::create(file).await?;
    let mut hasher = sha2::Sha512::new();
    while let Some(item) = bytes.next().await {
        let item = item?;
        hasher.update(&item);
        file.write_all(&item).await?;
    }
    file.flush().await?;
    let hash = hex::encode(hasher.finalize());
    if mod_
        .sha512
        .as_ref()
        .is_some_and(|expected| !expected.eq_ignore_ascii_case(&hash))
    {
        return Err(anyhow::Error::from(crate::error::ErrorCode::Checksum));
    }
    Ok(hash)
}

/// The `.quadrant-install-*` folder a pack install downloads into. It is
/// removed however the install ends, including when its future is dropped.
struct StagingDir(PathBuf);

impl Drop for StagingDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::ensure_default_app_config, ports::SettingsStore};
    use httpmock::prelude::*;
    use serde_json::Value;
    use std::{cell::RefCell, collections::HashMap, io::Read};
    use tempfile::tempdir;

    #[derive(Default)]
    struct MemoryStore {
        values: RefCell<HashMap<String, Value>>,
    }

    impl SettingsStore for MemoryStore {
        fn get_value(&self, key: &str) -> Result<Option<Value>> {
            Ok(self.values.borrow().get(key).cloned())
        }

        fn set_value(&self, key: &str, value: Value) -> Result<()> {
            self.values.borrow_mut().insert(key.to_string(), value);
            Ok(())
        }

        fn entries(&self) -> Result<Vec<(String, Value)>> {
            Ok(self
                .values
                .borrow()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect())
        }
    }

    #[derive(Default)]
    struct CollectingEvents {
        events: RefCell<Vec<BackendEvent>>,
    }

    impl EventSink for CollectingEvents {
        fn publish(&self, event: BackendEvent) -> Result<()> {
            self.events.borrow_mut().push(event);
            Ok(())
        }
    }

    fn setup_mc_folder() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let mc_folder = dir.path().join(".minecraft");
        std::fs::create_dir_all(mc_folder.join("modpacks")).unwrap();
        (dir, mc_folder)
    }

    #[test]
    fn a_failed_link_gives_the_mods_folder_back() {
        let (_dir, mc_folder) = setup_mc_folder();
        let mods_path = mc_folder.join("mods");
        let backup = mc_folder
            .join("modpacks")
            .join("mods-backup-20260101-000000");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("loose.jar"), "jar").unwrap();
        let failure = || {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "a privilege is not held",
            ))
        };

        assert!(restore_backup_on_error(failure(), Some(&backup), &mods_path).is_err());

        assert!(!backup.exists());
        assert!(mods_path.join("loose.jar").exists());
        // Nothing was set aside, so there is nothing to put back.
        assert!(restore_backup_on_error(failure(), None, &mods_path).is_err());
        assert!(mods_path.join("loose.jar").exists());
        assert!(restore_backup_on_error(Ok(()), Some(&backup), &mods_path).is_ok());
        assert!(mods_path.join("loose.jar").exists());
    }

    fn online_mod(id: &str, download_url: String) -> InstalledMod {
        InstalledMod {
            id: id.to_string(),
            source: ModSource::Online,
            download_url,
            name: id.to_string(),
            download_count: 0,
            version: String::new(),
            mod_type: String::new(),
            slug: String::new(),
            thumbnail_urls: Vec::new(),
            description: String::new(),
            license: String::new(),
            mod_icon_url: String::new(),
            sha512: None,
        }
    }

    #[tokio::test]
    async fn create_update_register_delete_and_discover_modpacks() {
        let (_dir, mc_folder) = setup_mc_folder();
        let existing = get_modpacks(&mc_folder, false).await.unwrap();
        create_modpack(&mc_folder, &existing, "alpha", "1.20.1", ModLoader::Fabric).unwrap();

        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "alpha");

        register_mod(
            &mc_folder,
            &listed,
            InstalledMod::minimal(
                "mod-1".to_string(),
                ModSource::Modrinth,
                "https://example.invalid/mod.jar".to_string(),
            ),
            "alpha",
        )
        .await
        .unwrap();

        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert_eq!(listed[0].mods.len(), 1);
        std::fs::write(modpack_path(&mc_folder, "alpha").join("mod.jar"), "jar").unwrap();

        let updated = update_modpack(
            &mc_folder,
            &listed,
            "alpha",
            Some("beta".to_string()),
            Some("1.21".to_string()),
            Some(ModLoader::NeoForge),
        )
        .unwrap();
        assert_eq!(updated.name, "beta");

        set_modpack_sync_date(&mc_folder, 42, "beta", Some("modpack-123")).unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert_eq!(listed[0].last_synced, 42_000);
        assert_eq!(listed[0].modpack_id.as_deref(), Some("modpack-123"));

        delete_mod(&mc_folder, &listed, "beta", "mod-1").unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert!(listed[0].mods.is_empty());

        delete_modpack(&mc_folder, &listed, "beta").unwrap();
        assert!(get_modpacks(&mc_folder, false).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn get_modpacks_reads_legacy_sync_metadata_without_modpack_id() {
        let (_dir, mc_folder) = setup_mc_folder();
        std::fs::create_dir_all(modpack_path(&mc_folder, "legacy")).unwrap();
        std::fs::write(
            modpack_path(&mc_folder, "legacy").join("modConfig.json"),
            serde_json::to_string_pretty(&InstalledModpack {
                mod_config_version: String::new(),
                quadrant_version: String::new(),
                name: "legacy".to_string(),
                version: "1.20.1".to_string(),
                mod_loader: ModLoader::Fabric,
                mods: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            modpack_path(&mc_folder, "legacy").join("quadrantSync.json"),
            r#"{"last_synced":99}"#,
        )
        .unwrap();

        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert_eq!(listed[0].last_synced, 99_000);
        assert_eq!(listed[0].modpack_id, None);
    }

    #[tokio::test]
    async fn get_modpack_reads_only_the_named_pack() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "one", "1.20.1", ModLoader::Fabric).unwrap();
        create_modpack(&mc_folder, &[], "two", "1.21", ModLoader::Quilt).unwrap();
        std::fs::write(
            modpack_path(&mc_folder, "two").join("modConfigV2.json"),
            "{",
        )
        .unwrap();

        let one = get_modpack(&mc_folder, "one").await.unwrap().unwrap();
        assert_eq!(one.name, "one");
        assert_eq!(one.version, "1.20.1");
        assert!(one.manifest_error.is_none());
        assert!(get_modpack(&mc_folder, "missing").await.unwrap().is_none());
        assert!(get_modpack(&mc_folder, "../one").await.is_err());
    }

    #[tokio::test]
    async fn listing_leaves_a_manifest_alone_when_no_entry_learned_a_name() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "bare", "1.20.1", ModLoader::Fabric).unwrap();
        let bare = InstalledMod::minimal(
            "listing-cache-test-id".to_string(),
            ModSource::Modrinth,
            "https://cdn.modrinth.com/data/x/versions/y/bare.jar".to_string(),
        );
        let manifest_path = modpack_path(&mc_folder, "bare").join("modConfigV2.json");
        let mut manifest: InstalledModpack =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest.mods.push(bare.clone());
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();
        let before = std::fs::read(&manifest_path).unwrap();
        ENRICH_FAILURES
            .lock()
            .unwrap()
            .insert(enrich_key(&bare), std::time::Instant::now());

        let listed = get_modpack(&mc_folder, "bare").await.unwrap().unwrap();
        assert_eq!(listed.mods.len(), 1);
        assert!(listed.mods[0].name.is_empty());
        assert_eq!(std::fs::read(&manifest_path).unwrap(), before);
    }

    #[test]
    fn export_modpack_to_zip() {
        let (_dir, mc_folder) = setup_mc_folder();
        std::fs::create_dir_all(modpack_path(&mc_folder, "alpha")).unwrap();
        std::fs::write(
            modpack_path(&mc_folder, "alpha").join("modConfigV2.json"),
            "{}",
        )
        .unwrap();
        std::fs::write(modpack_path(&mc_folder, "alpha").join("sample.jar"), "jar").unwrap();

        let destination = mc_folder.join("alpha.quadrantExport.zip");
        let events = CollectingEvents::default();
        export_modpack_to(&mc_folder, "alpha", &destination, &events).unwrap();

        let file = std::fs::File::open(destination).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(
            archive.by_name("modConfigV2.json").unwrap().compression(),
            zip::CompressionMethod::Bzip2
        );
        let mut file = archive.by_name("sample.jar").unwrap();
        assert_eq!(file.compression(), zip::CompressionMethod::Stored);
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "jar");

        let progress: Vec<f64> = events
            .events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                BackendEvent::QuadrantExportProgress(value) => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(progress.iter().filter(|value| **value >= 1.0).count(), 1);
        assert_eq!(progress.last(), Some(&1.0));
    }

    #[tokio::test]
    async fn install_modpack_downloads_files_and_updates_usage() {
        let server = MockServer::start();
        let mod_jar = server.mock(|when, then| {
            when.method(GET).path("/mod.jar");
            then.status(200).body("mod-bytes");
        });

        let (_dir, mc_folder) = setup_mc_folder();
        let store = MemoryStore::default();
        ensure_default_app_config(&store).unwrap();
        let events = CollectingEvents::default();

        install_modpack(
            &mc_folder,
            InstalledModpack {
                mod_config_version: String::new(),
                quadrant_version: String::new(),
                name: "alpha".to_string(),
                version: "1.20.1".to_string(),
                mod_loader: ModLoader::Fabric,
                mods: vec![InstalledMod {
                    id: "mod-1".to_string(),
                    source: ModSource::Modrinth,
                    download_url: format!("{}/mod.jar", server.base_url()),
                    name: "Test Mod".to_string(),
                    download_count: 0,
                    version: String::new(),
                    mod_type: String::new(),
                    slug: String::new(),
                    thumbnail_urls: Vec::new(),
                    description: String::new(),
                    license: String::new(),
                    mod_icon_url: String::new(),
                    sha512: None,
                }],
            },
            &store,
            &events,
        )
        .await
        .unwrap();

        mod_jar.assert();
        assert!(modpack_path(&mc_folder, "alpha").join("mod.jar").exists());
        let installed: InstalledModpack = serde_json::from_slice(
            &std::fs::read(modpack_path(&mc_folder, "alpha").join("modConfigV2.json")).unwrap(),
        )
        .unwrap();
        // An entry shared without a hash gets the one of the bytes written.
        assert_eq!(
            installed.mods[0].sha512,
            Some(crate::mc_mod::cache::sha512_hex(b"mod-bytes"))
        );
        assert_eq!(store.get_i64("modrinthUsage").unwrap(), Some(1));
        assert!(
            events
                .events
                .borrow()
                .contains(&BackendEvent::ModpackDownloadProgress(1.0))
        );
    }

    #[tokio::test]
    async fn install_modpack_replaces_changed_download_with_same_file_name() {
        let server = MockServer::start();
        let download = server.mock(|when, then| {
            when.method(GET).path("/v2/mod.jar");
            then.status(200).body("new version");
        });
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        let folder = modpack_path(&mc_folder, "alpha");
        let mut pack: InstalledModpack =
            serde_json::from_slice(&std::fs::read(folder.join("modConfigV2.json")).unwrap())
                .unwrap();
        pack.mods = vec![online_mod(
            "mod",
            format!("{}/v1/mod.jar", server.base_url()),
        )];
        std::fs::write(
            folder.join("modConfigV2.json"),
            serde_json::to_vec(&pack).unwrap(),
        )
        .unwrap();
        std::fs::write(folder.join("mod.jar"), "old version").unwrap();
        pack.mods[0].download_url = format!("{}/v2/mod.jar", server.base_url());

        install_modpack(
            &mc_folder,
            pack.clone(),
            &MemoryStore::default(),
            &CollectingEvents::default(),
        )
        .await
        .unwrap();

        download.assert();
        assert_eq!(
            std::fs::read_to_string(folder.join("mod.jar")).unwrap(),
            "new version"
        );
        let installed: InstalledModpack =
            serde_json::from_slice(&std::fs::read(folder.join("modConfigV2.json")).unwrap())
                .unwrap();
        assert_eq!(installed.mods[0].download_url, pack.mods[0].download_url);
    }

    #[tokio::test]
    async fn install_modpack_rejects_duplicate_file_names_before_writing() {
        let (_dir, mc_folder) = setup_mc_folder();
        let result = install_modpack(
            &mc_folder,
            InstalledModpack {
                mod_config_version: "2".to_string(),
                quadrant_version: String::new(),
                name: "alpha".to_string(),
                version: "1.20.1".to_string(),
                mod_loader: ModLoader::Fabric,
                mods: vec![
                    online_mod("one", "https://example.invalid/one/mod.jar".to_string()),
                    online_mod("two", "https://example.invalid/two/mod.jar".to_string()),
                ],
            },
            &MemoryStore::default(),
            &CollectingEvents::default(),
        )
        .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate download file name")
        );
        assert!(!modpack_path(&mc_folder, "alpha").exists());
    }

    #[tokio::test]
    async fn install_modpack_rejects_names_that_alias_another_pack_or_file() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "Survival", "1.20.1", ModLoader::Fabric).unwrap();
        let manifest = modpack_path(&mc_folder, "Survival").join("modConfigV2.json");
        let original = std::fs::read(&manifest).unwrap();
        let pack = |name: &str, mods: Vec<InstalledMod>| InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: String::new(),
            name: name.to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods,
        };
        let install = |pack: InstalledModpack| {
            let mc_folder = mc_folder.clone();
            async move {
                install_modpack(
                    &mc_folder,
                    pack,
                    &MemoryStore::default(),
                    &CollectingEvents::default(),
                )
                .await
            }
        };

        for name in ["Survival.", "Survival ", "NUL"] {
            assert!(install(pack(name, Vec::new())).await.is_err(), "{name:?}");
        }
        for url in [
            "https://example.invalid/NUL.jar",
            "https://example.invalid/x.jar:s",
        ] {
            let mods = vec![online_mod("one", url.to_string())];
            assert!(install(pack("Survival", mods)).await.is_err(), "{url}");
        }
        let mods = vec![
            online_mod("one", "https://example.invalid/one/Sodium.jar".to_string()),
            online_mod("two", "https://example.invalid/two/sodium.jar".to_string()),
        ];
        let error = install(pack("Survival", mods)).await.unwrap_err();
        assert!(error.to_string().contains("Duplicate download file name"));
        assert_eq!(std::fs::read(&manifest).unwrap(), original);
    }

    #[tokio::test]
    async fn update_modpack_renames_a_pack_by_case_only() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();

        let renamed = update_modpack(
            &mc_folder,
            &listed,
            "alpha",
            Some("Alpha".to_string()),
            None,
            None,
        )
        .unwrap();

        assert_eq!(renamed.name, "Alpha");
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|pack| pack.name.as_str())
                .collect::<Vec<_>>(),
            ["Alpha"]
        );
        assert!(
            update_modpack(
                &mc_folder,
                &listed,
                "Alpha",
                Some("Alpha.".to_string()),
                None,
                None
            )
            .is_err()
        );
    }

    fn read_manifest(mc_folder: &Path, name: &str) -> InstalledModpack {
        serde_json::from_slice(
            &std::fs::read(modpack_path(mc_folder, name).join("modConfigV2.json")).unwrap(),
        )
        .unwrap()
    }

    fn seed_pack(mc_folder: &Path, mods: Vec<InstalledMod>, files: &[(&str, &str)]) -> String {
        let folder = modpack_path(mc_folder, "alpha");
        std::fs::create_dir_all(&folder).unwrap();
        let manifest = serde_json::to_string_pretty(&InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: String::new(),
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods,
        })
        .unwrap();
        std::fs::write(folder.join("modConfigV2.json"), &manifest).unwrap();
        for (name, contents) in files {
            std::fs::write(folder.join(name), contents).unwrap();
        }
        manifest
    }

    fn incoming(mods: Vec<InstalledMod>) -> InstalledModpack {
        InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: String::new(),
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods,
        }
    }

    fn hashed(mod_: InstalledMod, contents: &[u8]) -> InstalledMod {
        InstalledMod {
            sha512: Some(crate::mc_mod::cache::sha512_hex(contents)),
            ..mod_
        }
    }

    /// Adds an entry behind the back of a listing, as another app sharing the
    /// game folder would.
    fn add_entry_elsewhere(mc_folder: &Path, mod_: InstalledMod) {
        let mut manifest = read_manifest(mc_folder, "alpha");
        manifest.mods.push(mod_);
        std::fs::write(
            modpack_path(mc_folder, "alpha").join("modConfigV2.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn entry_ids(mc_folder: &Path) -> Vec<String> {
        read_manifest(mc_folder, "alpha")
            .mods
            .into_iter()
            .map(|mod_| mod_.id)
            .collect()
    }

    #[tokio::test]
    async fn manifest_edits_from_a_stale_listing_keep_entries_written_since() {
        let (_dir, mc_folder) = setup_mc_folder();
        seed_pack(
            &mc_folder,
            vec![online_mod("old", "https://example.com/old.jar".to_string())],
            &[],
        );
        let stale = get_modpacks(&mc_folder, false).await.unwrap();

        add_entry_elsewhere(
            &mc_folder,
            online_mod("first", "https://example.com/first.jar".to_string()),
        );
        delete_mod(&mc_folder, &stale, "alpha", "old").unwrap();
        assert_eq!(entry_ids(&mc_folder), ["first"]);

        add_entry_elsewhere(
            &mc_folder,
            online_mod("second", "https://example.com/second.jar".to_string()),
        );
        register_mod(
            &mc_folder,
            &stale,
            online_mod("third", "https://example.com/third.jar".to_string()),
            "alpha",
        )
        .await
        .unwrap();
        assert_eq!(entry_ids(&mc_folder), ["first", "second", "third"]);

        let updated = update_modpack(
            &mc_folder,
            &stale,
            "alpha",
            None,
            Some("1.21".to_string()),
            None,
        )
        .unwrap();
        assert_eq!(entry_ids(&mc_folder), ["first", "second", "third"]);
        assert_eq!(updated.mods.len(), 3);
        assert_eq!(read_manifest(&mc_folder, "alpha").version, "1.21");

        // Registering what another writer already added is still refused.
        let duplicate = register_mod(
            &mc_folder,
            &stale,
            online_mod("second", "https://example.com/second.jar".to_string()),
            "alpha",
        )
        .await;
        assert!(duplicate.is_err());

        // The lock file beside the packs is neither a pack nor a stray file.
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert!(mc_folder.join("modpacks/.modpacks.lock").is_file());
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].unknown_mods);
    }

    #[test]
    fn concurrent_manifest_edits_lose_no_entry() {
        let (_dir, mc_folder) = setup_mc_folder();
        seed_pack(&mc_folder, Vec::new(), &[]);
        let stale = futures::executor::block_on(get_modpacks(&mc_folder, false)).unwrap();
        std::thread::scope(|scope| {
            for worker in 0..4 {
                let (mc_folder, stale) = (&mc_folder, &stale);
                scope.spawn(move || {
                    for index in 0..10 {
                        let id = format!("mod-{worker}-{index}");
                        let url = format!("https://example.com/{id}.jar");
                        futures::executor::block_on(register_mod(
                            mc_folder,
                            stale,
                            online_mod(&id, url),
                            "alpha",
                        ))
                        .unwrap();
                    }
                });
            }
        });
        assert_eq!(entry_ids(&mc_folder).len(), 40);
    }

    #[tokio::test]
    async fn a_file_that_fails_its_hash_leaves_the_existing_pack_untouched() {
        let server = MockServer::start();
        let tampered = server.mock(|when, then| {
            when.method(GET).path("/new.jar");
            then.status(200).body("tampered bytes");
        });
        let (_dir, mc_folder) = setup_mc_folder();
        let old = online_mod("old", "https://example.invalid/old.jar".to_string());
        let manifest = seed_pack(&mc_folder, vec![old.clone()], &[("old.jar", "old")]);
        let new = online_mod("new", format!("{}/new.jar", server.base_url()));

        let error = install_modpack(
            &mc_folder,
            incoming(vec![hashed(new, b"expected bytes")]),
            &MemoryStore::default(),
            &CollectingEvents::default(),
        )
        .await
        .unwrap_err();

        tampered.assert();
        assert_eq!(
            crate::error::user_facing(error).to_string(),
            "errorChecksum"
        );
        let folder = modpack_path(&mc_folder, "alpha");
        assert_eq!(
            std::fs::read_to_string(folder.join("modConfigV2.json")).unwrap(),
            manifest
        );
        assert!(folder.join("old.jar").exists());
        assert!(!folder.join("new.jar").exists());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
    }

    #[tokio::test]
    async fn install_modpack_verifies_hashes_and_records_reused_files() {
        let server = MockServer::start();
        let fresh = server.mock(|when, then| {
            when.method(GET).path("/fresh.jar");
            then.status(200).body("fresh bytes");
        });
        let replaced = server.mock(|when, then| {
            when.method(GET).path("/kept.jar");
            then.status(200).body("kept bytes");
        });
        let (_dir, mc_folder) = setup_mc_folder();
        let reused = online_mod("reused", format!("{}/reused.jar", server.base_url()));
        let kept = online_mod("kept", format!("{}/kept.jar", server.base_url()));
        seed_pack(
            &mc_folder,
            vec![reused.clone(), kept.clone()],
            &[
                ("reused.jar", "reused bytes"),
                ("kept.jar", "locally changed"),
            ],
        );
        let fresh_mod = online_mod("fresh", format!("{}/fresh.jar", server.base_url()));

        install_modpack(
            &mc_folder,
            incoming(vec![
                reused,
                hashed(kept, b"kept bytes"),
                hashed(fresh_mod, b"fresh bytes"),
            ]),
            &MemoryStore::default(),
            &CollectingEvents::default(),
        )
        .await
        .unwrap();

        fresh.assert();
        // The file on disk no longer matches the hash the pack names.
        replaced.assert();
        let installed = read_manifest(&mc_folder, "alpha");
        let hashes: Vec<_> = installed
            .mods
            .iter()
            .map(|mod_| mod_.sha512.clone().unwrap())
            .collect();
        assert_eq!(
            hashes,
            [b"reused bytes".as_slice(), b"kept bytes", b"fresh bytes"]
                .map(crate::mc_mod::cache::sha512_hex)
        );
        assert_eq!(
            std::fs::read(modpack_path(&mc_folder, "alpha").join("kept.jar")).unwrap(),
            b"kept bytes"
        );
    }

    #[tokio::test]
    async fn install_modpack_removes_obsolete_files_named_under_an_older_url_policy() {
        let (_dir, mc_folder) = setup_mc_folder();
        let legacy = online_mod("legacy", "http://example.invalid/legacy.jar".to_string());
        seed_pack(&mc_folder, vec![legacy], &[("legacy.jar", "legacy")]);

        install_modpack(
            &mc_folder,
            incoming(Vec::new()),
            &MemoryStore::default(),
            &CollectingEvents::default(),
        )
        .await
        .unwrap();

        assert!(
            !modpack_path(&mc_folder, "alpha")
                .join("legacy.jar")
                .exists()
        );
        assert!(read_manifest(&mc_folder, "alpha").mods.is_empty());
    }

    #[tokio::test]
    async fn register_mod_records_the_hash_of_the_file_in_the_pack() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        std::fs::write(modpack_path(&mc_folder, "alpha").join("found.jar"), "found").unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        let identified = |id: &str, file: &str| InstalledMod {
            sha512: Some("sent along".to_string()),
            ..online_mod(id, format!("https://example.invalid/{file}"))
        };

        register_mod(
            &mc_folder,
            &listed,
            identified("found", "found.jar"),
            "alpha",
        )
        .await
        .unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        register_mod(
            &mc_folder,
            &listed,
            identified("renamed", "gone.jar"),
            "alpha",
        )
        .await
        .unwrap();

        let mods = read_manifest(&mc_folder, "alpha").mods;
        assert_eq!(
            mods[0].sha512,
            Some(crate::mc_mod::cache::sha512_hex(b"found"))
        );
        assert_eq!(mods[1].sha512.as_deref(), Some("sent along"));
    }

    #[tokio::test]
    async fn backfill_records_missing_hashes_once_and_leaves_the_rest_alone() {
        let (_dir, mc_folder) = setup_mc_folder();
        let present = online_mod("present", "https://example.invalid/present.jar".to_string());
        let missing = online_mod("missing", "https://example.invalid/missing.jar".to_string());
        let known = InstalledMod {
            sha512: Some("already recorded".to_string()),
            ..online_mod("known", "https://example.invalid/known.jar".to_string())
        };
        seed_pack(
            &mc_folder,
            vec![present, missing, known],
            &[("present.jar", "present"), ("known.jar", "known")],
        );
        let broken = modpack_path(&mc_folder, "broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("modConfigV2.json"), "{not json").unwrap();
        std::fs::write(broken.join("mod.jar"), "jar").unwrap();
        std::fs::write(mc_folder.join("modpacks").join(".DS_Store"), "").unwrap();

        assert_eq!(backfill_mod_hashes(&mc_folder).await.unwrap(), 1);

        let mods = read_manifest(&mc_folder, "alpha").mods;
        assert_eq!(
            mods[0].sha512,
            Some(crate::mc_mod::cache::sha512_hex(b"present"))
        );
        assert_eq!(mods[1].sha512, None);
        assert_eq!(mods[2].sha512.as_deref(), Some("already recorded"));
        assert_eq!(
            std::fs::read_to_string(broken.join("modConfigV2.json")).unwrap(),
            "{not json"
        );

        let manifest = modpack_path(&mc_folder, "alpha").join("modConfigV2.json");
        let after_first = std::fs::read(&manifest).unwrap();
        let modified = std::fs::metadata(&manifest).unwrap().modified().unwrap();
        assert_eq!(backfill_mod_hashes(&mc_folder).await.unwrap(), 0);
        assert_eq!(std::fs::read(&manifest).unwrap(), after_first);
        assert_eq!(
            std::fs::metadata(&manifest).unwrap().modified().unwrap(),
            modified
        );
    }

    #[tokio::test]
    async fn backfill_without_a_modpacks_folder_does_nothing() {
        let dir = tempdir().unwrap();
        assert_eq!(backfill_mod_hashes(dir.path()).await.unwrap(), 0);
    }

    #[test]
    fn review_flags_only_new_or_changed_online_and_unhashed_entries() {
        let provider = |id: &str, url: &str| InstalledMod {
            sha512: Some("hash".to_string()),
            ..InstalledMod::minimal(id.to_string(), ModSource::Modrinth, url.to_string())
        };
        let unhashed = |id: &str, url: &str| InstalledMod {
            sha512: None,
            ..provider(id, url)
        };
        let online = |id: &str, url: &str| InstalledMod {
            sha512: Some("hash".to_string()),
            ..online_mod(id, url.to_string())
        };
        let current = vec![
            unhashed("kept", "https://cdn.modrinth.com/kept.jar"),
            online("link", "https://example.com/link-1.jar"),
            provider("bumped", "https://cdn.modrinth.com/bumped-1.jar"),
        ];

        let names =
            |mods: &[InstalledMod]| mods.iter().map(|mod_| mod_.id.clone()).collect::<Vec<_>>();
        // Unchanged entries need no review, even without a hash.
        let review = review_incoming_mods(&current, &current);
        assert!(review.online.is_empty() && review.unverified.is_empty());

        let incoming = vec![
            unhashed("kept", "https://cdn.modrinth.com/kept.jar"),
            online("link", "https://example.com/link-2.jar"),
            unhashed("bumped", "https://cdn.modrinth.com/bumped-2.jar"),
            provider("added", "https://cdn.modrinth.com/added.jar"),
            online_mod("new-link", "https://example.com/new.jar".to_string()),
        ];
        let review = review_incoming_mods(&current, &incoming);
        assert_eq!(names(&review.online), ["link", "new-link"]);
        assert_eq!(names(&review.unverified), ["bumped", "new-link"]);

        // A whole shared pack is reviewed against nothing.
        let review = review_incoming_mods(&[], &current);
        assert_eq!(names(&review.online), ["link"]);
        assert_eq!(names(&review.unverified), ["kept"]);
    }

    #[test]
    fn provider_entries_download_only_from_their_cdn() {
        let trusted = [
            (ModSource::Modrinth, "cdn.modrinth.com"),
            (ModSource::CurseForge, "edge.forgecdn.net"),
            (ModSource::CurseForge, "mediafilez.forgecdn.net"),
            (ModSource::Online, "example.com"),
        ];
        for (source, host) in trusted {
            assert!(is_trusted_download_host(&source, host), "{source:?} {host}");
        }
        let untrusted = [
            (ModSource::Modrinth, "evil.example"),
            (ModSource::Modrinth, "cdn.modrinth.com.evil.example"),
            (ModSource::Modrinth, "edge.forgecdn.net"),
            (ModSource::CurseForge, "evil.forgecdn.net"),
            (ModSource::CurseForge, "cdn.modrinth.com"),
        ];
        for (source, host) in untrusted {
            assert!(
                !is_trusted_download_host(&source, host),
                "{source:?} {host}"
            );
        }
        assert!(
            safe_download_file_name("https://evil.example/sodium.jar", &ModSource::Modrinth)
                .is_err()
        );
        assert!(
            safe_download_file_name("https://evil.example/sodium.jar", &ModSource::Online).is_ok()
        );
    }

    #[test]
    fn create_modpack_preserves_existing_manifest_with_stale_snapshot() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        let manifest = modpack_path(&mc_folder, "alpha").join("modConfigV2.json");
        let original = std::fs::read(&manifest).unwrap();

        assert!(create_modpack(&mc_folder, &[], "alpha", "1.21", ModLoader::Forge).is_err());
        assert_eq!(std::fs::read(&manifest).unwrap(), original);
    }

    #[tokio::test]
    async fn update_modpack_rejects_existing_destination_before_changing_manifest() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        let original_path = modpack_path(&mc_folder, "alpha");
        let original = std::fs::read(original_path.join("modConfigV2.json")).unwrap();
        // Even an empty destination created after listing belongs to the user.
        std::fs::create_dir(modpack_path(&mc_folder, "beta")).unwrap();

        assert!(
            update_modpack(
                &mc_folder,
                &listed,
                "alpha",
                Some("beta".to_string()),
                Some("1.21".to_string()),
                None,
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(original_path.join("modConfigV2.json")).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_dir(modpack_path(&mc_folder, "beta"))
                .unwrap()
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn install_modpack_rejects_traversal_names_and_file_names() {
        let server = MockServer::start();
        let (_dir, mc_folder) = setup_mc_folder();
        let store = MemoryStore::default();
        let events = CollectingEvents::default();

        let unsafe_pack = InstalledModpack {
            mod_config_version: String::new(),
            quadrant_version: String::new(),
            name: "../escape".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: Vec::new(),
        };
        assert!(
            install_modpack(&mc_folder, unsafe_pack, &store, &events)
                .await
                .is_err()
        );
        assert!(!mc_folder.join("escape").exists());

        let unsafe_file = InstalledModpack {
            mod_config_version: String::new(),
            quadrant_version: String::new(),
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: vec![online_mod(
                "unsafe",
                format!("{}/..%2Fescape.jar", server.base_url()),
            )],
        };
        assert!(
            install_modpack(&mc_folder, unsafe_file, &store, &events)
                .await
                .is_err()
        );
        assert!(!mc_folder.join("escape.jar").exists());
    }

    #[tokio::test]
    async fn failed_install_preserves_existing_pack_and_metadata() {
        let server = MockServer::start();
        let failed_download = server.mock(|when, then| {
            when.method(GET).path("/new.jar");
            then.status(500).body("failed");
        });
        let (_dir, mc_folder) = setup_mc_folder();
        let pack_folder = modpack_path(&mc_folder, "alpha");
        std::fs::create_dir_all(&pack_folder).unwrap();
        let old_pack = InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: "old".to_string(),
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: vec![online_mod(
                "old",
                "https://example.invalid/old.jar".to_string(),
            )],
        };
        let old_manifest = serde_json::to_string_pretty(&old_pack).unwrap();
        std::fs::write(pack_folder.join("modConfigV2.json"), &old_manifest).unwrap();
        std::fs::write(pack_folder.join("old.jar"), "old").unwrap();
        std::fs::write(pack_folder.join("quadrantSync.json"), "sync").unwrap();
        std::fs::write(pack_folder.join("notes.txt"), "user file").unwrap();

        let new_pack = InstalledModpack {
            mods: vec![online_mod("new", format!("{}/new.jar", server.base_url()))],
            quadrant_version: String::new(),
            ..old_pack
        };
        assert!(
            install_modpack(
                &mc_folder,
                new_pack,
                &MemoryStore::default(),
                &CollectingEvents::default(),
            )
            .await
            .is_err()
        );

        failed_download.assert();
        assert_eq!(
            std::fs::read_to_string(pack_folder.join("modConfigV2.json")).unwrap(),
            old_manifest
        );
        assert!(pack_folder.join("old.jar").exists());
        assert!(pack_folder.join("quadrantSync.json").exists());
        assert!(pack_folder.join("notes.txt").exists());
        assert!(!pack_folder.join("new.jar").exists());
        assert!(!has_staging_folder(&pack_folder));
    }

    fn has_staging_folder(pack_folder: &Path) -> bool {
        std::fs::read_dir(pack_folder).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".quadrant-install-")
        })
    }

    #[tokio::test]
    async fn dropped_install_removes_its_staging_folder() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/slow.jar");
            then.status(200)
                .delay(std::time::Duration::from_secs(10))
                .body("slow");
        });
        let (_dir, mc_folder) = setup_mc_folder();
        let pack = InstalledModpack {
            mod_config_version: "2".to_string(),
            quadrant_version: String::new(),
            name: "alpha".to_string(),
            version: "1.20.1".to_string(),
            mod_loader: ModLoader::Fabric,
            mods: vec![online_mod(
                "slow",
                format!("{}/slow.jar", server.base_url()),
            )],
        };
        let store = MemoryStore::default();
        let events = CollectingEvents::default();
        let install = install_modpack(&mc_folder, pack, &store, &events);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), install)
                .await
                .is_err()
        );

        assert!(!has_staging_folder(&modpack_path(&mc_folder, "alpha")));
    }

    #[tokio::test]
    async fn install_modpack_counts_provider_usage_once_per_install() {
        let server = MockServer::start();
        let jars = server.mock(|when, then| {
            when.method(GET).path_includes("/many-");
            then.status(200).body("jar");
        });
        let (_dir, mc_folder) = setup_mc_folder();
        let store = MemoryStore::default();
        store.set_i64("modrinthUsage", 5).unwrap();
        let events = CollectingEvents::default();
        let mods = (0..(DOWNLOAD_CONCURRENCY * 2 + 1))
            .map(|index| {
                InstalledMod::minimal(
                    format!("mod-{index}"),
                    ModSource::Modrinth,
                    format!("{}/many-{index}.jar", server.base_url()),
                )
            })
            .map(|mod_| InstalledMod {
                name: mod_.id.clone(),
                ..mod_
            })
            .collect::<Vec<_>>();
        let total = mods.len();
        install_modpack(
            &mc_folder,
            InstalledModpack {
                mod_config_version: "2".to_string(),
                quadrant_version: String::new(),
                name: "many".to_string(),
                version: "1.20.1".to_string(),
                mod_loader: ModLoader::Fabric,
                mods,
            },
            &store,
            &events,
        )
        .await
        .unwrap();

        jars.assert_calls(total);
        assert_eq!(
            store.get_i64("modrinthUsage").unwrap(),
            Some(5 + total as i64)
        );
        let progress: Vec<f64> = events
            .events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                BackendEvent::ModpackDownloadProgress(value) => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(progress.len(), total + 1);
        assert!(progress.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(progress.iter().filter(|value| **value >= 1.0).count(), 1);
        assert!(!has_staging_folder(&modpack_path(&mc_folder, "many")));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn apply_modpack_creates_symlink() {
        let (_dir, mc_folder) = setup_mc_folder();
        std::fs::create_dir_all(modpack_path(&mc_folder, "alpha")).unwrap();
        apply_modpack(&mc_folder, "alpha").unwrap();
        assert!(mc_folder.join("mods").is_symlink());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn apply_modpack_replaces_dangling_symlink() {
        let (_dir, mc_folder) = setup_mc_folder();
        std::fs::create_dir_all(modpack_path(&mc_folder, "alpha")).unwrap();
        std::fs::create_dir_all(modpack_path(&mc_folder, "beta")).unwrap();

        apply_modpack(&mc_folder, "alpha").unwrap();

        // Simulate the modpack directory being deleted outside of Quadrant,
        // leaving `mods` as a dangling symlink.
        std::fs::remove_dir_all(modpack_path(&mc_folder, "alpha")).unwrap();
        let mods_path = mc_folder.join("mods");
        assert!(mods_path.is_symlink());
        assert!(!mods_path.exists());

        // Applying another modpack should remove the broken link and re-point it.
        apply_modpack(&mc_folder, "beta").unwrap();
        assert!(mods_path.is_symlink());
        assert_eq!(
            mods_path.read_link().unwrap(),
            modpack_path(&mc_folder, "beta")
        );
    }

    #[test]
    fn validate_modpack_name_accepts_single_normal_components() {
        for name in ["alpha", "My Pack 2", "пак", "パック", "pack.v2"] {
            assert!(validate_modpack_name(name).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn validate_modpack_name_rejects_traversal_and_multi_component_paths() {
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "/etc",
            "../escape",
            "a/..",
            "a/./b",
            "modpacks/../../x",
        ] {
            assert!(validate_modpack_name(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn safe_download_file_name_extracts_and_decodes_names() {
        assert_eq!(
            safe_download_file_name(
                "https://cdn.modrinth.com/data/AANobbMI/versions/mod.jar",
                &ModSource::Modrinth,
            )
            .unwrap(),
            "mod.jar"
        );
        assert_eq!(
            safe_download_file_name(
                "https://cdn.modrinth.com/data/my%20mod%20v1.2.jar",
                &ModSource::Modrinth,
            )
            .unwrap(),
            "my mod v1.2.jar"
        );
        assert_eq!(
            safe_download_file_name(
                "https://mediafilez.forgecdn.net/files/1/2/jei.jar",
                &ModSource::CurseForge,
            )
            .unwrap(),
            "jei.jar"
        );
        assert_eq!(
            safe_download_file_name("https://example.com/files/пак.jar", &ModSource::Online)
                .unwrap(),
            "пак.jar"
        );
        assert_eq!(
            safe_download_file_name("https://cdn.modrinth.com/mod.jar/", &ModSource::Modrinth)
                .unwrap(),
            "mod.jar"
        );
    }

    #[test]
    fn safe_download_file_name_restricts_schemes() {
        assert!(
            safe_download_file_name("http://127.0.0.1:8080/mod.jar", &ModSource::Modrinth).is_ok()
        );
        assert!(
            safe_download_file_name("http://localhost:8080/mod.jar", &ModSource::Modrinth).is_err()
        );
        assert!(safe_download_file_name("http://example.com/mod.jar", &ModSource::Online).is_err());
        assert!(
            safe_download_file_name("ftp://cdn.modrinth.com/mod.jar", &ModSource::Modrinth)
                .is_err()
        );
        assert!(safe_download_file_name("file:///etc/passwd", &ModSource::Online).is_err());
    }

    #[test]
    fn safe_download_file_name_rejects_traversal_and_empty_names() {
        for url in [
            "https://cdn.modrinth.com/..%2Fescape.jar",
            "https://cdn.modrinth.com/%2E%2E",
            "https://cdn.modrinth.com/a%2Fb.jar",
            "https://cdn.modrinth.com/%2Fetc%2Fpasswd",
            "https://cdn.modrinth.com",
            "https://cdn.modrinth.com/",
            "not a url",
        ] {
            assert!(
                safe_download_file_name(url, &ModSource::Modrinth).is_err(),
                "{url}"
            );
        }
    }

    #[tokio::test]
    async fn modpack_operations_reject_missing_targets_and_duplicates() {
        let (_dir, mc_folder) = setup_mc_folder();
        let existing = get_modpacks(&mc_folder, false).await.unwrap();

        assert!(delete_modpack(&mc_folder, &existing, "ghost").is_err());
        assert!(apply_modpack(&mc_folder, "ghost").is_err());
        assert!(update_modpack(&mc_folder, &existing, "ghost", None, None, None).is_err());
        assert!(delete_mod(&mc_folder, &existing, "ghost", "mod-1").is_err());

        create_modpack(&mc_folder, &existing, "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert!(create_modpack(&mc_folder, &listed, "alpha", "1.20.1", ModLoader::Fabric).is_err());

        let mod_ = online_mod("mod-1", "https://example.invalid/mod.jar".to_string());
        register_mod(&mc_folder, &listed, mod_.clone(), "alpha")
            .await
            .unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert!(
            register_mod(&mc_folder, &listed, mod_, "alpha")
                .await
                .is_err()
        );
        assert!(delete_mod(&mc_folder, &listed, "alpha", "other-mod").is_err());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn apply_modpack_backs_up_real_mods_directory() {
        let (_dir, mc_folder) = setup_mc_folder();
        std::fs::create_dir_all(modpack_path(&mc_folder, "alpha")).unwrap();
        std::fs::create_dir_all(mc_folder.join("mods")).unwrap();
        std::fs::write(mc_folder.join("mods").join("loose.jar"), "jar").unwrap();

        apply_modpack(&mc_folder, "alpha").unwrap();

        assert!(mc_folder.join("mods").is_symlink());
        let backup = std::fs::read_dir(mc_folder.join("modpacks"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("mods-backup-")
            })
            .expect("backup folder should exist");
        assert!(backup.path().join("loose.jar").exists());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn apply_modpack_switches_between_applied_modpacks() {
        let (_dir, mc_folder) = setup_mc_folder();
        std::fs::create_dir_all(modpack_path(&mc_folder, "alpha")).unwrap();
        std::fs::create_dir_all(modpack_path(&mc_folder, "beta")).unwrap();

        apply_modpack(&mc_folder, "alpha").unwrap();
        apply_modpack(&mc_folder, "beta").unwrap();

        let mods_path = mc_folder.join("mods");
        assert_eq!(
            mods_path.read_link().unwrap(),
            modpack_path(&mc_folder, "beta")
        );
        // Switching must not delete the previously applied modpack's contents.
        assert!(modpack_path(&mc_folder, "alpha").exists());
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn delete_applied_modpack_removes_mods_link() {
        let (_dir, mc_folder) = setup_mc_folder();
        let existing = get_modpacks(&mc_folder, false).await.unwrap();
        create_modpack(&mc_folder, &existing, "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        apply_modpack(&mc_folder, "alpha").unwrap();
        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        assert!(listed[0].is_applied);

        delete_modpack(&mc_folder, &listed, "alpha").unwrap();
        assert!(!mc_folder.join("mods").is_symlink());
        assert!(!modpack_path(&mc_folder, "alpha").exists());
    }

    #[tokio::test]
    async fn an_unreadable_manifest_is_listed_as_broken_and_never_rewritten() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        let folder = modpack_path(&mc_folder, "alpha");
        let manifest = folder.join("modConfigV2.json");
        // A loader label from a newer Quadrant is as unreadable as a typo.
        let damaged = r#"{"name":"alpha","version":"1.21","modLoader":"Paper","mods":[]}"#;
        std::fs::write(&manifest, damaged).unwrap();
        set_modpack_sync_date(&mc_folder, 42, "alpha", Some("modpack-1")).unwrap();

        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        let broken = &listed[0];
        assert!(broken.manifest_error.is_some());
        assert!(broken.mods.is_empty());
        assert_eq!(broken.modpack_id.as_deref(), Some("modpack-1"));

        let refused = |result: Result<()>| {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("can't read"), "{error}");
        };
        refused(
            update_modpack(
                &mc_folder,
                &listed,
                "alpha",
                None,
                Some("1.21".to_string()),
                None,
            )
            .map(drop),
        );
        refused(
            register_mod(
                &mc_folder,
                &listed,
                online_mod("mod", "https://example.invalid/mod.jar".to_string()),
                "alpha",
            )
            .await,
        );
        refused(delete_mod(&mc_folder, &listed, "alpha", "mod").map(drop));
        refused(check_manifest_readable(&mc_folder, "alpha"));
        refused(
            install_modpack(
                &mc_folder,
                InstalledModpack {
                    mod_config_version: "2".to_string(),
                    quadrant_version: String::new(),
                    name: "alpha".to_string(),
                    version: "1.21".to_string(),
                    mod_loader: ModLoader::Fabric,
                    mods: Vec::new(),
                },
                &MemoryStore::default(),
                &CollectingEvents::default(),
            )
            .await,
        );
        assert_eq!(std::fs::read_to_string(&manifest).unwrap(), damaged);
    }

    #[tokio::test]
    async fn get_modpacks_tolerates_damaged_sync_metadata() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        create_modpack(&mc_folder, &[], "beta", "1.20.1", ModLoader::Fabric).unwrap();
        std::fs::write(
            modpack_path(&mc_folder, "alpha").join("quadrantSync.json"),
            "{not json",
        )
        .unwrap();

        let mut listed = get_modpacks(&mc_folder, false).await.unwrap();
        listed.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(
            listed
                .iter()
                .map(|pack| pack.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
        assert_eq!(listed[0].last_synced, 0);
        assert_eq!(listed[0].version, "1.20.1");
    }

    #[test]
    fn export_modpack_skips_directories_and_leaves_no_partial_archive_on_failure() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        let folder = modpack_path(&mc_folder, "alpha");
        std::fs::write(folder.join("sample.jar"), "jar").unwrap();
        std::fs::create_dir(folder.join(".quadrant-install-1")).unwrap();
        std::fs::write(folder.join(".quadrant-install-1").join("partial.jar"), "x").unwrap();

        let destination = mc_folder.join("alpha.quadrantExport.zip");
        export_modpack_to(
            &mc_folder,
            "alpha",
            &destination,
            &CollectingEvents::default(),
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&destination).unwrap()).unwrap();
        let mut names = Vec::new();
        for index in 0..archive.len() {
            names.push(
                archive
                    .by_index(index)
                    .unwrap()
                    .name()
                    .unwrap()
                    .into_owned(),
            );
        }
        assert_eq!(names, ["modConfigV2.json", "sample.jar"]);
        assert!(!mc_folder.join("alpha.quadrantExport.zip.part").exists());

        let missing_parent = mc_folder.join("missing").join("alpha.quadrantExport.zip");
        assert!(
            export_modpack_to(
                &mc_folder,
                "alpha",
                &missing_parent,
                &CollectingEvents::default(),
            )
            .is_err()
        );
        assert!(!missing_parent.exists());
        assert!(!mc_folder.join("missing").exists());
    }

    #[tokio::test]
    async fn delete_mod_removes_manifest_entry_even_when_download_url_fails_policy() {
        let (_dir, mc_folder) = setup_mc_folder();
        create_modpack(&mc_folder, &[], "alpha", "1.20.1", ModLoader::Fabric).unwrap();
        let folder = modpack_path(&mc_folder, "alpha");
        let mut pack: InstalledModpack =
            serde_json::from_slice(&std::fs::read(folder.join("modConfigV2.json")).unwrap())
                .unwrap();
        pack.mods = vec![online_mod(
            "legacy",
            "http://example.invalid/legacy.jar".to_string(),
        )];
        std::fs::write(
            folder.join("modConfigV2.json"),
            serde_json::to_vec(&pack).unwrap(),
        )
        .unwrap();
        std::fs::write(folder.join("legacy.jar"), "bytes").unwrap();

        let listed = get_modpacks(&mc_folder, false).await.unwrap();
        let updated = delete_mod(&mc_folder, &listed, "alpha", "legacy").unwrap();
        assert!(updated.mods.is_empty());
        let saved: InstalledModpack =
            serde_json::from_slice(&std::fs::read(folder.join("modConfigV2.json")).unwrap())
                .unwrap();
        assert!(saved.mods.is_empty());
        // The file stays because its name could not be validated.
        assert!(folder.join("legacy.jar").exists());
    }

    #[test]
    fn write_file_atomically_replaces_contents_and_cleans_temp_files() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("modConfigV2.json");
        write_file_atomically(&path, b"first").unwrap();
        write_file_atomically(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(write_file_atomically(dir.path().join("missing").join("x.json"), b"x").is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
