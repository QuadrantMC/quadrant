/** @format */

import { useTranslation } from "react-i18next";
import { ContentLocation } from "../../../intefaces";
import { locationTitle } from "../../../contentLocations";
import CopyToMenu from "./CopyToMenu";
import { CopyTarget, SectionFiles } from "./contentActions";

export interface BulkCopyMenuProps {
  getTargets: () => CopyTarget[];
  /** The control this menu speaks for, so only it shows as busy. */
  controlKey: string;
  /** Screen-reader name of the button, naming what would be copied. */
  label: string;
  /** Visible button text. */
  text: string;
  /** The key of the running action, or null while nothing is running. */
  pending: string | null;
  onPick: (to: ContentLocation, groups: SectionFiles[]) => void;
}

/**
 * A "copy these somewhere else" menu, counting the work each destination
 * would take. Every bulk control offers the same choice, so they share this.
 */
export default function BulkCopyMenu({
  getTargets,
  controlKey,
  label,
  text,
  pending,
  onPick,
}: BulkCopyMenuProps) {
  const { t } = useTranslation();
  return (
    <CopyToMenu
      text={text}
      label={label}
      busy={pending === controlKey}
      disabled={pending !== null}
      getTargets={getTargets}
      optionLabel={(target) =>
        t("installedContentCopyMissing", {
          name: locationTitle(target.location, t),
          missing: target.count,
        })
      }
      onPick={(target) => onPick(target.location, target.groups)}
    />
  );
}
