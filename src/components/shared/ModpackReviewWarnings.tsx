/** @format */

import { useTranslation } from "react-i18next";
import { MdWarning } from "react-icons/md";
import { ModpackReview, ReviewedMod } from "../../modpackReview";

function ModList({ mods, withHost }: { mods: ReviewedMod[]; withHost: boolean }) {
  return (
    <ul className="mt-1 max-h-40 list-disc overflow-y-auto pl-6">
      {mods.map((mod, index) => (
        <li key={index} className="break-words">
          {mod.name}
          {withHost && mod.host !== "" && ` (${mod.host})`}
        </li>
      ))}
    </ul>
  );
}

/** The warnings a pack earns before it is installed, with the mods they name. */
export default function ModpackReviewWarnings({
  review,
}: {
  review: ModpackReview;
}) {
  const { t } = useTranslation();
  return (
    <div role="alert" className="space-y-3 text-left text-sm">
      {review.unverified.length > 0 && (
        <div className="rounded-3xl bg-amber-900/60 p-3">
          <p className="flex items-center font-bold">
            <MdWarning aria-hidden="true" className="mr-2 h-5 w-5 shrink-0" />
            {t("reviewUnverifiedWarning")}
          </p>
          <ModList mods={review.unverified} withHost={false} />
        </div>
      )}
      {review.online.length > 0 && (
        <div className="rounded-3xl bg-amber-900/60 p-3">
          <p className="flex items-center font-bold">
            <MdWarning aria-hidden="true" className="mr-2 h-5 w-5 shrink-0" />
            {t("reviewOnlineWarning")}
          </p>
          <ModList mods={review.online} withHost />
        </div>
      )}
    </div>
  );
}
