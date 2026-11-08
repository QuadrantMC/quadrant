use quadrant_host::QuadrantHost;
use tauri::{AppHandle, Manager};

pub use quadrant_core::account::id::{
    AccountInfo, Notification, NotificationCursor, NotificationWsFrame, OAuth2Response,
};

host_commands! {
    get_account_info() -> AccountInfo;
    oauth2_login(code: String, redirect_uri: String, code_verifier: Option<String>) -> ();
    read_notification(notification_id: String) -> ();
}

#[tauri::command]
pub fn oauth2_client_id(app: AppHandle) -> String {
    app.state::<QuadrantHost>().oauth2_client_id()
}
