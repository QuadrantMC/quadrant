/** @format */

import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Button from "./Button";

describe("Button", () => {
  it("renders its children", () => {
    render(<Button onClick={() => {}}>Click me</Button>);
    expect(screen.getByRole("button", { name: "Click me" })).toBeInTheDocument();
  });

  it("fires onClick when pressed", async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Go</Button>);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(onClick).toHaveBeenCalledTimes(1);
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
