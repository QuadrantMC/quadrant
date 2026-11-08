/** @format */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ModSource, ModType, type IMod } from "./intefaces";
import quadrantLocale from "./i18n";

const hosts = vi.hoisted(() => ({
  configGet: vi.fn(),
  configSet: vi.fn(),
  configSave: vi.fn(),
  configDelete: vi.fn(),
  invoke: vi.fn(),
  clearProgressBar: vi.fn(),
  setProgressBar: vi.fn(),
  getMod: vi.fn(),
  listeners: {} as Record<string, (event: { payload: unknown }) => void>,
  openUrl: undefined as undefined | ((urls: string[]) => Promise<void>),
  mounts: [] as string[],
}));

vi.mock("./desktop", () => ({
  clearProgressBar: hosts.clearProgressBar,
  createDesktopStore: () => ({
    get: hosts.configGet,
    set: hosts.configSet,
    save: hosts.configSave,
    delete: hosts.configDelete,
    onKeyChange: vi.fn(async () => () => {}),
    onChange: vi.fn(async () => () => {}),
  }),
  getCurrentDesktopWindow: () => ({
    setProgressBar: hosts.setProgressBar,
    setDecorations: vi.fn(async () => {}),
    setEnabled: vi.fn(async () => {}),
    setFocus: vi.fn(async () => {}),
    minimize: vi.fn(),
    hide: vi.fn(),
  }),
  installUpdate: vi.fn(),
  invoke: hosts.invoke,
  isProductionBuild: async () => false,
  listen: async (
    event: string,
    callback: (event: { payload: unknown }) => void,
  ) => {
    hosts.listeners[event] = callback;
    return () => {};
  },
  onOpenUrl: async (callback: (urls: string[]) => Promise<void>) => {
    hosts.openUrl = callback;
    return () => {};
  },
  platform: async () => "windows",
}));
vi.mock("./tools", () => ({
  getMod: hosts.getMod,
  getQuadrantShareModpack: vi.fn(),
  requestCheckForUpdates: vi.fn(),
}));
vi.mock("./components/Pages/ApplyPage/Apply", () => ({
  default: () => <p>apply page</p>,
}));
vi.mock("./components/Pages/CurrentModpackPage/CurrentModpackPage", () => ({
  default: () => <p>current page</p>,
}));
vi.mock("./components/Pages/SearchPage/SearchPage", () => ({
  default: () => <p>search page</p>,
}));
vi.mock("./components/Pages/InstalledContentPage/InstalledContentPage", () => ({
  default: () => <p>installed page</p>,
}));
vi.mock("./components/Pages/AccountPage/AccountPage", () => ({
  default: () => <p>account page</p>,
}));
vi.mock("./components/Pages/SettingsPage/Settings", () => ({
  default: () => <p>settings page</p>,
}));
vi.mock("./components/Pages/ApplyPage/SharedModpackView", () => ({
  default: () => null,
}));
vi.mock("./components/Pages/ModInstallPage/ModInstallPage", async () => {
  const { useEffect } = await import("react");
  function InstallPageStub({ mod }: { mod: IMod }) {
    // Mount-only on purpose: a reused instance must not count as a new one.
    useEffect(() => {
      hosts.mounts.push(mod.id);
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);
    return <p>install page for {mod.name}</p>;
  }
  return { default: InstallPageStub };
});
// The bell lists the snackbar history; its counts show how often a message
// was raised.
vi.mock("./components/shared/Notifications", () => ({
  default: ({ snackBarHistory }: { snackBarHistory: { count: number }[] }) => (
    <ul>
      {snackBarHistory.map((item, index) => (
        <li key={index} data-testid="history">
          {item.count}
        </li>
      ))}
    </ul>
  ),
}));

import App from "./App";

function mod(id: string, modType = ModType.Mod): IMod {
  return {
    name: id,
    id,
    modType,
    source: ModSource.Modrinth,
    downloadCount: 0,
    version: "",
    dateModified: "",
    slug: id,
    thumbnailUrls: [],
    url: "",
    description: "",
    license: "",
    modIconUrl: "",
    downloadable: true,
    showPreviousVersion: false,
    newVersion: null,
    deleteable: false,
    autoinstallable: false,
    selectable: false,
    modpack: null,
    selectUrl: null,
  };
}

async function renderApp() {
  render(<App />);
  await waitFor(() => expect(hosts.openUrl).toBeTypeOf("function"));
  await screen.findByText("apply page");
}

const history = () =>
  screen.queryAllByTestId("history").map((item) => item.textContent);

beforeEach(() => {
  vi.clearAllMocks();
  hosts.mounts.length = 0;
  hosts.openUrl = undefined;
  for (const key of Object.keys(hosts.listeners)) delete hosts.listeners[key];
  hosts.configGet.mockResolvedValue(undefined);
  hosts.configSet.mockResolvedValue(undefined);
  hosts.configSave.mockResolvedValue(undefined);
  hosts.configDelete.mockResolvedValue(undefined);
  hosts.invoke.mockResolvedValue(undefined);
  hosts.clearProgressBar.mockResolvedValue(undefined);
  hosts.setProgressBar.mockResolvedValue(undefined);
  Element.prototype.scrollTo = vi.fn();
});

afterEach(async () => {
  await quadrantLocale.changeLanguage("en");
});

describe("App navigation", () => {
  const sidebarButton = (index: number) => screen.getAllByRole("button")[index];

  it("switches page at once and remembers it afterwards", async () => {
    let finishSave!: () => void;
    hosts.configSet.mockReturnValue(
      new Promise<void>((resolve) => {
        finishSave = resolve;
      }),
    );
    await renderApp();

    await userEvent.click(sidebarButton(1));

    expect(await screen.findByText("current page")).toBeInTheDocument();
    expect(hosts.configSet).toHaveBeenCalledWith(
      "lastPageName",
      "currentModpack",
    );
    finishSave();
  });

  it("still switches page when remembering it fails", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    hosts.configSet.mockRejectedValue(new Error("disk full"));
    await renderApp();

    await userEvent.click(sidebarButton(2));

    expect(await screen.findByText("search page")).toBeInTheDocument();
    await waitFor(() => expect(error).toHaveBeenCalled());
    error.mockRestore();
  });
});

describe("App deep links", () => {
  async function open(url: string) {
    await act(async () => hosts.openUrl?.([url]));
  }

  it("remounts the install page when a link for another mod arrives", async () => {
    hosts.getMod
      .mockResolvedValueOnce(mod("sodium"))
      .mockResolvedValueOnce(mod("iris"));
    await renderApp();

    await open("modrinth://mod/sodium");
    expect(await screen.findByText("install page for sodium")).toBeVisible();
    await open("modrinth://mod/iris");
    expect(await screen.findByText("install page for iris")).toBeVisible();

    expect(hosts.mounts).toEqual(["sodium", "iris"]);
  });

  it.each([ModType.Modpack, ModType.DataPack, ModType.Unknown])(
    "refuses a %s, which the install page could never install",
    async (modType) => {
      hosts.getMod.mockResolvedValue(mod("pack", modType));
      await renderApp();

      await open("curseforge://install?addonId=1");

      expect(
        await screen.findByText("This download is not supported"),
      ).toBeInTheDocument();
      expect(screen.queryByText(/install page for/)).toBeNull();
    },
  );

  it("words its messages in the language chosen in the settings", async () => {
    hosts.configGet.mockImplementation(async (key: string) =>
      key === "locale" ? "tr" : undefined,
    );
    await renderApp();
    await waitFor(() => expect(quadrantLocale.language).toBe("tr"));

    await open("curseforge://install?addonId=1/files/2");

    expect(
      await screen.findByText("Bu tür bir proje desteklenmiyor."),
    ).toBeInTheDocument();
  });

  describe("sign-in redirect", () => {
    // The sign-in start stores the state and PKCE verifier; the redirect
    // consumes them.
    function storeLogin(values: Record<string, string>) {
      hosts.configGet.mockImplementation(async (key: string) => values[key]);
      hosts.configDelete.mockImplementation(async (key: string) => {
        delete values[key];
      });
    }

    it("hands the stored verifier to the host and spends the state", async () => {
      storeLogin({ oauthState: "st", oauthVerifier: "ver" });
      await renderApp();

      await open("quadrantnext://login?state=st&code=abc");

      expect(hosts.invoke).toHaveBeenCalledExactlyOnceWith("oauth2_login", {
        code: "abc",
        redirectUri: "quadrantnext://login",
        codeVerifier: "ver",
      });
      expect(hosts.configDelete).toHaveBeenCalledWith("oauthState");
      expect(hosts.configDelete).toHaveBeenCalledWith("oauthVerifier");
    });

    it("rejects a replay of the same redirect", async () => {
      storeLogin({ oauthState: "st", oauthVerifier: "ver" });
      await renderApp();

      await open("quadrantnext://login?state=st&code=abc");
      await open("quadrantnext://login?state=st&code=abc");

      expect(hosts.invoke).toHaveBeenCalledTimes(1);
    });

    it("rejects a redirect whose state is not the one it started", async () => {
      storeLogin({ oauthState: "st", oauthVerifier: "ver" });
      await renderApp();

      await open("quadrantnext://login?state=other&code=abc");

      expect(hosts.invoke).not.toHaveBeenCalled();
      expect(hosts.configDelete).not.toHaveBeenCalled();
    });
  });

  it("refuses a link whose id would reach another provider endpoint", async () => {
    await renderApp();
    await open("curseforge://install?addonId=1/files/2");
    expect(
      await screen.findByText("This download is not supported"),
    ).toBeInTheDocument();
    expect(hosts.getMod).not.toHaveBeenCalled();
  });
});

describe("App sync warnings", () => {
  const warn = (payload: Record<string, unknown>) =>
    act(async () => hosts.listeners.modpackSyncWarning({ payload }));

  it("tells the user an update was held back for review", async () => {
    await renderApp();
    await waitFor(() =>
      expect(hosts.listeners.modpackSyncWarning).toBeTypeOf("function"),
    );

    await warn({
      modpackId: "id",
      name: "Survival",
      applied: false,
      onlineMods: ["Linked"],
      unverifiedMods: [],
    });

    // The snackbar fades in, so it is not visible the moment it is found.
    const message = await screen.findByText(
      "An update to Survival needs your review",
    );
    await waitFor(() => expect(message).toBeVisible());
  });

  it("warns how many files of an applied update could not be verified", async () => {
    await renderApp();
    await waitFor(() =>
      expect(hosts.listeners.modpackSyncWarning).toBeTypeOf("function"),
    );

    await warn({
      modpackId: "id",
      name: "Survival",
      applied: true,
      onlineMods: [],
      unverifiedMods: ["A", "B"],
    });

    const message = await screen.findByText(
      "Survival was updated, but Quadrant couldn't verify 2 of its files.",
    );
    await waitFor(() => expect(message).toBeVisible());
  });
});

describe("App transfer progress", () => {
  const emit = (event: string, payload: number) =>
    act(async () => hosts.listeners[event]({ payload }));

  it("hands the taskbar back when a modpack download finishes", async () => {
    await renderApp();
    await waitFor(() =>
      expect(hosts.listeners.modpackDownloadProgress).toBeTypeOf("function"),
    );

    await emit("modpackDownloadProgress", 0.5);
    expect(hosts.setProgressBar).toHaveBeenLastCalledWith({ progress: 50 });
    await emit("modpackDownloadProgress", 1);
    expect(hosts.clearProgressBar).toHaveBeenCalledTimes(1);
  });

  it("hands the taskbar back when an update download restarts from zero", async () => {
    await renderApp();
    await waitFor(() =>
      expect(hosts.listeners.updateDownloadProgress).toBeTypeOf("function"),
    );

    await emit("updateDownloadProgress", 0.4);
    await emit("updateDownloadProgress", 0);

    expect(hosts.clearProgressBar).toHaveBeenCalledTimes(1);
  });

  it("reports an export as finished once even when the host says so twice", async () => {
    await renderApp();
    await waitFor(() =>
      expect(hosts.listeners.quadrantExportProgress).toBeTypeOf("function"),
    );

    await emit("quadrantExportProgress", 0.5);
    await emit("quadrantExportProgress", 1);
    await emit("quadrantExportProgress", 1);

    // Only the finished message is raised through the history: the running
    // one does not count as a notification.
    expect(history()).toEqual(["1"]);
    expect(hosts.clearProgressBar).toHaveBeenCalled();
  });

  it("reports an export whose first event already completes it", async () => {
    await renderApp();
    await waitFor(() =>
      expect(hosts.listeners.quadrantExportStarted).toBeTypeOf("function"),
    );

    await emit("quadrantExportProgress", 1);
    await emit("quadrantExportStarted", 0);
    await emit("quadrantExportProgress", 1);

    expect(history()).toEqual(["2"]);
  });

  it("reports the next export as finished too", async () => {
    await renderApp();
    await waitFor(() =>
      expect(hosts.listeners.quadrantExportProgress).toBeTypeOf("function"),
    );

    await emit("quadrantExportProgress", 0.5);
    await emit("quadrantExportProgress", 1);
    await emit("quadrantExportProgress", 0.2);
    await emit("quadrantExportProgress", 1);

    expect(history()).toEqual(["2"]);
  });
});
