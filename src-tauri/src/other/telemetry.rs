use quadrant_host::QuadrantHost;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn send_telemetry(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .send_telemetry()
        .await
        .map_err(crate::command_error)
}

#[tauri::command]
pub async fn remove_telemetry(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .remove_telemetry()
        .await
        .map_err(crate::command_error)
}
