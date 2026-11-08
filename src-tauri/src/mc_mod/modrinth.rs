use crate::mc_mod::{GetModArgs, Mod};

pub use quadrant_core::mc_mod::modrinth::{ModrinthFile, ModrinthHash};

host_commands! {
    get_mod_modrinth(args: GetModArgs) -> Mod;
    get_mod_owners_modrinth(id: String) -> Vec<String>;
    get_mod_deps_modrinth(id: String) -> Vec<Mod>;
}
