use std::path::{Path, PathBuf};

use quadrant_host::QuadrantHost;
use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;
use tauri_plugin_fs::FsExt;

pub use quadrant_core::config::{get_config_dir, get_mc_folder};

const MINECRAFT_FOLDER_KEY: &str = "mcFolder";

/// The capability's watch scope only names the default Minecraft folders, and
/// the user can pick any folder. The modpack watchers watch the configured
/// one, so it joins the fs scope at startup and whenever the setting changes.
pub fn allow_watching_minecraft_folder(app: &AppHandle) {
    let folder = match app.state::<QuadrantHost>().get_minecraft_folder() {
        Ok(folder) => folder,
        Err(error) => {
            log::warn!("No Minecraft folder to watch: {error}");
            return;
        }
    };
    let scope = app.fs_scope();
    for root in watch_roots(&folder) {
        if let Err(error) = scope.allow_directory(&root, true) {
            log::warn!("Couldn't allow watching {}: {error}", root.display());
        }
    }
}

/// The folder as configured and, when a link leads elsewhere, where it
/// really is: the fs plugin checks a watched path once it is resolved.
fn watch_roots(folder: &Path) -> Vec<PathBuf> {
    let mut roots = vec![folder.to_path_buf()];
    if let Ok(real) = folder.canonicalize()
        && real != folder
    {
        roots.push(real);
    }
    roots
}

fn config_changed(app: &AppHandle, key: String) -> Result<(), tauri::Error> {
    if key == MINECRAFT_FOLDER_KEY {
        allow_watching_minecraft_folder(app);
    }
    app.emit("configChanged", key)
}

#[tauri::command]
pub fn get_minecraft_folder(app: AppHandle) -> Result<String, tauri::Error> {
    let path = app
        .state::<QuadrantHost>()
        .get_minecraft_folder()
        .map_err(crate::command_error)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn get_default_minecraft_folder() -> Result<String, tauri::Error> {
    let path = get_mc_folder()
        .map_err(crate::command_error)?
        .ok_or_else(|| {
            tauri::Error::from(anyhow::Error::from(
                quadrant_core::error::ErrorCode::NoMinecraftFolder,
            ))
        })?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn init_config(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .init_config()
        .map_err(crate::command_error)
}

#[tauri::command]
pub fn get_config_value(
    app: AppHandle,
    key: String,
) -> Result<Option<serde_json::Value>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_config_value(&key)
        .map_err(crate::command_error)
}

#[tauri::command]
pub fn set_config_value(
    app: AppHandle,
    key: String,
    value: serde_json::Value,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .set_config_value(&key, value)
        .map_err(crate::command_error)?;
    config_changed(&app, key)
}

#[tauri::command]
pub fn remove_config_value(app: AppHandle, key: String) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .remove_config_value(&key)
        .map_err(crate::command_error)?;
    config_changed(&app, key)
}

#[cfg(test)]
mod tests {
    use super::watch_roots;

    #[test]
    fn watch_roots_add_the_resolved_folder_when_it_differs() {
        let dir = std::env::temp_dir();
        let roots = watch_roots(&dir);
        assert_eq!(roots[0], dir);
        assert!(roots.contains(&dir.canonicalize().unwrap()));

        let missing = dir.join(format!("quadrant-no-such-folder-{}", std::process::id()));
        assert_eq!(watch_roots(&missing), [missing.clone()]);
    }
}
