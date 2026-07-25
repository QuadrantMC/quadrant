use crate::AppHandle;
use quadrant_host::QuadrantHost;
use tauri::Manager;

pub use quadrant_core::models::Article;

#[tauri::command]
pub async fn get_news(app: AppHandle) -> Result<Vec<Article>, tauri::Error> {
    app.state::<QuadrantHost>()
        .get_news()
        .await
        .map_err(tauri::Error::from)
}
