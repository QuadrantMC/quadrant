/** @format */

import { Menu, MenuButton, MenuItem, MenuItems } from "@headlessui/react";
import { MdCheck, MdContentCopy } from "react-icons/md";
import { menuItemClass, menuPanelClass } from "../../core/menuClasses";
import { CopyTarget } from "./contentActions";

export interface CopyToMenuProps {
  /** Called only while the menu is open, so closed rows cost nothing. */
  getTargets: () => CopyTarget[];
  /** Names one destination. The bulk controls also count what would move. */
  optionLabel: (target: CopyTarget) => string;
  /** Screen-reader name of the button, naming what would be copied. */
  label: string;
  /** Visible button text. The compact per-file control leaves it out. */
  text?: string;
  /** This control started the copy that is running. */
  busy: boolean;
  /** Some copy is running, so no control may start another. */
  disabled: boolean;
  onPick: (target: CopyTarget) => void;
}

function TargetItems({
  getTargets,
  optionLabel,
  onPick,
}: Pick<CopyToMenuProps, "getTargets" | "optionLabel" | "onPick">) {
  return getTargets().map((target) => {
    // Nothing left to copy there; the entry shows a check instead.
    const nothingToDo = target.count === 0;
    return (
      <MenuItem key={target.location.id}>
        <button
          type="button"
          disabled={nothingToDo}
          onClick={() => onPick(target)}
          className={"flex items-center gap-2 " + menuItemClass}
        >
          <span className="min-w-0 break-words">{optionLabel(target)}</span>
          {nothingToDo && (
            <MdCheck aria-hidden="true" className="w-5 h-5 ml-auto shrink-0" />
          )}
        </button>
      </MenuItem>
    );
  });
}

export default function CopyToMenu({
  getTargets,
  optionLabel,
  label,
  text,
  busy,
  disabled,
  onPick,
}: CopyToMenuProps) {
  return (
    <Menu as="div" className="relative shrink-0">
      <MenuButton
        aria-label={label}
        title={label}
        aria-busy={busy}
        disabled={disabled}
        className={
          "flex items-center justify-center h-10 rounded-4xl bg-slate-700 hover:bg-slate-600 font-extrabold text-sm hover:cursor-pointer disabled:opacity-50 disabled:cursor-default " +
          (text === undefined ? "w-10" : "w-max px-4")
        }
      >
        {text}
        <MdContentCopy
          aria-hidden="true"
          className={"w-5 h-5 " + (text === undefined ? "" : "ml-2")}
        />
      </MenuButton>
      <MenuItems anchor="bottom end" className={menuPanelClass}>
        <TargetItems
          getTargets={getTargets}
          optionLabel={optionLabel}
          onPick={onPick}
        />
      </MenuItems>
    </Menu>
  );
}
