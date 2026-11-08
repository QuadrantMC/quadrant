/** @format */

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ContentContext,
  ModLoader,
  ModSource,
  ModType,
  type IMod,
} from "../../intefaces";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  installMod: vi.fn(),
  installPage: vi.fn(),
  deleteMod: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("../../tools", () => ({
  installMod: mocks.installMod,
  deleteMod: mocks.deleteMod,
  installRemoteFile: vi.fn(),
  openIn: vi.fn(),
  registerMod: vi.fn(),
}));
vi.mock("../../desktop", () => ({
  createDesktopStore: () => mocks,
  listen: mocks.listen,
}));
vi.mock("../Pages/ModInstallPage/ModInstallPage", () => ({
  default: (props: unknown) => {
    mocks.installPage(props);
    return null;
  },
}));
import Mod from "./Mod";

const mod: IMod = {
  name: "Example",
  id: "example",
  downloadCount: 0,
  version: "",
  dateModified: "",
  modType: ModType.Mod,
  source: ModSource.Modrinth,
  slug: "example",
  thumbnailUrls: [],
  url: "https://example.com",
  description: "Example mod",
  license: "MIT",
  modIconUrl: "https://example.com/icon.png",
  downloadable: true,
  showPreviousVersion: false,
  newVersion: null,
  deleteable: false,
  autoinstallable: true,
  selectable: false,
  modpack: null,
  selectUrl: null,
};

const context = {
  changeContent: vi.fn(),
  back: vi.fn(),
  setSnackbar: vi.fn(),
  setSnackbarNoState: vi.fn(),
};

function renderCard(installed: boolean) {
  return render(
    <ContentContext.Provider value={context}>
      <Mod mod={mod} modpack={undefined} className="" installed={installed} />
    </ContentContext.Provider>,
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  mocks.get.mockImplementation(
    async (key: string) =>
      ({
        lastUsedModpack: "Different Pack",
        lastUsedVersion: "1.12.2",
        lastUsedAPI: ModLoader.Forge,
      })[key],
  );
  mocks.installMod.mockResolvedValue(undefined);
  mocks.listen.mockImplementation(async () => vi.fn());
});

describe("Mod", () => {
  it("loads an icon only from a provider CDN, with no referrer", () => {
    const { container, rerender } = render(
      <Mod
        mod={{ ...mod, modIconUrl: "https://cdn.modrinth.com/icon.png" }}
        modpack={undefined}
        className=""
      />,
    );
    const img = container.querySelector("img");
    expect(img).toHaveAttribute("src", "https://cdn.modrinth.com/icon.png");
    expect(img).toHaveAttribute("referrerpolicy", "no-referrer");
    expect(img).toHaveAttribute("loading", "lazy");

    rerender(
      <Mod
        mod={{ ...mod, modIconUrl: "https://tracker.example/pixel.png" }}
        modpack={undefined}
        className=""
      />,
    );
    expect(container.querySelector("img")).toBeNull();
  });

  it("installs search results using their explicit target instead of stale global settings", async () => {
    render(
      <Mod
        mod={mod}
        modpack={undefined}
        className=""
        installTarget={{
          name: "Selected Pack",
          version: "1.21.1",
          modLoader: ModLoader.Fabric,
        }}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(mocks.installMod).toHaveBeenCalledWith({
      id: mod.id,
      minecraftVersion: "1.21.1",
      loader: ModLoader.Fabric,
      source: mod.source,
      modType: mod.modType,
      modpack: "Selected Pack",
    });
  });

  it("quick installs a resource pack into its explicit target", async () => {
    render(
      <Mod
        mod={{ ...mod, modType: ModType.ResourcePack }}
        modpack={undefined}
        className=""
        installTarget={{
          name: "Selected Pack",
          version: "1.21.1",
          modLoader: ModLoader.Fabric,
        }}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(mocks.installMod).toHaveBeenCalledWith({
      id: mod.id,
      minecraftVersion: "1.21.1",
      loader: ModLoader.Fabric,
      source: mod.source,
      modType: ModType.ResourcePack,
      modpack: "Selected Pack",
    });
  });

  it("quick installs a pack into the folder the search page chose", async () => {
    render(
      <Mod
        mod={{ ...mod, modType: ModType.ShaderPack }}
        modpack={undefined}
        className=""
        installLocation="prism:1"
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(mocks.installMod).toHaveBeenCalledTimes(1));
    expect(mocks.installMod).toHaveBeenCalledWith(
      expect.objectContaining({
        modType: ModType.ShaderPack,
        contentLocation: "prism:1",
      }),
    );
  });

  it("never routes a mod to a folder, whatever the search page chose", async () => {
    // A mod always follows its modpack; the host rejects a folder for one.
    render(
      <Mod
        mod={mod}
        modpack={undefined}
        className=""
        installLocation="prism:1"
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(mocks.installMod).toHaveBeenCalledTimes(1));
    expect(mocks.installMod.mock.calls[0][0].contentLocation).toBeUndefined();
  });

  it("sends no modpack for an untargeted resource pack rather than the saved one", async () => {
    render(
      <Mod
        mod={{ ...mod, modType: ModType.ResourcePack }}
        modpack={undefined}
        className=""
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(mocks.installMod).toHaveBeenCalledTimes(1));
    expect(mocks.installMod).toHaveBeenCalledWith({
      id: mod.id,
      minecraftVersion: "1.12.2",
      loader: ModLoader.Forge,
      source: mod.source,
      modType: ModType.ResourcePack,
      modpack: "",
    });
  });

  it("still falls back to the saved modpack for an untargeted mod", async () => {
    render(<Mod mod={mod} modpack={undefined} className="" />);
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(mocks.installMod).toHaveBeenCalledTimes(1));
    expect(mocks.installMod).toHaveBeenCalledWith({
      id: mod.id,
      minecraftVersion: "1.12.2",
      loader: ModLoader.Forge,
      source: mod.source,
      modType: ModType.Mod,
      modpack: "Different Pack",
    });
  });

  it("allows retrying a download after reading saved settings fails", async () => {
    let failed = false;
    mocks.get.mockImplementation(async (key: string) => {
      if (key === "lastUsedAPI" && !failed) {
        failed = true;
        throw "settingsUnavailable";
      }
      return undefined;
    });
    render(<Mod mod={mod} modpack={undefined} className="" />);
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(mocks.installMod).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(mocks.installMod).toHaveBeenCalledTimes(1));
  });

  it("opens the install page instead of installing when the mod is already in the pack", async () => {
    renderCard(true);
    await userEvent.click(screen.getByRole("button", { name: "Installed" }));
    expect(context.changeContent).toHaveBeenCalled();
    expect(mocks.installMod).not.toHaveBeenCalled();
  });

  it("opens the install page on the card's own target modpack", async () => {
    const target = {
      name: "Selected Pack",
      version: "1.21.1",
      modLoader: ModLoader.Fabric,
    };
    render(
      <ContentContext.Provider value={context}>
        <Mod
          mod={mod}
          modpack={undefined}
          className=""
          installed
          installTarget={target}
        />
      </ContentContext.Provider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Installed" }));
    render(context.changeContent.mock.calls[0][0].content);
    expect(mocks.installPage).toHaveBeenCalledWith(
      expect.objectContaining({ installTarget: target }),
    );
  });

  it("installs directly when the same mod is not in the pack", async () => {
    renderCard(false);
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(mocks.installMod).toHaveBeenCalledTimes(1));
  });

  it("reports a finished one-click install through onInstalled", async () => {
    const onInstalled = vi.fn();
    render(
      <Mod mod={mod} modpack={undefined} className="" onInstalled={onInstalled} />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(onInstalled).toHaveBeenCalledTimes(1));
  });

  it("does not report an install that failed", async () => {
    mocks.installMod.mockRejectedValue("noVersion");
    const onInstalled = vi.fn();
    render(
      <ContentContext.Provider value={context}>
        <Mod
          mod={mod}
          modpack={undefined}
          className=""
          onInstalled={onInstalled}
        />
      </ContentContext.Provider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(context.setSnackbar).toHaveBeenCalled());
    expect(onInstalled).not.toHaveBeenCalled();
  });

  it("explains a blocked third-party download instead of echoing the key", async () => {
    mocks.installMod.mockRejectedValue("thirdPartyDownloadDisabled");
    renderCard(false);

    await userEvent.click(screen.getByRole("button", { name: "Download" }));

    await waitFor(() =>
      expect(context.setSnackbar).toHaveBeenCalledWith(
        expect.objectContaining({
          message:
            "This mod's author doesn't allow downloads from third-party apps. Open it in the browser to download it manually.",
        }),
      ),
    );
  });

  it("lets a failed one-click install be retried after partial progress", async () => {
    const handlers: Record<string, (event: unknown) => void> = {};
    mocks.listen.mockImplementation(
      async (name: string, cb: (event: unknown) => void) => {
        handlers[name] = cb;
        return vi.fn();
      },
    );
    let fail!: (error: Error) => void;
    mocks.installMod.mockReturnValueOnce(
      new Promise((_, reject) => {
        fail = reject;
      }),
    );
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ContentContext.Provider value={context}>
        <Mod mod={mod} modpack={undefined} className="" />
      </ContentContext.Provider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Download" }));
    await waitFor(() => expect(handlers.modDownloadProgress).toBeDefined());
    act(() =>
      handlers.modDownloadProgress({ payload: { modId: mod.id, progress: 40 } }),
    );
    expect(await screen.findByRole("button", { name: "40%" })).toBeVisible();

    await act(async () => fail(new Error("network")));

    const retry = await screen.findByRole("button", { name: "Download" });
    await userEvent.click(retry);
    expect(mocks.installMod).toHaveBeenCalledTimes(2);
  });

  describe("delete", () => {
    const deletable = { ...mod, deleteable: true };
    const renderDeletable = (modpack: string | undefined) =>
      render(
        <ContentContext.Provider value={context}>
          <Mod mod={deletable} modpack={modpack} className="" />
        </ContentContext.Provider>,
      );

    it("removes the card once the mod is deleted from its modpack", async () => {
      mocks.deleteMod.mockResolvedValue(undefined);
      renderDeletable("Pack");
      await userEvent.click(screen.getByRole("button", { name: /delete/i }));
      expect(mocks.deleteMod).toHaveBeenCalledWith("Pack", mod.id);
      await waitFor(() => expect(screen.queryByText("Example")).toBeNull());
    });

    it("keeps the card and reports the error when the delete fails", async () => {
      vi.spyOn(console, "error").mockImplementation(() => {});
      mocks.deleteMod.mockRejectedValue("errorFileInUse");
      renderDeletable("Pack");
      await userEvent.click(screen.getByRole("button", { name: /delete/i }));
      await waitFor(() => expect(context.setSnackbar).toHaveBeenCalled());
      expect(screen.getByText("Example")).toBeVisible();
    });

    it("never offers to delete from a modpack it was not given", () => {
      renderDeletable(undefined);
      expect(screen.queryByRole("button", { name: /delete/i })).toBeNull();
    });
  });
});
