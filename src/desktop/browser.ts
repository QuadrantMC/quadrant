/** @format */

import type {
  DesktopOAuthStartOptions,
  DesktopWindowProgressState,
} from "./types";
import type {
  DesktopStoreAdapter,
  DesktopWindowAdapter,
  RuntimeAdapter,
  UnlistenFn,
} from "./contract";

function unsupported(command: string): never {
  throw new Error(
    `Quadrant desktop runtime is unavailable in this environment. Tried to use: ${command}`,
  );
}

class BrowserStoreAdapter implements DesktopStoreAdapter {
  constructor(private readonly storeName: string) {}

  private getStorageKey(key: string) {
    return `quadrant:${this.storeName}:${key}`;
  }

  async get<T>(key: string): Promise<T | undefined> {
    const value = window.localStorage.getItem(this.getStorageKey(key));
    return value === null ? undefined : (JSON.parse(value) as T);
  }

  async set(key: string, value: unknown): Promise<void> {
    window.localStorage.setItem(this.getStorageKey(key), JSON.stringify(value));
  }

  async save(): Promise<void> {}

  async onChange(): Promise<UnlistenFn> {
    return () => {};
  }

  async onKeyChange(): Promise<UnlistenFn> {
    return () => {};
  }
}

class BrowserWindowAdapter implements DesktopWindowAdapter {
  async minimize(): Promise<void> {}
  async hide(): Promise<void> {}
  async setEnabled(): Promise<void> {}
  async setFocus(): Promise<void> {
    window.focus();
  }
  async unminimize(): Promise<void> {}
  async setProgressBar(_state: DesktopWindowProgressState): Promise<void> {}
  async setDecorations(_decorations: boolean): Promise<void> {}
}

export const browserRuntime: RuntimeAdapter = {
  createStore(name: string) {
    return new BrowserStoreAdapter(name);
  },
  async invoke<T>(_command: string): Promise<T> {
    unsupported("invoke");
  },
  async listen() {
    return () => {};
  },
  async watch() {
    return () => {};
  },
  async joinPath(...segments: string[]) {
    return segments.join("/");
  },
  async openDialog() {
    unsupported("openDialog");
  },
  async openExternal(url: string) {
    window.open(url, "_blank", "noopener,noreferrer");
  },
  async openPath() {
    unsupported("openPath");
  },
  async readClipboardText() {
    return navigator.clipboard.readText();
  },
  async writeClipboardText(text: string) {
    await navigator.clipboard.writeText(text);
  },
  async platform() {
    return "web";
  },
  async getRuntimeName() {
    return "Browser";
  },
  async getAppVersion() {
    return "web";
  },
  async getRuntimeVersion() {
    return "browser";
  },
  async requestCheckForUpdates() {},
  async installUpdate() {
    unsupported("installUpdate");
  },
  async isAutoupdateEnabled() {
    return false;
  },
  getWindow() {
    return new BrowserWindowAdapter();
  },
  async onOpenUrl() {
    return () => {};
  },
  async startOAuthServer(_options: DesktopOAuthStartOptions) {
    unsupported("startOAuthServer");
  },
  async cancelOAuthServer() {},
  async onOAuthUrl() {
    return () => {};
  },
  async fetch(input: string, init?: RequestInit) {
    return fetch(input, init);
  },
  isProductionBuild() {
    return import.meta.env.PROD;
  },
};
