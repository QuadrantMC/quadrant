use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Progress payload used by mod download and install events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModProgressPayload {
    pub mod_id: String,
    pub progress: i32,
}

/// Typed backend events that hosts can translate into frontend-specific events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackendEvent {
    /// Reports progress for a mod download.
    ModDownloadProgress(ModProgressPayload),
    /// Reports progress for placing or enabling a mod locally.
    ModInstallProgress(ModProgressPayload),
    /// Reports progress for a modpack content download.
    ModpackDownloadProgress(f64),
    /// Reports progress while exporting a modpack archive.
    QuadrantExportProgress(f64),
    /// Emits the share submission payload used by the existing frontend flow.
    QuadrantShareSubmission(Value),
    /// Signals that the host should refresh notifications.
    RefreshNotifications(Value),
    /// Signals that the host should re-check account token state.
    RecheckAccountToken,
    /// Signals that a settings key was changed by the backend (for example by
    /// cloud settings sync) so frontend subscribers can re-read it.
    ConfigChanged(String),
    /// An automatic Quadrant Sync update of a local pack needs the user. It was
    /// held back (`applied` false) because it adds or changes Online mods, or
    /// applied with new or changed files that carry no hash. Both lists hold
    /// mod names.
    ModpackSyncWarning {
        modpack_id: String,
        name: String,
        applied: bool,
        online_mods: Vec<String>,
        unverified_mods: Vec<String>,
    },
}
