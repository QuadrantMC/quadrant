/** @format */

import quadrantLocale from "../../../i18n";
import {
  ContentContext,
  FetchedIdentifiedMod,
  IMod,
  LocalModpack,
  ModpackViewContext,
  ModSource,
  ModType,
} from "../../../intefaces";
import { useContext, useEffect, useRef, useState } from "react";
import { mapLimit } from "../../../mapLimit";
import { getMod, getModUpdate, identifyUnknownMods } from "../../../tools";
import Mod from "../Mod";
import { useTranslation } from "react-i18next";
import CircularProgress from "../../core/CircularProgress";

import Button from "../../core/Button";
import CancelButton from "../../core/CancelButton";
import BrokenModpackNotice from "../BrokenModpackNotice";
import { MdPermIdentity, MdUpdate } from "react-icons/md";

type ModpackViewProps = LocalModpack & {
  /** Set by openers that pushed this view, so there is a page to return to. */
  showBack?: boolean;
};

const PLACEHOLDER_MOD: IMod = {
  autoinstallable: false,
  deleteable: false,
  description: "-",
  downloadCount: 0,
  id: "-",
  modIconUrl:
    "https://github.com/QuadrantMC/quadrant/raw/next/public/logo.svg",
  downloadable: false,
  name: "-",
  source: ModSource.Online,
  thumbnailUrls: [],
  url: "",
  modType: ModType.Mod,
  license: "-",
  showPreviousVersion: false,
  slug: "-",
  version: "",
  dateModified: "",
  newVersion: null,
  modpack: "",
  selectable: false,
  selectUrl: null,
};

// Provider requests in flight at once; a big pack would otherwise fire one per
// mod and trip the providers' rate limits.
const FETCH_CONCURRENCY = 6;

export default function ModpackView({ showBack, ...modpack }: ModpackViewProps) {
  const context = useContext(ContentContext);
  const localMods = modpack.mods;
  const [mods, setMods] = useState<IMod[]>([]);
  const [updates, setUpdates] = useState<IMod[]>([]);
  const [toIdentify, setToIdentify] = useState<FetchedIdentifiedMod[]>([]);
  const [showUpdates, setShowUpdates] = useState(false);
  const [showIdentify, setShowIdentify] = useState(false);
  const updatesRequest = useRef(0);
  const loadMods = async (isCancelled: () => boolean) => {
    if (!localMods || localMods.length === 0) {
      setMods([PLACEHOLDER_MOD]);
      return;
    }

    const fetchedMods = await mapLimit(
      localMods,
      FETCH_CONCURRENCY,
      async (mod) => {
        if (isCancelled()) {
          return null;
        }
        try {
          return await getMod(
            {
              id: mod.id,
              deletable: true,
              downloadable: false,
              modpack: modpack.name,
              modLoader: modpack.modLoader,
              versionTarget: "",
              showPreviousVersion: false,
              selectable: false,
              selectUrl: null,
            },
            mod.source,
          );
        } catch (error) {
          console.error("Failed to retrieve mod:", error);
          return null;
        }
      },
    );

    if (isCancelled()) {
      return;
    }
    const validMods = fetchedMods.filter((mod): mod is IMod => mod !== null);

    if (validMods.length === 0) {
      setMods([PLACEHOLDER_MOD]);
      return;
    }

    validMods.sort((a, b) => b.downloadCount - a.downloadCount);
    setMods(validMods);
  };

  const checkForUpdates = async () => {
    const request = ++updatesRequest.current;
    setUpdates([]);
    const normalMods: IMod[] = JSON.parse(JSON.stringify(mods));
    const fetchedUpdates = await mapLimit(
      normalMods,
      FETCH_CONCURRENCY,
      async (mod) => {
        if (request !== updatesRequest.current) {
          return null;
        }
        try {
          return await getModUpdate(
            mod,
            modpack.version,
            modpack.modLoader,
            modpack.name,
          );
        } catch (e) {
          console.error("Error while checking for updates: " + e);
          return null;
        }
      },
    );
    if (request !== updatesRequest.current) {
      return;
    }

    const newUpdates = fetchedUpdates.filter(
      (update): update is IMod => update !== null,
    );
    newUpdates.sort((a, b) => b.downloadCount - a.downloadCount);

    setUpdates(newUpdates);
    if (newUpdates.length === 0) {
      setShowUpdates(false);
    }
  };

  const getIdentifiedMods = async (isCancelled: () => boolean) => {
    const newMods = await identifyUnknownMods(modpack.name);
    const newFetchedMods: FetchedIdentifiedMod[] = [];
    for (const mod of newMods) {
      if (isCancelled()) {
        return;
      }
      const proposed = await getMod(
        {
          deletable: false,
          id: mod.installed_mod.id,
          downloadable: false,
          showPreviousVersion: false,
          versionTarget: "",
          selectable: true,
          selectUrl: mod.installed_mod.downloadUrl,
          modLoader: modpack.modLoader,
          modpack: modpack.name,
        },
        mod.installed_mod.source,
      );
      const existing = newFetchedMods.find(
        (fetched) => fetched.fileName === mod.file_name,
      );
      if (existing) {
        existing.proposedMods.push(proposed);
      } else {
        newFetchedMods.push({
          proposedMods: [proposed],
          fileName: mod.file_name,
        });
      }
    }
    if (isCancelled()) {
      return;
    }
    if (newFetchedMods.length === 0) {
      setShowIdentify(false);
    }
    setToIdentify(newFetchedMods);
  };

  const { t } = useTranslation();
  // The cards belong to one pack's contents, so a different pack or a changed
  // mod list must refetch them. Keyed by content rather than array identity:
  // the parent re-reads the packs on every folder event.
  const modsSignature = (localMods ?? [])
    .map((mod) => `${mod.source}:${mod.id}`)
    .join(",");
  // Toggling Update or Identify only changes which view is shown, so these
  // effects must not depend on those flags: that refetched every mod.
  useEffect(() => {
    let cancelled = false;
    setMods([]);
    loadMods(() => cancelled).catch((e) => console.error(e));
    return () => {
      cancelled = true;
    };
  }, [modpack.name, modsSignature]);

  useEffect(() => {
    if (!showIdentify) {
      setToIdentify([]);
      return;
    }
    let cancelled = false;
    getIdentifiedMods(() => cancelled).catch((e) => console.error(e));
    return () => {
      cancelled = true;
    };
  }, [modpack.name, showIdentify]);

  useEffect(() => {
    if (!showUpdates) {
      updatesRequest.current++;
      setUpdates([]);
    }
  }, [showUpdates]);

  const modCount =
    showUpdates && updates.length > 0
      ? updates.filter((mod) => mod.downloadable === true).length
      : mods.length === 1 && mods[0] === PLACEHOLDER_MOD
        ? 0
        : mods.length;

  return (
    <div className="w-auto p-4 rounded-4xl flex flex-1 flex-col">
      <ModpackViewContext.Provider
        value={{
          removeMod: async (id: string) => {
            let newToIdentify = [...toIdentify];
            newToIdentify = newToIdentify.filter(
              (mod) =>
                mod.proposedMods.filter((mod) => mod.id === id).length === 0,
            );
            setToIdentify(newToIdentify);
            if (newToIdentify.length === 0) {
              setShowIdentify(false);
            }
          },
        }}
      >
        <div className="bg-slate-700 p-4 flex align-middle rounded-4xl mt-2 mb-5 w-full font-bold items-center justify-center ">
          {showBack && (
            <CancelButton
              onClick={() => void context.back()}
              className="self-center!"
            />
          )}
          <div className="flex items-center justify-center mx-auto">
            <p className="text-center ">
              {modpack.name} | {modpack.modLoader} | {modpack.version} |{" "}
              {t("modCount", { amount: modCount })}{" "}
              {modpack.lastSynced > 0 && (
                <>
                  |{" "}
                  {t("localSyncDate", {
                    date: new Intl.DateTimeFormat(quadrantLocale.language, {
                      weekday: "short",
                      day: "2-digit",
                      month: "2-digit",
                      year: "2-digit",
                      hour: "2-digit",
                      minute: "2-digit",
                      hour12: false,
                    }).format(new Date(modpack.lastSynced)),
                  })}
                </>
              )}
            </p>
            <Button
              disabled={!!modpack.manifestError}
              onClick={() => {
                if (showUpdates) {
                  setShowUpdates(false);
                  return;
                }
                setShowUpdates(true);
                // Not awaited: a pending button ignores clicks, and the user
                // must be able to hide the view while the checks still run.
                checkForUpdates().catch(console.error);
              }}
              className="bg-blue-600 hover:bg-blue-700 ml-4 w-fit px-4 flex items-center "
            >
              {t("update")}
              <MdUpdate className="w-6 ml-2 h-6"></MdUpdate>
            </Button>
            {modpack.unknownMods && (
              <Button
                onClick={async () => {
                  // The [showIdentify] effect runs identification; calling it
                  // here too fingerprinted every unknown jar twice.
                  setShowIdentify(!showIdentify);
                }}
                className="bg-slate-800 px-4 hover:bg-slate-600 mx-2 w-fit flex items-center "
              >
                {t("identifyUnknownMods")}
                <MdPermIdentity className="w-6 ml-2 h-6"></MdPermIdentity>
              </Button>
            )}
          </div>
        </div>

        {modpack.manifestError && (
          <BrokenModpackNotice error={modpack.manifestError} />
        )}
        {showIdentify ? (
          toIdentify.length == 0 ? (
            <div className="bg-slate-800 w-full center h-min p-4 rounded-4xl">
              <CircularProgress></CircularProgress>
            </div>
          ) : (
            <>
              {toIdentify.map((mod) => {
                return (
                  <div
                    key={mod.fileName}
                    className="bg-slate-800 rounded-4xl p-4 my-2"
                  >
                    <h1 className="font-bold">{mod.fileName} -&gt;</h1>
                    <div className="flex flex-row w-full mt-8">
                      {mod.proposedMods.map((proposed) => {
                        return (
                          <Mod
                            key={proposed.source + proposed.id}
                            className="w-max mx-2"
                            mod={proposed}
                            modpack={modpack.name}
                          ></Mod>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
            </>
          )
        ) : !showUpdates ? (
          mods.length == 0 ? (
            <div className="bg-slate-800 w-full center h-min p-4 rounded-4xl">
              <CircularProgress></CircularProgress>
            </div>
          ) : (
            <div className="bg-slate-800 p-4 rounded-4xl mr-4 ml-2 mb-12 grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4 gap-4">
              {mods.map((mod) => {
                return (
                  <Mod
                    key={mod.source + mod.id}
                    className=" "
                    mod={mod}
                    modpack={modpack.name}
                  ></Mod>
                );
              })}
            </div>
          )
        ) : updates.length == 0 ? (
          <div className="bg-slate-800 w-full center h-min p-4 rounded-4xl">
            <CircularProgress></CircularProgress>
          </div>
        ) : (
          <div className="bg-slate-800 p-4 rounded-4xl mr-4 ml-2 mb-12 grid grid-cols-3 2xl:grid-cols-4 gap-6">
            {updates.map((mod) => {
              return (
                <Mod
                  key={mod.source + mod.id}
                  className=" "
                  mod={mod}
                  modpack={modpack.name}
                ></Mod>
              );
            })}
          </div>
        )}
      </ModpackViewContext.Provider>
    </div>
  );
}
