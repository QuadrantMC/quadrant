/** @format */

import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import ErrorBoundary from "./ErrorBoundary";

function Broken(): never {
  throw new Error("render failed");
}

describe("ErrorBoundary", () => {
  it("renders its children while nothing fails", () => {
    render(
      <ErrorBoundary>
        <p>fine</p>
      </ErrorBoundary>,
    );
    expect(screen.getByText("fine")).toBeInTheDocument();
  });

  it("replaces a crashed subtree with a reload prompt", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary>
        <Broken />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
    error.mockRestore();
  });
});
