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
import { useContext, useEffect, useState } from "react";
import { getMod, getModUpdate, identifyUnknownMods } from "../../../tools";
import Mod from "../Mod";
import { useTranslation } from "react-i18next";
import CircularProgress from "../../core/CircularProgress";

import Button from "../../core/Button";
import CancelButton from "../../core/CancelButton";
import { MdPermIdentity, MdUpdate } from "react-icons/md";

type ModpackViewProps = LocalModpack & {
  /** Set by openers that pushed this view, so there is a page to return to. */
  showBack?: boolean;
};

export default function ModpackView({ showBack, ...modpack }: ModpackViewProps) {
  const context = useContext(ContentContext);
  const localMods = modpack.mods;
  const [mods, setMods] = useState<IMod[]>([]);
  const [updates, setUpdates] = useState<IMod[]>([]);
  const [toIdentify, setToIdentify] = useState<FetchedIdentifiedMod[]>([]);
  const [showUpdates, setShowUpdates] = useState(false);
  const [showIdentify, setShowIdentify] = useState(false);
  const [modCount, setModCount] = useState(mods.length);
  const placeholderMod: IMod = {
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

  const updateModpackDetails = async (isCancelled: () => boolean) => {
    setMods([]);
    if (!showUpdates) {
      setModCount(0);
    }
    if (showIdentify) {
      await getIdentifiedMods();
    }

    if (!localMods || localMods.length === 0) {
      setMods([placeholderMod]);
      return;
    }

    const fetchedMods = await Promise.all(
      localMods.map(async (mod) => {
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
      }),
    );

    if (isCancelled()) {
      return;
    }
    const validMods = fetchedMods.filter((mod): mod is IMod => mod !== null);

    if (validMods.length === 0) {
      setMods([placeholderMod]);
      if (!showUpdates) {
        setModCount(0);
      }
      return;
    }

    validMods.sort((a, b) => b.downloadCount - a.downloadCount);
    setMods(validMods);
    if (!showUpdates) {
      setModCount(validMods.length);
    }
  };

  const checkForUpdates = async () => {
    setUpdates([]);
    const normalMods: IMod[] = JSON.parse(JSON.stringify(mods));
    const fetchedUpdates = await Promise.all(
      normalMods.map(async (mod) => {
        try {
          const update = await getModUpdate(
            mod,
            modpack.version,
            modpack.modLoader,
            modpack.name,
          );
          return update;
        } catch (e) {
          console.log("Error while checking for updates: " + e);
          return null;
        }
      }),
    );

    const newUpdates = fetchedUpdates.filter(
      (update): update is IMod => update !== null,
    );
    newUpdates.sort((a, b) => b.downloadCount - a.downloadCount);

    setUpdates(newUpdates);
    if (newUpdates.length === 0) {
      setShowUpdates(false);
      setModCount(mods.length);
    } else {
      const downloadableCount = newUpdates.filter(
        (mod) => mod.downloadable === true,
      ).length;
      setModCount(downloadableCount);
    }
  };

  const getIdentifiedMods = async () => {
    console.log("Getting mods");
    const newMods = await identifyUnknownMods(modpack.name);
    const newFetchedMods: FetchedIdentifiedMod[] = [];
    console.log("New mods: " + newMods);
    for (const mod of newMods) {
      console.log(mod);
      if (
        newFetchedMods.filter(
          (fetchedMod) => fetchedMod.fileName === mod.file_name,
        ).length === 0
      ) {
        console.log("Adding a new mod from source.");
        newFetchedMods.push({
          proposedMods: [
            await getMod(
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
            ),
          ],
          fileName: mod.file_name,
        });
      } else {
        console.log("Adding mod from an alternate source.");
        const index = newFetchedMods.findIndex(
          (fMod) => fMod.fileName === mod.file_name,
        );
        newFetchedMods[index].proposedMods.push(
          await getMod(
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
          ),
        );
      }
    }
    if (newFetchedMods.length === 0) {
      setToIdentify([]);
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
  // A single effect covers mount and toggle changes. A separate mount-only
  // effect used to run alongside it, fetching every mod twice on open.
  useEffect(() => {
    let cancelled = false;
    if (showIdentify === false) {
      setToIdentify([]);
    }
    if (showUpdates === false) {
      setUpdates([]);
    }
    updateModpackDetails(() => cancelled).catch((e) => console.error(e));
    return () => {
      cancelled = true;
    };
  }, [modpack.name, modsSignature, showIdentify, showUpdates]);

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
              onClick={async () => {
                if (showUpdates) {
                  setShowUpdates(false);
                  return;
                }
                setShowUpdates(true);
                await checkForUpdates();
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
