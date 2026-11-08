/** @format */

import { useContext, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ContentContext, InstalledModpack } from "../../../intefaces";
import { installModpack } from "../../../tools";
import { clearProgressBar, invoke, listen } from "../../../desktop";
import { useReportError } from "../../../useReportError";

// Progress events have no request id. Run these installs one at a time, even
// across page changes, and suppress duplicate writes to the same local folder.
let installQueue: Promise<void> = Promise.resolve();
const pendingInstalls = new Set<string>();

export interface ModpackSyncTarget {
  syncedAt: number;
  modpackId: string;
}

export interface ModpackInstallState {
  /** Starts the install. No-op while one is already running. */
  install: () => Promise<void>;
  /** 0..1 while installing, 1 when idle. */
  progress: number;
}

/**
 * Installs a modpack manifest into the local modpacks folder, reporting the
 * backend's download progress and a snackbar on completion or failure. When a
 * sync target is given the local sync metadata is stamped afterwards so the
 * pack pairs with its Quadrant Sync record.
 */
export function useModpackInstall(
  modpack: InstalledModpack,
  syncTarget?: ModpackSyncTarget,
): ModpackInstallState {
  const { t } = useTranslation();
  const reportError = useReportError();
  const context = useContext(ContentContext);
  const [progress, setProgress] = useState(1);
  const requestedRef = useRef(false);
  const activeRef = useRef(false);

  useEffect(() => {
    let isUnmounted = false;
    let unlisten: (() => void | Promise<void>) | null = null;

    const attachListener = async () => {
      unlisten = await listen("modpackDownloadProgress", (event: any) => {
        // The event is global; only the instance that started the install
        // should reflect it.
        if (isUnmounted || !activeRef.current) {
          return;
        }
        // Only the command promise confirms success, including sync metadata.
        setProgress(Math.min(event.payload, 0.99));
      });

      if (isUnmounted && unlisten) {
        await unlisten();
        unlisten = null;
      }
    };

    attachListener().catch(console.error);

    return () => {
      isUnmounted = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  const install = async () => {
    if (requestedRef.current || pendingInstalls.has(modpack.name)) {
      return;
    }
    requestedRef.current = true;
    pendingInstalls.add(modpack.name);
    setProgress(0);
    const operation = installQueue.then(async () => {
      activeRef.current = true;
      try {
        await installModpack(modpack);
        if (syncTarget) {
          await invoke("set_modpack_sync_date", {
            time: syncTarget.syncedAt,
            modpack: modpack.name,
            modpackId: syncTarget.modpackId,
          });
        }
        context.setSnackbar({
          className: "bg-emerald-600",
          message: t("downloadSuccess"),
          timeout: 5000,
        });
      } catch (e: any) {
        // A failed install leaves its last percentage on the taskbar.
        void clearProgressBar().catch(console.error);
        reportError(e);
      } finally {
        activeRef.current = false;
        requestedRef.current = false;
        pendingInstalls.delete(modpack.name);
        setProgress(1);
      }
    });
    installQueue = operation.catch(console.error);
    await operation;
  };

  return { install, progress };
}
