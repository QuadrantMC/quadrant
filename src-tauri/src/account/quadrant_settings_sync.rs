use quadrant_host::QuadrantHost;
use tauri::{AppHandle, Manager};

host_commands! {
    submit_quadrant_settings() -> ();
}

/// The pull's outcome is for the other frontends; the webview only needs to
/// know it worked.
#[tauri::command]
pub async fn get_quadrant_settings(app: AppHandle) -> Result<(), tauri::Error> {
    app.state::<QuadrantHost>()
        .get_quadrant_settings()
        .await
        .map(|_| ())
        .map_err(crate::command_error)
}
