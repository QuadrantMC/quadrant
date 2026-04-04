/** @format */

import { Tab, TabGroup, TabList, TabPanel, TabPanels } from "@headlessui/react";
import { useTranslation } from "react-i18next";
import SharePage from "./SharePage";
import SyncPage from "./SyncPage";
import { createContext, useContext, useEffect, useState } from "react";
import { getAccountInfo } from "../../../tools";
import { motion } from "motion/react";
import { InstalledModpack } from "../../../intefaces";
import { listen } from "@tauri-apps/api/event";
import { ContentContext } from "../../../intefaces";
import { MdCheck } from "react-icons/md";

export interface IShareSyncContext {
  changeTab: (index: number) => void;
  setModpack: (modpack: InstalledModpack) => void;
  setSync: (time: number) => void;
  setModpackId: (modpackId: string | null) => void;
}

export const ShareSyncContext = createContext<IShareSyncContext>({
  changeTab: () => {},
  setModpack: () => {},
  setSync: () => {},
  setModpackId: () => {},
});

export default function ShareSyncPage() {
  const { t } = useTranslation();

  const [syncActive, setSyncActive] = useState(false);
  const [selectedTab, setSelectedTab] = useState(0);
  const [preselectedModpack, setPreselectedModpack] = useState<
    InstalledModpack | undefined
  >();

  const [modpackSync, setModpackSync] = useState<number | null>(null);
  const [modpackId, setModpackId] = useState<string | null>(null);

  const contentContext = useContext(ContentContext);

  useEffect(() => {
    let isUnmounted = false;
    const cleanupFns: Array<() => void> = [];

    const effect = async () => {
      try {
        const accountInfo = await getAccountInfo();
        if (!isUnmounted && accountInfo.quadrant_sync_limit !== 0) {
          setSyncActive(true);
        }

        const unlisten = await listen(
          "quadrantShareSubmission",
          async (event: any) => {
            const usesLeft = event.payload.uses_left;
            if (isUnmounted) {
              return;
            }
            contentContext.setSnackbar({
              message: (
                <span className="flex">
                  <MdCheck className="w-6 h-6 mx-2" />
                  {t("copiedToClipboard", { amount: usesLeft })}
                </span>
              ),
              className: "bg-emerald-700 rounded-4xl",
              timeout: 5000,
            });
          },
        );
        if (isUnmounted) {
          unlisten();
        } else {
          cleanupFns.push(unlisten);
        }
      } catch (e) {
        console.error(e);
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
  }, [contentContext, t]);

  const MotionTab = motion(Tab);

  return (
    <motion.div
      initial={{ y: 24, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      exit={{ y: 24, opacity: 0 }}
      className="flex flex-1 flex-col items-center w-full h-full transform-gpu backface-hidden will-change-[transform,opacity]"
    >
      <ShareSyncContext.Provider
        value={{
          changeTab: (index) => setSelectedTab(index),
          setModpack: (modpack) => setPreselectedModpack(modpack),
          setSync: (time) => setModpackSync(time),
          setModpackId: (newModpackId) => setModpackId(newModpackId),
        }}
      >
        <TabGroup
          selectedIndex={selectedTab}
          onChange={setSelectedTab}
          className="w-full flex flex-col my-4 items-center justify-start h-[90%]"
        >
          {syncActive && (
            <motion.div
              className="w-[75%] justify-center items-start align-top text-center rounded-4xl flex h-min"
              initial={{ y: -24, opacity: 0.1 }}
              animate={{ y: 0, opacity: 1 }}
              transition={{
                bounce: 1,
                stiffness: 0,
                type: "tween",
              }}
            >
              <TabList className="flex flex-row w-[75%] h-min align-top rounded-4xl font-extrabold text-center items-start justify-center my-2">
                <MotionTab
                  onClick={() => {
                    setModpackSync(null);
                    setPreselectedModpack(undefined);
                    setModpackId(null);
                  }}
                  whileHover={{ scale: 1.1, y: -5 }}
                  whileTap={{ scale: 0.9 }}
                  className="flex flex-col w-max hover:bg-cyan-300 bg-slate-800 p-4 rounded-4xl mx-4 text-center data-selected:bg-cyan-300 data-selected:text-slate-900 hover:text-slate-900 data-selected:shadow-cyan-300 ease-linear duration-300"
                >
                  {t("importMods")}
                </MotionTab>
                <MotionTab
                  whileHover={{ scale: 1.1, y: -5 }}
                  whileTap={{ scale: 0.9 }}
                  className="flex flex-col w-max hover:bg-sky-300 bg-slate-800 p-4 rounded-4xl mx-4 text-center data-selected:bg-sky-300 data-selected:text-slate-900 hover:text-slate-900 ease-linear duration-300"
                >
                  {t("Quadrant Sync")}
                </MotionTab>
              </TabList>
            </motion.div>
          )}
          <TabPanels className="flex flex-col w-full h-full items-center justify-center mt-2">
            <TabPanel
              className={
                "w-full h-full flex flex-col items-center justify-center"
              }
            >
              <SharePage
                preselectedModpack={preselectedModpack}
                modpackSync={modpackSync}
                modpackId={modpackId}
              />
            </TabPanel>
            <TabPanel className={"w-full h-full flex-col flex items-center"}>
              <SyncPage />
            </TabPanel>
          </TabPanels>
        </TabGroup>
      </ShareSyncContext.Provider>
    </motion.div>
  );
}
