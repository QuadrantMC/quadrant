/** @format */

import {
  AccountInfo,
  Article,
  ContentLocation,
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
  PrismInstance,
  PrismSyncPlan,
  SearchCategory,
  SyncedModpack,
  UniversalModFile,
} from "./intefaces";
import {
  createDesktopStore,
  emit,
  invoke,
  openDialog,
  openExternal,
  platform,
  requestCheckForUpdates as requestDesktopCheckForUpdates,
  writeClipboardText,
} from "./desktop";

const configStore = createDesktopStore("config.json");

export async function applyModpack(name: string): Promise<void> {
  try {
    await invoke("frontend_apply_modpack", { name });
  } catch (e) {
    console.error(e);
    const os = await platform();
    if (os === "windows") {
      await openIn(
        "https://github.com/QuadrantMC/quadrant/wiki/Fixing-Windows-issues",
      );
    }
    throw e;
  }
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
    const normalizedQuery = searchQuery.toLowerCase();
    const filtered = res.filter((modpack) => {
      if (
        modpack.name.toLowerCase().includes(normalizedQuery) ||
        modpack.version.toLowerCase().includes(normalizedQuery) ||
        modpack.modLoader.toLowerCase().includes(normalizedQuery)
      ) {
        return modpack;
      }
    });
    return filtered;
  }

  return res;
}

export async function searchMods(args: GlobalSearchModsArgs): Promise<IMod[]> {
  // Provider order reflects the requested sort (relevance / updated / …); the
  // SearchPage merges and applies the final ordering, so don't re-sort here.
  return await invoke<IMod[]>("search_mods", { args });
}

export async function getCategories(
  source: ModSource,
  modType: ModType,
): Promise<SearchCategory[]> {
  return await invoke<SearchCategory[]>("get_categories", {
    source,
    modType: modType.toString(),
  });
}

export async function getMod(
  args: GetModArgs,
  source: ModSource,
): Promise<IMod> {
  if (source === ModSource.CurseForge) {
    const mod = await invoke<IMod>("get_mod_curseforge", { args: args });
    return mod;
  } else if (source === ModSource.Modrinth) {
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
    const mcFolder = await configStore.get<string>("mcFolder");
    if (mcFolder !== undefined) {
      return mcFolder;
    }
  }
  return await invoke<string>("get_minecraft_folder");
}

export async function getDefaultMinecraftFolder(): Promise<string> {
  return await invoke<string>("get_default_minecraft_folder");
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
  } else if (source === ModSource.Modrinth) {
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
  } else if (source === ModSource.Modrinth) {
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

export async function openIn(url: string) {
  // Not logged: the sign-in URL carries the OAuth state and loopback port.
  await openExternal(url);
}

export interface InstallModOptions {
  id: string;
  minecraftVersion: string;
  loader: ModLoader;
  source: ModSource;
  modType: ModType;
  modpack: string;
  fileId?: string;
  /**
   * A `ContentLocation.id` forcing where a resource pack or shader lands.
   * Unset keeps the automatic behaviour, and mods ignore it entirely. The host
   * rejects an empty string, so "automatic" is always the absent value.
   */
  contentLocation?: string;
}

export async function installMod({
  id,
  minecraftVersion,
  loader,
  source,
  modType,
  modpack,
  fileId,
  contentLocation,
}: InstallModOptions) {
  await invoke("install_mod", {
    id: id,
    minecraftVersion: minecraftVersion,
    modLoader: loader,
    source: source,
    modpack: modpack,
    modType: modType,
    fileId: fileId,
    contentLocation: contentLocation,
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
  const response = await invoke<{ code: string | number }>("share_modpack", {
    modpackName: modpack,
  });
  await writeClipboardText(
    `https://usequadrant.dev/modpack/${String(response.code)}`,
  );
}

export async function shareModpackRaw(modpack: InstalledModpack) {
  const response = await invoke<{ code: string | number }>(
    "share_modpack_raw",
    {
      modConfig: modpack,
    },
  );
  await writeClipboardText(
    `https://usequadrant.dev/modpack/${String(response.code)}`,
  );
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
  await requestDesktopCheckForUpdates();
};

export const getNews = async () => {
  const res = await invoke<Article[]>("get_news");
  return res;
};

export const exportModpack = async (name: string) => {
  const destination = await openDialog({
    mode: "save",
    multiple: false,
    title: "Export Modpack",
    defaultPath: `${name}.quadrantExport.zip`,
  });
  if (typeof destination !== "string" || destination.length === 0) {
    return;
  }
  // An export can finish with its first event, so the shell can't tell a new
  // export from the previous one's second completion without being told.
  await emit("quadrantExportStarted");
  await invoke("export_modpack_to", {
    modpack: name,
    destination,
  });
};

export const identifyUnknownMods = async (
  modpack: string,
): Promise<IdentifiedMod[]> => {
  return await invoke("identify_modpack", { modpack: modpack });
};

export const registerMod = async (mod: LocalMod, modpack: string) => {
  await invoke("register_mod", { mod: mod, modpack: modpack });
};

/** Empty unless experimental features are on and Prism Launcher is installed. */
export async function getPrismInstances(): Promise<PrismInstance[]> {
  return await invoke<PrismInstance[]>("get_prism_instances");
}

export async function applyModpackToPrismInstance(
  name: string,
  instanceId: string,
) {
  await invoke("apply_modpack_to_prism_instance", { name, instanceId });
}

export async function detachPrismInstance(instanceId: string) {
  await invoke("detach_prism_instance", { instanceId });
}

/**
 * What applying `name` would rewrite on each instance, `null` meaning the
 * instance keeps what it has. Empty unless experimental features are on.
 */
export async function getPrismSyncPlans(
  name: string,
): Promise<PrismSyncPlan[]> {
  return await invoke<PrismSyncPlan[]>("get_prism_sync_plans", { name });
}

/**
 * The Minecraft folder first, then one entry per Prism instance the host found.
 * Pass `false` to name the locations only, leaving their pack lists empty.
 */
export async function getInstalledContent(
  includeFiles = true,
): Promise<ContentLocation[]> {
  return await invoke<ContentLocation[]>("get_installed_content", {
    includeFiles,
  });
}

export async function openContentFolder(locationId: string, modType: ModType) {
  await invoke("open_content_folder", { locationId, modType });
}

/** Copies packs between two locations, overwriting, and answers how many moved. */
export async function copyContent(
  fromLocation: string,
  toLocation: string,
  modType: ModType,
  fileNames: string[],
): Promise<number> {
  return await invoke<number>("copy_content", {
    fromLocation,
    toLocation,
    modType,
    fileNames,
  });
}

/** Permanently removes packs from a location, answering how many went. */
export async function deleteContent(
  location: string,
  modType: ModType,
  fileNames: string[],
): Promise<number> {
  return await invoke<number>("delete_content", {
    location,
    modType,
    fileNames,
  });
}
