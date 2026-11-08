/** @format */

import { useEffect, useRef, useState } from "react";
import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import { motion } from "motion/react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { MdDelete, MdExpandMore, MdFolder, MdRefresh } from "react-icons/md";
import {
  ContentFile,
  ContentLocation,
  ContentSection,
} from "../../../intefaces";
import {
  copyContent,
  deleteContent,
  getInstalledContent,
  openContentFolder,
} from "../../../tools";
import { locationTitle, sectionTitle } from "../../../contentLocations";
import quadrantLocale from "../../../i18n";
import Button from "../../core/Button";
import CircularProgress from "../../core/CircularProgress";
import BulkCopyMenu from "./BulkCopyMenu";
import CopyToMenu from "./CopyToMenu";
import {
  controlKey,
  copyAllControlKey,
  copyTargets,
  countFiles,
  FileNameIndex,
  groupBySection,
  indexFileNames,
  pruneSelection,
  SectionFiles,
  SectionRef,
  selectAllIn,
  selectedIn,
  SelectedFile,
  selectionControlKey,
  toggleSelection,
} from "./contentActions";
import { useReportError } from "../../../useReportError";
import { useReportSuccess } from "../../../useReportSuccess";

const cardClass = "flex flex-col bg-slate-900 p-4 rounded-4xl mx-5 my-5 h-max";
const actionClass =
  "flex items-center justify-center h-10 shrink-0 text-sm bg-slate-700 hover:bg-slate-600 px-4 w-max";

/** How long an armed delete waits for its confirming click. */
const ARM_TIMEOUT_MS = 4000;

interface SizeUnit {
  /** An `Intl.NumberFormat` unit identifier. */
  unit: string;
  bytes: number;
}

const SIZE_UNITS: SizeUnit[] = [
  { unit: "byte", bytes: 1 },
  { unit: "kilobyte", bytes: 1024 },
  { unit: "megabyte", bytes: 1024 ** 2 },
  { unit: "gigabyte", bytes: 1024 ** 3 },
];

// Building an `Intl` formatter is expensive and every row needs one, so they
// are kept per locale — the language can change while the app runs.
const sizeFormatters = new Map<string, Intl.NumberFormat>();
const dateFormatters = new Map<string, Intl.DateTimeFormat>();

function cached<T>(cache: Map<string, T>, key: string, build: () => T): T {
  const hit = cache.get(key);
  if (hit !== undefined) {
    return hit;
  }
  const built = build();
  cache.set(key, built);
  return built;
}

function formatSize(bytes: number): string {
  let chosen = SIZE_UNITS[0];
  for (const candidate of SIZE_UNITS) {
    if (bytes >= candidate.bytes) {
      chosen = candidate;
    }
  }
  const locale = quadrantLocale.language;
  const formatter = cached(
    sizeFormatters,
    `${locale}|${chosen.unit}`,
    () =>
      new Intl.NumberFormat(locale, {
        style: "unit",
        unit: chosen.unit,
        unitDisplay: "narrow",
        maximumFractionDigits: 1,
      }),
  );
  return formatter.format(bytes / chosen.bytes);
}

function formatModified(modified: number): string {
  const locale = quadrantLocale.language;
  const formatter = cached(
    dateFormatters,
    locale,
    () =>
      new Intl.DateTimeFormat(locale, {
        day: "2-digit",
        month: "2-digit",
        year: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      }),
  );
  return formatter.format(new Date(modified));
}

function describeFile(file: ContentFile, t: TFunction): string {
  const parts = [
    file.isDirectory ? t("installedContentFolderEntry") : formatSize(file.size),
  ];
  if (file.modified !== 0) {
    parts.push(formatModified(file.modified));
  }
  return parts.join(" · ");
}

interface CopyRequest {
  /** The control the copy was started from. */
  key: string;
  to: ContentLocation;
  /** One `copyContent` each, run in order. */
  groups: SectionFiles[];
}

interface DeleteRequest {
  /** The control the delete was started from. */
  key: string;
  /** One `deleteContent` each, run in order. */
  groups: SectionFiles[];
}

interface ContentActions {
  locations: ContentLocation[];
  /** What each location holds, so no control has to derive it per render. */
  nameIndex: FileNameIndex;
  /** The running copy or delete's key, or null while nothing is running. */
  pending: string | null;
  /** The delete control waiting for its confirming click, or null. */
  armedKey: string | null;
  selection: SelectedFile[];
  copy: (request: CopyRequest) => void;
  /** Arms the control, or runs the delete when it is already armed. */
  askDelete: (request: DeleteRequest) => void;
  disarm: () => void;
  select: (next: SelectedFile[]) => void;
}

interface DeleteButtonProps {
  request: DeleteRequest;
  actions: ContentActions;
  label: string;
  armedLabel: string;
  /** Shown while idle. The per-file control is icon-only and leaves it out. */
  text?: string;
  armedText: string;
}

function DeleteButton({
  request,
  actions,
  label,
  armedLabel,
  text,
  armedText,
}: DeleteButtonProps) {
  const armed = actions.armedKey === request.key;
  // An armed control always says so; only an idle per-file one is icon-only.
  const labelled = armed || text !== undefined;
  return (
    <Button
      onClick={() => actions.askDelete(request)}
      onBlur={() => {
        if (armed) {
          actions.disarm();
        }
      }}
      disabled={actions.pending !== null}
      aria-label={armed ? armedLabel : label}
      title={armed ? armedLabel : label}
      className={
        "flex items-center justify-center h-10 shrink-0 text-sm " +
        (armed
          ? "bg-red-700 hover:bg-red-600 "
          : "bg-slate-800 hover:bg-red-700 ") +
        (labelled ? "px-4 w-max" : "w-10")
      }
    >
      {armed ? armedText : text}
      <MdDelete
        aria-hidden="true"
        className={"w-5 h-5 " + (labelled ? "ml-2" : "")}
      />
    </Button>
  );
}

interface ContentFileRowProps {
  section: SectionRef;
  file: ContentFile;
  actions: ContentActions;
  selected: boolean;
}

function ContentFileRow({
  section,
  file,
  actions,
  selected,
}: ContentFileRowProps) {
  const { t } = useTranslation();
  const rowKey = controlKey(section, file.fileName);
  const entry: SelectedFile = { ...section, fileName: file.fileName };

  return (
    <li className="bg-slate-900 rounded-4xl px-4 py-2 my-1 flex items-center gap-3">
      <input
        type="checkbox"
        aria-label={file.fileName}
        checked={selected}
        disabled={actions.pending !== null}
        onChange={() =>
          actions.select(toggleSelection(actions.selection, entry))
        }
        className="shrink-0 w-5 h-5 accent-emerald-600 hover:cursor-pointer disabled:cursor-default"
      />
      <div className="min-w-0 flex-1">
        <p className="font-bold break-words">{file.fileName}</p>
        <p className="text-md text-slate-400">{describeFile(file, t)}</p>
      </div>
      {actions.locations.length > 1 && (
        <CopyToMenu
          label={t("installedContentCopyFileLabel", { file: file.fileName })}
          busy={actions.pending === rowKey}
          disabled={actions.pending !== null}
          getTargets={() =>
            copyTargets(actions.locations, actions.nameIndex, [entry])
          }
          optionLabel={(target) => locationTitle(target.location, t)}
          onPick={(target) =>
            actions.copy({
              key: rowKey,
              to: target.location,
              groups: target.groups,
            })
          }
        />
      )}
      <DeleteButton
        request={{
          key: rowKey,
          groups: [{ ...section, fileNames: [file.fileName] }],
        }}
        actions={actions}
        label={t("installedContentDeleteLabel", { file: file.fileName })}
        armedLabel={t("installedContentDeleteConfirmLabel", {
          file: file.fileName,
        })}
        armedText={t("installedContentDeleteConfirm")}
      />
    </li>
  );
}

interface SelectionBarProps {
  actions: ContentActions;
}

/**
 * One bar for the whole page: a tick made anywhere counts towards it, so a
 * selection spanning two sections or two locations acts as one.
 */
function SelectionBar({ actions }: SelectionBarProps) {
  const { t } = useTranslation();
  const groups = groupBySection(actions.selection);

  return (
    <div className="sticky top-0 z-20 flex flex-wrap items-center gap-2 bg-slate-900 rounded-4xl px-4 py-3 mx-6 mt-1">
      <span className="font-extrabold text-sm mr-auto">
        {t("installedContentSelected", { count: actions.selection.length })}
      </span>
      <Button
        onClick={() => actions.select([])}
        disabled={actions.pending !== null}
        className={actionClass}
      >
        {t("installedContentClearSelection")}
      </Button>
      {actions.locations.length > 1 && (
        <BulkCopyMenu
          text={t("installedContentCopySelected")}
          label={t("installedContentCopySelectedLabel")}
          controlKey={selectionControlKey}
          pending={actions.pending}
          getTargets={() =>
            copyTargets(actions.locations, actions.nameIndex, actions.selection)
          }
          onPick={(to, picked) =>
            actions.copy({ key: selectionControlKey, to, groups: picked })
          }
        />
      )}
      <DeleteButton
        request={{ key: selectionControlKey, groups }}
        actions={actions}
        label={t("installedContentDeleteSelected")}
        text={t("installedContentDeleteSelected")}
        armedLabel={t("installedContentDeleteSelectedConfirm", {
          amount: actions.selection.length,
        })}
        armedText={t("installedContentDeleteSelectedConfirm", {
          amount: actions.selection.length,
        })}
      />
    </div>
  );
}

interface ContentSectionViewProps {
  location: ContentLocation;
  section: ContentSection;
  actions: ContentActions;
}

function ContentSectionView({
  location,
  section,
  actions,
}: ContentSectionViewProps) {
  const { t } = useTranslation();
  const reportError = useReportError();
  const ref: SectionRef = {
    locationId: location.id,
    modType: section.modType,
  };
  const title = sectionTitle(section.modType, t);
  const locationName = locationTitle(location, t);
  const selected = new Set(selectedIn(actions.selection, ref));
  const allNames = section.files.map((file) => file.fileName);
  const allFiles = allNames.map((fileName) => ({ ...ref, fileName }));
  const allKey = copyAllControlKey(ref);

  return (
    <Disclosure as="div" className="bg-slate-800 rounded-4xl p-4 my-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <DisclosureButton className="group flex min-w-0 flex-1 items-center gap-x-3 text-start hover:cursor-pointer">
          <span className="font-extrabold text-lg">{title}</span>
          <span className="text-md text-slate-400">{section.files.length}</span>
          <MdExpandMore
            aria-hidden="true"
            className="h-5 w-5 shrink-0 group-data-open:rotate-180"
          />
        </DisclosureButton>
        {actions.locations.length > 1 && section.files.length > 0 && (
          <BulkCopyMenu
            text={t("installedContentCopyAll")}
            label={t("installedContentCopyAllLabel", {
              section: title,
              name: locationName,
            })}
            controlKey={allKey}
            pending={actions.pending}
            getTargets={() =>
              copyTargets(actions.locations, actions.nameIndex, allFiles)
            }
            onPick={(to, picked) =>
              actions.copy({ key: allKey, to, groups: picked })
            }
          />
        )}
        <Button
          onClick={async () => {
            try {
              await openContentFolder(location.id, section.modType);
            } catch (e: any) {
              reportError(e);
            }
          }}
          className={actionClass + " self-center"}
          aria-label={t("installedContentOpenFolderLabel", {
            section: title,
            name: locationName,
          })}
        >
          {t("installedContentOpenFolder")}
          <MdFolder aria-hidden="true" className="w-5 h-5 mx-2" />
        </Button>
      </div>
      <DisclosurePanel>
        {section.files.length === 0 ? (
          <p className="text-md text-slate-400 mt-3">
            {t("installedContentEmpty")}
          </p>
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2 mt-3">
              <Button
                onClick={() =>
                  actions.select(selectAllIn(actions.selection, ref, allNames))
                }
                disabled={actions.pending !== null}
                aria-label={t("installedContentSelectAllLabel", {
                  section: title,
                  name: locationName,
                })}
                className={actionClass}
              >
                {t("installedContentSelectAll")}
              </Button>
            </div>
            <ul className="mt-3 flex flex-col">
              {section.files.map((file) => (
                <ContentFileRow
                  key={file.fileName}
                  section={ref}
                  file={file}
                  actions={actions}
                  selected={selected.has(file.fileName)}
                />
              ))}
            </ul>
          </>
        )}
      </DisclosurePanel>
    </Disclosure>
  );
}

interface LocationCardProps {
  location: ContentLocation;
  /** The first location is always expanded; the rest fold away. */
  collapsible: boolean;
  actions: ContentActions;
}

function LocationCard({ location, collapsible, actions }: LocationCardProps) {
  const { t } = useTranslation();
  const title = locationTitle(location, t);
  const hasContent = location.sections.some(
    (section) => section.files.length > 0,
  );

  const header = (
    <div className="min-w-0 flex-1 text-start">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="text-2xl font-extrabold break-words">{title}</span>
        {location.kind === "prism" && (
          <span className="bg-slate-700 rounded-full px-3 py-1 text-xs font-extrabold text-slate-300">
            {t("prismLauncher")}
          </span>
        )}
      </div>
      <span className="block text-md text-slate-400 break-all">
        {location.path}
      </span>
    </div>
  );

  const sections = location.sections.map((section) => (
    <ContentSectionView
      key={section.modType}
      location={location}
      section={section}
      actions={actions}
    />
  ));

  if (!collapsible) {
    return (
      <div className={cardClass}>
        {header}
        {sections}
      </div>
    );
  }

  return (
    <Disclosure as="div" className={cardClass} defaultOpen={hasContent}>
      <DisclosureButton className="group flex w-full items-center gap-3 hover:cursor-pointer">
        {header}
        <MdExpandMore
          aria-hidden="true"
          className="h-6 w-6 shrink-0 group-data-open:rotate-180"
        />
      </DisclosureButton>
      <DisclosurePanel>{sections}</DisclosurePanel>
    </Disclosure>
  );
}

export default function InstalledContentPage() {
  const { t } = useTranslation();
  const reportError = useReportError();
  const reportSuccess = useReportSuccess();
  // `undefined` while the first load is in flight; a failed load settles on an
  // empty list so the page stays usable and the refresh button acts as the retry.
  const [locations, setLocations] = useState<ContentLocation[] | undefined>(
    undefined,
  );
  const [reloadToken, setReloadToken] = useState(0);
  const [pending, setPending] = useState<string | null>(null);
  const [armedKey, setArmedKey] = useState<string | null>(null);
  const [selection, setSelection] = useState<SelectedFile[]>([]);
  // Listing loads are stamped in start order, so a slow earlier one can never
  // overwrite a later one: the first load, the refresh button and the reload
  // after an action can all be in flight at once.
  const loadRef = useRef(0);

  /** Fetches the listing, keeping it only while it is still the newest load. */
  const loadListing = async (generation: number) => {
    const loaded = await getInstalledContent();
    if (generation !== loadRef.current) {
      return;
    }
    setLocations(loaded);
    setSelection((current) => pruneSelection(current, loaded));
  };

  useEffect(() => {
    setLocations(undefined);
    setSelection([]);
    const generation = ++loadRef.current;
    loadListing(generation).catch((error) => {
      // A load the user already superseded must not settle the page.
      if (generation === loadRef.current) {
        setLocations([]);
      }
      reportError(error);
    });
    // `reportError` is rebuilt on every App render, so depending on it would
    // turn each reported failure into another load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reloadToken]);

  // An armed delete waits for its confirming click, then forgets it was asked.
  useEffect(() => {
    if (armedKey === null) {
      return;
    }
    const id = setTimeout(() => setArmedKey(null), ARM_TIMEOUT_MS);
    return () => clearTimeout(id);
  }, [armedKey]);

  // Replaces the listing in place: clearing it first would drop the cards,
  // closing every section the user opened.
  const refresh = () => loadListing(++loadRef.current);

  /** Runs one copy or delete, announcing the message it answers with. */
  const runAction = async (key: string, work: () => Promise<string>) => {
    setPending(key);
    try {
      reportSuccess(await work());
      await refresh();
    } catch (error) {
      reportError(error);
    } finally {
      setPending(null);
    }
  };

  /**
   * Runs `each` over the groups in order, stopping at the first failure. A
   * failure that comes after something already moved still reloads, so the
   * listing never shows files that are already gone.
   */
  const runGroups = async (
    groups: SectionFiles[],
    each: (group: SectionFiles) => Promise<number>,
  ): Promise<number> => {
    let done = 0;
    for (const group of groups) {
      try {
        done += await each(group);
      } catch (error) {
        if (done > 0) {
          await refresh();
        }
        throw error;
      }
    }
    return done;
  };

  const runCopy = (request: CopyRequest) => {
    if (pending !== null || request.groups.length === 0) {
      return;
    }
    setArmedKey(null);
    void runAction(request.key, async () => {
      const copied = await runGroups(request.groups, (group) =>
        copyContent(
          group.locationId,
          request.to.id,
          group.modType,
          group.fileNames,
        ),
      );
      return t("installedContentCopied", {
        count: copied,
        name: locationTitle(request.to, t),
      });
    });
  };

  const askDelete = (request: DeleteRequest) => {
    if (pending !== null || request.groups.length === 0) {
      return;
    }
    if (armedKey !== request.key) {
      setArmedKey(request.key);
      return;
    }
    setArmedKey(null);
    const asked = countFiles(request.groups);
    void runAction(request.key, async () => {
      const removed = await runGroups(request.groups, (group) =>
        deleteContent(group.locationId, group.modType, group.fileNames),
      );
      return asked === 1
        ? t("installedContentDeleted", { file: request.groups[0].fileNames[0] })
        : t("installedContentDeletedCount", { count: removed });
    });
  };

  const actions: ContentActions = {
    locations: locations ?? [],
    nameIndex: indexFileNames(locations ?? []),
    pending,
    armedKey,
    selection,
    copy: runCopy,
    askDelete,
    disarm: () => setArmedKey(null),
    select: setSelection,
  };

  return (
    <motion.div
      initial={{ y: 24, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      exit={{ y: 24, opacity: 0 }}
      className="flex flex-1 flex-col w-full transform-gpu backface-hidden will-change-[transform,opacity]"
    >
      <div className="flex flex-row items-center gap-2 mx-8 my-4">
        <h1 className="flex-1 min-w-0 font-extrabold text-2xl">
          {t("installedContent")}
        </h1>
        <Button
          onClick={() => {
            setReloadToken((token) => token + 1);
          }}
          // A reload mid-action would swap the cards for the spinner and race
          // the reload the action does itself.
          disabled={pending !== null}
          className="flex shrink-0 items-center justify-center w-10 h-10 rounded-full bg-slate-700 text-slate-200 hover:bg-slate-600 hover:text-white disabled:opacity-50 disabled:cursor-default"
          title={t("installedContentRefresh")}
          aria-label={t("installedContentRefresh")}
        >
          <MdRefresh aria-hidden="true" className="w-5 h-5" />
        </Button>
      </div>
      {selection.length > 0 && <SelectionBar actions={actions} />}
      {locations === undefined ? (
        <div className="flex flex-1 items-center justify-center">
          <CircularProgress />
        </div>
      ) : locations.length === 0 ? null : (
        <div className="bg-slate-800 rounded-4xl mx-6 my-4">
          {locations.map((location, index) => (
            <LocationCard
              key={location.id}
              location={location}
              collapsible={index > 0}
              actions={actions}
            />
          ))}
        </div>
      )}
      <div className="h-1"></div>
    </motion.div>
  );
}
