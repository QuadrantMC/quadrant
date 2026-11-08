/** @format */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  ContentContext,
  ModLoader,
  type LocalModpack,
} from "../../../intefaces";
import { ModSource } from "../../../intefaces";

const createModpack = vi.fn();
const updateModpack = vi.fn();
const setSnackbar = vi.fn();

vi.mock("../../../tools", () => ({
  createModpack: (...a: unknown[]) => createModpack(...a),
  updateModpack: (...a: unknown[]) => updateModpack(...a),
}));

import ModpackEditDialog from "./ModpackEditDialog";

function pack(over: Partial<LocalModpack>): LocalModpack {
  return {
    name: "",
    version: "1.20.1",
    modLoader: ModLoader.Fabric,
    isApplied: false,
    lastSynced: 0,
    mods: [],
    unknownMods: false,
    ...over,
  };
}

function renderDialog(
  initial: LocalModpack,
  props: {
    isCreate?: boolean;
    onClose?: () => void;
    onSaved?: () => void;
  } = {},
) {
  const onClose = props.onClose ?? vi.fn();
  const onSaved = props.onSaved ?? vi.fn();
  render(
    <ContentContext.Provider
      value={{
        back: vi.fn(),
        changeContent: vi.fn(),
        changePage: vi.fn(),
        setSnackbar,
        setSnackbarNoState: vi.fn(),
      }}
    >
      <ModpackEditDialog
        open
        onClose={onClose}
        isCreate={props.isCreate ?? true}
        versions={[
          { version: "1.20.1", versionType: "release" },
          { version: "1.21", versionType: "release" },
        ]}
        loaderProviders={[ModSource.Modrinth]}
        initial={initial}
        originalName={initial.name}
        onSaved={onSaved}
      />
    </ContentContext.Provider>,
  );
  return { onClose, onSaved };
}

const save = () =>
  screen.getByRole("button", { name: /create a new modpack|update/i });

beforeEach(() => {
  vi.clearAllMocks();
  createModpack.mockResolvedValue(undefined);
  updateModpack.mockResolvedValue(undefined);
});

describe("ModpackEditDialog", () => {
  it("says what is missing and keeps Save off until it is provided", async () => {
    renderDialog(pack({ name: "" }));

    expect(
      screen.getByText("Enter a name for the modpack."),
    ).toBeInTheDocument();
    expect(save()).toBeDisabled();

    await userEvent.type(screen.getByRole("textbox"), "Survival");
    expect(screen.queryByText("Enter a name for the modpack.")).toBeNull();
    expect(save()).toBeEnabled();
  });

  it("will not create a pack without a mod loader", () => {
    renderDialog(pack({ name: "Survival", modLoader: ModLoader.Unknown }));
    expect(
      screen.getByText("Pick a mod loader for the modpack."),
    ).toBeInTheDocument();
    expect(save()).toBeDisabled();
  });

  it("will not save a pack without a Minecraft version", () => {
    renderDialog(pack({ name: "Survival", version: "" }));
    expect(
      screen.getByText("Pick a Minecraft version for the modpack."),
    ).toBeInTheDocument();
    expect(save()).toBeDisabled();
  });

  it("creates the pack and closes", async () => {
    const { onClose, onSaved } = renderDialog(pack({ name: "Survival" }));
    await userEvent.click(save());
    await waitFor(() =>
      expect(createModpack).toHaveBeenCalledWith(
        "Survival",
        "1.20.1",
        ModLoader.Fabric,
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(onSaved).toHaveBeenCalled();
  });

  it("saves on Enter in the name field", async () => {
    renderDialog(pack({ name: "" }));
    await userEvent.type(screen.getByRole("textbox"), "Survival{Enter}");
    await waitFor(() => expect(createModpack).toHaveBeenCalledTimes(1));
  });

  it("does not submit an invalid form with Enter", async () => {
    renderDialog(pack({ name: "" }));
    await userEvent.type(screen.getByRole("textbox"), "{Enter}");
    expect(createModpack).not.toHaveBeenCalled();
  });

  it("explains a backend refusal and stays open so the name can be fixed", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    createModpack.mockRejectedValue("errorModpackExists");
    const { onClose } = renderDialog(pack({ name: "Survival" }));

    await userEvent.click(save());

    await waitFor(() => expect(setSnackbar).toHaveBeenCalledTimes(1));
    const [{ message }] = setSnackbar.mock.calls[0];
    expect(message).not.toBe("Invalid data");
    expect(message).toMatch(/exist/i);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("updates an existing pack under its original name", async () => {
    renderDialog(pack({ name: "Old" }), { isCreate: false });
    await userEvent.click(save());
    await waitFor(() =>
      expect(updateModpack).toHaveBeenCalledWith(
        "Old",
        expect.objectContaining({ name: "Old", version: "1.20.1" }),
      ),
    );
  });
});
