/** @format */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ContentContext,
  ModLoader,
  type PrismInstance,
  type PrismSyncPlan,
} from "../../../intefaces";

const exportModpack = vi.fn();
const deleteModpack = vi.fn();
const applyModpackToPrismInstance = vi.fn();
const detachPrismInstance = vi.fn();
const getPrismSyncPlans = vi.fn();
const syncModpack = vi.fn();

vi.mock("../../../tools", () => ({
  applyModpack: vi.fn(),
  applyModpackToPrismInstance: (...a: unknown[]) =>
    applyModpackToPrismInstance(...a),
  deleteModpack: (...a: unknown[]) => deleteModpack(...a),
  detachPrismInstance: (...a: unknown[]) => detachPrismInstance(...a),
  exportModpack: (...a: unknown[]) => exportModpack(...a),
  getPrismSyncPlans: (...a: unknown[]) => getPrismSyncPlans(...a),
  shareModpack: vi.fn(),
  syncModpack: (...a: unknown[]) => syncModpack(...a),
}));

import LocalModpackCard from "./LocalModpackCard";

const setSnackbar = vi.fn();

function instance(over: Partial<PrismInstance>): PrismInstance {
  return {
    id: "instance-1",
    name: "Survival",
    minecraftVersion: "1.20.1",
    modLoader: ModLoader.Fabric,
    modLoaderVersion: "0.15.0",
    appliedModpack: null,
    ...over,
  };
}

function renderCard(
  props: {
    prismInstances?: PrismInstance[];
    onChanged?: () => void;
    manifestError?: string | null;
  } = {},
) {
  return render(
    <ContentContext.Provider
      value={{
        back: () => {},
        setSnackbar,
        changeContent: () => {},
        setSnackbarNoState: () => {},
      }}
    >
      <LocalModpackCard
        modpack={{
          name: "Alpha Pack",
          version: "1.20.1",
          modLoader: ModLoader.Fabric,
          isApplied: false,
          lastSynced: 0,
          mods: [],
          unknownMods: false,
          manifestError: props.manifestError,
        }}
        prismInstances={props.prismInstances}
        onChanged={props.onChanged ?? (() => {})}
        onEdit={() => {}}
      />
    </ContentContext.Provider>,
  );
}

function plan(over: Partial<PrismSyncPlan>): PrismSyncPlan {
  return {
    instanceId: "instance-1",
    minecraftVersion: null,
    modLoader: null,
    ...over,
  };
}

/** Opens the menu, which is what makes it ask the host for the sync plans. */
async function openPrismMenu() {
  await userEvent.click(screen.getByRole("button", { name: /prism launcher/i }));
}

beforeEach(() => {
  vi.clearAllMocks();
  getPrismSyncPlans.mockResolvedValue([]);
  syncModpack.mockResolvedValue(undefined);
});

describe("LocalModpackCard", () => {
  it("surfaces an export failure through the error snackbar", async () => {
    // The export must be awaited; otherwise the rejection escapes the catch
    // and the user is never told the export failed.
    exportModpack.mockRejectedValue("exportFailed");
    renderCard();

    await userEvent.click(screen.getByRole("button", { name: /export/i }));

    await waitFor(() => expect(setSnackbar).toHaveBeenCalledTimes(1));
    expect(setSnackbar.mock.calls[0][0]).toMatchObject({
      className: expect.stringContaining("bg-red-700"),
    });
  });

  it("does not show an error when the export succeeds", async () => {
    exportModpack.mockResolvedValue(undefined);
    renderCard();

    await userEvent.click(screen.getByRole("button", { name: /export/i }));

    await waitFor(() =>
      expect(exportModpack).toHaveBeenCalledWith("Alpha Pack"),
    );
    expect(setSnackbar).not.toHaveBeenCalled();
  });
});

describe("LocalModpackCard with an unreadable manifest", () => {
  it("says why and locks the actions the host would refuse", async () => {
    renderCard({ manifestError: "expected value at line 1" });

    expect(screen.getByRole("alert")).toHaveTextContent(
      "can't read this modpack's modConfigV2.json (expected value at line 1)",
    );
    for (const name of [/^share$/i, /^sync$/i, /^update$/i]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
    // Applying, exporting and deleting still work on a broken pack.
    expect(screen.getByRole("button", { name: /^apply$/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /^delete$/i })).toBeEnabled();
  });

  it("shows nothing extra for a readable pack", () => {
    renderCard({ manifestError: null });

    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: /^share$/i })).toBeEnabled();
  });
});

describe("LocalModpackCard delete", () => {
  const openDelete = () =>
    userEvent.click(screen.getByRole("button", { name: /^delete$/i }));
  const confirmDelete = () =>
    userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Delete",
      }),
    );

  it("asks first and removes nothing until the question is answered yes", async () => {
    deleteModpack.mockResolvedValue(undefined);
    const onChanged = vi.fn();
    renderCard({ onChanged });

    await openDelete();

    expect(await screen.findByText("Delete Alpha Pack?")).toBeVisible();
    expect(deleteModpack).not.toHaveBeenCalled();

    await confirmDelete();
    await waitFor(() =>
      expect(deleteModpack).toHaveBeenCalledExactlyOnceWith("Alpha Pack"),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("keeps the modpack when the question is cancelled", async () => {
    renderCard();

    await openDelete();
    await userEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Cancel",
      }),
    );

    expect(deleteModpack).not.toHaveBeenCalled();
  });

  it("reports a failed delete", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    deleteModpack.mockRejectedValue("errorFileInUse");
    renderCard();

    await openDelete();
    await confirmDelete();

    await waitFor(() => expect(setSnackbar).toHaveBeenCalledTimes(1));
    expect(setSnackbar.mock.calls[0][0]).toMatchObject({
      className: expect.stringContaining("bg-red-700"),
    });
    error.mockRestore();
  });
});

describe("LocalModpackCard sync", () => {
  const conflictTitle = "Cloud copy is newer";

  async function clickSync() {
    await userEvent.click(screen.getByRole("button", { name: /^sync$/i }));
  }

  it("pushes without overwriting and confirms when the cloud accepts it", async () => {
    const onChanged = vi.fn();
    syncModpack.mockResolvedValue(undefined);
    renderCard({ onChanged });

    await clickSync();

    await waitFor(() => expect(setSnackbar).toHaveBeenCalledTimes(1));
    expect(syncModpack).toHaveBeenCalledTimes(1);
    expect(syncModpack).toHaveBeenCalledWith(
      expect.objectContaining({ name: "Alpha Pack" }),
      false,
    );
    expect(setSnackbar.mock.calls[0][0]).toMatchObject({
      className: "bg-emerald-600 rounded-4xl",
    });
    expect(onChanged).toHaveBeenCalled();
    expect(screen.queryByText(conflictTitle)).not.toBeInTheDocument();
  });

  it("asks before replacing a newer cloud copy instead of reporting an error", async () => {
    syncModpack.mockRejectedValue("errorCloudSyncNewer");
    renderCard();

    await clickSync();

    expect(await screen.findByText(conflictTitle)).toBeInTheDocument();
    expect(
      screen.getByText(/The cloud copy of Alpha Pack was updated/),
    ).toBeInTheDocument();
    expect(setSnackbar).not.toHaveBeenCalled();
  });

  it("retries with overwrite when the replacement is confirmed", async () => {
    const onChanged = vi.fn();
    syncModpack.mockRejectedValueOnce("errorCloudSyncNewer");
    syncModpack.mockResolvedValue(undefined);
    renderCard({ onChanged });

    await clickSync();
    await userEvent.click(
      await screen.findByRole("button", { name: /sync anyway/i }),
    );

    await waitFor(() => expect(syncModpack).toHaveBeenCalledTimes(2));
    expect(syncModpack).toHaveBeenLastCalledWith(
      expect.objectContaining({ name: "Alpha Pack" }),
      true,
    );
    await waitFor(() => expect(setSnackbar).toHaveBeenCalledTimes(1));
    expect(setSnackbar.mock.calls[0][0]).toMatchObject({
      className: "bg-emerald-600 rounded-4xl",
    });
    expect(onChanged).toHaveBeenCalled();
    expect(screen.queryByText(conflictTitle)).not.toBeInTheDocument();
  });

  it("pushes nothing more when the replacement is cancelled", async () => {
    syncModpack.mockRejectedValue("errorCloudSyncNewer");
    renderCard();

    await clickSync();
    await userEvent.click(
      await screen.findByRole("button", { name: /cancel/i }),
    );

    await waitFor(() =>
      expect(screen.queryByText(conflictTitle)).not.toBeInTheDocument(),
    );
    expect(syncModpack).toHaveBeenCalledTimes(1);
    expect(setSnackbar).not.toHaveBeenCalled();
  });

  it("reports any other sync failure without asking anything", async () => {
    syncModpack.mockRejectedValue("errorNetwork");
    renderCard();

    await clickSync();

    await waitFor(() => expect(setSnackbar).toHaveBeenCalledTimes(1));
    expect(setSnackbar.mock.calls[0][0]).toMatchObject({
      className: "bg-red-700",
      message:
        "Couldn't reach the server. Check your internet connection and try again.",
    });
    expect(screen.queryByText(conflictTitle)).not.toBeInTheDocument();
  });
});

describe("LocalModpackCard Prism Launcher menu", () => {
  it("offers no Prism button without instances", () => {
    renderCard();
    expect(
      screen.queryByRole("button", { name: /prism launcher/i }),
    ).not.toBeInTheDocument();

    renderCard({ prismInstances: [] });
    expect(
      screen.queryByRole("button", { name: /prism launcher/i }),
    ).not.toBeInTheDocument();
  });

  it("lists every instance with its version and loader", async () => {
    renderCard({
      prismInstances: [
        instance({ id: "a", name: "Survival" }),
        instance({
          id: "b",
          name: "Creative",
          minecraftVersion: "1.21",
          modLoader: ModLoader.NeoForge,
        }),
      ],
    });

    await openPrismMenu();

    expect(
      screen.getByRole("menuitem", { name: /Survival/ }),
    ).toHaveTextContent("1.20.1 | Fabric");
    expect(
      screen.getByRole("menuitem", { name: /Creative/ }),
    ).toHaveTextContent("1.21 | NeoForge");
  });

  it("asks the host what applying would rewrite, once, on the first opening", async () => {
    renderCard({ prismInstances: [instance({ id: "a" })] });
    expect(getPrismSyncPlans).not.toHaveBeenCalled();

    await openPrismMenu();
    await waitFor(() =>
      expect(getPrismSyncPlans).toHaveBeenCalledWith("Alpha Pack"),
    );

    await openPrismMenu();
    await openPrismMenu();
    expect(getPrismSyncPlans).toHaveBeenCalledTimes(1);
  });

  it("warns with the version and loader the host says it would write", async () => {
    getPrismSyncPlans.mockResolvedValue([
      plan({
        instanceId: "a",
        minecraftVersion: "1.20.1",
        modLoader: ModLoader.Fabric,
      }),
    ]);
    renderCard({
      prismInstances: [
        instance({ id: "a", minecraftVersion: "1.21", modLoader: ModLoader.NeoForge }),
      ],
    });

    await openPrismMenu();

    expect(
      await screen.findByText("Will be switched to 1.20.1 · Fabric"),
    ).toBeInTheDocument();
  });

  it("names only the parts the plan rewrites, and nothing on a linked instance", async () => {
    getPrismSyncPlans.mockResolvedValue([
      plan({ instanceId: "a", minecraftVersion: "1.20.1" }),
      plan({ instanceId: "b", minecraftVersion: "1.20.1" }),
    ]);
    renderCard({
      prismInstances: [
        instance({ id: "a", name: "Survival", minecraftVersion: "1.21" }),
        instance({
          id: "b",
          name: "Creative",
          minecraftVersion: "1.21",
          appliedModpack: "Alpha Pack",
        }),
      ],
    });

    await openPrismMenu();

    await waitFor(() =>
      expect(
        screen.getByRole("menuitem", { name: /Survival/ }),
      ).toHaveTextContent("Will be switched to 1.20.1"),
    );
    expect(
      screen.getByRole("menuitem", { name: /Survival/ }),
    ).not.toHaveTextContent("· Fabric");
    expect(
      screen.getByRole("menuitem", { name: /Creative/ }),
    ).not.toHaveTextContent("Will be switched");
  });

  it("says nothing about an instance the plan leaves untouched", async () => {
    getPrismSyncPlans.mockResolvedValue([plan({ instanceId: "a" })]);
    renderCard({ prismInstances: [instance({ id: "a" })] });

    await openPrismMenu();
    await waitFor(() => expect(getPrismSyncPlans).toHaveBeenCalled());

    expect(screen.queryByText(/Will be switched/)).toBeNull();
  });

  it("keeps the menu usable when the plans cannot be fetched", async () => {
    getPrismSyncPlans.mockRejectedValue(new Error("boom"));
    renderCard({ prismInstances: [instance({ id: "a" })] });

    await openPrismMenu();
    await waitFor(() => expect(getPrismSyncPlans).toHaveBeenCalled());

    expect(screen.getByRole("menuitem", { name: /Survival/ })).toBeTruthy();
    expect(screen.queryByText(/Will be switched/)).toBeNull();
    expect(setSnackbar).not.toHaveBeenCalled();
  });

  it("applies the modpack to an unlinked instance and refreshes", async () => {
    const onChanged = vi.fn();
    applyModpackToPrismInstance.mockResolvedValue(undefined);
    renderCard({ prismInstances: [instance({ id: "a" })], onChanged });

    await openPrismMenu();
    await userEvent.click(screen.getByRole("menuitem", { name: /Survival/ }));

    await waitFor(() =>
      expect(applyModpackToPrismInstance).toHaveBeenCalledWith(
        "Alpha Pack",
        "a",
      ),
    );
    expect(detachPrismInstance).not.toHaveBeenCalled();
    expect(onChanged).toHaveBeenCalled();
    // The plans are re-read, since applying is what changes them.
    expect(getPrismSyncPlans).toHaveBeenCalledTimes(2);
    expect(setSnackbar.mock.calls[0][0]).toMatchObject({
      className: "bg-emerald-600 rounded-4xl",
    });
  });

  it("detaches an instance that is already linked to this modpack", async () => {
    detachPrismInstance.mockResolvedValue(undefined);
    renderCard({
      prismInstances: [instance({ id: "a", appliedModpack: "Alpha Pack" })],
    });

    await openPrismMenu();
    await userEvent.click(screen.getByRole("menuitem", { name: /Survival/ }));

    await waitFor(() => expect(detachPrismInstance).toHaveBeenCalledWith("a"));
    expect(applyModpackToPrismInstance).not.toHaveBeenCalled();
    expect(getPrismSyncPlans).toHaveBeenCalledTimes(2);
  });

  it("reports a failed apply through the error snackbar", async () => {
    applyModpackToPrismInstance.mockRejectedValue("errorPrismInstanceMissing");
    renderCard({ prismInstances: [instance({})] });

    await openPrismMenu();
    await userEvent.click(screen.getByRole("menuitem", { name: /Survival/ }));

    await waitFor(() => expect(setSnackbar).toHaveBeenCalledTimes(1));
    expect(setSnackbar.mock.calls[0][0]).toMatchObject({
      className: "bg-red-700",
      message: "That Prism Launcher instance no longer exists.",
    });
  });

  it("names the instances this modpack is applied to", () => {
    renderCard({
      prismInstances: [
        instance({ id: "a", name: "Survival", appliedModpack: "Alpha Pack" }),
        instance({ id: "b", name: "Creative", appliedModpack: "Alpha Pack" }),
        instance({ id: "c", name: "Other", appliedModpack: "Beta Pack" }),
      ],
    });

    expect(
      screen.getByText("Applied to Prism Launcher: Survival, Creative"),
    ).toBeInTheDocument();
  });
});
