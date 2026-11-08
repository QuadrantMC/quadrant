/** @format */

import { useTranslation } from "react-i18next";
import { MdErrorOutline } from "react-icons/md";

/** Why a modpack whose manifest can't be read is listed but locked. */
export default function BrokenModpackNotice({ error }: { error: string }) {
  const { t } = useTranslation();
  return (
    <div
      role="alert"
      className="my-2 flex items-start rounded-3xl border border-red-400/40 bg-red-950/60 p-3 text-left text-sm font-bold text-red-100"
    >
      <MdErrorOutline aria-hidden="true" className="mr-2 h-5 w-5 shrink-0" />
      <p className="min-w-0 break-words">{t("modpackUnreadable", { error })}</p>
    </div>
  );
}
