use crate::mc_mod::{GetModArgs, Mod};

pub use quadrant_core::mc_mod::curseforge::ModFile;

host_commands! {
    get_mod_curseforge(args: GetModArgs) -> Mod;
    get_mod_owners_curseforge(id: String) -> Vec<String>;
    get_mod_deps_curseforge(id: String) -> Vec<Mod>;
}
