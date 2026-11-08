/** @format */

import { useContext, useEffect, useRef, useState } from "react";
import {
  ContentContext,
  LocalModpack,
  MinecraftVersion,
  ModLoader,
} from "../../../intefaces";
import { createModpack, updateModpack } from "../../../tools";
import { useTranslation } from "react-i18next";
import Button from "../../core/Button";
import CancelButton from "../../core/CancelButton";
import { AnimatePresence, motion } from "motion/react";
import {
  Description,
  Dialog,
  DialogBackdrop,
  DialogPanel,
  DialogTitle,
  Field,
  Fieldset,
  Input,
  Label,
  Select,
} from "@headlessui/react";
import LoaderOptions from "../../shared/LoaderOption";
import { ModLoaderProvider } from "../../../modLoaders";
import { useReportError } from "../../../useReportError";

export interface ModpackEditDialogProps {
  open: boolean;
  onClose: () => void;
  isCreate: boolean;
  versions: MinecraftVersion[];
  loaderProviders: ModLoaderProvider[];
  initial: LocalModpack;
  originalName: string;
  onSaved: () => void | Promise<void>;
}

export default function ModpackEditDialog({
  open,
  onClose,
  isCreate,
  versions,
  loaderProviders,
  initial,
  originalName,
  onSaved,
}: ModpackEditDialogProps) {
  const { t } = useTranslation();
  const context = useContext(ContentContext);
  const reportError = useReportError();
  const [draft, setDraft] = useState(initial);
  const savingRef = useRef(false);

  // Re-seed on each open: the parent sets the target modpack and flips `open`
  // in the same handler, so opening is the only moment the draft is authoritative.
  useEffect(() => {
    if (open) {
      setDraft(initial);
    }
  }, [open, initial]);

  // A new pack needs a loader too; an existing one may predate that choice.
  const invalidReason =
    draft.name.trim().length === 0
      ? t("modpackNameRequired")
      : draft.version === ""
        ? t("modpackVersionRequired")
        : isCreate && draft.modLoader === ModLoader.Unknown
          ? t("modpackLoaderRequired")
          : null;

  const save = async () => {
    if (invalidReason !== null || savingRef.current) {
      return;
    }
    savingRef.current = true;
    try {
      if (isCreate) {
        await createModpack(draft.name, draft.version, draft.modLoader);
        await onSaved();
      } else {
        await updateModpack(originalName, draft);
        await onSaved();
      }
      context.setSnackbar({
        message: draft.name + " | " + draft.version + " | " + draft.modLoader,
        className: "bg-emerald-600 rounded-4xl",
        timeout: 5000,
      });
      onClose();
    } catch (e: any) {
      // The dialog stays open so a taken or invalid name can be corrected.
      reportError(e);
    } finally {
      savingRef.current = false;
    }
  };

  return (
    <Dialog open={open} onClose={onClose} className="relative z-50">
      <DialogBackdrop className="fixed inset-0 bg-slate-950/30 " />
      <AnimatePresence>
        <motion.div
          initial={{ opacity: 0, y: -24, scale: 0.98 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 16, scale: 0.98 }}
          className="fixed inset-0 flex w-screen items-center justify-center p-4"
        >
          <DialogPanel className="max-w-xl space-y-4 rounded-4xl bg-slate-800 p-8">
            <DialogTitle className="font-black text-xl">
              {isCreate ? t("createModpack") : t("update")}
            </DialogTitle>
            <Description>
              {isCreate ? t("createModpack") : t("updateModpackDetails")}
            </Description>
            <Fieldset>
              <Field>
                <Label className="block my-2 font-bold">
                  {t("chooseVersion")}
                </Label>
                <Select
                  className="bg-slate-700 w-full p-2 rounded-4xl font-semibold hover:bg-slate-600"
                  name="version"
                  value={draft.version}
                  onChange={(newValue) => {
                    const modpack: LocalModpack = JSON.parse(
                      JSON.stringify(draft),
                    );
                    modpack.version = newValue.target.value;
                    setDraft(modpack);
                  }}
                >
                  {draft.version === "" && (
                    <option value={""} className="rounded-4xl font-semibold">
                      -
                    </option>
                  )}
                  {versions.map((version) => {
                    return (
                      <option
                        value={version.version}
                        defaultChecked={version.version == draft.version}
                        className="rounded-4xl font-semibold"
                        key={version.version}
                      >
                        {version.version}
                      </option>
                    );
                  })}
                </Select>
              </Field>
              <Field>
                <Label className="block my-2 font-bold">
                  {t("choosePreferredAPI")}
                </Label>
                <Select
                  className="bg-slate-700 w-full p-2 rounded-4xl font-semibold hover:bg-slate-600"
                  name="modLoader"
                  value={draft.modLoader}
                  onChange={(newValue) => {
                    const modpack: LocalModpack = JSON.parse(
                      JSON.stringify(draft),
                    );
                    modpack.modLoader = newValue.target.value as ModLoader;
                    setDraft(modpack);
                  }}
                >
                  <LoaderOptions
                    loader={draft.modLoader}
                    providers={loaderProviders}
                  />
                </Select>
              </Field>
              <Field>
                <Label className="block my-2 font-bold">
                  {isCreate ? t("name") : t("chooseModpack")}
                </Label>
                <Input
                  className="bg-slate-700 focus:bg-slate-600 focus: focus:border-2 focus:border-slate-500 w-full p-2 rounded-4xl font-semibold hover:bg-slate-600"
                  name="modLoader"
                  type="text"
                  value={draft.name}
                  autoComplete="off"
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      void save();
                    }
                  }}
                  onChange={(newValue) => {
                    const modpack: LocalModpack = JSON.parse(
                      JSON.stringify(draft),
                    );
                    const newName = newValue.target.value.replace(
                      /[<>:"/\\|?*]/g,
                      "",
                    );

                    modpack.name = newName;
                    setDraft(modpack);
                  }}
                ></Input>
              </Field>
            </Fieldset>
            {invalidReason !== null && (
              <p className="text-sm text-slate-400">{invalidReason}</p>
            )}
            <div className="flex gap-4">
              <Button
                className="bg-emerald-600 hover:bg-emerald-700"
                disabled={invalidReason !== null}
                onClick={save}
              >
                {isCreate ? t("createModpack") : t("update")}
              </Button>
              <CancelButton onClick={onClose} />
            </div>
          </DialogPanel>
        </motion.div>
      </AnimatePresence>
    </Dialog>
  );
}
