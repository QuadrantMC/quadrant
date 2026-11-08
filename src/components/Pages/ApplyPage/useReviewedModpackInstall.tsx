/** @format */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { InstalledModpack } from "../../../intefaces";
import { hasWarnings, ModpackReview, reviewMods } from "../../../modpackReview";
import ConfirmDialog from "../../core/ConfirmDialog";
import ModpackReviewWarnings from "../../shared/ModpackReviewWarnings";
import {
  ModpackInstallState,
  ModpackSyncTarget,
  useModpackInstall,
} from "./useModpackInstall";

export interface ReviewedModpackInstall extends ModpackInstallState {
  /** The dialog `install` opens; render it next to the button. */
  reviewDialog: React.ReactNode;
}

/**
 * {@link useModpackInstall}, except that a pack with files Quadrant can't
 * verify, or with mods from arbitrary links, asks first and names them.
 */
export function useReviewedModpackInstall(
  modpack: InstalledModpack,
  syncTarget?: ModpackSyncTarget,
): ReviewedModpackInstall {
  const { t } = useTranslation();
  const { install: installNow, progress } = useModpackInstall(
    modpack,
    syncTarget,
  );
  const [review, setReview] = useState<ModpackReview | null>(null);

  const install = async () => {
    const found = reviewMods(modpack.mods);
    if (hasWarnings(found)) {
      setReview(found);
      return;
    }
    await installNow();
  };

  const reviewDialog = (
    <ConfirmDialog
      open={review !== null}
      title={t("reviewInstallTitle", { name: modpack.name })}
      message={review && <ModpackReviewWarnings review={review} />}
      confirmLabel={t("reviewInstallAnyway")}
      onCancel={() => setReview(null)}
      onConfirm={installNow}
    />
  );

  return { install, progress, reviewDialog };
}
