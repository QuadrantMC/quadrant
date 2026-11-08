/** @format */

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { InstalledModpack } from "../../../intefaces";
import { hasWarnings, ModpackReview, reviewMods } from "../../../modpackReview";
import { getModpacks } from "../../../tools";
import ConfirmDialog from "../../core/ConfirmDialog";
import ModpackReviewWarnings from "../../shared/ModpackReviewWarnings";
import {
  ModpackInstallState,
  ModpackSyncTarget,
  useModpackInstall,
} from "./useModpackInstall";

export interface ReviewedModpackInstall extends ModpackInstallState {
  /** The dialogs `install` opens; render them next to the button. */
  reviewDialog: React.ReactNode;
}

/**
 * {@link useModpackInstall}, except that it asks first before replacing a
 * same-named local pack, and before installing a pack with files Quadrant
 * can't verify or with mods from arbitrary links.
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
  const [isReplaceOpen, setIsReplaceOpen] = useState(false);

  const reviewThenInstall = async () => {
    const found = reviewMods(modpack.mods);
    if (hasWarnings(found)) {
      setReview(found);
      return;
    }
    await installNow();
  };

  // An install writes to the folder named after the pack, so a same-named
  // local pack would silently lose its mods, version and loader. Only the
  // user's own copy is meant to be replaced: a local pack that has no cloud
  // link yet, or is linked to the very pack being installed. One linked to a
  // different cloud pack (say, a friend's pack that shares its name) is not.
  const install = async () => {
    const taken = await getModpacks(false)
      .then((locals) =>
        locals.some(
          (local) =>
            local.name.toLowerCase() === modpack.name.toLowerCase() &&
            (!syncTarget ||
              (!!local.modpackId && local.modpackId !== syncTarget.modpackId)),
        ),
      )
      // The install reports the same failure to the user.
      .catch(() => false);
    if (taken) {
      setIsReplaceOpen(true);
      return;
    }
    await reviewThenInstall();
  };

  const reviewDialog = (
    <>
      <ConfirmDialog
        open={isReplaceOpen}
        title={t("replaceModpackQuestion", { name: modpack.name })}
        message={t("replaceModpackText", { name: modpack.name })}
        confirmLabel={t("replace")}
        onCancel={() => setIsReplaceOpen(false)}
        onConfirm={reviewThenInstall}
      />
      <ConfirmDialog
        open={review !== null}
        title={t("reviewInstallTitle", { name: modpack.name })}
        message={review && <ModpackReviewWarnings review={review} />}
        confirmLabel={t("reviewInstallAnyway")}
        onCancel={() => setReview(null)}
        onConfirm={installNow}
      />
    </>
  );

  return { install, progress, reviewDialog };
}
