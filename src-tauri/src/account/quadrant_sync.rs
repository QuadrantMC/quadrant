use crate::modpacks::general::LocalModpack;

pub use quadrant_core::account::quadrant_sync::{ModpackOwner, SyncedModpack};

host_commands! {
    get_synced_modpacks(show_owners: bool, modpack_id: Option<String>) -> Vec<SyncedModpack>;
    kick_member(modpack_id: String, username: String) -> ();
    invite_member(modpack_id: String, username: String, admin: bool) -> ();
    delete_synced_modpack(modpack_id: String) -> ();
    sync_modpack(modpack: LocalModpack, overwrite: bool) -> ();
    answer_invite(modpack_id: String, notification_id: String, answer: bool) -> ();
}
