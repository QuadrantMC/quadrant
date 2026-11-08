/** @format */

import { useEffect, useState } from "react";
import { getMinecraftFolder, getModpacks } from "../../../tools";
import { LocalModpack } from "../../../intefaces";
import ModpackView from "../../shared/Pages/ModpackView";
import { motion } from "motion/react";
import { watchModpacks } from "../../../modpackWatch";

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
    let unwatch: (() => void) | undefined;
    const effect = async () => {
      await updateModpack();
      if (cancelled) return;

      const stop = await watchModpacks(await getMinecraftFolder(false), () => {
        if (!cancelled) {
          void updateModpack().catch(console.error);
        }
      });
      if (cancelled) {
        stop();
      } else {
        unwatch = stop;
      }
    };
    effect().catch((error) => console.error(error));

    return () => {
      cancelled = true;
      unwatch?.();
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
            key={currentModpack.name}
            isApplied={currentModpack.isApplied}
            lastSynced={currentModpack.lastSynced}
            modLoader={currentModpack.modLoader}
            mods={currentModpack.mods}
            name={currentModpack.name}
            version={currentModpack.version}
            unknownMods={currentModpack.unknownMods}
            manifestError={currentModpack.manifestError}
          ></ModpackView>
        ) : (
          <div>-</div>
        )}
      </motion.div>
    </>
  );
}
