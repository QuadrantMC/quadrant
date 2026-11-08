/** @format */

import {
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ContentContext,
  type IContentContext,
  ModLoader,
  type Page,
  type SnackbarHistoryItem,
  type SnackbarState,
} from "./intefaces";
import { type PageWithScroll, popContent, pushContent } from "./contentHistory";
import "./App.css";
import { I18nextProvider, useTranslation } from "react-i18next";
import ApplyPage from "./components/Pages/ApplyPage/Apply";
import quadrantLocale from "./i18n";
import {
  MdAccountCircle,
  MdArchive,
  MdCheck,
  MdClear,
  MdClose,
  MdDescription,
  MdFolder,
  MdInstallDesktop,
  MdMinimize,
  MdSearch,
  MdSettings,
} from "react-icons/md";
import { AnimatePresence, motion, MotionConfig } from "motion/react";
import {
  getMod,
  getQuadrantShareModpack,
  requestCheckForUpdates,
} from "./tools";
import Button from "./components/core/Button";
import CircularProgress from "./components/core/CircularProgress";
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
  clearProgressBar,
  type UpdateSwitchPayload,
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
import { isInstallable } from "./contentLocations";
import { appendSnackbarHistory } from "./snackbar";

// Pages load on first use, so the window opens without parsing all of them.
const SettingsPage = lazy(() => import("./components/Pages/SettingsPage/Settings"));
const CurrentModpackPage = lazy(() => import("./components/Pages/CurrentModpackPage/CurrentModpackPage"));
const InstalledContentPage = lazy(() => import("./components/Pages/InstalledContentPage/InstalledContentPage"));
const SearchPage = lazy(() => import("./components/Pages/SearchPage/SearchPage"));
const ModInstallPage = lazy(() => import("./components/Pages/ModInstallPage/ModInstallPage"));
const AccountPage = lazy(() => import("./components/Pages/AccountPage/AccountPage"));
const SharedModpackView = lazy(() => import("./components/Pages/ApplyPage/SharedModpackView"));

function App() {
  const { t } = useTranslation();
  const pages: Page[] = [
    {
      content: <ApplyPage />,
      title: t("apply"),
      name: "apply",
      icon: <MdCheck className="duration-0 w-6 h-6" />,
      style: " hover:bg-emerald-400 data-[selected=true]:bg-emerald-900 ",
      main: true,
    },
    {
      content: <CurrentModpackPage />,
      title: t("currentModpack"),
      name: "currentModpack",
      icon: <MdDescription className="duration-0 w-6 h-6" />,
      style: " hover:bg-blue-400 data-[selected=true]:bg-blue-900 ",
      main: true,
    },
    {
      content: <SearchPage />,
      title: t("search"),
      name: "search",
      icon: <MdSearch className="duration-0 w-6 h-6" />,
      style: " hover:bg-sky-400 data-[selected=true]:bg-sky-900 ",
      main: true,
    },
    {
      content: <InstalledContentPage />,
      title: t("installedContent"),
      name: "installedContent",
      icon: <MdFolder className="duration-0 w-6 h-6" />,
      style: " hover:bg-violet-400 data-[selected=true]:bg-violet-900 ",
      main: true,
    },
    {
      content: <AccountPage />,
      title: t("account"),
      name: "account",
      icon: <MdAccountCircle className="duration-0 w-6 h-6" />,
      style: " hover:bg-orange-400 data-[selected=true]:bg-orange-900 ",
      main: true,
    },
    {
      content: <SettingsPage />,
      title: t("settings"),
      name: "settings",
      icon: <MdSettings className="duration-0 w-6 h-6" />,
      style: " hover:bg-slate-400 data-[selected=true]:bg-slate-900 ",
      main: true,
    },
  ];
  const [page, setPage] = useState(pages[0]);
  const [content, setContent] = useState<Page>(pages[0]);
  const [updateDownloadProgress, setUpdateDownloadProgress] = useState(0);
  const [updateSwitch, setUpdateSwitch] = useState<UpdateSwitchPayload>(null);
  const configRef = useRef(createDesktopStore("config.json"));
  const config = configRef.current;
  // A ref, not state: nothing renders from it, and every navigation must see
  // the latest history even when called from an older render's closure.
  const contentHistoryRef = useRef<PageWithScroll[]>([]);
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
          if (progressValue === 0 || progressValue === 1) {
            // Zero is how a failed download hands the taskbar back.
            void clearProgressBar().catch(console.error);
          } else {
            void currentWindow
              .setProgressBar({ progress: Math.round(progressValue * 100) })
              .catch(console.error);
          }
        },
      );
      if (isUnmounted) {
        updateDownloadUnlisten();
      } else {
        cleanupFns.push(updateDownloadUnlisten);
      }

      const updateSwitchUnlisten = await listen<UpdateSwitchPayload>(
        "updateSwitch",
        (e) => {
          if (!isUnmounted) {
            setUpdateSwitch(e.payload);
          }
        },
      );
      if (isUnmounted) {
        updateSwitchUnlisten();
      } else {
        cleanupFns.push(updateSwitchUnlisten);
      }

      // The host reports completion once per file and again when it finishes,
      // so only the first completion of an export earns the success message.
      let exportCompleted = false;
      const exportStartedUnlisten = await listen("quadrantExportStarted", () => {
        exportCompleted = false;
      });
      if (isUnmounted) {
        exportStartedUnlisten();
      } else {
        cleanupFns.push(exportStartedUnlisten);
      }
      const exportProgressUnlisten = await listen(
        "quadrantExportProgress",
        async (e: any) => {
          if (isUnmounted) {
            return;
          }
          const progress = Math.round(e.payload * 100);
          const message = (
            <span className="flex">
              <span>{quadrantLocale.t("export")}</span>
              <MdArchive className="w-6 h-6 mx-2" /> {progress}%
            </span>
          );

          if (e.payload >= 1) {
            void clearProgressBar().catch(console.error);
            if (exportCompleted) {
              return;
            }
            exportCompleted = true;
            contextFunctions.setSnackbar({
              className: "bg-emerald-600 rounded-4xl",
              message,
              timeout: 15000,
            });
          } else {
            exportCompleted = false;
            void currentWindow
              .setProgressBar({ progress })
              .catch(console.error);
            contextFunctions.setSnackbarNoState({
              message,
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
          if (progress >= 100) {
            void clearProgressBar().catch(console.error);
          } else {
            void currentWindow
              .setProgressBar({ progress })
              .catch(console.error);
          }
        },
      );
      if (isUnmounted) {
        modpackDownloadUnlisten();
      } else {
        cleanupFns.push(modpackDownloadUnlisten);
      }

      const [
        currentPlatform,
        savedPageName,
        savedUiScale,
        nativeDecorationsValue,
      ] = await Promise.all([
        platform(),
        config.get<string>("lastPageName"),
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
        // A legacy numeric "lastPage" matches no name, so those users land on pages[0] once.
        const initialPage =
          pages.find((candidate) => candidate.name === savedPageName) ??
          pages[0];
        isLinuxRef.current = currentPlatform === "linux";
        setNativeDecorations(resolvedNativeDecorations);
        await currentWindow.setDecorations(resolvedNativeDecorations);
        setPage(initialPage);
        setContent(initialPage);
        contentHistoryRef.current = [
          {
            page: initialPage,
            scrollPositionX: 0,
            scrollPositionY: 0,
          },
        ];
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

      const locale = await config.get<string>("locale");
      if (locale) {
        await quadrantLocale.changeLanguage(locale);
      }

      const deepLinkUnlisten = await onOpenUrl(async (urls) => {
        if (isUnmounted) {
          return;
        }
        const showUnsupported = () => {
          contextFunctions.setSnackbar({
            message: quadrantLocale.t("unsupportedDownload"),
            className: "bg-red-700",
            timeout: 5000,
          });
        };
        for (const gottenUrl of urls) {
          try {
            const action = resolveDeepLink(gottenUrl);
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
              const codeVerifier = await config.get<string>("oauthVerifier");
              await config.delete("oauthState");
              await config.delete("oauthVerifier");
              if (action.code === null) {
                return;
              }
              await invoke("oauth2_login", {
                code: action.code,
                redirectUri: action.redirectUri,
                codeVerifier,
              });
              return;
            }

            if (action.kind === "importModpack") {
              const resolved = await getQuadrantShareModpack(action.code);
              const randomString = Math.random().toString(36).substring(2, 10);
              contextFunctions.changeContent({
                content: (
                  <SharedModpackView key={randomString} modpack={resolved} />
                ),
                name: randomString,
                icon: <></>,
                title: resolved.name,
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
            // The install page can only install mods, resource packs and
            // shaders; its Download would always fail for the rest.
            if (!isInstallable(mod.modType)) {
              showUnsupported();
              return;
            }
            const randomString = Math.random().toString(36).substring(2, 10);
            contextFunctions.changeContent({
              // Keyed so a link arriving while an install page is open
              // remounts it instead of keeping the previous mod's state.
              content: (
                <ModInstallPage key={mod.id} mod={mod} fileId={action.fileId} />
              ),
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

  const [snackbarState, setSnackbarState] = useState<SnackbarState>({
    message: "",
    className: "hidden",
    timeout: 0,
  });

  const [snackbarEnabled, setSnackbarEnabled] = useState<boolean>(false);

  // Stable on purpose: this feeds every page through context, and the app
  // re-renders on each download or export progress tick.
  const contextFunctions = useMemo<IContentContext>(() => {
    const updateContentWithTransition = (
      update: () => void,
      skipTransition = false,
    ) => {
      if (
        !skipTransition &&
        !isLinuxRef.current &&
        typeof document.startViewTransition === "function"
      ) {
        document.startViewTransition(update);
        return;
      }

      update();
    };

    return {
      back: async () => {
        const popped = popContent(contentHistoryRef.current);
        if (!popped) return;
        const previousEntry = popped.entry;
        contentHistoryRef.current = popped.history;

        const leaving = currentContentRef.current;
        currentContentRef.current = previousEntry.page;
        updateContentWithTransition(() => {
          setContent(previousEntry.page);
        }, leaving.ownTransition === true);

        // Wait a short time to ensure the new content is rendered before scrolling.
        setTimeout(() => {
          contentRef.current?.scrollTo({
            top: Math.round(previousEntry.scrollPositionY),
            left: Math.round(previousEntry.scrollPositionX),
            behavior: "instant",
          });
        }, 50);
      },
      changeContent: (component) => {
        const scrollPositionX = contentRef.current?.scrollLeft ?? 0;
        const scrollPositionY = contentRef.current?.scrollTop ?? 0;
        contentHistoryRef.current = pushContent(
          contentHistoryRef.current,
          currentContentRef.current,
          component,
          { x: scrollPositionX, y: scrollPositionY },
        );
        contentRef.current?.scrollTo({
          top: 0,
          left: 0,
          behavior: "instant",
        });
        currentContentRef.current = component;
        updateContentWithTransition(() => {
          setContent(component);
        }, component.ownTransition === true);
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
  }, []);

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
                          // `page` holds an object from an earlier render's
                          // `pages` array, so identity comparison never matches.
                          (isSelected ? "bg-slate-700" : "bg-slate-800")
                        }
                        key={i}
                        onClick={() => {
                          setPage(p);
                          setContent(p);
                          contentHistoryRef.current = [
                            {
                              page: p,
                              scrollPositionX:
                                contentRef.current?.scrollLeft ?? 0,
                              scrollPositionY:
                                contentRef.current?.scrollTop ?? 0,
                            },
                          ];
                          contentRef.current?.scrollTo({
                            top: 0,
                            left: 0,
                            behavior: "instant",
                          });
                          // Remembering the page must never hold up, or fail,
                          // the navigation itself.
                          config
                            .set("lastPageName", p.name)
                            .then(() => config.save())
                            .catch(console.error);
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
                      {/* `content` keeps the title translated at the time it
                          was set; look it up again so locale changes apply. */}
                      {pages.find((p) => p.name === content.name)?.title ??
                        content.title}
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
                            <p>
                              {updateSwitch === "quadrantNative"
                                ? t("switchToQuadrantNative")
                                : t("appUpdate")}
                            </p>{" "}
                            <MdInstallDesktop className="ml-2 w-6" />
                          </div>
                        ) : (
                          (updateDownloadProgress * 100).toFixed(0) + "%"
                        )}
                      </Button>
                    )}
                    <div
                      className={
                        (nativeDecorations
                          ? ""
                          : "bg-slate-800 rounded-full ") +
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
                            <MdMinimize />
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
                            <MdClose />
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
                  <Suspense fallback={<CircularProgress />}>
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
                  </Suspense>
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
                      "bottom-8 left-8 fixed w-max max-w-[min(36rem,calc(100vw-4rem))] h-max p-4 rounded-4xl flex flex-col items-center justify-center font-bold text-slate-50 transform-gpu backface-hidden will-change-[transform,opacity] " +
                      snackbarState.className
                    }
                  >
                    <div className="flex items-center gap-3">
                      <p>{snackbarState.message}</p>
                      <Button
                        fullRound
                        className="bg-slate-900/50 hover:bg-slate-900/70 shrink-0"
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
