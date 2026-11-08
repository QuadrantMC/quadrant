import type {
  DesktopBackendEventEnvelope,
  DesktopCommand,
  DesktopDialogOptions,
  DesktopOAuthStartOptions,
  DesktopWatchOptions,
  DesktopWindowProgressState,
} from "./types";

export type UnlistenFn = () => void | Promise<void>;

export interface DesktopStoreAdapter {
  get<T>(key: string): Promise<T | undefined>;
  set(key: string, value: unknown): Promise<void>;
  /** Removes the key, so `get` reports it as unset rather than as a null value. */
  delete(key: string): Promise<void>;
  save(): Promise<void>;
  onChange(listener: (key: string) => void): Promise<UnlistenFn>;
  onKeyChange<T>(
    key: string,
    listener: (value: T | null) => void,
  ): Promise<UnlistenFn>;
}

export interface DesktopWindowAdapter {
  minimize(): Promise<void>;
  hide(): Promise<void>;
  setEnabled(enabled: boolean): Promise<void>;
  setFocus(): Promise<void>;
  unminimize(): Promise<void>;
  setProgressBar(state: DesktopWindowProgressState): Promise<void>;
  setDecorations(decorations: boolean): Promise<void>;
}

export interface RuntimeAdapter {
  // Every desktop host implements this contract so the renderer can stay
  // runtime-neutral and feature code never has to import Tauri APIs directly.
  createStore(name: string): DesktopStoreAdapter;
  invoke<T>(command: DesktopCommand, payload?: unknown): Promise<T>;
  listen<T = unknown>(
    event: string,
    listener: (event: DesktopBackendEventEnvelope<T>) => void,
  ): Promise<UnlistenFn>;
  /** Raises a renderer-side event for every `listen` subscriber to hear. */
  emit(event: string, payload?: unknown): Promise<void>;
  watch(
    targetPath: string,
    listener: () => void,
    options?: DesktopWatchOptions,
  ): Promise<UnlistenFn>;
  joinPath(...segments: string[]): Promise<string>;
  openDialog(options: DesktopDialogOptions): Promise<string | string[] | null>;
  openExternal(url: string): Promise<void>;
  openPath(targetPath: string): Promise<void>;
  readClipboardText(): Promise<string>;
  writeClipboardText(text: string): Promise<void>;
  platform(): Promise<string>;
  getRuntimeName(): Promise<string>;
  getAppVersion(): Promise<string>;
  getRuntimeVersion(): Promise<string>;
  requestCheckForUpdates(): Promise<void>;
  installUpdate(): Promise<void>;
  isAutoupdateEnabled(): Promise<boolean>;
  getWindow(): DesktopWindowAdapter;
  onOpenUrl(listener: (urls: string[]) => void): Promise<UnlistenFn>;
  startOAuthServer(options: DesktopOAuthStartOptions): Promise<number>;
  cancelOAuthServer(port: number): Promise<void>;
  onOAuthUrl(listener: (url: string) => void): Promise<UnlistenFn>;
  fetch(input: string, init?: RequestInit): Promise<Response>;
  isProductionBuild(): boolean;
}
