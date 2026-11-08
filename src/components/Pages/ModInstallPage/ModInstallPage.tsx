/** @format */

import {
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  ContentContext,
  ContentLocation,
  IMod,
  LocalModpack,
  MinecraftVersion,
  ModLoader,
  ModProgress,
  ModSource,
  ModType,
} from "../../../intefaces";
import Button from "../../core/Button";
import CancelButton from "../../core/CancelButton";
import {
  MdDownload,
  MdExtension,
  MdOpenInNew,
  MdPerson,
} from "react-icons/md";
import { animate, motion } from "motion/react";
import { useTranslation } from "react-i18next";
import {
  getInstalledContent,
  getModDependencies,
  getModOwners,
  getModpacks,
  getUserURL,
  getVersions,
  installMod,
  openIn,
} from "../../../tools";
import Mod from "../../shared/Mod";
import LoaderOptions from "../../shared/LoaderOption";
import LinearProgress from "../../core/LinearProgress";
import { createDesktopStore, listen } from "../../../desktop";
import { loaderProvidersForSource } from "../../../modLoaders";
import { findInstalledIn, isInstalledIn } from "../../../installedMods";
import { isPackType, locationOptionLabel } from "../../../contentLocations";
import { useReportError } from "../../../useReportError";

export interface IModInstallPageProps {
  mod: IMod;
  fileId?: string;
  /** Screen rect of the card that opened this page; the details card grows out of it. */
  originRect?: DOMRect;
  /** The modpack the opener is installing into; wins over saved choices. */
  installTarget?: Pick<LocalModpack, "name">;
  /** The `ContentLocation.id` the opener already chose, if it still exists. */
  installLocation?: string;
  onInstalled?: () => void;
}

interface IModOwner {
  name: string;
  url: string;
}

const panelClass = "bg-slate-800 rounded-[28px] p-5 min-w-0";
const labelClass = "block text-[13px] font-extrabold text-slate-400 mb-1.5";
const selectClass =
  "w-full h-10 px-4 rounded-full border-none bg-slate-700 text-sm font-bold text-slate-100 outline-none cursor-pointer hover:brightness-110";
const expandSpring = { type: "spring", stiffness: 320, damping: 32 } as const;

const panelRadius = 28;
const revealed = {
  x: 0,
  y: 0,
  clipPath: `inset(0px 0px 0px 0px round ${panelRadius}px)`,
};

/**
 * Translate plus clip that shows only a `rect`-sized window of `el` at rect's
 * position, so the content inside never stretches. Null when either has no size.
 */
function revealFrom(el: HTMLElement, rect: DOMRect | undefined) {
  if (!rect) {
    return null;
  }
  const to = el.getBoundingClientRect();
  if (to.width === 0 || to.height === 0 || rect.width === 0) {
    return null;
  }
  const right = Math.max(0, to.width - rect.width);
  const bottom = Math.max(0, to.height - rect.height);
  return {
    x: rect.left - to.left,
    y: rect.top - to.top,
    clipPath: `inset(0px ${right}px ${bottom}px 0px round ${panelRadius}px)`,
  };
}

const actionClass =
  "flex items-center justify-center gap-2 h-10 px-4 rounded-full text-sm text-white whitespace-nowrap";

export default function ModInstallPage(props: IModInstallPageProps) {
  const mod = props.mod;

  const context = useContext(ContentContext);
  const { t, i18n } = useTranslation();
  const reportError = useReportError();
  const installTargetName = props.installTarget?.name;
  const openerLocation = props.installLocation;
  const [versions, setVersions] = useState<MinecraftVersion[]>([]);
  const [modpacks, setModpacks] = useState<LocalModpack[]>([]);
  const [contentLocations, setContentLocations] = useState<ContentLocation[]>(
    [],
  );
  /** A `ContentLocation.id`, or `""` for the automatic placement. */
  const [contentLocation, setContentLocation] = useState<string>("");
  const [version, setVersion] = useState<string>("");
  const [modpack, setModpack] = useState<string>("");
  const [loader, setLoader] = useState<string>("");
  const [owners, setOwners] = useState<IModOwner[]>([]);
  const [deps, setDeps] = useState<IMod[]>([]);
  const [clipIcons, setClipIcons] = useState(true);
  const [modInstallProgress, setModInstallProgress] = useState<number>(0);
  const [modDownloadProgress, setModDownloadProgress] = useState<number>(0);
  const [isInstalling, setIsInstalling] = useState(false);
  const [isReady, setIsReady] = useState(false);
  const installInFlightRef = useRef(false);
  const configRef = useRef(createDesktopStore("config.json"));
  const config = configRef.current;
  const detailsRef = useRef<HTMLElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const expanded = props.originRect !== undefined;
  const restPanels = () =>
    Array.from(
      rootRef.current?.querySelectorAll<HTMLElement>("[data-expand-rest]") ??
        [],
    );

  useLayoutEffect(() => {
    const details = detailsRef.current;
    const from = details && revealFrom(details, props.originRect);
    if (!details || !from) {
      return;
    }
    // Fire-and-forget on purpose: stopping these in a cleanup would leave the
    // card frozen mid-transform when StrictMode replays the effect.
    animate(
      details,
      {
        x: [from.x, revealed.x],
        y: [from.y, revealed.y],
        clipPath: [from.clipPath, revealed.clipPath],
      },
      expandSpring,
    );
    for (const panel of restPanels()) {
      animate(
        panel,
        { opacity: [0, 1], y: [16, 0] },
        { ...expandSpring, delay: 0.12 },
      );
    }
    // Runs once for the opening card; the origin never changes while mounted.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const close = async () => {
    const details = detailsRef.current;
    const atRest = details && ["", "none"].includes(details.style.transform);
    const to = atRest ? revealFrom(details, props.originRect) : null;
    if (details && to) {
      const collapse = Promise.all([
        animate(details, to, { duration: 0.22, ease: "easeInOut" }),
        ...restPanels().map((panel) =>
          animate(panel, { opacity: 0 }, { duration: 0.15 }),
        ),
      ]);
      // Never let a stalled animation strand the user on this page.
      await Promise.race([collapse, new Promise((r) => setTimeout(r, 400))]);
    }
    context.back();
  };

  useEffect(() => {
    let cancelled = false;
    setIsReady(false);
    const effect = async () => {
      // Only packs can be routed to a folder of the user's choosing; a mod
      // always follows its modpack.
      const placeable = isPackType(mod.modType);
      const [
        availableVersions,
        availableModpacks,
        savedVersion,
        savedLoader,
        savedModpack,
        availableLocations,
      ] = await Promise.all([
        // Live provider request: offline, it must not keep Download disabled.
        getVersions().catch((error) => {
          console.error(error);
          return [] as MinecraftVersion[];
        }),
        getModpacks(),
        config.get<string>("lastUsedVersion"),
        config.get<string>("lastUsedAPI"),
        config.get<string>("lastUsedModpack"),
        // The picker only names the folders, so their files are left unlisted.
        // A host that cannot list its folders costs the user the picker, not
        // the install, so this branch settles rather than rejects.
        placeable
          ? getInstalledContent(false).catch((error) => {
              console.error(error);
              return [] as ContentLocation[];
            })
          : Promise.resolve([] as ContentLocation[]),
      ]);
      if (cancelled) return;
      // Reconcile the saved choices with what actually exists: a deleted pack
      // or an unavailable version must never reach the install call.
      const explicitTarget = availableModpacks.find(
        (entry) => entry.name === installTargetName,
      );
      // Resource packs and shaders are routed by the pack the opener scoped the
      // search to, never by a saved or guessed one.
      const target =
        mod.modType === ModType.Mod
          ? (explicitTarget ??
            availableModpacks.find((entry) => entry.name === savedModpack) ??
            availableModpacks.find((entry) => entry.isApplied) ??
            availableModpacks[0])
          : explicitTarget;
      // Keep a saved choice while it is still offered; otherwise fall back to
      // the target pack, so the install never submits an unavailable value.
      const wantedVersion = explicitTarget?.version ?? savedVersion;
      const initialVersion =
        availableVersions.find((entry) => entry.version === wantedVersion)
          ?.version ??
        target?.version ??
        availableVersions[0]?.version ??
        "";
      setVersions(availableVersions);
      setModpacks(availableModpacks);
      setVersion(initialVersion);
      setLoader(
        explicitTarget?.modLoader ||
          savedLoader ||
          target?.modLoader ||
          ModLoader.Unknown,
      );
      setModpack(target?.name ?? "");
      setContentLocations(availableLocations);
      // A single location is not a choice, so it is never sent. With a target
      // pack, following it stays the default and the user opts out of it.
      const fallbackLocation =
        availableLocations.length > 1 && explicitTarget === undefined
          ? availableLocations[0].id
          : "";
      // The opener's choice wins, but only while that folder is still listed.
      const opened = availableLocations.find(
        (entry) => entry.id === openerLocation,
      );
      setContentLocation(opened?.id ?? fallbackLocation);
      setIsReady(true);
    };
    effect().catch(console.error);
    return () => {
      cancelled = true;
    };
  }, [config, mod.modType, installTargetName, openerLocation]);

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void | Promise<void>> = [];
    setDeps([]);
    setOwners([]);
    const effect = async () => {
      const [newOwners, newDeps, roundIcons] = await Promise.all([
        getModOwners(mod.source, mod.id),
        getModDependencies(mod.source, mod.id),
        config.get<boolean>("clipIcons"),
      ]);
      const newOwnersList = await Promise.all(
        newOwners.map(async (owner) => ({
          name: owner,
          url: await getUserURL(owner, mod.source),
        })),
      );
      // A previously viewed mod's response must not overwrite the current one.
      if (cancelled) return;
      setClipIcons(roundIcons ?? true);
      setDeps(newDeps);
      setOwners(newOwnersList);
    };
    effect().catch(console.error);
    for (const [eventName, setProgress] of [
      ["modInstallProgress", setModInstallProgress],
      ["modDownloadProgress", setModDownloadProgress],
    ] as const) {
      void listen<ModProgress>(eventName, (event) => {
        if (!cancelled && event.payload.modId === mod.id) {
          setProgress(event.payload.progress);
        }
      })
        .then((unlisten) => {
          if (cancelled) return unlisten();
          unlisteners.push(unlisten);
        })
        .catch(console.error);
    }
    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => void unlisten());
    };
  }, [config, mod.id, mod.source]);

  const isCurseForge = mod.source === ModSource.CurseForge;
  const modSource = isCurseForge
    ? "CurseForge"
    : mod.source === ModSource.Modrinth
      ? "Modrinth"
      : "?";
  // Install flow targets one provider, so hide loaders that provider cannot resolve.
  const loaderProviders = loaderProvidersForSource(mod.source);
  const downloadCount = Intl.NumberFormat(i18n.language, {
    compactDisplay: "short",
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(mod.downloadCount);
  const pickTargets = props.fileId === undefined;
  const showProgress = isInstalling || modDownloadProgress > 0;
  const selectedModpack = modpacks.find((entry) => entry.name === modpack);
  // Only a target that really exists can be followed automatically.
  const hasInstallTarget = modpacks.some(
    (entry) => entry.name === installTargetName,
  );
  const installedEntry = findInstalledIn(mod, selectedModpack);
  const alreadyInstalled = installedEntry !== undefined;
  // Matched by slug across providers: say whose copy goes, since a shared
  // slug is strong but not certain evidence that it is the same mod.
  const replacesOtherProvider =
    installedEntry !== undefined && installedEntry.source !== mod.source;

  // Never submit an empty version or a pack that is not in the list.
  const canInstall =
    isReady &&
    !isInstalling &&
    (!pickTargets || version !== "") &&
    (mod.modType !== ModType.Mod || modpack !== "");

  const install = async () => {
    if (!canInstall || installInFlightRef.current) {
      return;
    }
    installInFlightRef.current = true;
    setIsInstalling(true);
    try {
      await installMod({
        id: mod.id,
        minecraftVersion: version,
        loader: loader as ModLoader,
        source: mod.source,
        modType: mod.modType,
        modpack,
        fileId: props.fileId,
        contentLocation: contentLocation === "" ? undefined : contentLocation,
      });
      // Not awaited: a slow or failed reload must not hold the button or
      // surface as an install failure.
      getModpacks().then(setModpacks).catch(console.error);
      props.onInstalled?.();
    } catch (e: any) {
      reportError(e);
    } finally {
      installInFlightRef.current = false;
      setIsInstalling(false);
    }
  };

  const actions = (
    <>
      <Button
        onClick={() => void install()}
        disabled={!canInstall}
        className={
          actionClass +
          " flex-1 " +
          (canInstall
            ? "bg-emerald-600 hover:bg-emerald-700"
            : "bg-slate-700 cursor-not-allowed")
        }
      >
        <MdDownload className="size-5" />
        {t(alreadyInstalled ? "reinstall" : "download")}
      </Button>
      <Button
        onClick={() => void openIn(mod.url)}
        className={actionClass + " flex-1 bg-slate-700 hover:bg-slate-600"}
      >
        <MdOpenInNew className="size-5" />
        {t("openInTheWeb")}
      </Button>
    </>
  );

  return (
    <motion.div
      ref={rootRef}
      initial={expanded ? false : { y: 24, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      exit={{ y: -24, opacity: 0 }}
      className="flex flex-col gap-3 px-4 pt-2 pb-5 transform-gpu [backface-visibility:hidden] [will-change:transform,opacity]"
      key={mod.name}
    >
      <CancelButton onClick={() => void close()} />

      <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_22rem] items-start">
        <div className="flex flex-col gap-3 min-w-0">
          <section ref={detailsRef} className={panelClass}>
            <div className="flex items-start gap-4">
              <img
                src={mod.modIconUrl}
                alt=""
                className={
                  "flex-none w-16 h-16 object-cover bg-slate-900 " +
                  (clipIcons ? "rounded-full" : "rounded-2xl")
                }
              />
              <div className="flex-1 min-w-0 flex flex-col gap-1">
                <h2 className="text-xl font-black tracking-tight leading-tight text-wrap">
                  {mod.name}
                </h2>
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs font-bold text-slate-400">
                  <span
                    className={
                      "px-2 py-0.5 rounded-full text-[11px] font-extrabold " +
                      (isCurseForge
                        ? "bg-orange-500/20 text-orange-400"
                        : "bg-emerald-500/15 text-emerald-400")
                    }
                  >
                    {t(mod.modType.toLowerCase(), { source: modSource })}
                  </span>
                  <span className="flex items-center gap-1">
                    <MdDownload className="size-3.5" />
                    {downloadCount}
                  </span>
                  {mod.source === ModSource.Modrinth && mod.license && (
                    <span>{t("licensedUnder", { license: mod.license })}</span>
                  )}
                </div>
                {mod.description.trim() && (
                  <p className="text-sm leading-snug text-slate-300 text-pretty">
                    {mod.description}
                  </p>
                )}
                {owners.length > 0 && (
                  <div className="flex flex-wrap items-center gap-1.5 mt-1">
                    {owners.map((owner) => (
                      <button
                        key={owner.name}
                        onClick={() => void openIn(owner.url)}
                        title={t("openInTheWeb")}
                        className="flex items-center gap-1 h-7 px-2.5 rounded-full bg-slate-700 text-xs font-bold text-slate-200 hover:bg-slate-600 cursor-pointer"
                      >
                        <MdPerson className="size-3.5" />
                        {owner.name}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            </div>

            {mod.thumbnailUrls.length > 0 && (
              <div className="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-4 gap-2 mt-4">
                {mod.thumbnailUrls.map((thumbnail) => (
                  <img
                    key={thumbnail}
                    src={thumbnail}
                    alt=""
                    onClick={() => void openIn(thumbnail)}
                    className="w-full aspect-video rounded-2xl object-cover bg-slate-900 cursor-pointer hover:brightness-110 transition-[filter]"
                  />
                ))}
              </div>
            )}
          </section>
          {deps.length > 0 && (
            <section
              data-expand-rest
              className={panelClass + " flex flex-col gap-2"}
            >
              <div className="flex items-center gap-2 text-[13px] font-extrabold text-slate-400">
                <MdExtension className="size-4" />
                {t("dependencies").replace(/:\s*$/, "")}
                <span className="min-w-5 h-5 px-1.5 rounded-full bg-blue-600 text-white text-[11px] inline-flex items-center justify-center">
                  {deps.length}
                </span>
              </div>
              <div className="grid gap-2 md:grid-cols-2 2xl:grid-cols-3">
                {deps.map((dependency) => (
                  <Mod
                    key={dependency.id}
                    mod={dependency}
                    modpack={undefined}
                    className="w-full"
                    installed={isInstalledIn(dependency, selectedModpack)}
                    onInstalled={props.onInstalled}
                  />
                ))}
              </div>
            </section>
          )}
        </div>

        <div data-expand-rest className="flex flex-col gap-3">
          <section className={panelClass + " flex flex-col gap-3"}>
            {pickTargets && (
              <label>
                <span className={labelClass}>{t("chooseVersion")}</span>
                <select
                  className={selectClass}
                  name="version"
                  autoComplete="off"
                  value={version}
                  onChange={async (e) => {
                    setVersion(e.target.value);
                    await config.set("lastUsedVersion", e.target.value);
                    await config.save();
                  }}
                >
                  {version &&
                    !versions.some((entry) => entry.version === version) && (
                      <option value={version}>{version}</option>
                    )}
                  {versions.map((versionOption) => (
                    <option
                      value={versionOption.version}
                      key={versionOption.version}
                    >
                      {versionOption.version}
                    </option>
                  ))}
                </select>
              </label>
            )}

            {mod.modType === ModType.Mod && pickTargets && (
              <label>
                <span className={labelClass}>{t("choosePreferredAPI")}</span>
                <select
                  className={selectClass}
                  name="modLoader"
                  autoComplete="off"
                  value={loader}
                  onChange={async (e) => {
                    setLoader(e.target.value);
                    await config.set("lastUsedAPI", e.target.value);
                    await config.save();
                  }}
                >
                  <LoaderOptions loader={loader} providers={loaderProviders} />
                </select>
              </label>
            )}

            {mod.modType === ModType.Mod && (
              <label>
                <span className={labelClass}>{t("chooseModpack")}</span>
                <select
                  className={selectClass}
                  name="modpack"
                  autoComplete="off"
                  value={modpack}
                  onChange={async (e) => {
                    const picked = modpacks.find(
                      (i) => i.name === e.target.value,
                    );
                    setModpack(e.target.value);
                    if (!picked) {
                      return;
                    }
                    setLoader(picked.modLoader.toString());
                    setVersion(picked.version);
                    await config.set("lastUsedVersion", picked.version);
                    await config.set(
                      "lastUsedAPI",
                      picked.modLoader.toString(),
                    );
                    await config.set("lastUsedModpack", picked.name);
                    await config.save();
                  }}
                >
                  {modpacks.map((option) => (
                    <option key={option.name} value={option.name}>
                      {option.name} · {option.modLoader} · {option.version}
                    </option>
                  ))}
                </select>
              </label>
            )}

            {contentLocations.length > 1 && (
              <label>
                <span className={labelClass}>
                  {t("installedContentInstallTo")}
                </span>
                <select
                  className={selectClass}
                  name="contentLocation"
                  autoComplete="off"
                  value={contentLocation}
                  onChange={(e) => setContentLocation(e.target.value)}
                >
                  {hasInstallTarget && (
                    <option value="">
                      {t("installedContentInstallAutomatic", {
                        modpack: installTargetName,
                      })}
                    </option>
                  )}
                  {contentLocations.map((option) => (
                    <option key={option.id} value={option.id}>
                      {locationOptionLabel(option, t)}
                    </option>
                  ))}
                </select>
              </label>
            )}

            {alreadyInstalled && (
              <div className="text-[11.5px] leading-snug text-amber-300 bg-amber-900/25 border border-amber-700/20 px-3 py-2 rounded-2xl">
                {replacesOtherProvider
                  ? t("alreadyInstalledOtherProvider", {
                      modpack,
                      source: isCurseForge ? "Modrinth" : "CurseForge",
                    })
                  : t("alreadyInstalledIn", { modpack })}
              </div>
            )}
            {showProgress && (
              <div className="flex flex-col gap-1.5">
                <LinearProgress progress={modDownloadProgress} />
                <LinearProgress progress={modInstallProgress} />
              </div>
            )}
            <div className="flex flex-wrap gap-2 mt-1">{actions}</div>
          </section>
        </div>
      </div>
    </motion.div>
  );
}
