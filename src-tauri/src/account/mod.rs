use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

pub mod id;
pub mod quadrant_settings_sync;
pub mod quadrant_share;
pub mod quadrant_sync;

#[tauri::command]
pub fn set_secret(key: String, value: String, app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .set_secret(key, value)
        .map_err(tauri::Error::from)
}

pub fn get_account_token() -> Result<String, anyhow::Error> {
    Err(anyhow::anyhow!(
        "account token access moved to quadrant-host"
    ))
}

pub fn get_refresh_token() -> Result<String, anyhow::Error> {
    Err(anyhow::anyhow!(
        "refresh token access moved to quadrant-host"
    ))
}

#[tauri::command]
pub fn clear_account_token(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .clear_account_token()
        .map_err(tauri::Error::from)
}
