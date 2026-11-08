/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { browserRuntime } from "./browser";
import type { UnlistenFn } from "./contract";
import { createDesktopStore } from "./index";
import { __setRuntimeForTests } from "./runtime";

const subscriptions: UnlistenFn[] = [];

beforeEach(() => {
  window.localStorage.clear();
  __setRuntimeForTests(browserRuntime);
});

afterEach(async () => {
  for (const unsubscribe of subscriptions.splice(0)) {
    await unsubscribe();
  }
  __setRuntimeForTests(undefined);
});

describe("browser settings subscriptions", () => {
  it("notifies subscribers through independently resolved store adapters", async () => {
    const reader = createDesktopStore("config.json");
    const writer = createDesktopStore("config.json");
    const onChange = vi.fn();
    const onScaleChange = vi.fn();
    subscriptions.push(await reader.onChange(onChange));
    subscriptions.push(await reader.onKeyChange("uiScale", onScaleChange));

    await writer.set("uiScale", 150);

    expect(await reader.get("uiScale")).toBe(150);
    // uiScale is a synced setting, so the edit also advances the sync clock.
    expect(onChange.mock.calls).toEqual([["uiScale"], ["lastSettingsUpdated"]]);
    expect(onScaleChange).toHaveBeenCalledExactlyOnceWith(150);
  });

  it("isolates store names and keys, including null values", async () => {
    const store = createDesktopStore("config.json");
    const listener = vi.fn();
    subscriptions.push(await store.onKeyChange("uiScale", listener));

    await createDesktopStore("other.json").set("uiScale", 200);
    await store.set("language", "tr");
    expect(listener).not.toHaveBeenCalled();

    await store.set("uiScale", null);
    expect(listener).toHaveBeenCalledExactlyOnceWith(null);
  });

  it("stops delivering changes after either kind of subscription is removed", async () => {
    const store = createDesktopStore("config.json");
    const onChange = vi.fn();
    const onKeyChange = vi.fn();
    const unsubscribeAll = await store.onChange(onChange);
    const unsubscribeKey = await store.onKeyChange("uiScale", onKeyChange);
    subscriptions.push(unsubscribeAll, unsubscribeKey);

    await unsubscribeAll();
    await unsubscribeKey();
    await store.set("uiScale", 150);

    expect(onChange).not.toHaveBeenCalled();
    expect(onKeyChange).not.toHaveBeenCalled();
  });

  it("forgets a deleted key, so a later read finds nothing", async () => {
    const store = createDesktopStore("config.json");
    await store.set("uiScale", 150);

    await store.delete("uiScale");

    expect(await store.get("uiScale")).toBeUndefined();
  });

  it("tells both kinds of subscriber that a key was deleted", async () => {
    const store = createDesktopStore("config.json");
    await store.set("uiScale", 150);
    const onChange = vi.fn();
    const onScaleChange = vi.fn();
    const onLanguageChange = vi.fn();
    subscriptions.push(await store.onChange(onChange));
    subscriptions.push(await store.onKeyChange("uiScale", onScaleChange));
    subscriptions.push(await store.onKeyChange("language", onLanguageChange));

    await store.delete("uiScale");

    expect(onChange.mock.calls).toEqual([["uiScale"], ["lastSettingsUpdated"]]);
    // A removed key reads back as null, the same as one written null.
    expect(onScaleChange).toHaveBeenCalledExactlyOnceWith(null);
    expect(onLanguageChange).not.toHaveBeenCalled();
  });

  it("deletes within one store, leaving the same key in another alone", async () => {
    const config = createDesktopStore("config.json");
    const other = createDesktopStore("other.json");
    await config.set("uiScale", 150);
    await other.set("uiScale", 200);

    await config.delete("uiScale");

    expect(await other.get("uiScale")).toBe(200);
  });

  it("preserves the previous value when a write cannot be serialized", async () => {
    const store = createDesktopStore("config.json");
    await store.set("uiScale", 150);

    await expect(store.set("uiScale", undefined)).rejects.toThrow(TypeError);
    expect(await store.get("uiScale")).toBe(150);
  });
});

describe("settings sync clock", () => {
  const clock = (store: ReturnType<typeof createDesktopStore>) =>
    store.get<string>("lastSettingsUpdated");

  it("advances when the user edits a synced setting", async () => {
    const store = createDesktopStore("config.json");
    expect(await clock(store)).toBeUndefined();

    await store.set("curseforge", false);

    expect(Date.parse((await clock(store)) as string)).not.toBeNaN();
  });

  it("advances when the user removes a synced setting", async () => {
    const store = createDesktopStore("config.json");
    await store.delete("uiScale");
    expect(await clock(store)).toBeDefined();
  });

  it.each(["lastPageName", "oauthState", "searchFilters", "mcFolder"])(
    "stays put when %s changes",
    async (key) => {
      const store = createDesktopStore("config.json");
      await store.set(key, "value");
      expect(await clock(store)).toBeUndefined();
    },
  );

  it("stays put for stores other than the config", async () => {
    await createDesktopStore("other.json").set("uiScale", 150);
    expect(await clock(createDesktopStore("other.json"))).toBeUndefined();
  });
});
