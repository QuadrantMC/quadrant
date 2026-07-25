use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

pub use quadrant_core::account::id::{
    AccountInfo, Notification, NotificationCursor, NotificationWsFrame, OAuth2Response,
};

#[tauri::command]
pub async fn get_account_info(app: AppHandle) -> Result<AccountInfo, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_account_info()
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub async fn oauth2_login(
    code: String,
    redirect_uri: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .oauth2_login(code, redirect_uri)
        .await
        .map_err(tauri::Error::from)
}

#[tauri::command]
pub fn oauth2_client_id(app: AppHandle) -> String {
    app.state::<QuadrantHost>().oauth2_client_id()
}

#[tauri::command]
pub async fn read_notification(
    notification_id: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .read_notification(notification_id)
        .await
        .map_err(tauri::Error::from)
}
