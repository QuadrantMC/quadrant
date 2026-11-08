/** @format */

import { useEffect, useState } from "react";
import { getMinecraftFolder, getModpacks } from "../../../tools";
import { LocalModpack } from "../../../intefaces";
import ModpackView from "../../shared/Pages/ModpackView";
import { motion } from "motion/react";
import { joinPath, watch } from "../../../desktop";

export default function CurrentModpackPage() {
  const [currentModpack, setCurrentModpack] = useState<LocalModpack>();
  useEffect(() => {
    let cancelled = false;
    let requestId = 0;
    const updateModpack = async () => {
      const currentRequest = ++requestId;
      const newModpack = (await getModpacks(false)).find(
        (modpack) => modpack.isApplied,
      );
      if (!cancelled && currentRequest === requestId) {
        setCurrentModpack(newModpack);
      }
    };
    const effect = async () => {
      await updateModpack();
      if (cancelled) return;

      const mcFolder = await joinPath(await getMinecraftFolder(false), "mods");
      if (cancelled) return;

      const unwatchMods = await watch(
        mcFolder,
        () => {
          if (!cancelled) {
            void updateModpack().catch(console.error);
          }
        },
        {
          delayMs: 500,
        },
      );

      return unwatchMods;
    };

    let unwatch: (() => void | Promise<void>) | undefined;
    effect()
      .then((result) => {
        if (result) {
          if (!cancelled) {
            unwatch = result;
          } else {
            result();
          }
        }
      })
      .catch((error) => console.error(error));
            key={currentModpack.name}

    return () => {
      cancelled = true;
      if (unwatch) {
        unwatch();
      }
    };
  }, []);

  return (
    <>
      <motion.div
        initial={{ y: 24, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: 24, opacity: 0 }}
        className="transform-gpu [backface-visibility:hidden] [will-change:transform,opacity]"
      >
        {currentModpack !== undefined ? (
          <ModpackView
            isApplied={currentModpack.isApplied}
            lastSynced={currentModpack.lastSynced}
            modLoader={currentModpack.modLoader}
            mods={currentModpack.mods}
            name={currentModpack.name}
            version={currentModpack.version}
            unknownMods={currentModpack.unknownMods}
          ></ModpackView>
        ) : (
          <div>-</div>
        )}
      </motion.div>
    </>
  );
}
