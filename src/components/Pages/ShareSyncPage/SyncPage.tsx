/** @format */

import { useEffect, useState } from "react";
import { LocalModpack, SyncContext, SyncedModpack } from "../../../intefaces";
import { getModpacks, getSyncedModpacks } from "../../../tools";

import SyncedModpackComponent from "./SyncedModpack/SyncedModpack";
import { motion } from "motion/react";
import { listen } from "@tauri-apps/api/event";

export default function SyncPage() {
  const [modpacks, setModpacks] = useState<SyncedModpack[]>([]);
  const [localModpacks, setLocalModpacks] = useState<LocalModpack[]>([]);

  const fetchSyncedModpacks = async () => {
    const syncedModpacks = await getSyncedModpacks(true);
    const localModpacks = await getModpacks(true);
    syncedModpacks.sort((a, b) => b.last_synced - a.last_synced);
    setModpacks(syncedModpacks);
    setLocalModpacks(localModpacks);
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    fetchSyncedModpacks().catch(console.error);
    listen<string>("refreshSyncedModpacks", () => {
      void fetchSyncedModpacks();
    })
      .then((cleanup) => {
        unlisten = cleanup;
      })
      .catch(console.error);

    return () => {
      unlisten?.();
    };
  }, []);

  return (
    <SyncContext.Provider
      value={{ refreshSyncedModpacks: fetchSyncedModpacks }}
    >
      <motion.div
        initial={{ y: 24, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        className="items-center justify-center flex flex-col flex-1 w-full transform-gpu backface-hidden will-change-[transform,opacity]"
      >
        <div className="bg-slate-800 flex flex-1 flex-col rounded-4xl w-[98%]  ">
          {modpacks.map((modpack) => {
            const localModpack =
              localModpacks.find(
                (candidate) => candidate.modpackId === modpack.modpack_id,
              ) ??
              localModpacks.find(
                (candidate) =>
                  !candidate.modpackId && candidate.name === modpack.name,
              );
            return (
              <SyncedModpackComponent
                modpack={modpack}
                localModpack={localModpack}
                key={modpack.modpack_id}
              ></SyncedModpackComponent>
            );
          })}
        </div>
      </motion.div>
    </SyncContext.Provider>
  );
}
