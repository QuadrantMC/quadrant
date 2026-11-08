/** @format */

import {
  Dialog,
  DialogBackdrop,
  DialogPanel,
  DialogTitle,
} from "@headlessui/react";
import Button from "./Button";
import CancelButton from "./CancelButton";

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: React.ReactNode;
  confirmLabel: string;
  onConfirm: () => void | Promise<unknown>;
  onCancel: () => void;
}

/** Asks before an action that cannot be undone. */
export default function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Dialog open={open} onClose={onCancel} className="relative z-50">
      <DialogBackdrop className="fixed inset-0 opacity-60 bg-slate-950/30" />
      <div className="fixed inset-0 flex w-screen items-center justify-center p-4">
        <DialogPanel className="max-w-xl space-y-4 rounded-4xl bg-slate-800 p-8">
          <DialogTitle className="font-black text-xl">{title}</DialogTitle>
          <div>{message}</div>
          <div className="flex">
            <Button
              className="bg-red-700 hover:bg-red-600 w-full"
              onClick={async () => {
                // Started before closing, so it still sees the state it was asked in.
                const done = onConfirm();
                onCancel();
                await done;
              }}
            >
              {confirmLabel}
            </Button>
            <CancelButton className="ml-2 w-full" onClick={onCancel} />
          </div>
        </DialogPanel>
      </div>
    </Dialog>
  );
}
