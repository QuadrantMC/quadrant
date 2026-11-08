use crate::modpacks::general::InstalledModpack;

pub use quadrant_core::account::quadrant_share::{
    QuadrantShareResponse, QuadrantShareSubmission, QuadrantShareSubmissionResponse,
};

host_commands! {
    share_modpack(modpack_name: String) -> QuadrantShareSubmissionResponse;
    share_modpack_raw(mod_config: InstalledModpack) -> QuadrantShareSubmissionResponse;
    get_quadrant_share_modpack(code: String) -> InstalledModpack;
}
