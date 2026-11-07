/**
 * eslint-disable @typescript-eslint/no-empty-object-type
 *
 * @format
 */

import type { AccountNotification, Article, ModProgress } from "../intefaces";

export const HOST_COMMANDS = [
  "get_modpacks",
  "frontend_apply_modpack",
  "delete_mod",
  "update_modpack",
  "create_modpack",
  "delete_modpack",
  "register_mod",
  "install_modpack",
  "export_modpack_to",
  "set_modpack_sync_date",
  "get_news",
  "get_minecraft_folder",
  "get_default_minecraft_folder",
  "get_modpacks_folder",
  "open_modpacks_folder",
  "init_config",
  "get_config_value",
  "set_config_value",
  "remove_config_value",
  "search_mods",
  "get_categories",
  "get_versions",
  "get_user_url",
  "install_mod",
  "install_remote_file",
  "identify_modpack",
  "check_mod_updates",
  "get_mod_modrinth",
  "get_mod_owners_modrinth",
  "get_mod_deps_modrinth",
  "set_secret",
  "clear_account_token",
  "get_account_info",
  "oauth2_login",
  "oauth2_client_id",
  "read_notification",
  "get_synced_modpacks",
  "kick_member",
  "invite_member",
  "delete_synced_modpack",
  "sync_modpack",
  "answer_invite",
  "share_modpack",
  "share_modpack_raw",
  "get_quadrant_share_modpack",
  "get_quadrant_settings",
  "submit_quadrant_settings",
  "send_telemetry",
  "remove_telemetry",
  "get_mod_curseforge",
  "get_mod_owners_curseforge",
  "get_mod_deps_curseforge",
  "get_prism_instances",
  "apply_modpack_to_prism_instance",
  "detach_prism_instance",
  "get_prism_sync_plans",
  "get_installed_content",
  "open_content_folder",
  "copy_content",
  "delete_content",
] as const;

export type DesktopCommand = (typeof HOST_COMMANDS)[number];

export const ProgressBarStatus = {
  None: "none",
  Normal: "normal",
  Error: "error",
  Paused: "paused",
  Indeterminate: "indeterminate",
} as const;

export type ProgressBarStatusValue =
  (typeof ProgressBarStatus)[keyof typeof ProgressBarStatus];

export type UpdateDownloadProgressPayload = number;

/** The app a download switches to, or `null` for an update of this one. */
export type UpdateSwitchPayload = "quadrantNative" | null;

export interface QuadrantShareSubmissionPayload {
  code?: string | number;
  uses_left?: number;
}

export interface DesktopEventMap {
  modDownloadProgress: ModProgress;
  modInstallProgress: ModProgress;
  modpackDownloadProgress: number;
  quadrantExportProgress: number;
  quadrantShareSubmission: QuadrantShareSubmissionPayload;
  refreshNotifications: AccountNotification[];
  recheckAccountToken: null;
  refreshSyncedModpacks: string | null;
  updateDownloadProgress: UpdateDownloadProgressPayload;
  updateSwitch: UpdateSwitchPayload;
}

export type DesktopEventName = keyof DesktopEventMap;

export interface DesktopDialogOptions {
  mode?: "open" | "save";
  multiple?: boolean;
  directory?: boolean;
  recursive?: boolean;
  title?: string;
  defaultPath?: string;
}

export interface DesktopWatchOptions {
  delayMs?: number;
  /** Also watch subdirectories. Off by default. */
  recursive?: boolean;
}

export interface DesktopOAuthStartOptions {
  response?: string;
  ports?: number[];
}

export interface DesktopWindowProgressState {
  progress: number;
  status?: ProgressBarStatusValue;
}

export interface DesktopBackendEventEnvelope<T = unknown> {
  event: string;
  payload: T;
}

export type DesktopNewsArticle = Article;
