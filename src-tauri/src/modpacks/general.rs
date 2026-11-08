use std::path::PathBuf;

use quadrant_core::ports::Shell;
use quadrant_host::QuadrantHost;
use tauri::{AppHandle, Manager};

pub use quadrant_core::models::{InstalledModpack, LocalModpack, ModLoader, SyncInfo};

use crate::tauri_adapter::TauriShell;

#[tauri::command]
pub async fn get_modpacks(hide_free: bool, app: AppHandle) -> Vec<LocalModpack> {
    app.state::<QuadrantHost>()
        .get_modpacks(hide_free)
        .await
        .unwrap_or_else(|error| {
            // The renderer treats this as "no packs"; leave a trace so an empty
            // list caused by an unreadable modpacks folder is diagnosable.
            log::error!("Failed to list modpacks: {error}");
            Vec::new()
        })
}

#[tauri::command]
pub fn frontend_apply_modpack(name: String, app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .frontend_apply_modpack(name)
        .map_err(crate::command_error)
}

host_commands! {
    install_modpack(mod_config: InstalledModpack) -> ();
}

#[tauri::command]
pub async fn set_modpack_sync_date(
    time: u64,
    modpack: String,
    modpack_id: Option<String>,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .set_modpack_sync_date(time, modpack, modpack_id)
        .map_err(crate::command_error)
}

#[tauri::command]
pub async fn export_modpack(modpack: String, app: AppHandle) -> Result<(), tauri::Error> {
    let shell = TauriShell::new(app.clone());
    let destination = shell
        .choose_export_path(&format!("{modpack}.quadrantExport.zip"))
        .map_err(crate::command_error)?;
    let Some(destination) = destination else {
        return Ok(());
    };

    app.state::<QuadrantHost>()
        .export_modpack_to(modpack, destination)
        .await
        .map_err(crate::command_error)
}

#[tauri::command]
pub async fn export_modpack_to(
    modpack: String,
    destination: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    // The save dialog runs in the webview, so this path comes from there.
    // Only an archive may be written, never any file the renderer names.
    let Some(destination) = export_archive_path(PathBuf::from(destination)) else {
        return Err(crate::command_error(anyhow::anyhow!(
            "an export must be saved as a .zip file"
        )));
    };
    app.state::<QuadrantHost>()
        .export_modpack_to(modpack, destination)
        .await
        .map_err(crate::command_error)
}

/// `path` if it names a `.zip` file, else `path` with `.zip` added, the way
/// the save dialog would have if it had a filter. `None` when `path` names no
/// file at all.
fn export_archive_path(path: PathBuf) -> Option<PathBuf> {
    let is_archive = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        && path.file_stem().is_some_and(|stem| !stem.is_empty());
    if is_archive {
        return Some(path);
    }
    path.file_name()?;
    let mut archive = path.into_os_string();
    archive.push(".zip");
    Some(archive.into())
}

#[cfg(test)]
mod tests {
    use super::export_archive_path;
    use std::path::PathBuf;

    #[test]
    fn exports_are_written_only_to_zip_files() {
        for (chosen, written) in [
            (
                "C:\\Users\\a\\Pack.quadrantExport.zip",
                "C:\\Users\\a\\Pack.quadrantExport.zip",
            ),
            ("/home/a/pack.ZIP", "/home/a/pack.ZIP"),
            ("/home/a/mypack", "/home/a/mypack.zip"),
            ("C:\\Users\\a\\.bashrc", "C:\\Users\\a\\.bashrc.zip"),
            ("/home/a/.profile", "/home/a/.profile.zip"),
            ("/home/a/pack.zip.sh", "/home/a/pack.zip.sh.zip"),
            ("/home/a/.zip", "/home/a/.zip.zip"),
        ] {
            assert_eq!(
                export_archive_path(PathBuf::from(chosen)),
                Some(PathBuf::from(written)),
                "{chosen}"
            );
        }
        for refused in ["", "/", "/home/a/.."] {
            assert_eq!(
                export_archive_path(PathBuf::from(refused)),
                None,
                "{refused}"
            );
        }
    }
}
