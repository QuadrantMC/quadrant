/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// The config store is a single shared mock so a test can assert what the
// component persisted through it.
const storeGet = vi.fn();
const storeSet = vi.fn();
const storeSave = vi.fn();
const storeOnKeyChange = vi.fn();
const invoke = vi.fn();

const getMinecraftFolder = vi.fn();
const getDefaultMinecraftFolder = vi.fn();
const openIn = vi.fn();
const requestCheckForUpdates = vi.fn();

vi.mock("../../../tools", () => ({
  getMinecraftFolder: (...a: unknown[]) => getMinecraftFolder(...a),
  getDefaultMinecraftFolder: (...a: unknown[]) => getDefaultMinecraftFolder(...a),
  openIn: (...a: unknown[]) => openIn(...a),
  requestCheckForUpdates: (...a: unknown[]) => requestCheckForUpdates(...a),
}));

vi.mock("../../../desktop", () => ({
  createDesktopStore: () => ({
    get: (...a: unknown[]) => storeGet(...a),
    set: (...a: unknown[]) => storeSet(...a),
    save: (...a: unknown[]) => storeSave(...a),
    onKeyChange: (...a: unknown[]) => storeOnKeyChange(...a),
  }),
  getAppVersion: () => Promise.resolve("26.7.0"),
  getRuntimeName: () => Promise.resolve("Tauri"),
  getRuntimeVersion: () => Promise.resolve("2.0.0"),
  invoke: (...a: unknown[]) => invoke(...a),
  isAutoupdateEnabled: () => Promise.resolve(true),
  openDialog: vi.fn(),
}));

import SettingsPage from "./Settings";

beforeEach(() => {
  vi.clearAllMocks();
  // Unknown keys resolve to undefined, letting the component apply its
  // documented defaults.
  storeGet.mockResolvedValue(undefined);
  storeSet.mockResolvedValue(undefined);
  storeSave.mockResolvedValue(undefined);
  storeOnKeyChange.mockResolvedValue(() => {});
  getMinecraftFolder.mockResolvedValue("/home/user/.minecraft");
  invoke.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.restoreAllMocks();
});

async function renderSettled() {
  render(<SettingsPage />);
  // Wait for the async initializeValues() effect to finish loading.
  await waitFor(() =>
    expect(screen.getByText("/home/user/.minecraft")).toBeInTheDocument(),
  );
}

describe("SettingsPage", () => {
  it("renders once the initial config load resolves", async () => {
    await renderSettled();
    expect(
      screen.getByText("Some settings require a relaunch!"),
    ).toBeInTheDocument();
    // The version line interpolates the mocked runtime values.
    expect(screen.getByText(/26\.7\.0/)).toBeInTheDocument();
  });

  it("persists the CurseForge toggle through the config store", async () => {
    await renderSettled();
    const toggle = screen.getByRole("switch", { name: "CurseForge" });
    expect(toggle).toHaveAttribute("aria-checked", "false");

    await userEvent.click(toggle);

    expect(storeSet).toHaveBeenCalledWith("curseforge", true);
    expect(storeSave).toHaveBeenCalled();
    await waitFor(() =>
      expect(toggle).toHaveAttribute("aria-checked", "true"),
    );
  });

  it("persists the Modrinth toggle through the config store", async () => {
    await renderSettled();
    const toggle = screen.getByRole("switch", { name: "Modrinth" });
    await userEvent.click(toggle);
    expect(storeSet).toHaveBeenCalledWith("modrinth", true);
  });

  it("persists the developer-mode toggle", async () => {
    await renderSettled();
    const toggle = screen.getByRole("switch", { name: "Developer mode" });
    await userEvent.click(toggle);
    expect(storeSet).toHaveBeenCalledWith("devMode", true);
  });

  it("enabling data collection persists the flag and sends telemetry", async () => {
    await renderSettled();
    const toggle = screen.getByRole("switch", { name: "Collect data" });
    await userEvent.click(toggle);

    expect(storeSet).toHaveBeenCalledWith("collectUserData", true);
    expect(invoke).toHaveBeenCalledWith("send_telemetry");
    expect(invoke).not.toHaveBeenCalledWith("remove_telemetry");
  });
});
