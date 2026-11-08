/** @format */

import { useContext, useEffect, useMemo, useState } from "react";
import { IMod, InstalledModpack, ModSource } from "../../../intefaces";
import { useTranslation } from "react-i18next";
import Button from "../../core/Button";
import CancelButton from "../../core/CancelButton";
import { getMod } from "../../../tools";
import Mod from "../../shared/Mod";
import { ContentContext } from "../../../intefaces";
import { MdDownload } from "react-icons/md";
import { hasWarnings, reviewMods } from "../../../modpackReview";
import ModpackReviewWarnings from "../../shared/ModpackReviewWarnings";
import { ModpackSyncTarget } from "./useModpackInstall";
import { useReviewedModpackInstall } from "./useReviewedModpackInstall";

export interface SharedModpackViewProps {
  modpack: InstalledModpack;
  syncTarget?: ModpackSyncTarget;
}

export default function SharedModpackView({
  modpack,
  syncTarget,
}: SharedModpackViewProps) {
  const [mods, setMods] = useState<IMod[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const { t } = useTranslation();
  const context = useContext(ContentContext);
  const { install, progress, reviewDialog } = useReviewedModpackInstall(
    modpack,
    syncTarget,
  );
  // Online mods have no provider page, so the cards below can't show them;
  // the warnings list them with their hosts.
  const review = useMemo(() => reviewMods(modpack.mods), [modpack.mods]);
  const providerMods = useMemo(
    () => modpack.mods.filter((mod) => mod.source !== ModSource.Online),
    [modpack.mods],
  );

  useEffect(() => {
    let cancelled = false;
    const effect = async () => {
      setMods([]);
      setIsLoading(true);

      const fetchMods = async () => {
        for (const mod of providerMods) {
          if (cancelled) return;
          try {
            const newMod = await getMod(
              {
                deletable: false,
                id: mod.id,
                downloadable: false,
                showPreviousVersion: false,
                versionTarget: "",
                modLoader: modpack.modLoader,
                modpack: modpack.name,
                selectable: false,
                selectUrl: null,
              },
              mod.source,
            );
            if (!cancelled) {
              setMods((prevMods) => [...prevMods, newMod]);
            }
          } catch (error) {
            console.error("Failed to fetch mod:", error);
          }
        }
        if (!cancelled) setIsLoading(false);
      };

      fetchMods().catch((error) => {
        console.error("Failed to load shared modpack", error);
        if (!cancelled) setIsLoading(false);
      });
    };
    effect().catch(console.error);
    return () => {
      cancelled = true;
    };
  }, [modpack, providerMods]);

  return (
    <div className="flex w-full min-w-0 flex-col gap-3 px-4 pt-2 pb-5">
      <CancelButton onClick={() => context.back()} />
      <section className="min-w-0 rounded-4xl bg-slate-700 p-4 font-bold">
        <div className="flex flex-wrap items-center justify-center gap-4">
          <h2 className="min-w-0 text-center text-base break-words">
            {modpack.name} | {modpack.modLoader} | {modpack.version} |{" "}
            {t("modCount", { amount: modpack.mods.length })}
          </h2>
          <Button
            className={
              "flex w-fit shrink-0 items-center px-4 " +
              (progress === 1
                ? "bg-emerald-600 hover:bg-emerald-700"
                : "bg-slate-800 cursor-not-allowed")
            }
            onClick={install}
          >
            {progress === 1 ? t("download") : (progress * 100).toFixed(2) + "%"}
            <MdDownload aria-hidden="true" className="w-6 h-6 ml-2" />
          </Button>
        </div>
        {isLoading && (
          <div
            role="status"
            className="mt-4 flex items-center justify-center gap-3 text-sm text-slate-300"
          >
            <span
              aria-hidden="true"
              className="size-6 shrink-0 animate-spin rounded-full border-2 border-slate-600 border-t-emerald-400 motion-reduce:animate-none"
            />
            {t("loadingMods", {
              loaded: mods.length,
              total: providerMods.length,
            })}
          </div>
        )}
        {hasWarnings(review) && (
          <div className="mt-4 font-normal">
            <ModpackReviewWarnings review={review} />
          </div>
        )}
      </section>
      {reviewDialog}
      {mods.length > 0 && (
        <div className="grid min-w-0 grid-cols-1 gap-4 rounded-4xl bg-slate-800 p-4 md:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
          {mods.map((mod) => (
            <Mod
              key={mod.source + mod.id}
              mod={mod}
              modpack={modpack.name}
              className="w-full min-w-0"
            />
          ))}
        </div>
      )}
    </div>
  );
}
