/** @format */

import { lazy, useContext } from "react";
import { useTranslation } from "react-i18next";
import { motion } from "motion/react";
import { MdDownload, MdInfo, MdShare } from "react-icons/md";
import {
  AccountInfo,
  ContentContext,
  InstalledModpack,
  SyncedModpack,
} from "../../../intefaces";
import { shareModpackRaw } from "../../../tools";
import Button from "../../core/Button";
import CloudMembersPanel from "./CloudMembersPanel";
import ModpackBadges from "./ModpackBadges";
import { formatSyncDate } from "./syncDates";
import { useModpackInstall } from "./useModpackInstall";
import { useReportError } from "../../../useReportError";

export interface SyncedModpackProps {
  modpack: SyncedModpack;
  accountInfo: AccountInfo | null;
}

const SharedModpackView = lazy(() => import("./SharedModpackView"));

/** A Quadrant Sync modpack that is not installed locally. */
export default function SyncedModpackComponent({
  modpack,
  accountInfo,
}: SyncedModpackProps) {
  const { t } = useTranslation();
  const reportError = useReportError();
  const contentContext = useContext(ContentContext);

  const modConfigObject: InstalledModpack = {
    name: modpack.name,
    mods: JSON.parse(modpack.mods),
    modLoader: modpack.mod_loader,
    version: modpack.minecraft_version,
  };
  const syncTarget = {
    syncedAt: modpack.last_synced,
    modpackId: modpack.modpack_id,
  };
  const { install, progress } = useModpackInstall(
    modConfigObject,
    syncTarget,
  );

  const openDetails = () => {
    contentContext.changeContent({
      content: (
        <SharedModpackView modpack={modConfigObject} syncTarget={syncTarget} />
      ),
      name: modpack.modpack_id + Math.random().toString(36).substring(2, 10),
      title: modpack.name,
      style: "",
      main: false,
      icon: <></>,
    });
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 24 }}
      animate={{ y: 0, opacity: 1 }}
      whileHover={{ y: -5 }}
      transition={{ duration: 0.15, ease: "linear" }}
      exit={{ opacity: 0, y: -24 }}
      className="flex flex-col bg-slate-900 hover:bg-slate-950 p-4 rounded-4xl mx-5 my-5 h-max hover:shadow-lg hover:shadow-slate-950 transform-gpu backface-hidden will-change-[transform,opacity]"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h1 className="text-2xl font-extrabold max-w-full w-fit">
          {modpack.name}
        </h1>
        <ModpackBadges installed={false} synced />
      </div>
      <p className="text-md text-slate-400">
        {modpack.minecraft_version} | {modpack.mod_loader} |{" "}
        {t("modCount", { amount: modConfigObject.mods.length })} |{" "}
        {t("cloudSyncDate", { date: formatSyncDate(modpack.last_synced) })}
      </p>
      <div className="my-2 flex overflow-x-auto flex-wrap h-max flex-row items-center text-sm justify-start text-center w-full">
        <Button
          className={
            "flex items-center self-center px-4 w-max h-10 justify-center m-2 " +
            (progress === 1
              ? "bg-emerald-600 hover:bg-emerald-700"
              : "bg-slate-800 cursor-not-allowed")
          }
          onClick={install}
        >
          {progress === 1 ? t("download") : (progress * 100).toFixed(2) + "%"}
          <MdDownload className="w-5 h-5 mx-2" />
        </Button>
        <Button
          className="flex items-center self-center bg-blue-600 hover:bg-blue-700 px-4 m-2 w-max h-10 justify-center"
          onClick={async () => {
            try {
              await shareModpackRaw(modConfigObject);
            } catch (e: any) {
              reportError(e);
            }
          }}
        >
          {t("share")}
          <MdShare className="w-5 h-5 mx-2" />
        </Button>
        <Button
          className="flex items-center self-center bg-slate-800 hover:bg-slate-700 px-4 w-max m-2 h-10 justify-center"
          onClick={openDetails}
        >
          {t("details")}
          <MdInfo className="w-5 h-5 mx-2" />
        </Button>
      </div>
      <CloudMembersPanel modpack={modpack} accountInfo={accountInfo} />
    </motion.div>
  );
}
