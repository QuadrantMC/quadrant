use quadrant_host::QuadrantHost;
use tauri::{AppHandle, Manager};

pub mod id;
pub mod quadrant_settings_sync;
pub mod quadrant_share;
pub mod quadrant_sync;

#[tauri::command]
pub async fn clear_account_token(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .clear_account_token()
        .await
        .map_err(crate::command_error)
}
