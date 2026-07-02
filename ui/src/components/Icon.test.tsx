import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Icon } from "../components/Icon";

describe("Icon", () => {
  it("renders a known icon name", () => {
    const { container } = render(<Icon name="plus" />);
    const svg = container.querySelector("svg");
    expect(svg).toBeInTheDocument();
    expect(svg?.getAttribute("width")).toBe("14"); // default size
  });

  it("accepts a custom size", () => {
    const { container } = render(<Icon name="search" size={24} />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("width")).toBe("24");
  });

  it("renders null path content for unknown icon", () => {
    const { container } = render(<Icon name={"nonexistent" as never} />);
    const svg = container.querySelector("svg");
    expect(svg).toBeInTheDocument(); // svg shell still renders
  });

  it("applies className", () => {
    const { container } = render(<Icon name="bot" className="test-cls" />);
    const svg = container.querySelector("svg");
    expect(svg?.classList.contains("test-cls")).toBe(true);
  });
});
