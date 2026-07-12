import { StrictMode } from "react";
import { render, screen, waitFor } from "@testing-library/react";

import { describe, expect, it, vi } from "vitest";
import { MilkdownRichEditor } from "./MilkdownRichEditor";

const fixture = [
  "---\r",
  "title: Rich fixture\r",
  "---\r",
  "# Héllo 世界",
  "",
  "See [[docs/design.md|Design]] and `[[literal.md]]`.",
  "",
  "<!-- keep this exact -->",
  "",
  "Footnote[^1]",
  "",
  "[^1]: detail",
  "",
  "[design]: /design.md \"Design\"",
  "",
  ":::warning {#careful}",
  "raw directive",
  ":::",
  "",
  "<Component value={\"raw\"} />",
  "",
  "import X from \"./x\"",
  "",
  "export default function Foo() {}",
  "",
  "````md",
  "```",
  "[[nested-fence.md]]",
  "```",
  "````",
].join("\n");

describe("MilkdownRichEditor", () => {
  it("mounts the real editor in StrictMode without normalizing the loaded note", async () => {
    const onChange = vi.fn();
    const { container } = render(
      <StrictMode>
        <MilkdownRichEditor markdown={fixture} onChange={onChange} />
      </StrictMode>,
    );

    await waitFor(() => expect(container.querySelectorAll(".ProseMirror")).toHaveLength(1));
    expect(screen.getByTestId("vault-rich-editor")).toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("emits rich edits while retaining frontmatter and preserved constructs", async () => {
    const onChange = vi.fn();
    const { container } = render(<MilkdownRichEditor markdown={fixture} onChange={onChange} />);
    const editor = await waitFor(() => {
      const element = container.querySelector<HTMLElement>(".ProseMirror");
      expect(element).not.toBeNull();
      return element!;
    });

    editor.dispatchEvent(new InputEvent("beforeinput", {
      bubbles: true,
      cancelable: true,
      data: "!",
      inputType: "insertText",
    }));

    await waitFor(() => expect(onChange).toHaveBeenCalled(), { timeout: 10_000 });
    const emitted = onChange.mock.lastCall?.[0] as string;
    expect(emitted.startsWith("---\r\ntitle: Rich fixture\r\n---\r\n")).toBe(true);
    for (const preserved of ["<!-- keep this exact -->", "[^1]: detail", "[design]: /design.md \"Design\"", ":::warning {#careful}", "<Component value={\"raw\"} />", "import X from \"./x\"", "export default function Foo() {}", "[[nested-fence.md]]"]) {
      expect(emitted).toContain(preserved);
    }
  }, 30_000);
});
