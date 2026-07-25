use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

#[tauri::command]
pub async fn get_quadrant_settings(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .get_quadrant_settings()
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn submit_quadrant_settings(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .submit_quadrant_settings()
        .await
        .map_err(tauri::Error::from)
}
