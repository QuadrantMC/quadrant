/** @format */

import { describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Button from "./Button";

describe("Button", () => {
  it("renders its children", () => {
    render(<Button onClick={() => {}}>Click me</Button>);
    expect(
      screen.getByRole("button", { name: "Click me" }),
    ).toBeInTheDocument();
  });

  it("fires onClick when pressed", async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Go</Button>);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("prevents clicks while disabled", async () => {
    const onClick = vi.fn();
    render(
      <Button onClick={onClick} disabled>
        Download
      </Button>,
    );
    const button = screen.getByRole("button", { name: "Download" });
    expect(button).toBeDisabled();
    await userEvent.click(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("applies the caller's className and the full-round variant", () => {
    render(
      <Button onClick={() => {}} className="custom-class" fullRound>
        R
      </Button>,
    );
    const button = screen.getByRole("button", { name: "R" });
    expect(button.className).toContain("custom-class");
    expect(button.className).toContain("rounded-full");
  });
});

describe("Button in-flight guard", () => {
  it("runs an async handler once however many times it is clicked", async () => {
    let finish!: () => void;
    const onClick = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    render(<Button onClick={onClick}>Share</Button>);
    const button = screen.getByRole("button", { name: "Share" });
    await userEvent.dblClick(button);
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(button).toBeDisabled();
    await act(async () => finish());
    expect(button).toBeEnabled();
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalledTimes(2);
  });

  it("re-enables after the handler rejects", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const onClick = vi.fn(() => Promise.reject(new Error("boom")));
    render(<Button onClick={onClick}>Go</Button>);
    const button = screen.getByRole("button", { name: "Go" });
    await userEvent.click(button);
    await waitFor(() => expect(button).toBeEnabled());
    expect(error).toHaveBeenCalled();
    error.mockRestore();
  });
});
