/** @format */

import { useContext, useEffect, useRef, useState } from "react";
import {
  LocalModpack,
  MinecraftVersion,
  ModLoader,
  PrismInstance,
  SyncContext,
} from "../../../intefaces";
import {
  applyModpack,
  createModpack,
  deleteModpack,
  getMinecraftFolder,
  getModpacks,
  getPrismInstances,
  getQuadrantShareModpack,
  getVersions,
  openModpacksFolder,
} from "../../../tools";
import { useTranslation } from "react-i18next";
import Button from "../../core/Button";
import { AnimatePresence, motion } from "motion/react";
import "./Apply.css";
import { MdAdd, MdCheck, MdClear, MdFolder } from "react-icons/md";
import { ContentContext } from "../../../intefaces";
import LocalModpackCard from "./LocalModpackCard";
import ModpackEditDialog from "./ModpackEditDialog";
import { createDesktopStore, listen } from "../../../desktop";
import { watchModpacks } from "../../../modpackWatch";
import quadrantLocale from "../../../i18n";
import { isModpackMissingError } from "../../../errors";
import { useReportError } from "../../../useReportError";
import {
  loaderProvidersFromSettings,
  ModLoaderProvider,
} from "../../../modLoaders";
import { parseShareCode } from "../../../deepLinks";
import SharedModpackView from "./SharedModpackView";
import SyncedModpackComponent from "./SyncedModpack";
import { useSyncedModpacks } from "./useSyncedModpacks";
import { mergeModpacks } from "./mergeModpacks";

const toolbarActionClass =
  "flex shrink-0 items-center justify-center gap-2 h-10 px-4 rounded-full text-sm text-white whitespace-nowrap";
const toolbarIconClass =
  "flex shrink-0 items-center justify-center w-10 h-10 rounded-full bg-slate-700 text-slate-200 hover:text-white";

export default function ApplyPage() {
  const [modpacks, setModpacks] = useState<LocalModpack[]>([]);
  const [prismInstances, setPrismInstances] = useState<PrismInstance[]>([]);
  const {
    accountInfo,
    syncedModpacks,
    refresh: refreshSyncedModpacks,
  } = useSyncedModpacks();

  const { t } = useTranslation();
  const reportError = useReportError();
  const [isUpdateDialogOpen, setIsUpdateDialogOpen] = useState(false);
  const [isDialogToCreate, setIsDialogToCreate] = useState(false);

  const [modpackToUpdate, setModpackToUpdate] = useState<LocalModpack>({
    version: "",
    name: "",
    mods: [],
    isApplied: false,
    lastSynced: 0,
    modLoader: ModLoader.Unknown,
    unknownMods: false,
  });
  const [defaultModpack, setDefaultModpack] = useState<LocalModpack>({
    name: "",
    version: "",
    modLoader: ModLoader.Unknown,
    isApplied: false,
    lastSynced: 0,
    mods: [],
    unknownMods: false,
  });
  const [originalModpackName, setOriginalModpackName] = useState("free");
  const [versions, setVersions] = useState<MinecraftVersion[]>([]);
  const [searchQuery, setSearchQuery] = useState<string>("");
  const [isResolvingShareCode, setIsResolvingShareCode] = useState(false);
  const [loaderProviders, setLoaderProviders] = useState<ModLoaderProvider[]>(
    loaderProvidersFromSettings(true, true),
  );
  const context = useContext(ContentContext);

  // Get the modpacks for the first time and listen for changes to the Minecraft folder from the backend
  useEffect(() => {
    let isUnmounted = false;
    const cleanupFns: Array<() => void> = [];

    const effect = async () => {
      const configStore = createDesktopStore("config.json");
      const [versions, availableModpacks, curseForgeEnabled, modrinthEnabled] =
        await Promise.all([
          // Live provider request: offline, it must not hide the local packs.
          getVersions().catch((error) => {
            console.error(error);
            return [] as MinecraftVersion[];
          }),
          getModpacks(),
          configStore.get<boolean>("curseforge"),
          configStore.get<boolean>("modrinth"),
          // The instance list only changes outside the watched folders, so it
          // is loaded here and after a card acts, never on a watch event.
          refreshPrismInstances(),
        ]);
      if (isUnmounted) {
        return;
      }
      setModpacks(availableModpacks);
      setVersions(versions);
      setLoaderProviders(
        loaderProvidersFromSettings(curseForgeEnabled, modrinthEnabled),
      );

      setDefaultModpack({
        name: "",
        version: versions[0]?.version ?? "",
        modLoader: ModLoader.Unknown,
        isApplied: false,
        lastSynced: 0,
        mods: [],
        unknownMods: false,
      });

      const unwatch = await watchModpacks(
        await getMinecraftFolder(false),
        () => {
          if (!isUnmounted) void updateModpacks().catch(console.error);
        },
      );
      if (isUnmounted) unwatch();
      else cleanupFns.push(unwatch);

      const unlisten = await listen(
        "quadrantShareSubmission",
        async (event: any) => {
          const usesLeft = event.payload.uses_left;
          if (isUnmounted) {
            return;
          }
          context.setSnackbar({
            message: (
              <span className="flex">
                <MdCheck className="w-5 h-5 mx-2" />
                {quadrantLocale.t("copiedToClipboard", { amount: usesLeft })}
              </span>
            ),
            className: "bg-emerald-600 rounded-4xl",
            timeout: 5000,
          });
        },
      );
      if (isUnmounted) {
        unlisten();
      } else {
        cleanupFns.push(unlisten);
      }
    };

    effect().catch(console.error);

    return () => {
      isUnmounted = true;
      while (cleanupFns.length > 0) {
        const cleanup = cleanupFns.pop();
        try {
          cleanup?.();
        } catch (error) {
          console.error(error);
        }
      }
    };
  }, []);

  // The backend gates this on the experimental flag and answers with an empty
  // list while it is off, so the flag is never read here.
  const refreshPrismInstances = async () => {
    try {
      setPrismInstances(await getPrismInstances());
    } catch (error) {
      console.error(error);
      // Never keep offering instances the host can no longer enumerate.
      setPrismInstances([]);
    }
  };

  // Watch events burst while installs write files, so an older listing can
  // resolve after a newer one; only the latest request may land.
  const modpacksRequestRef = useRef(0);
  const updateModpacks = async () => {
    const request = ++modpacksRequestRef.current;
    const newModpacks = await getModpacks();
    if (request === modpacksRequestRef.current) {
      setModpacks(newModpacks);
    }
  };

  const shareCode = parseShareCode(searchQuery);
  const rows = mergeModpacks(modpacks, syncedModpacks, searchQuery);

  const openSharedModpack = async () => {
    if (shareCode === null || isResolvingShareCode) {
      return;
    }
    setIsResolvingShareCode(true);
    try {
      const resolved = await getQuadrantShareModpack(shareCode);
      const randomString = Math.random().toString(36).substring(2, 10);
      context.changeContent({
        name: shareCode + randomString,
        title: resolved.name,
        style: "",
        main: false,
        icon: <></>,
        content: <SharedModpackView modpack={resolved} />,
      });
    } catch (e: any) {
      console.error(e);
      context.setSnackbar({
        message: t("unsupportedDownload"),
        className: "bg-red-700 rounded-4xl",
        timeout: 5000,
      });
    } finally {
      setIsResolvingShareCode(false);
    }
  };

  return (
    <>
      <motion.div
        initial={{ y: 24, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: 24, opacity: 0 }}
        className="flex flex-1 flex-col w-full transform-gpu backface-hidden will-change-[transform,opacity]"
      >
        <div className="flex flex-row items-center gap-2 mx-8 my-4">
          <input
            placeholder={t("searchModpacksPlaceholder")}
            className="input min-w-0 flex-1 h-10 px-4 rounded-full bg-slate-700 text-sm font-bold text-slate-100 placeholder:font-normal placeholder:text-slate-400 outline-none focus:bg-slate-600"
            onChange={(event) => setSearchQuery(event.target.value)}
            autoComplete="off"
            value={searchQuery}
          ></input>
          <Button
            onClick={() => {
              setIsDialogToCreate(true);
              setIsUpdateDialogOpen(true);
              setModpackToUpdate(defaultModpack);
            }}
            className={
              toolbarActionClass + " bg-emerald-600 hover:bg-emerald-700"
            }
          >
            <MdAdd aria-hidden="true" className="w-5 h-5" />
            {t("createModpack")}
          </Button>
          <Button
            onClick={async () => {
              const firstVersion = versions[0]?.version;
              if (!firstVersion) {
                return;
              }
              try {
                try {
                  await deleteModpack("free");
                } catch (error) {
                  // A missing temporary modpack is the normal first run.
                  if (!isModpackMissingError(error)) throw error;
                }
                await createModpack("free", firstVersion, ModLoader.Unknown);
                await applyModpack("free");
                await updateModpacks();
              } catch (error) {
                reportError(error);
              }
            }}
            className={toolbarIconClass + " hover:bg-red-700"}
            title={t("clear")}
            aria-label={t("clear")}
          >
            <MdClear aria-hidden="true" className="w-5 h-5" />
          </Button>
          <Button
            onClick={async () => {
              await openModpacksFolder();
            }}
            className={toolbarIconClass + " hover:bg-slate-600"}
            title={t("openModpacksFolder")}
            aria-label={t("openModpacksFolder")}
          >
            <MdFolder aria-hidden="true" className="w-5 h-5" />
          </Button>
        </div>
        {shareCode !== null && (
          <div className="bg-slate-800 rounded-4xl mx-8 mb-2 p-4 flex flex-col items-center font-bold">
            <p>{t("shareCodeDetected", { code: shareCode })}</p>
            <Button
              className={
                "mt-4 w-full " +
                (isResolvingShareCode
                  ? "bg-slate-700 cursor-not-allowed"
                  : "bg-emerald-600 hover:bg-emerald-700")
              }
              onClick={openSharedModpack}
            >
              {isResolvingShareCode ? t("loadingMore") : t("download")}
            </Button>
          </div>
        )}
        <SyncContext.Provider value={{ refreshSyncedModpacks }}>
          <div className="bg-slate-800 rounded-4xl mx-6 my-4">
            <AnimatePresence>
              {rows.map(({ local, synced }) =>
                local ? (
                  <LocalModpackCard
                    key={"local:" + local.name}
                    modpack={local}
                    synced={synced}
                    accountInfo={accountInfo}
                    prismInstances={prismInstances}
                    onChanged={async () => {
                      await updateModpacks();
                      await refreshPrismInstances();
                    }}
                    onEdit={(target) => {
                      setIsUpdateDialogOpen(true);
                      setIsDialogToCreate(false);
                      setOriginalModpackName(
                        JSON.parse(JSON.stringify(target.name)),
                      );
                      setModpackToUpdate(target);
                    }}
                  />
                ) : (
                  synced && (
                    <SyncedModpackComponent
                      key={"cloud:" + synced.modpack_id}
                      modpack={synced}
                      accountInfo={accountInfo}
                    />
                  )
                ),
              )}
            </AnimatePresence>
          </div>
        </SyncContext.Provider>
        <ModpackEditDialog
          open={isUpdateDialogOpen}
          onClose={() => setIsUpdateDialogOpen(false)}
          isCreate={isDialogToCreate}
          versions={versions}
          loaderProviders={loaderProviders}
          initial={modpackToUpdate}
          originalName={originalModpackName}
          onSaved={updateModpacks}
        />
        <div className="h-1"></div>
      </motion.div>
    </>
  );
}
