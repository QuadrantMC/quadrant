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

const STORE_CHANGE_EVENT = "quadrant:store-changed";

interface StoreChange {
  storeName: string;
  key: string;
  value: unknown;
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
    const serialized = JSON.stringify(value);
    if (serialized === undefined) {
      throw new TypeError("Settings values must be JSON-serializable");
    }
    window.localStorage.setItem(this.getStorageKey(key), serialized);
    // The native storage event does not fire in the window that made the write.
    // Share notifications across adapters, since DesktopStore resolves a new
    // adapter for each operation.
    window.dispatchEvent(
      new CustomEvent<StoreChange>(STORE_CHANGE_EVENT, {
        detail: {
          storeName: this.storeName,
          key,
          value: JSON.parse(serialized),
        },
      }),
    );
  }

  async delete(key: string): Promise<void> {
    window.localStorage.removeItem(this.getStorageKey(key));
    window.dispatchEvent(
      new CustomEvent<StoreChange>(STORE_CHANGE_EVENT, {
        detail: { storeName: this.storeName, key, value: null },
      }),
    );
  }

  async save(): Promise<void> {}

  private subscribe(listener: (change: StoreChange) => void): UnlistenFn {
    const handleChange = (event: Event) => {
      const change = (event as CustomEvent<StoreChange>).detail;
      if (change.storeName === this.storeName) {
        listener(change);
      }
    };
    window.addEventListener(STORE_CHANGE_EVENT, handleChange);
    return () => window.removeEventListener(STORE_CHANGE_EVENT, handleChange);
  }

  async onChange(listener: (key: string) => void): Promise<UnlistenFn> {
    return this.subscribe((change) => listener(change.key));
  }

  async onKeyChange<T>(
    key: string,
    listener: (value: T | null) => void,
  ): Promise<UnlistenFn> {
    return this.subscribe((change) => {
      if (change.key === key) {
        listener((change.value as T | null) ?? null);
      }
    });
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
  async emit() {},
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
  isProductionBuild() {
    return import.meta.env.PROD;
  },
};
