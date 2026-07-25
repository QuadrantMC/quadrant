use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

pub use quadrant_core::account::quadrant_sync::{ModpackOwner, SyncedModpack};

#[tauri::command]
pub async fn get_synced_modpacks(
    show_owners: bool,
    modpack_id: Option<String>,
    app: AppHandle,
) -> Result<Vec<SyncedModpack>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_synced_modpacks(show_owners, modpack_id)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn kick_member(
    modpack_id: String,
    username: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .kick_member(modpack_id, username)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn invite_member(
    modpack_id: String,
    username: String,
    admin: bool,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .invite_member(modpack_id, username, admin)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn delete_synced_modpack(modpack_id: String, app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .delete_synced_modpack(modpack_id)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn sync_modpack(
    modpack: crate::modpacks::general::LocalModpack,
    overwrite: bool,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .sync_modpack(modpack, overwrite)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn answer_invite(
    modpack_id: String,
    notification_id: String,
    answer: bool,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .answer_invite(modpack_id, notification_id, answer)
        .await
        .map_err(tauri::Error::from)
}
