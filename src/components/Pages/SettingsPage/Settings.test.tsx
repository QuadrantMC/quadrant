/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// The config store is a single shared mock so a test can assert what the
// component persisted through it.
const storeGet = vi.fn();
const storeSet = vi.fn();
const storeDelete = vi.fn();
const storeSave = vi.fn();
const storeOnKeyChange = vi.fn();
const invoke = vi.fn();
const openDialog = vi.fn();

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
    delete: (...a: unknown[]) => storeDelete(...a),
    save: (...a: unknown[]) => storeSave(...a),
    onKeyChange: (...a: unknown[]) => storeOnKeyChange(...a),
  }),
  getAppVersion: () => Promise.resolve("26.7.0"),
  getRuntimeName: () => Promise.resolve("Tauri"),
  getRuntimeVersion: () => Promise.resolve("2.0.0"),
  invoke: (...a: unknown[]) => invoke(...a),
  isAutoupdateEnabled: () => Promise.resolve(true),
  openDialog: (...a: unknown[]) => openDialog(...a),
}));

import SettingsPage from "./Settings";

beforeEach(() => {
  vi.clearAllMocks();
  // Unknown keys resolve to undefined, letting the component apply its
  // documented defaults.
  storeGet.mockResolvedValue(undefined);
  storeSet.mockResolvedValue(undefined);
  storeDelete.mockResolvedValue(undefined);
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

  it("offers the GPUI channels and checks the picked one for updates", async () => {
    await renderSettled();
    const picker = screen.getByRole("combobox", { name: "Update channel" });
    expect(
      Array.from(picker.querySelectorAll("option")).map((option) => [
        option.value,
        option.textContent,
      ]),
    ).toEqual([
      ["stable", "Stable"],
      ["preview", "Preview"],
      ["gpui-preview", "GPUI Preview"],
      ["gpui-experimental", "GPUI Experimental"],
    ]);

    await userEvent.selectOptions(picker, "gpui-experimental");

    expect(storeSet).toHaveBeenCalledWith("channel", "gpui-experimental");
    await waitFor(() => expect(requestCheckForUpdates).toHaveBeenCalled());
  });
});

describe("SettingsPage Prism Launcher folder override", () => {
  async function renderWithExperimental() {
    storeGet.mockImplementation(async (key: string) =>
      key === "experimentalFeatures" ? true : undefined,
    );
    await renderSettled();
  }

  it("stays hidden while experimental features are off", async () => {
    await renderSettled();
    expect(screen.queryByText("Prism Launcher folder")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Choose folder" }),
    ).not.toBeInTheDocument();
  });

  it("reports an unset override as auto-detected", async () => {
    await renderWithExperimental();
    expect(screen.getByText("Prism Launcher folder")).toBeInTheDocument();
    expect(screen.getByText("Detected automatically")).toBeInTheDocument();
  });

  it("persists the folder picked from the dialog", async () => {
    openDialog.mockResolvedValue("/home/user/.local/share/PrismLauncher");
    await renderWithExperimental();

    await userEvent.click(
      screen.getByRole("button", { name: "Choose folder" }),
    );

    await waitFor(() =>
      expect(storeSet).toHaveBeenCalledWith(
        "prismLauncherFolder",
        "/home/user/.local/share/PrismLauncher",
      ),
    );
    expect(storeSave).toHaveBeenCalled();
    expect(
      screen.getByText("/home/user/.local/share/PrismLauncher"),
    ).toBeInTheDocument();
  });

  it("keeps the stored override when the dialog is cancelled", async () => {
    openDialog.mockResolvedValue(null);
    await renderWithExperimental();

    await userEvent.click(
      screen.getByRole("button", { name: "Choose folder" }),
    );

    await waitFor(() => expect(openDialog).toHaveBeenCalled());
    expect(storeSet).not.toHaveBeenCalledWith(
      "prismLauncherFolder",
      expect.anything(),
    );
  });

  it("deletes the key and falls back to auto-detection on reset", async () => {
    storeGet.mockImplementation(async (key: string) =>
      key === "experimentalFeatures"
        ? true
        : key === "prismLauncherFolder"
          ? "/opt/prism"
          : undefined,
    );
    await renderSettled();
    await waitFor(() =>
      expect(screen.getByText("/opt/prism")).toBeInTheDocument(),
    );

    await userEvent.click(screen.getByRole("button", { name: "Reset" }));

    await waitFor(() =>
      expect(storeDelete).toHaveBeenCalledWith("prismLauncherFolder"),
    );
    expect(storeSave).toHaveBeenCalled();
    expect(screen.getByText("Detected automatically")).toBeInTheDocument();
  });
});
