/** @format */

import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { ModLoader, ModSource } from "../../intefaces";
import LoaderOptions from "./LoaderOption";
import { getModLoaderOptions } from "../../modLoaders";

function renderOptions(props: Parameters<typeof LoaderOptions>[0]) {
  // <option> must live inside a <select> to be valid DOM.
  const { container } = render(
    <select>
      <LoaderOptions {...props} />
    </select>,
  );
  return Array.from(container.querySelectorAll("option"));
}

describe("LoaderOptions", () => {
  it("renders the unknown placeholder plus every loader for the default providers", () => {
    const options = renderOptions({ loader: ModLoader.Unknown });
    // "-" placeholder + all loader options.
    expect(options).toHaveLength(1 + getModLoaderOptions().length);
    expect(options[0]).toHaveValue(ModLoader.Unknown);
  });

  it("restricts options to the given provider", () => {
    const options = renderOptions({
      loader: ModLoader.Unknown,
      providers: [ModSource.CurseForge],
    });
    const labels = options.slice(1).map((o) => o.textContent);
    // Modrinth-only loaders such as Rift must not appear.
    expect(labels).not.toContain("Rift");
    expect(labels).toContain("Fabric");
  });
});
