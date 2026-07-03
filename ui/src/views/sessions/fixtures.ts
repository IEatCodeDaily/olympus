/**
 * Mock-shaped fixtures (labelled; flip to real data without layout change).
 * The backend doesn't emit these yet (outline, context, terminal, usage).
 */

/** FIXTURE: session outline items (backend doesn't emit yet). */
export const MOCK_OUTLINE: string[] = [
  "The auth gate needs the loopback-origin check…",
  "↳ plan · reorder gate, allowlist, 403",
  "↳ patch · auth.rs",
  "Yes — add the regression test…",
  "↳ test · 43/43 pass · PR #142",
];

/** FIXTURE: session context (todo + git) — backend doesn't emit yet. */
export const MOCK_CTX = {
  todos: [
    { done: true, text: "Reorder origin check before token compare" },
    { done: true, text: "Return 403 on origin failure" },
    { done: false, text: "Add regression test — remote origin + valid token" },
  ],
  branch: "fix/auth-gate-order",
  pr: "#142 · open",
};

/** FIXTURE: bottom-panel terminal output. */
export const MOCK_TERMINAL_LINES: Array<{ cls: string; text: string }> = [
  { cls: "g", text: "$ cargo test -p control-plane auth::" },
  { cls: "d", text: "   Compiling control-plane v0.3.1" },
  { cls: "g", text: "    Finished test [unoptimized + debuginfo]" },
  { cls: "d", text: "running 6 tests ...... ok" },
  { cls: "a", text: "warning: unused import: `std::env` → auth.rs:3" },
  { cls: "g", text: "test result: ok. 6 passed; 0 failed" },
];
