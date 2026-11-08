/** @format */

import { createContext } from "react";

export interface IContentContext {
  changeContent: (component: Page) => void;
  back: () => void;
  setSnackbar: (newSnackBarState: SnackbarState) => void;
  setSnackbarNoState(newSnackBarState: SnackbarState): void;
}
export interface ISyncContext {
  refreshSyncedModpacks: () => void;
}
export const SyncContext = createContext<ISyncContext>({
  refreshSyncedModpacks: () => {},
});
export interface IModpackViewContext {
  removeMod: (id: string) => void;
}

export const ModpackViewContext = createContext<IModpackViewContext>({
  removeMod: (_: string) => {},
});

// Interface for grouped snackbar history items
export interface SnackbarHistoryItem extends SnackbarState {
  count: number;
  id: string; // unique key for rendering
}

export const ContentContext = createContext<IContentContext>({
  back: async () => {},
  changeContent: () => {},
  setSnackbar: (_: SnackbarState) => {},
  setSnackbarNoState: (_: SnackbarState) => {},
});

export interface LocalModpack {
  name: string;
  version: string;
  modLoader: ModLoader;
  isApplied: boolean;
  lastSynced: number;
  modpackId?: string | null;
  mods: LocalMod[];
  unknownMods: boolean;
}

/** A Prism Launcher instance Quadrant can point at one of its modpacks. */
export interface PrismInstance {
  id: string;
  name: string;
  minecraftVersion: string | null;
  modLoader: ModLoader;
  modLoaderVersion: string | null;
  /** Name of the Quadrant modpack this instance is currently linked to. */
  appliedModpack: string | null;
}

/** One entry of a resource pack or shader pack folder. */
export interface ContentFile {
  fileName: string;
  /** Bytes. Always 0 for a directory. */
  size: number;
  /** Milliseconds since the epoch, or 0 when the host couldn't read it. */
  modified: number;
  isDirectory: boolean;
}

/** One kind of pack a location holds, with the files it currently has. */
export interface ContentSection {
  modType: ModType;
  /** Empty when the listing was asked for the locations alone. */
  files: ContentFile[];
}

/** A folder Quadrant reads packs from: the Minecraft one, or a Prism instance. */
export interface ContentLocation {
  /** `"minecraft"`, or `"prism:<instanceId>"`. */
  id: string;
  kind: "minecraft" | "prism";
  /** Empty for the Minecraft folder, which has no name of its own. */
  name: string;
  path: string;
  /** Resource packs first, then shaders. */
  sections: ContentSection[];
}

/** What applying a modpack would rewrite on one instance; `null` stays put. */
export interface PrismSyncPlan {
  instanceId: string;
  minecraftVersion: string | null;
  modLoader: ModLoader | null;
}

export interface ModpackOwner {
  username: string;
  admin: boolean;
}

export interface SyncedModpack {
  name: string;
  modpack_id: string;
  minecraft_version: string;
  mod_loader: ModLoader;
  mods: string;
  owners: ModpackOwner[];
  last_synced: number;
}

export interface InstalledModpack {
  name: string;
  version: string;
  modLoader: ModLoader;
  mods: LocalMod[];
}

export enum ModSource {
  CurseForge = "ModSource.curseForge",
  Modrinth = "ModSource.modRinth",
  Online = "ModSource.online",
}

export interface LocalMod {
  id: string;
  downloadUrl: string;
  source: ModSource;
  slug?: string;
}

export interface Page {
  title: string;
  content: React.ReactNode;
  icon: React.ReactNode;
  name: string;
  style: string;
  main: boolean;
  /** The page animates its own enter and leave, so App skips the view transition. */
  ownTransition?: boolean;
}

export enum ModType {
  Mod = "Mod",
  ResourcePack = "ResourcePack",
  // The backend's spelling, so a shader it returns compares equal to this.
  ShaderPack = "ShaderPack",
  Modpack = "Modpack",
  DataPack = "DataPack",
  Unknown = "Unknown",
}

export interface IMod {
  name: string;
  id: string;
  downloadCount: number;
  version: string;
  dateModified: string;
  modType: ModType;
  source: ModSource;
  slug: string;
  thumbnailUrls: string[];
  url: string;
  description: string;
  license: string;
  modIconUrl: string;
  downloadable: boolean;
  showPreviousVersion: boolean;
  newVersion: UniversalModFile | null;
  deleteable: boolean;
  autoinstallable: boolean;
  selectable: boolean;
  modpack: string | null;
  selectUrl: string | null;
}

export interface UniversalModFile {
  id: string;
  fileName: string;
  downloadUrl: string;
  sha1: string;
  size: number;
}

export interface MinecraftVersion {
  version: string;
  versionType: string;
}

export interface GlobalSearchModsArgs {
  source: ModSource;
  query: string;
  modType: string;
  /** Treat results as auto-installable into the current modpack context. */
  filterOn: boolean;
  /** Explicit Minecraft version filter. Empty or "any" disables it. */
  gameVersion: string;
  /** Explicit loader filter. Empty / Unknown disables it. */
  modLoader: string;
  /** Provider-specific category identifiers (CF numeric ids / Modrinth slugs). */
  categories: string[];
  /** Restrict to open-source projects (Modrinth only). */
  openSource: boolean;
  /** Sort key: relevance | downloads | name | updated. */
  sortBy: string;
  /** Zero-based offset into the provider's result set, for "search further"
   *  pagination. Omitted or 0 returns the first page. */
  offset?: number;
}

/** A selectable search facet returned by `getCategories`. */
export interface SearchCategory {
  /** Provider-specific id passed back in {@link GlobalSearchModsArgs.categories}. */
  id: string;
  /** Human-readable display label. */
  name: string;
  /** Grouping header: categories | resolutions | features | performance impact. */
  header: string;
  /** Provider this facet belongs to. */
  source: ModSource;
}

export interface IdentifiedMod {
  installed_mod: LocalMod;
  file_name: string;
}

export interface FetchedIdentifiedMod {
  proposedMods: IMod[];
  fileName: string;
}

export interface GetModArgs {
  id: string;
  downloadable: boolean;
  showPreviousVersion: boolean;
  deletable: boolean;
  versionTarget: string;
  modLoader: ModLoader;
  modpack: string;
  selectable: boolean;
  selectUrl: string | null;
}

export enum ModLoader {
  Forge = "Forge",
  Fabric = "Fabric",
  NeoForge = "NeoForge",
  Babric = "Babric",
  BtaBabric = "BTA (Babric)",
  JavaAgent = "Java Agent",
  LegacyFabric = "Legacy Fabric",
  LiteLoader = "LiteLoader",
  RisugamisModLoader = "Risugami's ModLoader",
  NilLoader = "NilLoader",
  Ornithe = "Ornithe",
  Rift = "Rift",
  Quilt = "Quilt",
  Unknown = "Unknown",
}

export interface ModProgress {
  modId: string;
  progress: number;
}
export interface SnackbarState {
  message: React.ReactNode;
  className: string;
  timeout: number;
}

export interface AccountInfo {
  id: string;
  name: string;
  email: string;
  quadrant_sync_limit: number;
  quadrant_share_limit: number;
  login: string;
  notifications: AccountNotification[];
}

export interface AccountNotification {
  notification_id: string;
  user_id: string;
  notification_type?: string | null;
  resource_id?: string | null;
  message: string;
  created_at: string;
  created_at_unix: number;
  read: boolean;
}

export interface Article {
  title: string;
  link: string;
  summary: string;
  date: Date;
  guid: string;
  new: boolean;
}
