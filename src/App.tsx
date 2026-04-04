/** @format */

import { useEffect, useRef, useState } from "react";
import {
  ContentContext,
  IContentContext,
  ModLoader,
  ModSource,
  ModType,
  Page,
  SnackbarHistoryItem,
  SnackbarState,
} from "./intefaces";
import "./App.css";
import { I18nextProvider, useTranslation } from "react-i18next";
import ApplyPage from "./components/Pages/ApplyPage/Apply";
import SettingsPage from "./components/Pages/SettingsPage/Settings";
import quadrantLocale from "./i18n";
import { LazyStore } from "@tauri-apps/plugin-store";
import {
  MdAccountCircle,
  MdArchive,
  MdCheck,
  MdClear,
  MdClose,
  MdDescription,
  MdInstallDesktop,
  MdMinimize,
  MdSearch,
  MdSettings,
  MdSync,
} from "react-icons/md";
import CurrentModpackPage from "./components/Pages/CurrentModpackPage/CurrentModpackPage";
import { AnimatePresence, motion, MotionConfig } from "motion/react";
import SearchPage from "./components/Pages/SearchPage/SearchPage";
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { getCurrentWindow, ProgressBarStatus } from "@tauri-apps/api/window";
import { getMod, requestCheckForUpdates } from "./tools";
import ModInstallPage from "./components/Pages/ModInstallPage/ModInstallPage";
import AccountPage from "./components/Pages/AccountPage/AccountPage";
import { invoke } from "@tauri-apps/api/core";
import ShareSyncPage from "./components/Pages/ShareSyncPage/ShareSyncPage";
import Button from "./components/core/Button";
import { listen } from "@tauri-apps/api/event";
import Notifications from "./components/shared/Notifications";
import { platform } from "@tauri-apps/plugin-os";

interface PageWithScroll {
  scrollPositionX: number;
  scrollPositionY: number;
  page: Page;
}

function App() {
  const { t } = useTranslation();
  const pages: Page[] = [
    {
      content: <ApplyPage />,
      title: t("apply"),
      name: "apply",
      icon: <MdCheck className="duration-0 w-8 h-8" />,
      style: " hover:bg-emerald-400 data-[selected=true]:bg-emerald-900 ",
      main: true,
    },
    {
      content: <CurrentModpackPage />,
      title: t("currentModpack"),
      name: "currentModpack",
      icon: <MdDescription className="duration-0 w-8 h-8" />,
      style: " hover:bg-blue-400 data-[selected=true]:bg-blue-900 ",
      main: true,
    },
    {
      content: <SearchPage />,
      title: t("search"),
      name: "search",
      icon: <MdSearch className="duration-0 w-8 h-8" />,
      style: " hover:bg-sky-400 data-[selected=true]:bg-sky-900 ",
      main: true,
    },
    {
      content: <ShareSyncPage />,
      title: t("importMods"),
      name: "shareSync",
      icon: <MdSync className="duration-0 w-8 h-8" />,
      style: " hover:bg-cyan-400 data-[selected=true]:bg-cyan-900 ",
      main: true,
    },
    {
      content: <AccountPage />,
      title: t("account"),
      name: "account",
      icon: <MdAccountCircle className="duration-0 w-8 h-8" />,
      style: " hover:bg-orange-400 data-[selected=true]:bg-orange-900 ",
      main: true,
    },
    {
      content: <SettingsPage />,
      title: t("settings"),
      name: "settings",
      icon: <MdSettings className="duration-0 w-8 h-8" />,
      style: " hover:bg-gray-700 data-[selected=true]:bg-black/25 ",
      main: true,
    },
  ];
  const [page, setPage] = useState(pages[0]);
  const [content, setContent] = useState<Page>(pages[0]);
  const [updateDownloadProgress, setUpdateDownloadProgress] = useState(0);
  const configRef = useRef<LazyStore | null>(null);
  if (configRef.current === null) {
    configRef.current = new LazyStore("config.json");
  }
  const config = configRef.current!;
  const [contentHistory, setContentHistory] = useState<PageWithScroll[]>([]);
  const [extendedNavigation, setExtendedNavigation] = useState(false);
  const [isLinux, setIsLinux] = useState(false);

  const [snackBarHistory, setSnackbarHistory] = useState<SnackbarHistoryItem[]>(
    [],
  );

  const contentRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let isUnmounted = false;
    const disableContextMenu = (event: MouseEvent) => event.preventDefault();
    const cleanupFns: Array<() => void> = [];
    let requestUpdatesTimeout: ReturnType<typeof setTimeout> | null = null;

    const effect = async () => {
      const updateDownloadUnlisten = await listen(
        "updateDownloadProgress",
        (e: any) => {
          if (isUnmounted) {
            return;
          }
          const progressValue = Number(e.payload);
          setUpdateDownloadProgress(progressValue);
          currentWindow.setProgressBar({
            progress: Math.round(progressValue * 100),
          });
          if (progressValue === 1) {
            currentWindow.setProgressBar({
              progress: 0,
              status: ProgressBarStatus.None,
            });
          }
        },
      );
      if (isUnmounted) {
        updateDownloadUnlisten();
      } else {
        cleanupFns.push(updateDownloadUnlisten);
      }

      const disableRightClickUnlisten = await listen(
        "disableRightClick",
        () => {
          document.addEventListener("contextmenu", disableContextMenu);
        },
      );
      if (isUnmounted) {
        disableRightClickUnlisten();
      } else {
        cleanupFns.push(() => {
          disableRightClickUnlisten();
          document.removeEventListener("contextmenu", disableContextMenu);
        });
      }

      const exportProgressUnlisten = await listen(
        "quadrantExportProgress",
        async (e: any) => {
          if (isUnmounted) {
            return;
          }
          console.log("Raw progress: " + e.payload);
          const progress = Math.round(e.payload * 100);

          console.log(progress);

          currentWindow.setProgressBar({ progress: progress });
          if (e.payload === 1) {
            currentWindow.setProgressBar({
              progress: 0,
              status: ProgressBarStatus.None,
            });
            contextFunctions.setSnackbar({
              className: "bg-emerald-700 rounded-4xl",
              message: (
                <span className="flex">
                  <span>{t("export")}</span>
                  <MdArchive className="w-6 h-6 mx-2" /> {progress}%
                </span>
              ),
              timeout: 15000,
            });
          } else {
            contextFunctions.setSnackbarNoState({
              message: (
                <span className="flex">
                  <span>{t("export")}</span>
                  <MdArchive className="w-6 h-6 mx-2" /> {progress}%
                </span>
              ),
              className: "bg-gray-700 rounded-4xl",
              timeout: 500000,
            });
          }
        },
      );
      if (isUnmounted) {
        exportProgressUnlisten();
      } else {
        cleanupFns.push(exportProgressUnlisten);
      }

      const modpackDownloadUnlisten = await listen(
        "modpackDownloadProgress",
        (e: any) => {
          if (isUnmounted) {
            return;
          }
          const progress = Math.round(e.payload);
          currentWindow.setProgressBar({
            progress: progress,
          });
          if (progress === 1) {
            currentWindow.setProgressBar({
              progress: 0,
              status: ProgressBarStatus.None,
            });
          }
        },
      );
      if (isUnmounted) {
        modpackDownloadUnlisten();
      } else {
        cleanupFns.push(modpackDownloadUnlisten);
      }

      const [currentPlatform, extendedNavigationValue, lastPageIndex] =
        await Promise.all([
          platform(),
          config.get<boolean>("extendedNavigation"),
          config.get<number>("lastPage"),
        ]);

      if (!isUnmounted) {
        const resolvedExtendedNavigation = extendedNavigationValue ?? false;
        const initialPage = pages[lastPageIndex ?? 0] ?? pages[0];
        setIsLinux(currentPlatform === "linux");
        setExtendedNavigation(resolvedExtendedNavigation);
        setPage(initialPage);
        setContent(initialPage);
        setContentHistory([
          {
            page: initialPage,
            scrollPositionX: 0,
            scrollPositionY: 0,
          },
        ]);
      }

      const extendedNavigationUnlisten = await config.onKeyChange<boolean>(
        "extendedNavigation",
        async (newValue) => {
          if (!isUnmounted) {
            setExtendedNavigation(newValue ?? false);
          }
        },
      );
      if (isUnmounted) {
        extendedNavigationUnlisten();
      } else {
        cleanupFns.push(extendedNavigationUnlisten);
      }

      const configChangeUnlisten = await config.onChange(async (key) => {
        if (key === "lastSettingsUpdated") {
          return;
        }
        await config.set("lastSettingsUpdated", new Date().toISOString());
      });
      if (isUnmounted) {
        configChangeUnlisten();
      } else {
        cleanupFns.push(configChangeUnlisten);
      }

      const locale = await config.get<string>("locale");
      if (locale) {
        await quadrantLocale.changeLanguage(locale);
      }

      const deepLinkUnlisten = await onOpenUrl(async (urls) => {
        if (isUnmounted) {
          return;
        }
        console.log("deep link:", urls);
        for (const gottenUrl of urls) {
          const url = URL.parse(gottenUrl);
          const actionType = url?.protocol;
          console.log("url:", url);
          await currentWindow.setFocus();
          console.log("url protocol:", actionType);
          if (actionType === "curseforge:") {
            const action = url!.pathname.replace("/", "");
            console.log("CurseForge action:", action);
            if (action.includes("/install\\")) {
              console.log("CurseForge action is not install");
              contextFunctions.setSnackbar({
                message: t("unsupportedDownload"),
                className: "bg-red-500 text-white",
                timeout: 5000,
              });
              return;
            }
            console.log("Getting mod");
            const modId = url!.searchParams.get("addonId") ?? "";
            const fileId = url!.searchParams.get("fileId") ?? undefined;
            const mod = await getMod(
              {
                deletable: false,
                id: modId,
                downloadable: true,
                showPreviousVersion: false,
                versionTarget: "",
                modpack: "",
                modLoader: ModLoader.Unknown,
                selectable: false,
                selectUrl: null,
              },
              ModSource.CurseForge,
            );
            if (mod.modType === ModType.Unknown) {
              contextFunctions.setSnackbar({
                message: t("unsupportedDownload"),
                className: "bg-red-500 text-white",
                timeout: 5000,
              });
              return;
            }
            const randomString = Math.random().toString(36).substring(2, 10);

            contextFunctions.changeContent({
              content: <ModInstallPage mod={mod} fileId={fileId} />,
              name: randomString,
              icon: <></>,
              title: mod.name,
              style: "",
              main: false,
            });
          } else if (actionType === "modrinth:") {
            console.log(url!.pathname.split("/"));

            const action = url?.pathname.split("/")[2] ?? url?.host ?? "";

            console.log("Modrinth action:", action);
            if (
              action.includes("mod") ||
              action.includes("resourcepack") ||
              action.includes("shader")
            ) {
              console.log("Getting mod");
              // This gets the slug, not the ID, but it doesn't matter for Modrinth
              let modId = "";
              url?.pathname.split("/").forEach((val) => {
                if (val !== "") {
                  modId = val;
                }
              });
              console.log("Modrinth mod ID: ", modId);
              const mod = await getMod(
                {
                  deletable: false,
                  id: modId,
                  downloadable: true,
                  showPreviousVersion: false,
                  versionTarget: "",
                  modpack: "",
                  modLoader: ModLoader.Unknown,
                  selectable: false,
                  selectUrl: null,
                },
                ModSource.Modrinth,
              );
              if (mod.modType === ModType.Unknown) {
                contextFunctions.setSnackbar({
                  message: t("unsupportedDownload"),
                  className: "bg-red-500 text-white",
                  timeout: 5000,
                });
                return;
              }
              // Random string
              const randomString = Math.random().toString(36).substring(2, 10);

              contextFunctions.changeContent({
                content: <ModInstallPage mod={mod} />,
                name: randomString,
                icon: <></>,
                title: mod.name,
                style: "",
                main: false,
              });
              return;
            }
            console.log("Modrinth action is not supported");
            contextFunctions.setSnackbar({
              message: t("unsupportedDownload"),
              className: "bg-red-500 text-white",
              timeout: 5000,
            });
            return;
          } else if (actionType === "quadrantnext:") {
            const actions = url!.pathname.split("/");
            console.log(actions);
            if (!actions.includes("login") && url!.host !== "login") {
              console.log("Not login");
              return;
            }

            const oAuthState = await config.get<string>("oauthState");

            const providedState = url!.searchParams.get("state");
            console.log("State: " + oAuthState);
            console.log("Provided state: " + providedState);
            if (providedState !== oAuthState) {
              return;
            }
            const code = url!.searchParams.get("code");
            console.log("Code: " + code);
            if (code === null) {
              return;
            }
            const redirectUri = gottenUrl.split("#")[0].split("?")[0];
            await invoke("oauth2_login", {
              code: code,
              redirectUri: redirectUri,
            });
          }
        }
      });
      if (isUnmounted) {
        deepLinkUnlisten();
      } else {
        cleanupFns.push(deepLinkUnlisten);
      }
      requestUpdatesTimeout = setTimeout(() => {
        void requestCheckForUpdates();
      }, 10000);
    };
    effect().catch((error) => {
      console.error(error);
    });
    return () => {
      isUnmounted = true;
      if (requestUpdatesTimeout !== null) {
        clearTimeout(requestUpdatesTimeout);
      }
      while (cleanupFns.length > 0) {
        const cleanup = cleanupFns.pop();
        try {
          cleanup?.();
        } catch (error) {
          console.error(error);
        }
      }
      document.removeEventListener("contextmenu", disableContextMenu);
    };
  }, []);

  const updateContentWithTransition = (update: () => void) => {
    if (!isLinux && typeof document.startViewTransition === "function") {
      document.startViewTransition(update);
      return;
    }

    update();
  };

  const contextFunctions: IContentContext = {
    back: async () => {
      // If there's no previous history, do nothing.
      if (contentHistory.length < 2) return;

      // Create a copy of the history and remove the current entry.
      const newHistory = [...contentHistory];
      newHistory.pop();

      // The new last item is the previous page.
      const previousEntry = newHistory[newHistory.length - 1];

      // Update state with the previous page.
      updateContentWithTransition(() => {
        setContent(previousEntry.page);
      });
      setContentHistory(newHistory);

      // Wait a short time to ensure the new content is rendered before scrolling.
      setTimeout(() => {
        contentRef.current?.scrollTo({
          top: Math.round(previousEntry.scrollPositionY),
          left: Math.round(previousEntry.scrollPositionX),
          behavior: "smooth",
        });
      }, 50);
    },
    changeContent: (component) => {
      const newHistory = [...contentHistory];
      console.log(component);
      newHistory.push({
        page: component,
        scrollPositionX: 0,
        scrollPositionY: 0,
      });
      newHistory[newHistory.length - 2].scrollPositionX =
        contentRef.current?.scrollLeft ?? 0;
      newHistory[newHistory.length - 2].scrollPositionY =
        contentRef.current?.scrollTop ?? 0;

      setContentHistory(newHistory);
      contentRef.current?.scrollTo({
        top: 0,
        left: 0,
        behavior: "instant",
      });
      console.log(newHistory);
      updateContentWithTransition(() => {
        setContent(component);
      });
    },
    changePage: (name) => {
      const newPage = pages.filter((pg) => pg.name === name);
      updateContentWithTransition(() => {
        setContent(newPage[0]);
      });
      const newHistory = [...contentHistory];
      newHistory.push({
        page: newPage[0],
        scrollPositionX: 0,
        scrollPositionY: 0,
      });
      setContentHistory(newHistory);
      contentRef.current?.scrollTo({
        top: 0,
        left: 0,
        behavior: "instant",
      });
    },
    setSnackbar(newSnackBarState) {
      setSnackbarState(newSnackBarState);
      setSnackbarEnabled(true);

      // Convert message to string for comparison (handles React nodes)
      const messageKey =
        typeof newSnackBarState.message === "string"
          ? newSnackBarState.message
          : JSON.stringify(newSnackBarState.message);

      setSnackbarHistory((prevHistory) => {
        // Check if there's an existing notification with the same message
        const existingIndex = prevHistory.findIndex((item) => {
          const existingKey =
            typeof item.message === "string"
              ? item.message
              : JSON.stringify(item.message);
          return (
            existingKey === messageKey &&
            item.className === newSnackBarState.className
          );
        });

        let newHistory: SnackbarHistoryItem[];

        if (existingIndex !== -1) {
          // Increment count of existing notification and move it to the end
          const existingItem = prevHistory[existingIndex];
          newHistory = [
            ...prevHistory.slice(0, existingIndex),
            ...prevHistory.slice(existingIndex + 1),
            { ...existingItem, count: existingItem.count + 1 },
          ];
        } else {
          // Add new notification with count of 1
          newHistory = [
            ...prevHistory,
            {
              ...newSnackBarState,
              count: 1,
              id: Math.random().toString(36).substring(2, 10),
            },
          ];
        }

        // Limit to 5 most recent grouped notifications
        if (newHistory.length > 5) {
          newHistory = newHistory.slice(-5);
        }

        return newHistory;
      });
    },
    setSnackbarNoState(newSnackBarState) {
      setSnackbarState(newSnackBarState);
      setSnackbarEnabled(true);
    },
  };

  const [snackbarState, setSnackbarState] = useState<SnackbarState>({
    message: "",
    className: "hidden",
    timeout: 0,
  });

  const [snackbarEnabled, setSnackbarEnabled] = useState<boolean>(false);

  useEffect(() => {
    if (snackbarEnabled) {
      const timeoutId = setTimeout(() => {
        setSnackbarEnabled(false);
      }, snackbarState.timeout);
      return () => {
        clearTimeout(timeoutId);
      };
    }
  }, [snackbarEnabled, snackbarState.timeout]);
  const currentWindow = getCurrentWindow();
  return (
    <I18nextProvider i18n={quadrantLocale}>
      <MotionConfig>
        <AnimatePresence>
          <ContentContext.Provider value={contextFunctions}>
            <main className="flex flex-1 p-0 h-screen w-screen disableSelect ">
              <div className="flex items-center justify-center ">
                <div className="w-16 min-w-min mx-2 flex flex-col items-center justify-center border-slate-700 ">
                  {pages.map((p, i) => {
                    const isSelected = p.name == page.name;
                    return (
                      <Button
                        animate
                        data-selected={isSelected}
                        className={
                          "text-center items-center justify-center flex flex-col align-center w-full min-w-fit wrap-break-word relative min-h-fit transition-colors duration-200 ease-linear font-extrabold py-4 p-1 my-1 rounded-4xl " +
                          p.style +
                          (page === p ? "bg-slate-600" : "bg-slate-800")
                        }
                        key={i}
                        onClick={async () => {
                          await config.set("lastPage", i);
                          await config.save();
                          setPage(p);
                          setContent(p);
                          setContentHistory([
                            {
                              page: p,
                              scrollPositionX:
                                contentRef.current?.scrollLeft ?? 0,
                              scrollPositionY:
                                contentRef.current?.scrollTop ?? 0,
                            },
                          ]);
                          contentRef.current?.scrollTo({
                            top: 0,
                            left: 0,
                            behavior: "instant",
                          });
                        }}
                      >
                        <div className="grid place-content-center ">
                          {p.icon}
                        </div>
                        <AnimatePresence>
                          {extendedNavigation && (
                            <motion.p
                              initial={{ opacity: 0 }}
                              animate={{ opacity: 1 }}
                              exit={{ opacity: 0 }}
                              className="overflow-hidden text-xs wrap-break-word w-fit"
                            >
                              {p.title}
                            </motion.p>
                          )}
                        </AnimatePresence>
                      </Button>
                    );
                  })}
                </div>
                <div className="border-2 h-svh border-slate-700"></div>
              </div>
              <div className="flex flex-1 flex-col text-2xl w-full overflow-y-auto">
                <div
                  data-tauri-drag-region
                  className="border-b-4 w-full border-slate-700 flex items-center shadow-2xl shadow-slate-900"
                >
                  <h1
                    data-tauri-drag-region
                    className="font-extrabold mt-4 h-full w-full"
                  >
                    <p className=" bg-slate-700 my-4 p-2 rounded-4xl w-fit mx-4 px-6 ">
                      {content.title}
                    </p>
                  </h1>
                  <div
                    data-tauri-drag-region
                    className="w-full items-center justify-end flex h-full mx-8"
                  >
                    {updateDownloadProgress !== 0 && (
                      <Button
                        className={
                          (updateDownloadProgress !== 1
                            ? "bg-slate-700 hover:bg-slate-600 "
                            : "bg-emerald-700 hover:bg-emerald-800") +
                          " mr-4 rounded-4xl p-2.5 px-6 "
                        }
                        onClick={async () => {
                          if (updateDownloadProgress === 1) {
                            await invoke("install_update");
                          }
                        }}
                      >
                        {updateDownloadProgress === 1 ? (
                          <div className="flex align-middle justify-center items-center place-content-center">
                            <p>{t("appUpdate")}</p>{" "}
                            <MdInstallDesktop className="ml-2 w-6" />
                          </div>
                        ) : (
                          (updateDownloadProgress * 100).toFixed(0) + "%"
                        )}
                      </Button>
                    )}
                    <div className="bg-slate-800 p-2 flex rounded-full items-center justify-center">
                      <Notifications
                        snackBarHistory={snackBarHistory}
                        setSnackbarHistory={setSnackbarHistory}
                      />

                      <Button
                        fullRound
                        className="bg-slate-700 hover:bg-slate-600 mx-2"
                        onClick={async () => {
                          await currentWindow.minimize();
                        }}
                      >
                        <MdMinimize />
                      </Button>
                      <Button
                        fullRound
                        className="bg-slate-700 hover:bg-slate-600 ml-2"
                        onClick={async () => {
                          await currentWindow.hide();
                          await currentWindow.setEnabled(false);
                        }}
                      >
                        <MdClose />
                      </Button>
                    </div>
                  </div>
                </div>
                <motion.div
                  initial={{ y: 24, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  exit={{ y: 24, opacity: 0 }}
                  layoutScroll
                  className="h-full overflow-y-auto transform-gpu backface-hidden will-change-[transform,opacity]"
                  transition={{ type: "keyframes", duration: 0.1 }}
                  // key={content.name}
                  ref={contentRef}
                >
                  {content.main !== true && content.content}
                  <div
                    className={
                      "h-full content-main " +
                      (content.main === true && content.name === page.name
                        ? ""
                        : "hidden")
                    }
                  >
                    <AnimatePresence>{page.content}</AnimatePresence>
                  </div>
                </motion.div>
              </div>
              {/* Snackbar */}
              <AnimatePresence>
                {snackbarEnabled && (
                  <motion.div
                    initial={{ opacity: 0, y: 24 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: 16 }}
                    className={
                      "bottom-8 left-8 fixed w-max h-max p-4 rounded-4xl flex flex-col items-center justify-center font-bold text-slate-50 transform-gpu backface-hidden will-change-[transform,opacity] " +
                      snackbarState.className
                    }
                  >
                    <div className="flex items-center gap-3">
                      <p>{snackbarState.message}</p>
                      <Button
                        fullRound
                        className="bg-slate-900/50 hover:bg-slate-900/70"
                        onClick={() => {
                          setSnackbarEnabled(false);
                        }}
                      >
                        <MdClear className="w-4 h-4" />
                      </Button>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </main>
          </ContentContext.Provider>
        </AnimatePresence>
      </MotionConfig>
    </I18nextProvider>
  );
}

export default App;
