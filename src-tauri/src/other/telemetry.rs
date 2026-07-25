use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

pub use quadrant_core::telemetry::AppInfo;

pub async fn get_telemetry_info(app: AppHandle) -> AppInfo {
    app.state::<QuadrantHost>()
        .get_telemetry_info()
        .await
        .expect("failed to gather telemetry info")
}

#[tauri::command]
pub async fn send_telemetry(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .send_telemetry()
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn remove_telemetry(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .remove_telemetry()
        .await
        .map_err(tauri::Error::from)
}
