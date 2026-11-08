use std::path::{Path, PathBuf};

use quadrant_core::{Result, ports::Shell};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[derive(Clone)]
pub struct TauriShell {
    app: AppHandle,
}

impl TauriShell {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Shell for TauriShell {
    fn open_url(&self, url: &str) -> Result<()> {
        open::that_detached(url).map_err(anyhow::Error::from)
    }

    fn open_path(&self, path: &Path) -> Result<()> {
        open::that_detached(path).map_err(anyhow::Error::from)
    }

    fn choose_export_path(&self, suggested_name: &str) -> Result<Option<PathBuf>> {
        let selected = self
            .app
            .dialog()
            .file()
            .add_filter("Quadrant Export", &["quadrantExport.zip"])
            .set_file_name(suggested_name)
            .blocking_save_file();
        Ok(selected.and_then(|file| file.into_path().ok()))
    }
}
