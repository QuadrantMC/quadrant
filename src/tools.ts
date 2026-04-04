import { invoke } from "@tauri-apps/api/core";
import {
  AccountInfo,
  Article,
  GetModArgs,
  GlobalSearchModsArgs,
  IdentifiedMod,
  IMod,
  InstalledModpack,
  LocalMod,
  LocalModpack,
  MinecraftVersion,
  ModLoader,
  ModSource,
  ModType,
  SyncedModpack,
  UniversalModFile,
} from "./intefaces";
import { platform } from "@tauri-apps/plugin-os";

import { LazyStore } from "@tauri-apps/plugin-store";

export async function applyModpack(name: string): Promise<void> {
  try {
    await invoke("frontend_apply_modpack", { name });
  } catch (e) {
    console.error(e);
    const os = await platform();
    if (os === "windows") {
      await openIn(
        "https://github.com/quadrantmc/quadrant/wiki/Fixing-Windows-issues",
      );
    }
  }
  return;
}

export async function getModpacks(
  hideFree = true,
  searchQuery: string | undefined = undefined,
): Promise<LocalModpack[]> {
  const res = await invoke<[LocalModpack]>("get_modpacks", { hideFree });
  res.sort((a, b) => b.lastSynced - a.lastSynced);
  const hasApplied = res.filter((modpack) => modpack.isApplied).length > 0;
  if (hasApplied) {
    res.sort((a, b) =>
      a.isApplied === b.isApplied ? 0 : a.isApplied ? -1 : 1,
    );
  }
  if (searchQuery) {
    const filtered = res.filter((modpack) => {
      if (
        modpack.name.toLowerCase().includes(searchQuery) ||
        modpack.version.toLowerCase().includes(searchQuery) ||
        modpack.modLoader.toLowerCase().includes(searchQuery)
      ) {
        return modpack;
      }
    });
    return filtered;
  }

  return res;
}

export async function searchMods(args: GlobalSearchModsArgs): Promise<IMod[]> {
  const res = await invoke<IMod[]>("search_mods", { args });
  res.sort((a, b) => b.downloadCount - a.downloadCount);

  return res;
}

export async function getMod(
  args: GetModArgs,
  source: ModSource,
): Promise<IMod> {
  if (source === ModSource.CurseForge) {
    const mod = await invoke<IMod>("get_mod_curseforge", { args: args });
    return mod;
  } else if (ModSource.Modrinth) {
    const mod = await invoke<IMod>("get_mod_modrinth", { args: args });
    return mod;
  } else {
    throw Error("Can't get an unknown mod");
  }
}

export async function getVersions(): Promise<MinecraftVersion[]> {
  const res = await invoke<MinecraftVersion[]>("get_versions");
  return res;
}

export async function getUserURL(
  username: string,
  source: ModSource,
): Promise<string> {
  const res = await invoke<string>("get_user_url", {
    username: username,
    source: source,
  });
  return res;
}

export async function getMinecraftFolder(
  onlyRealFolder: boolean = false,
): Promise<string> {
  if (!onlyRealFolder) {
    const store = await new LazyStore("config.json");

    const mcFolder = await store?.get<string>("mcFolder");
    if (mcFolder !== undefined) {
      return mcFolder;
    }
  }
  return await invoke<string>("get_minecraft_folder");
}

export async function initConfig() {
  return await invoke("init_config");
}

export async function deleteMod(modpackName: string, modId: string) {
  console.log("Removing mod from: " + modpackName + "\nMod: " + modId);
  await invoke("delete_mod", { modpackName: modpackName, modId: modId });
  return;
}

export async function getModOwners(
  source: ModSource,
  modId: string,
): Promise<string[]> {
  if (source === ModSource.CurseForge) {
    const owners = await invoke<string[]>("get_mod_owners_curseforge", {
      id: modId,
    });
    return owners;
  } else if (ModSource.Modrinth) {
    const owners = await invoke<string[]>("get_mod_owners_modrinth", {
      id: modId,
    });
    return owners;
  }
  return [];
}

export async function getModDependencies(
  source: ModSource,
  modId: string,
): Promise<IMod[]> {
  if (source === ModSource.CurseForge) {
    const deps = await invoke<IMod[]>("get_mod_deps_curseforge", {
      id: modId,
    });
    return deps;
  } else if (ModSource.Modrinth) {
    const deps = await invoke<IMod[]>("get_mod_deps_modrinth", {
      id: modId,
    });
    return deps;
  }
  return [];
}

export async function updateModpack(
  originalModpack: string,
  newDetails: LocalModpack,
) {
  await invoke("update_modpack", {
    modpackSource: originalModpack,
    name: newDetails.name,
    version: newDetails.version,
    modLoader: newDetails.modLoader,
  });
}

export async function createModpack(
  name: string,
  version: string,
  modLoader: ModLoader,
) {
  await invoke("create_modpack", {
    name: name,
    version: version,
    modLoader: modLoader,
  });
}

export async function deleteModpack(name: string) {
  await invoke("delete_modpack", {
    name: name,
  });
}

export function shuffle(array: any[]) {
  let currentIndex = array.length;

  // While there remain elements to shuffle...
  while (currentIndex != 0) {
    // Pick a remaining element...
    const randomIndex = Math.floor(Math.random() * currentIndex);
    currentIndex--;

    // And swap it with the current element.
    [array[currentIndex], array[randomIndex]] = [
      array[randomIndex],
      array[currentIndex],
    ];
  }
}

export async function openIn(url: string) {
  console.log("Opening link: " + url);
  await invoke("open_link", {
    url: url,
  });
}

export async function installMod(
  id: string,
  minecraft_version: string,
  loader: ModLoader,
  source: ModSource,
  modType: ModType,
  modpack: string,
  fileId?: string,
) {
  await invoke("install_mod", {
    id: id,
    minecraftVersion: minecraft_version,
    modLoader: loader,
    source: source,
    modpack: modpack,
    modType: modType,
    fileId: fileId,
  });
}

export async function openModpacksFolder() {
  await invoke("open_modpacks_folder");
}

export async function getAccountInfo(): Promise<AccountInfo> {
  return await invoke<AccountInfo>("get_account_info");
}

export async function clearAccountToken() {
  return await invoke("clear_account_token");
}

export async function getModUpdate(
  mod: IMod,
  mcVersion: string,
  loader: ModLoader,
  modpackName: string,
): Promise<IMod | null> {
  const res = await invoke<IMod | null>("check_mod_updates", {
    modToUpdate: mod,
    minecraftVersion: mcVersion,
    modLoader: loader,
    modpackName: modpackName,
  });
  return res;
}

export async function installRemoteFile(
  file: UniversalModFile,
  modType: ModType,
  modpack: string | undefined,
  source: ModSource,
  id: string,
) {
  await invoke("install_remote_file", {
    file: file,
    modType: modType,
    modpack: modpack,
    source: source,
    id: id,
  });
}

export async function shareModpack(modpack: string) {
  await invoke("share_modpack", { modpackName: modpack });
}

export async function shareModpackRaw(modpack: InstalledModpack) {
  await invoke("share_modpack_raw", { modConfig: modpack });
}

export async function getQuadrantShareModpack(code: string) {
  const res = await invoke<InstalledModpack>("get_quadrant_share_modpack", {
    code: code,
  });
  return res;
}

export async function installModpack(modpack: InstalledModpack) {
  await invoke("install_modpack", { modConfig: modpack });
  return;
}

export async function getSyncedModpacks(
  showOwners: boolean,
  modpackId?: string,
) {
  const res = await invoke<SyncedModpack[]>("get_synced_modpacks", {
    showOwners,
    modpackId,
  });
  return res;
}

export async function kickMember(modpackId: string, username: string) {
  await invoke("kick_member", { modpackId: modpackId, username: username });
}
export async function inviteMember(
  modpackId: string,
  username: string,
  admin: boolean,
) {
  await invoke("invite_member", {
    modpackId: modpackId,
    username: username,
    admin: admin,
  });
}
export async function syncModpack(modpack: LocalModpack, overwrite: boolean) {
  await invoke("sync_modpack", {
    modpack: modpack,
    overwrite: overwrite,
  });
}

export async function readNotification(notificationId: string) {
  await invoke("read_notification", {
    notificationId: notificationId,
  });
}

export async function answerInvite(
  modpackId: string,
  notificationId: string,
  answer: boolean,
) {
  await invoke("answer_invite", {
    modpackId: modpackId,
    notificationId: notificationId,
    answer: answer,
  });
}

export const requestCheckForUpdates = async () => {
  await invoke("request_check_for_updates");
};

export const getNews = async () => {
  const res = await invoke<Article[]>("get_news");
  return res;
};

export const exportModpack = async (name: string) => {
  await invoke("export_modpack", { modpack: name });
};

export const identifyUnknownMods = async (
  modpack: string,
): Promise<IdentifiedMod[]> => {
  return await invoke("identify_modpack", { modpack: modpack });
};

export const registerMod = async (mod: LocalMod, modpack: string) => {
  await invoke("register_mod", { mod: mod, modpack: modpack });
};
