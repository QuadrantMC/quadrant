/** @format */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContentContext, ModLoader } from "../../../intefaces";

const kickMember = vi.fn();
const invoke = vi.fn();
const setSnackbar = vi.fn();
const refreshSyncedModpacks = vi.fn();

vi.mock("../../../tools", () => ({
  inviteMember: vi.fn(),
  kickMember: (...a: unknown[]) => kickMember(...a),
  installModpack: vi.fn(),
}));
vi.mock("../../../desktop", () => ({
  clearProgressBar: vi.fn(async () => {}),
  invoke: (...a: unknown[]) => invoke(...a),
  listen: async () => () => {},
}));

import CloudMembersPanel from "./CloudMembersPanel";
import { SyncContext } from "../../../intefaces";

const modpack = {
  name: "Team Pack",
  modpack_id: "pack-1",
  minecraft_version: "1.20.1",
  mod_loader: ModLoader.Fabric,
  mods: "[]",
  owners: [
    { username: "me", admin: true },
    { username: "friend", admin: false },
  ],
  last_synced: 1,
};

async function renderPanel() {
  render(
    <ContentContext.Provider
      value={{
        back: vi.fn(),
        changeContent: vi.fn(),
        setSnackbar,
        setSnackbarNoState: vi.fn(),
      }}
    >
      <SyncContext.Provider value={{ refreshSyncedModpacks }}>
        <CloudMembersPanel
          modpack={modpack}
          accountInfo={{ login: "me" } as never}
        />
      </SyncContext.Provider>
    </ContentContext.Provider>,
  );
  await userEvent.click(screen.getByRole("button", { name: /quadrant sync/i }));
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("CloudMembersPanel", () => {
  it("asks before kicking a member and kicks only on confirmation", async () => {
    kickMember.mockResolvedValue(undefined);
    await renderPanel();

    await userEvent.click(await screen.findByRole("button", { name: "Kick" }));
    expect(await screen.findByText("Remove friend?")).toBeVisible();
    expect(kickMember).not.toHaveBeenCalled();

    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Kick" }),
    );
    await waitFor(() =>
      expect(kickMember).toHaveBeenCalledExactlyOnceWith("pack-1", "friend"),
    );
    expect(refreshSyncedModpacks).toHaveBeenCalled();
  });

  it("does not kick when the question is cancelled", async () => {
    await renderPanel();

    await userEvent.click(await screen.findByRole("button", { name: "Kick" }));
    await userEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Cancel",
      }),
    );

    expect(kickMember).not.toHaveBeenCalled();
  });

  it("shows a failed kick instead of leaving an unhandled rejection", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    kickMember.mockRejectedValue("errorForbidden");
    await renderPanel();

    await userEvent.click(await screen.findByRole("button", { name: "Kick" }));
    await userEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Kick",
      }),
    );

    await waitFor(() => expect(setSnackbar).toHaveBeenCalledTimes(1));
    expect(setSnackbar.mock.calls[0][0]).toMatchObject({
      className: expect.stringContaining("bg-red-700"),
    });
    error.mockRestore();
  });

  it("asks before deleting the cloud copy", async () => {
    invoke.mockResolvedValue(undefined);
    await renderPanel();

    await userEvent.click(
      await screen.findByRole("button", { name: "Delete" }),
    );
    expect(
      await screen.findByText("Delete Team Pack from Quadrant Sync?"),
    ).toBeVisible();
    expect(invoke).not.toHaveBeenCalled();

    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Delete",
      }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("delete_synced_modpack", {
        modpackId: "pack-1",
      }),
    );
  });
});
