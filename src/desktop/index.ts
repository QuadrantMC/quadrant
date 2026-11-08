import type { RuntimeAdapter } from "./contract";
import type {
  DesktopCommand,
  DesktopDialogOptions,
  DesktopOAuthStartOptions,
  DesktopWatchOptions,
  DesktopWindowProgressState,
} from "./types";
import { ProgressBarStatus } from "./types";
export type { UpdateSwitchPayload } from "./types";
import { getRuntimeAdapter } from "./runtime";
import { bumpsSettingsClock } from "../settingsSync";

async function withRuntime<T>(
  callback: (runtime: RuntimeAdapter) => Promise<T>,
): Promise<T> {
  // Runtime detection is lazy so browser-based tooling and tests do not have
  // to eagerly load Tauri modules.
  const runtime = await getRuntimeAdapter();
  return callback(runtime);
}

export class DesktopStore {
  constructor(private readonly name: string) {}

  private async adapter() {
    // Stores are resolved through the active runtime so config/update storage
    // keeps the same call sites in the renderer for both backends.
    const runtime = await getRuntimeAdapter();
    return runtime.createStore(this.name);
  }

  async get<T>(key: string): Promise<T | undefined> {
    return (await this.adapter()).get<T>(key);
  }

  async set(key: string, value: unknown): Promise<void> {
    const adapter = await this.adapter();
    await adapter.set(key, value);
    await this.markSettingsEdited(adapter, key);
  }

  async delete(key: string): Promise<void> {
    const adapter = await this.adapter();
    await adapter.delete(key);
    await this.markSettingsEdited(adapter, key);
  }

  // The settings sync compares this timestamp to decide which side is newer,
  // so it moves only when the user changed a synced setting.
  private async markSettingsEdited(
    adapter: ReturnType<RuntimeAdapter["createStore"]>,
    key: string,
  ) {
    if (this.name === "config.json" && bumpsSettingsClock(key)) {
      await adapter.set("lastSettingsUpdated", new Date().toISOString());
    }
  }

  async save(): Promise<void> {
    await (await this.adapter()).save();
  }

  async onChange(listener: (key: string) => void) {
    return (await this.adapter()).onChange(listener);
  }

  async onKeyChange<T>(key: string, listener: (value: T | null) => void) {
    return (await this.adapter()).onKeyChange<T>(key, listener);
  }
}

export function createDesktopStore(name: string) {
  return new DesktopStore(name);
}

export async function invoke<T>(
  command: DesktopCommand,
  payload?: unknown,
): Promise<T> {
  return withRuntime((runtime) => runtime.invoke<T>(command, payload));
}

export async function listen<T = unknown>(
  event: string,
  listener: (event: { payload: T; event: string }) => void,
) {
  return withRuntime((runtime) => runtime.listen<T>(event, listener));
}

export async function watch(
  targetPath: string,
  listener: () => void,
  options?: DesktopWatchOptions,
) {
  return withRuntime((runtime) => runtime.watch(targetPath, listener, options));
}

export async function joinPath(...segments: string[]) {
  return withRuntime((runtime) => runtime.joinPath(...segments));
}

export async function openDialog(options: DesktopDialogOptions) {
  return withRuntime((runtime) => runtime.openDialog(options));
}

export async function openExternal(url: string) {
  await withRuntime((runtime) => runtime.openExternal(url));
}

export async function openPath(targetPath: string) {
  await withRuntime((runtime) => runtime.openPath(targetPath));
}

export async function readClipboardText() {
  return withRuntime((runtime) => runtime.readClipboardText());
}

export async function writeClipboardText(text: string) {
  await withRuntime((runtime) => runtime.writeClipboardText(text));
}

export async function platform() {
  return withRuntime((runtime) => runtime.platform());
}

export async function getRuntimeName() {
  return withRuntime((runtime) => runtime.getRuntimeName());
}

export async function getAppVersion() {
  return withRuntime((runtime) => runtime.getAppVersion());
}

export async function getRuntimeVersion() {
  return withRuntime((runtime) => runtime.getRuntimeVersion());
}

export async function requestCheckForUpdates() {
  await withRuntime((runtime) => runtime.requestCheckForUpdates());
}

export async function installUpdate() {
  await withRuntime((runtime) => runtime.installUpdate());
}

export async function isAutoupdateEnabled() {
  return withRuntime((runtime) => runtime.isAutoupdateEnabled());
}

export function getCurrentDesktopWindow() {
  return {
    minimize: () => withRuntime((runtime) => runtime.getWindow().minimize()),
    hide: () => withRuntime((runtime) => runtime.getWindow().hide()),
    setEnabled: (enabled: boolean) =>
      withRuntime((runtime) => runtime.getWindow().setEnabled(enabled)),
    setFocus: () => withRuntime((runtime) => runtime.getWindow().setFocus()),
    unminimize: () =>
      withRuntime((runtime) => runtime.getWindow().unminimize()),
    setProgressBar: (state: DesktopWindowProgressState) =>
      withRuntime((runtime) => runtime.getWindow().setProgressBar(state)),
    setDecorations: (decorations: boolean) =>
      withRuntime((runtime) => runtime.getWindow().setDecorations(decorations)),
  };
}

export async function onOpenUrl(listener: (urls: string[]) => void) {
  return withRuntime((runtime) => runtime.onOpenUrl(listener));
}

export async function startOAuthServer(options: DesktopOAuthStartOptions) {
  return withRuntime((runtime) => runtime.startOAuthServer(options));
}

export async function cancelOAuthServer(port: number) {
  await withRuntime((runtime) => runtime.cancelOAuthServer(port));
}

export async function onOAuthUrl(listener: (url: string) => void) {
  return withRuntime((runtime) => runtime.onOAuthUrl(listener));
}

export async function desktopFetch(input: string, init?: RequestInit) {
  return withRuntime((runtime) => runtime.fetch(input, init));
}

export async function isProductionBuild() {
  return withRuntime(async (runtime) => runtime.isProductionBuild());
}

export { ProgressBarStatus };
