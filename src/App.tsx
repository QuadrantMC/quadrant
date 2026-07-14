/** @format */

import { useEffect, useRef, useState } from "react";
import {
  ContentContext,
  type IContentContext,
  ModLoader,
  ModType,
  type Page,
  type SnackbarHistoryItem,
  type SnackbarState,
} from "./intefaces";
import "./App.css";
import { I18nextProvider, useTranslation } from "react-i18next";
import ApplyPage from "./components/Pages/ApplyPage/Apply";
import SettingsPage from "./components/Pages/SettingsPage/Settings";
import quadrantLocale from "./i18n";
import * as md from "react-icons/md";
import CurrentModpackPage from "./components/Pages/CurrentModpackPage/CurrentModpackPage";
import { AnimatePresence, motion, MotionConfig } from "motion/react";
import SearchPage from "./components/Pages/SearchPage/SearchPage";
import { getMod, requestCheckForUpdates } from "./tools";
import ModInstallPage from "./components/Pages/ModInstallPage/ModInstallPage";
import AccountPage from "./components/Pages/AccountPage/AccountPage";
import ShareSyncPage from "./components/Pages/ShareSyncPage/ShareSyncPage";
import Button from "./components/core/Button";
import Notifications from "./components/shared/Notifications";
import {
  createDesktopStore,
  getCurrentDesktopWindow,
  installUpdate,
  invoke,
  isProductionBuild,
  listen,
  onOpenUrl,
  platform,
  ProgressBarStatus,
} from "./desktop";
import {
  applyUiScale,
  COMPACT_UI_SCALE,
  getAppliedUiScale,
  setUiScale,
  UI_SCALE_KEY,
  UI_SCALE_STEP,
} from "./uiScale";
import { resolveDeepLink } from "./deepLinks";
import { appendSnackbarHistory } from "./snackbar";

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
      icon: <md.MdCheck className="duration-0 w-6 h-6" />,
      style: " hover:bg-emerald-400 data-[selected=true]:bg-emerald-900 ",
      main: true,
    },
    {
      content: <CurrentModpackPage />,
      title: t("currentModpack"),
      name: "currentModpack",
      icon: <md.MdDescription className="duration-0 w-6 h-6" />,
      style: " hover:bg-blue-400 data-[selected=true]:bg-blue-900 ",
      main: true,
    },
    {
      content: <SearchPage />,
      title: t("search"),
      name: "search",
      icon: <md.MdSearch className="duration-0 w-6 h-6" />,
      style: " hover:bg-sky-400 data-[selected=true]:bg-sky-900 ",
      main: true,
    },
    {
      content: <ShareSyncPage />,
      title: t("importMods"),
      name: "shareSync",
      icon: <md.MdSync className="duration-0 w-6 h-6" />,
      style: " hover:bg-cyan-400 data-[selected=true]:bg-cyan-900 ",
      main: true,
    },
    {
      content: <AccountPage />,
      title: t("account"),
      name: "account",
      icon: <md.MdAccountCircle className="duration-0 w-6 h-6" />,
      style: " hover:bg-orange-400 data-[selected=true]:bg-orange-900 ",
      main: true,
    },
    {
      content: <SettingsPage />,
      title: t("settings"),
      name: "settings",
      icon: <md.MdSettings className="duration-0 w-6 h-6" />,
      style: " hover:bg-slate-400 data-[selected=true]:bg-slate-900 ",
      main: true,
    },
  ];
  const [page, setPage] = useState(pages[0]);
  const [content, setContent] = useState<Page>(pages[0]);
  const [updateDownloadProgress, setUpdateDownloadProgress] = useState(0);
  const configRef = useRef(createDesktopStore("config.json"));
  const config = configRef.current;
  const [contentHistory, setContentHistory] = useState<PageWithScroll[]>([]);
  const isLinuxRef = useRef(false);
  const [nativeDecorations, setNativeDecorations] = useState(false);

  const [snackBarHistory, setSnackbarHistory] = useState<SnackbarHistoryItem[]>(
    [],
  );

  const contentRef = useRef<HTMLDivElement | null>(null);
  const currentContentRef = useRef<Page>(content);

  useEffect(() => {
    currentContentRef.current = content;
  }, [content]);

  useEffect(() => {
    let isUnmounted = false;
    const disableContextMenu = (event: MouseEvent) => event.preventDefault();
    const cleanupFns: Array<() => void> = [];
    let requestUpdatesTimeout: ReturnType<typeof setTimeout> | null = null;

    const effect = async () => {
      if (await isProductionBuild()) {
        document.addEventListener("contextmenu", disableContextMenu);
      }

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
              className: "bg-emerald-600 rounded-4xl",
              message: (
                <span className="flex">
                  <span>{t("export")}</span>
                  <md.MdArchive className="w-6 h-6 mx-2" /> {progress}%
                </span>
              ),
              timeout: 15000,
            });
          } else {
            contextFunctions.setSnackbarNoState({
              message: (
                <span className="flex">
                  <span>{t("export")}</span>
                  <md.MdArchive className="w-6 h-6 mx-2" /> {progress}%
                </span>
              ),
              className: "bg-slate-700 rounded-4xl",
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
          const progress = Math.round(e.payload * 100);
          currentWindow.setProgressBar({
            progress: progress,
          });
          if (progress >= 100) {
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

      const [currentPlatform, lastPageIndex, savedUiScale, nativeDecorationsValue] =
        await Promise.all([
          platform(),
          config.get<number>("lastPage"),
          config.get<number>(UI_SCALE_KEY),
          config.get<boolean>("nativeDecorations"),
        ]);
      if (!isUnmounted) {
        applyUiScale(savedUiScale ?? COMPACT_UI_SCALE);
      }

      // Keeps the scale in sync when it changes elsewhere (settings sync).
      const uiScaleUnlisten = await config.onKeyChange<number>(
        UI_SCALE_KEY,
        (newValue) => {
          if (!isUnmounted) {
            applyUiScale(newValue ?? COMPACT_UI_SCALE);
          }
        },
      );
      if (isUnmounted) {
        uiScaleUnlisten();
      } else {
        cleanupFns.push(uiScaleUnlisten);
      }

      if (!isUnmounted) {
        const resolvedNativeDecorations = nativeDecorationsValue ?? false;
        const initialPage = pages[lastPageIndex ?? 0] ?? pages[0];
        isLinuxRef.current = currentPlatform === "linux";
        setNativeDecorations(resolvedNativeDecorations);
        await currentWindow.setDecorations(resolvedNativeDecorations);
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

      const nativeDecorationsUnlisten = await config.onKeyChange<boolean>(
        "nativeDecorations",
        async (newValue) => {
          if (!isUnmounted) {
            const resolved = newValue ?? false;
            setNativeDecorations(resolved);
            await currentWindow.setDecorations(resolved);
          }
        },
      );
      if (isUnmounted) {
        nativeDecorationsUnlisten();
      } else {
        cleanupFns.push(nativeDecorationsUnlisten);
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
        const showUnsupported = () => {
          contextFunctions.setSnackbar({
            message: t("unsupportedDownload"),
            className: "bg-red-700",
            timeout: 5000,
          });
        };
        for (const gottenUrl of urls) {
          try {
            const action = resolveDeepLink(gottenUrl);
            console.log("deep link action:", gottenUrl, action);
            await currentWindow.setEnabled(true);
            await currentWindow.setFocus();

            if (action.kind === "none") {
              continue;
            }

            if (action.kind === "unsupported") {
              showUnsupported();
              return;
            }

            if (action.kind === "oauthLogin") {
              const oAuthState = await config.get<string>("oauthState");
              if (action.providedState !== oAuthState) {
                return;
              }
              if (action.code === null) {
                return;
              }
              await invoke("oauth2_login", {
                code: action.code,
                redirectUri: action.redirectUri,
              });
              return;
            }

            if (action.kind === "importModpack") {
              const randomString = Math.random().toString(36).substring(2, 10);
              contextFunctions.changeContent({
                content: <ShareSyncPage sharedCode={action.code} />,
                name: randomString,
                icon: <md.MdSync className="duration-0 w-6 h-6" />,
                title: t("importMods"),
                style: "",
                main: false,
              });
              return;
            }

            const mod = await getMod(
              {
                deletable: false,
                id: action.modId,
                downloadable: true,
                showPreviousVersion: false,
                versionTarget: "",
                modpack: "",
                modLoader: ModLoader.Unknown,
                selectable: false,
                selectUrl: null,
              },
              action.source,
            );
            if (mod.modType === ModType.Unknown) {
              showUnsupported();
              return;
            }
            const randomString = Math.random().toString(36).substring(2, 10);
            contextFunctions.changeContent({
              content: <ModInstallPage mod={mod} fileId={action.fileId} />,
              name: randomString,
              icon: <></>,
              title: mod.name,
              style: "",
              main: false,
            });
            if (action.stopAfter) {
              return;
            }
          } catch (error) {
            console.error("Failed to handle deep link", error);
            showUnsupported();
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

  // Zoom shortcuts: Ctrl/Cmd with +, -, 0 and Ctrl + mouse wheel.
  useEffect(() => {
    const zoomBy = (direction: number) => {
      void setUiScale(getAppliedUiScale() + direction * UI_SCALE_STEP);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.ctrlKey && !event.metaKey) {
        return;
      }
      if (event.key === "+" || event.key === "=") {
        event.preventDefault();
        zoomBy(1);
      } else if (event.key === "-" || event.key === "_") {
        event.preventDefault();
        zoomBy(-1);
      } else if (event.key === "0") {
        event.preventDefault();
        void setUiScale(COMPACT_UI_SCALE);
      }
    };
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey) {
        return;
      }
      event.preventDefault();
      if (event.deltaY !== 0) {
        zoomBy(event.deltaY < 0 ? 1 : -1);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("wheel", onWheel);
    };
  }, []);

  const updateContentWithTransition = (update: () => void) => {
    if (
      !isLinuxRef.current &&
      typeof document.startViewTransition === "function"
    ) {
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
      const scrollPositionX = contentRef.current?.scrollLeft ?? 0;
      const scrollPositionY = contentRef.current?.scrollTop ?? 0;
      console.log(component);
      setContentHistory((previousHistory) => {
        const newHistory =
          previousHistory.length > 0
            ? [...previousHistory]
            : [
                {
                  page: currentContentRef.current,
                  scrollPositionX,
                  scrollPositionY,
                },
              ];
        const previousEntryIndex = newHistory.length - 1;
        newHistory[previousEntryIndex] = {
          ...newHistory[previousEntryIndex],
          scrollPositionX,
          scrollPositionY,
        };
        newHistory.push({
          page: component,
          scrollPositionX: 0,
          scrollPositionY: 0,
        });
        console.log(newHistory);
        return newHistory;
      });
      contentRef.current?.scrollTo({
        top: 0,
        left: 0,
        behavior: "instant",
      });
      currentContentRef.current = component;
      updateContentWithTransition(() => {
        setContent(component);
      });
    },
    changePage: (name) => {
      const newPage = pages.filter((pg) => pg.name === name);
      if (!newPage[0]) {
        return;
      }
      updateContentWithTransition(() => {
        setPage(newPage[0]);
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

      const id = Math.random().toString(36).substring(2, 10);
      setSnackbarHistory((prevHistory) =>
        appendSnackbarHistory(prevHistory, newSnackBarState, id),
      );
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
  }, [snackbarEnabled, snackbarState]);
  const currentWindow = getCurrentDesktopWindow();
  return (
    <I18nextProvider i18n={quadrantLocale}>
      <MotionConfig>
        <AnimatePresence>
          <ContentContext.Provider value={contextFunctions}>
            <main className="flex flex-1 p-0 h-screen w-screen disableSelect ">
              <div className="flex items-center justify-center ">
                <div className="w-fit mx-2 flex flex-col items-center justify-center border-slate-700 ">
                  {pages.map((p, i) => {
                    const isSelected = p.name == page.name;
                    return (
                      <Button
                        animate
                        fullRound
                        data-selected={isSelected}
                        className={
                          "text-center items-center justify-center flex flex-col align-center w-12 h-12 relative transition-colors duration-200 ease-linear font-extrabold my-1 " +
                          p.style +
                          (page === p ? "bg-slate-700" : "bg-slate-800")
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
                      </Button>
                    );
                  })}
                </div>
                <div className="border-2 h-svh border-slate-700"></div>
              </div>
              <div className="flex flex-1 flex-col text-base w-full overflow-y-auto">
                <div
                  data-tauri-drag-region
                  className="border-b-4 w-full border-slate-700 flex items-center shadow-2xl shadow-slate-900"
                >
                  <h1
                    data-tauri-drag-region
                    className="font-extrabold mt-2 h-full w-full text-lg"
                  >
                    <p className=" bg-slate-700 my-2 p-1.5 rounded-4xl w-fit mx-4 px-4 ">
                      {content.title}
                    </p>
                  </h1>
                  <div
                    data-tauri-drag-region
                    className="w-full items-center justify-end flex h-full mx-4"
                  >
                    {updateDownloadProgress !== 0 && (
                      <Button
                        className={
                          (updateDownloadProgress !== 1
                            ? "bg-slate-700 hover:bg-slate-600 "
                            : "bg-emerald-600 hover:bg-emerald-700") +
                          " mr-4 rounded-4xl p-1.5 px-4 "
                        }
                        onClick={async () => {
                          if (updateDownloadProgress === 1) {
                            await installUpdate();
                          }
                        }}
                      >
                        {updateDownloadProgress === 1 ? (
                          <div className="flex align-middle justify-center items-center place-content-center">
                            <p>{t("appUpdate")}</p>{" "}
                            <md.MdInstallDesktop className="ml-2 w-6" />
                          </div>
                        ) : (
                          (updateDownloadProgress * 100).toFixed(0) + "%"
                        )}
                      </Button>
                    )}
                    <div
                      className={
                        (nativeDecorations ? "" : "bg-slate-800 rounded-full ") +
                        "p-2 flex items-center justify-center"
                      }
                    >
                      <Notifications
                        snackBarHistory={snackBarHistory}
                        setSnackbarHistory={setSnackbarHistory}
                      />

                      {/* When native decorations are enabled the OS draws the
                          window controls, so hide our custom ones. */}
                      {!nativeDecorations && (
                        <>
                          <Button
                            fullRound
                            className="bg-slate-700 hover:bg-slate-600 mx-2"
                            onClick={async () => {
                              await currentWindow.minimize();
                            }}
                          >
                            <md.MdMinimize />
                          </Button>
                          <Button
                            fullRound
                            className="bg-slate-700 hover:bg-slate-600 ml-2"
                            onClick={async () => {
                              await currentWindow.hide();
                              // `setEnabled(false)` maps to the Windows-only
                              // EnableWindow API; it keeps the hidden window from
                              // being reactivated from the taskbar. On macOS it
                              // only greys the window out and prevents it from
                              // hiding into the tray, so limit it to Windows.
                              if ((await platform()) === "windows") {
                                await currentWindow.setEnabled(false);
                              }
                            }}
                          >
                            <md.MdClose />
                          </Button>
                        </>
                      )}
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
                        <md.MdClear className="w-4 h-4" />
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
