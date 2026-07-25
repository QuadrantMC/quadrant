use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

pub use quadrant_core::account::quadrant_share::{
    QuadrantShareResponse, QuadrantShareSubmission, QuadrantShareSubmissionResponse,
};

#[tauri::command]
pub async fn share_modpack(
    modpack_name: String,
    app: AppHandle,
) -> Result<QuadrantShareSubmissionResponse, tauri::Error> {
    app.state::<QuadrantHost>()
        .share_modpack(modpack_name)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn share_modpack_raw(
    mod_config: crate::modpacks::general::InstalledModpack,
    app: AppHandle,
) -> Result<QuadrantShareSubmissionResponse, tauri::Error> {
    app.state::<QuadrantHost>()
        .share_modpack_raw(mod_config)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn get_quadrant_share_modpack(
    code: String,
    app: AppHandle,
) -> Result<crate::modpacks::general::InstalledModpack, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_quadrant_share_modpack(code)
        .await
        .map_err(tauri::Error::from)
}
