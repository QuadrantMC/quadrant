import type { RuntimeAdapter } from "./contract";
import { browserRuntime } from "./browser";

let runtimePromise: Promise<RuntimeAdapter> | undefined;

function resolveRuntime(): Promise<RuntimeAdapter> {
  // Tauri injects globals into the webview, which makes it safe to lazy-load
  // the Tauri adapter only when we know we are inside that shell.
  if ("__TAURI_INTERNALS__" in window || "__TAURI__" in window) {
    return import("./tauri").then((module) => module.tauriRuntime);
  }

  // The browser adapter keeps Storybook/tests/plain Vite previews usable even
  // when no desktop shell is available.
  return Promise.resolve(browserRuntime);
}

export function getRuntimeAdapter(): Promise<RuntimeAdapter> {
  if (!runtimePromise) {
    runtimePromise = resolveRuntime();
  }
  return runtimePromise;
}

/**
 * Test-only escape hatch: inject a fake adapter, or pass `undefined` to
 * clear the memoized runtime so the next call re-resolves it.
 */
export function __setRuntimeForTests(adapter: RuntimeAdapter | undefined) {
  runtimePromise = adapter ? Promise.resolve(adapter) : undefined;
}
