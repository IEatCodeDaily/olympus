# Postmortem 0018: Vault editor and workbench passed shallow QA

## Summary

The Vault shipped with a lossy WYSIWYG editor wrapped in a Rich/Source mode switch. A broad regex classified common Markdown constructs as unsafe and forcibly disabled Rich mode. The workspace also placed layout controls in a dedicated toolbar above each pane's tab strip, while newly created split panes opened empty and could not be resized.

## Impact

Normal notes containing comments, reference links, footnotes, directives, MDX-like tags, or malformed frontmatter repeatedly dropped users into a separate source editor. The interaction model was unpredictable, consumed vertical space, and did not match the VS Code-style workbench promised by the product design. The user became the first meaningful end-to-end QA pass.

## Root causes

1. Milkdown/Crepe cannot losslessly round-trip every Markdown extension stored in a Vault. The implementation responded by maintaining two editors and a growing source-only denylist instead of choosing one lossless editing model.
2. Unit tests mocked `MilkdownRichEditor`, so they proved wrapper branching but never mounted the real third-party editor.
3. The browser journey created and viewed notes but never entered edit mode, typed representative Markdown, or inspected the Rich/Source fallback behavior.
4. Workspace tests covered pane-count state transitions but not pane chrome, split contents, resizing, or actual rendered geometry.
5. Visual review accepted screenshots showing the separate Layout toolbar and empty split pane rather than comparing the result against the stated VS Code workbench reference.

## Resolution

- Replace Milkdown/Crepe and the Rich/Source switch with one CodeMirror-backed, byte-preserving Markdown editor.
- Add live-preview decorations: inactive Markdown markers collapse while headings, emphasis, links, lists, and blockquotes retain document-like presentation; the active line reveals canonical syntax for direct editing.
- Keep conflict documents exact and fully visible, with an explicit unresolved-conflict warning instead of changing editor modes.
- Preserve Vault mention, label, and wikilink suggestions through native CodeMirror completion.
- Put layout actions on the active pane's tab row, seed new split groups with the active document, and add mouse/keyboard-resizable split separators.
- Add real component tests and a focused Maestro journey that enters edit mode, asserts the absence of Rich/Source controls, captures the editor, and captures the split workspace.

## Verification

The change is gated by focused Vitest coverage for lossless documents, extended Markdown, conflicts, completion, live preview, integrated pane chrome, split seeding, and split resizing; TypeScript typecheck; production build; and focused Maestro desktop screenshots reviewed by an agent.
