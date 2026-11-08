import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import {
  emit as tauriEmit,
  listen as tauriListen,
} from "@tauri-apps/api/event";
import { getVersion, getTauriVersion } from "@tauri-apps/api/app";
import {
  getCurrentWindow,
  ProgressBarStatus as TauriProgressBarStatus,
} from "@tauri-apps/api/window";
import { join as tauriJoin } from "@tauri-apps/api/path";
import {
  open as openDialog,
  save as saveDialog,
} from "@tauri-apps/plugin-dialog";
import { watch as watchPath } from "@tauri-apps/plugin-fs";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { platform as tauriPlatform } from "@tauri-apps/plugin-os";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import {
  cancel as cancelOAuthServer,
  onUrl as onOAuthUrl,
  start as startOAuthServer,
} from "@fabianlars/tauri-plugin-oauth";
import type { Event } from "@tauri-apps/api/event";
import type {
  RuntimeAdapter,
  DesktopStoreAdapter,
  DesktopWindowAdapter,
} from "./contract";
import type {
  DesktopCommand,
  DesktopOAuthStartOptions,
  DesktopWindowProgressState,
  ProgressBarStatusValue,
} from "./types";

// Work around an upstream Tauri bug that surfaces on macOS (WKWebView). The
// event plugin injects `__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener`,
// which dereferences `listeners[eventId].handlerId` with no null check (see
// `unlisten_js_script` in tauri's `event/mod.rs`). When a listener entry is
// already gone — a double unlisten, or a teardown that races the async
// listener-registration eval — this throws
// "undefined is not an object (evaluating 'listeners[eventId].handlerId')".
// Because `_unlisten` calls it synchronously inside an async function, the
// throw becomes an unhandled promise rejection instead of a catchable error,
// so callers cannot guard it themselves. Patch the internal to be null-safe.
function patchEventUnlisten(): void {
  const internals = (
    window as unknown as {
      __TAURI_EVENT_PLUGIN_INTERNALS__?: {
        unregisterListener?: (event: string, eventId: number) => void;
        __quadrantPatched?: boolean;
      };
    }
  ).__TAURI_EVENT_PLUGIN_INTERNALS__;

  if (!internals || internals.__quadrantPatched) {
    return;
  }

  const original = internals.unregisterListener;
  if (typeof original !== "function") {
    return;
  }

  internals.unregisterListener = (event: string, eventId: number) => {
    try {
      return original(event, eventId);
    } catch (error) {
      // The listener entry was already torn down; nothing left to unregister.
      console.warn("Ignored stale Tauri event unlisten", {
        event,
        eventId,
        error,
      });
    }
  };
  internals.__quadrantPatched = true;
}

patchEventUnlisten();

function mapProgressStatus(
  status: ProgressBarStatusValue | undefined,
): TauriProgressBarStatus | undefined {
  switch (status) {
    case "none":
      return TauriProgressBarStatus.None;
    case "error":
      return TauriProgressBarStatus.Error;
    case "paused":
      return TauriProgressBarStatus.Paused;
    case "indeterminate":
      return TauriProgressBarStatus.Indeterminate;
    case "normal":
    default:
      return TauriProgressBarStatus.Normal;
  }
}

class TauriStoreAdapter implements DesktopStoreAdapter {
  // The whole app persists settings through a single host-backed config
  // store, addressed via the get_config_value/set_config_value commands and
  // the "configChanged" event. The store name is kept for API compatibility
  // but there is only one store.
  constructor(_name: string) {}

  async get<T>(key: string): Promise<T | undefined> {
    const value = await tauriInvoke<T | null>("get_config_value", { key });
    return value ?? undefined;
  }

  async set(key: string, value: unknown): Promise<void> {
    await tauriInvoke("set_config_value", { key, value });
  }

  async delete(key: string): Promise<void> {
    await tauriInvoke("remove_config_value", { key });
  }

  async save(): Promise<void> {
    // Writes are persisted immediately by the host store; nothing to flush.
  }

  async onChange(listener: (key: string) => void) {
    return tauriListen<string>("configChanged", (event) =>
      listener(event.payload),
    );
  }

  async onKeyChange<T>(key: string, listener: (value: T | null) => void) {
    return tauriListen<string>("configChanged", (event) => {
      if (event.payload === key) {
        void this.get<T>(key).then((value) => listener(value ?? null));
      }
    });
  }
}

class TauriWindowAdapter implements DesktopWindowAdapter {
  private readonly window = getCurrentWindow();

  async minimize(): Promise<void> {
    await this.window.minimize();
  }

  async hide(): Promise<void> {
    await this.window.hide();
  }

  async setEnabled(enabled: boolean): Promise<void> {
    await this.window.setEnabled(enabled);
  }

  async setFocus(): Promise<void> {
    await this.window.setFocus();
  }

  async unminimize(): Promise<void> {
    await this.window.unminimize();
  }

  async setProgressBar(state: DesktopWindowProgressState): Promise<void> {
    await this.window.setProgressBar({
      progress: state.progress,
      status: mapProgressStatus(state.status),
    });
  }

  async setDecorations(decorations: boolean): Promise<void> {
    await this.window.setDecorations(decorations);
  }
}

export const tauriRuntime: RuntimeAdapter = {
  createStore(name: string) {
    return new TauriStoreAdapter(name);
  },
  async invoke<T>(command: DesktopCommand, payload?: unknown) {
    return tauriInvoke<T>(
      command,
      payload as Record<string, unknown> | undefined,
    );
  },
  async listen<T = unknown>(
    event: string,
    listener: (event: Event<T>) => void,
  ) {
    return tauriListen<T>(event, listener);
  },
  async emit(event: string, payload?: unknown) {
    await tauriEmit(event, payload);
  },
  async watch(targetPath: string, listener, options) {
    return watchPath(targetPath, listener, options);
  },
  async joinPath(...segments: string[]) {
    return tauriJoin(...segments);
  },
  async openDialog(options) {
    if (options?.mode === "save") {
      return saveDialog({
        title: options.title,
        defaultPath: options.defaultPath,
      });
    }

    return openDialog({
      multiple: options?.multiple,
      directory: options?.directory,
      recursive: options?.recursive,
      title: options?.title,
      defaultPath: options?.defaultPath,
    });
  },
  async openExternal(url: string) {
    await openUrl(url);
  },
  async openPath(targetPath: string) {
    await openPath(targetPath);
  },
  async readClipboardText() {
    return readText();
  },
  async writeClipboardText(text: string) {
    await writeText(text);
  },
  async platform() {
    return tauriPlatform();
  },
  async getRuntimeName() {
    return "Tauri";
  },
  async getAppVersion() {
    return getVersion();
  },
  async getRuntimeVersion() {
    return getTauriVersion();
  },
  async requestCheckForUpdates() {
    await tauriInvoke("request_check_for_updates");
  },
  async installUpdate() {
    await tauriInvoke("install_update");
  },
  async isAutoupdateEnabled() {
    return tauriInvoke<boolean>("is_autoupdate_enabled");
  },
  getWindow() {
    return new TauriWindowAdapter();
  },
  async onOpenUrl(listener) {
    const currentUrls = await getCurrent();
    if (currentUrls && currentUrls.length > 0) {
      listener(currentUrls);
    }
    return onOpenUrl(listener);
  },
  async startOAuthServer(options: DesktopOAuthStartOptions) {
    return startOAuthServer(options);
  },
  async cancelOAuthServer(port: number) {
    await cancelOAuthServer(port);
  },
  async onOAuthUrl(listener) {
    return onOAuthUrl(listener);
  },
  isProductionBuild() {
    return import.meta.env.PROD;
  },
};
