# Olympus Full E2E Coverage Pipeline — Implementation Plan

> **For Hermes:** execute task-by-task (kanban cards or direct). Each task is
> independently verifiable. TDD where code is produced.

**Goal:** A complete, always-on e2e pipeline that covers every Olympus surface
and API feature, produces **screenshots and videos of every run** (not just
failures), publishes a browsable HTML report, and gates merges — locally via
`make verify` and in CI.

**Architecture:** Three test tiers sharing one artifact convention:
(1) Playwright UI e2e (MSW-mocked, deterministic) with per-test video +
per-step screenshots; (2) Playwright "live" smoke tier against the real
control plane (`:8799`) for the send→stream→persist chain; (3) Rust API
integration tests (already exist, 282). Artifacts land in
`ui/test-results/` + `ui/playwright-report/`, get post-processed into a
`test-evidence/<run-ts>/` bundle (contact-sheet PNG + per-suite MP4s via
ffmpeg), and the QA-engineer profile consumes/produces the same bundle.

**Tech stack:** Playwright 1.61 (already installed), MSW mocks (exist),
ffmpeg (present at /usr/bin/ffmpeg), GitHub Actions, existing Makefile.

**Current state (verified on disk):**
- `ui/playwright.config.ts`: 2 projects (chromium-desktop, mobile-chrome/Pixel 7),
  `screenshot: "only-on-failure"`, `video: "retain-on-failure"`, `reporter: "list"`,
  webServer on :5188 with MSW forced.
- 3 spec files: `fleet.spec.ts`, `pin-archive-history.spec.ts`, `selection.spec.ts`
  (23 tests, all green).
- No `.github/workflows/`. `make verify` = rust gates + `verify-ui`.
- Backend: 282 cargo tests. New surfaces with ZERO e2e coverage: History table
  interactions (pin/unpin from table, archived toggle), Composer (model
  selector, thinking level, + menu), Vaults, Projects (new UI pending merge),
  Agents page, Usage page, subsession flows, repo attach.

---

## Phase 1 — Artifact foundation (evidence on EVERY run)

### Task 1.1: Always-on video + screenshots + HTML report

**Files:** Modify `ui/playwright.config.ts`

Change the `use` block and reporter:

```ts
  reporter: [
    ["list"],
    ["html", { outputFolder: "playwright-report", open: "never" }],
    ["json", { outputFile: "test-results/results.json" }],
  ],
  use: {
    baseURL: "http://127.0.0.1:5188",
    trace: "retain-on-failure",
    screenshot: "on",          // was only-on-failure — evidence for EVERY test
    video: "on",               // was retain-on-failure — video for EVERY test
    viewport: { width: 1280, height: 800 },
  },
  outputDir: "test-results",
```

**Verify:** `npx playwright test --project=chromium-desktop tests/e2e/fleet.spec.ts`
then `ls test-results/*/video.webm | wc -l` ≥ 5 (one per test, pass or fail)
and `ls playwright-report/index.html`.

**Cost note:** video-on adds ~15–25% runtime. Acceptable; suite is ~35s
without it. If it crosses 3 min, flip video to `"on"` only in CI via
`process.env.CI ? "on" : "retain-on-failure"`.

**Commit:** `feat(e2e): always-on video/screenshot evidence + HTML/JSON reporters`

### Task 1.2: Named step-screenshots helper

**Files:** Create `ui/tests/e2e/helpers/evidence.ts`

```ts
import type { Page, TestInfo } from "@playwright/test";

/** Numbered, named screenshot attached to the report AND kept on disk. */
export async function snap(page: Page, testInfo: TestInfo, name: string) {
  const idx = (testInfo as any)._snapIdx = ((testInfo as any)._snapIdx ?? 0) + 1;
  const file = testInfo.outputPath(`${String(idx).padStart(2, "0")}-${name}.png`);
  await page.screenshot({ path: file, fullPage: false });
  await testInfo.attach(name, { path: file, contentType: "image/png" });
}
```

Usage in specs: `await snap(page, testInfo, "history-filtered");` (test fns
take `async ({ page }, testInfo)`).

**Verify:** add one `snap` call to an existing fleet test, run it, confirm the
PNG exists in `test-results/<test-dir>/01-*.png` and shows in the HTML report.

**Commit:** `feat(e2e): snap() step-screenshot helper`

### Task 1.3: Evidence bundle post-processor (contact sheet + mp4)

**Files:** Create `ui/scripts/evidence-bundle.sh` (bash, ffmpeg + montage-free)

```bash
#!/usr/bin/env bash
# Bundle a Playwright run into test-evidence/<ts>/: convert webm→mp4 (h264,
# plays everywhere incl. the Hermes chat viewer), build a contact-sheet PNG
# from all screenshots, copy the HTML report.
set -euo pipefail
TS=$(date +%Y%m%d-%H%M%S)
OUT="test-evidence/$TS"
mkdir -p "$OUT/videos" "$OUT/shots"

# 1. videos: webm → mp4 named after the test dir
find test-results -name 'video.webm' | while read -r v; do
  name=$(basename "$(dirname "$v")")
  ffmpeg -loglevel error -y -i "$v" -c:v libx264 -pix_fmt yuv420p \
    -movflags +faststart "$OUT/videos/$name.mp4"
done

# 2. screenshots: flatten with test-dir prefix
find test-results -name '*.png' | while read -r p; do
  name="$(basename "$(dirname "$p")")-$(basename "$p")"
  cp "$p" "$OUT/shots/$name"
done

# 3. contact sheet (6 columns) — ffmpeg tile filter, no imagemagick needed
shots=("$OUT"/shots/*.png)
if [ ${#shots[@]} -gt 0 ]; then
  n=${#shots[@]}; rows=$(( (n + 5) / 6 ))
  ffmpeg -loglevel error -y \
    $(printf -- '-i %q ' "${shots[@]}") \
    -filter_complex "scale=320:-1 [t]; ... " 2>/dev/null || \
  ffmpeg -loglevel error -y -pattern_type glob -i "$OUT/shots/*.png" \
    -vf "scale=320:-1,tile=6x${rows}" -frames:v 1 "$OUT/contact-sheet.png"
fi

# 4. report + summary
cp -r playwright-report "$OUT/report" 2>/dev/null || true
cp test-results/results.json "$OUT/" 2>/dev/null || true
echo "$OUT"
```

Note for implementer: the ffmpeg `tile` filter needs uniform input sizes —
`scale=320:-2,pad=320:240` each input first, or use the glob-pattern branch
(all Playwright shots share the viewport size, so glob works). Test both
branches; keep whichever produces a valid PNG, delete the other.

**Verify:** run the suite, `bash scripts/evidence-bundle.sh`, then:
- `ls test-evidence/*/videos/*.mp4 | wc -l` ≥ 23
- `file test-evidence/*/contact-sheet.png` → PNG image data
- open `test-evidence/<ts>/report/index.html` in a browser.

**Commit:** `feat(e2e): evidence bundle — mp4 conversion + contact sheet + report`

### Task 1.4: Makefile + gitignore wiring

**Files:** Modify `Makefile`, `.gitignore`

```make
e2e:            ## full e2e (desktop+mobile) with evidence bundle
	cd ui && npx playwright test; bash scripts/evidence-bundle.sh

e2e-desktop:    ## fast inner loop, desktop only, no bundle
	cd ui && npx playwright test --project=chromium-desktop
```
Add `verify-ui` → depends on `e2e-desktop` (mobile+bundle stays in `e2e`/CI).
`.gitignore`: `ui/test-results/`, `ui/playwright-report/`, `ui/test-evidence/`.

**Verify:** `make e2e-desktop` green; `git status` shows no artifact noise.

**Commit:** `chore(e2e): make e2e / e2e-desktop targets, ignore artifacts`

---

## Phase 2 — Coverage gap closure (mocked tier)

One spec file per surface. Every test uses `snap()` at each meaningful state
so the video + numbered screenshots tell the story. All specs run in BOTH
projects unless tagged `@desktop-only` (hover-dependent) — mobile coverage is
free after Phase 1.

### Task 2.1: `ui/tests/e2e/composer.spec.ts`
- model selector: open pill → options are agent-scoped → select → label updates
  (regression for the model-passthrough bug: assert the POST body contains
  `model` — intercept via `page.route` on `**/api/sessions/*/messages`).
- thinking level: select high → persists in localStorage across reload.
- send: optimistic message appears exactly ONCE (regression for the duplicate
  bug: count `.msg-user` nodes with same text == 1 after mock echo).
- + menu opens/closes.

### Task 2.2: `ui/tests/e2e/history-table.spec.ts`
- column sort order sanity, channel filter → all rows show that channel tag,
  time-range filter, archived toggle reveals archived rows with tag,
  pin from table → PINNED section in sidebar (desktop-only),
  unarchive from table restores row, "Show more" paging reveals next 100.

### Task 2.3: `ui/tests/e2e/vaults.spec.ts`
- vault list renders, create vault, open note tree, open note, edit + save
  round-trip (MSW), tables/graph tabs render without error.

### Task 2.4: `ui/tests/e2e/projects.spec.ts`
- MSW handlers for `/api/projects` (fixtures: 2 projects) — the projects
  worker added these; verify present in `ui/src/mocks/handlers.ts`, add if the
  UI merge dropped them.
- list renders, create project, bind vault/repo/board via pickers, detail
  pane reflects bindings, delete project.

### Task 2.5: `ui/tests/e2e/session-lifecycle.spec.ts`
- new session via agent picker (shows node label + "needs login" badge state
  from fixtures), send → thinking indicator → mock reply renders,
  spinner in sidebar while running AND while selected (regression),
  hover card shows node/agent/model (desktop-only),
  pin → PINNED section; archive → gone from RECENT, present in History.
- permission prompt: MSW emits `permission.required` frame → amber prompt
  renders → Allow resolves it.

### Task 2.6: `ui/tests/e2e/subsessions.spec.ts`
- Add MSW handlers for `POST/GET /api/sessions/:id/subsessions` + `complete`.
- parent chat shows the child-spawn affordance (whatever UI lands — if none
  yet, test the API contract through the mock and mark UI part TODO),
- complete(pass) → system message `[subsession … pass]` appears in parent
  transcript.

**Each task:** write spec → run desktop → run mobile → snap() at key states →
commit. Expected end-state: **~60–70 e2e tests**, all with video evidence.

---

## Phase 3 — Live smoke tier (real backend, real agent)

Mocked tests can't catch bridge/ACP regressions (the "internal error" class).
A small, tagged tier runs against the REAL control plane.

### Task 3.1: `ui/playwright.live.config.ts`
- baseURL `http://127.0.0.1:5177` (real vite + real :8799 via proxy), no
  webServer block (asserts services running; fail fast with clear message via
  `globalSetup` that curls `/api/health`).
- `video: "on"`, single project chromium-desktop, `testDir: tests/live`.

### Task 3.2: `ui/tests/live/smoke.spec.ts` (3 tests, ≤2 min)
1. create session (agent glm52) → send "say PONG" → assert PONG streams into
   the transcript within 120s (regression for ensure_runtime/silent-failure).
2. pin + archive round-trip → survives `systemctl --user restart olympus`?
   No — do NOT restart the service from a test. Instead assert via a second
   full page reload that pinned state persisted (event-log persistence proxy).
3. History page loads 1800+ real sessions, filter narrows, open one.

### Task 3.3: `make e2e-live` target + docs
Runs the live config; documented as operator-run / nightly-cron only (it
spends real tokens). Wire a Hermes cron (nightly) that runs it and drops the
evidence bundle path into the ops channel.

---

## Phase 4 — CI pipeline

### Task 4.1: `.github/workflows/e2e.yml`

```yaml
name: e2e
on: [push, pull_request]
jobs:
  rust:
    runs-on: ubuntu-latest
    steps: [checkout, rust-toolchain, cargo test -p olympus-control-plane,
            cargo clippy -- -D warnings, cargo fmt --check]
  ui-e2e:
    runs-on: ubuntu-latest
    steps:
      - checkout + setup-node 24 + npm ci (ui/)
      - npx playwright install --with-deps chromium
      - npm run typecheck && npm run test
      - npx playwright test          # both projects, MSW — no backend needed
      - run: bash scripts/evidence-bundle.sh
        if: always()
      - uses: actions/upload-artifact@v4
        if: always()
        with: { name: e2e-evidence, path: ui/test-evidence/, retention-days: 14 }
```
ffmpeg is preinstalled on ubuntu-latest runners. The live tier does NOT run
in CI (needs real Hermes + creds).

### Task 4.2: PR annotation
Add `--reporter=github` to the CI playwright invocation (inline annotations
on the PR diff for failures) alongside html/json.

---

## Phase 5 — QA-engineer integration

### Task 5.1: Patch qa-engineer skill/prompt with the pipeline contract
The profile's system prompt already mandates clarify→test-live→automate→
prove-it-fails. Add the concrete artifact contract:
- evidence bundles live at `ui/test-evidence/<ts>/` (mp4 + contact sheet +
  HTML report) — ALWAYS attach the contact sheet + relevant mp4 paths in
  reports (`![shot](/abs/path.png)`, `[video](/abs/path.mp4)`).
- new feature = new spec in `ui/tests/e2e/` using `snap()`; must be seen
  failing once (break the assertion, run, restore).

### Task 5.2: Nightly QA cron
Hermes cron on the qa-engineer profile: run `make e2e` + `make e2e-live`,
post pass/fail + evidence paths. Escalate on failure with the failing test's
mp4 attached.

---

## Validation (whole plan)

- `make e2e` → all tests green, `test-evidence/<ts>/` has ≥60 mp4s, a valid
  contact sheet, HTML report opens.
- Kill a feature on purpose (e.g. revert the spinner fix) → the matching test
  fails AND its video shows the missing spinner → restore.
- CI run on a PR uploads the artifact and annotates failures.

## Risks / open questions

1. **Video-on runtime cost** — mitigated by `e2e-desktop` fast lane without
   bundle; measure and cap suite at 5 min.
2. **MSW fixtures drift from real API** — the live smoke tier is the canary;
   also the Rust route tests pin the wire contract.
3. **Mobile project doubles runtime** — keep; it already caught the off-screen
   dialog bug. Tag hover-dependent tests `@desktop-only` as established.
4. **Projects/subsessions UI still settling** (worker merge just landed) —
   Tasks 2.4/2.6 should be executed AFTER the projects UI is reconciled into
   the current History/sidebar code, else specs chase a moving target.
5. **webm→mp4 conversion time** (~1s/video × 60) — acceptable; parallelize
   with `xargs -P4` if it grows.

## Suggested execution order

1.1 → 1.2 → 1.3 → 1.4 (foundation, ~1h, do directly)
2.1 → 2.5 (highest-regression-value surfaces first)
2.2 → 2.3 → 2.4 → 2.6 (parallelizable as kanban cards; 2.4/2.6 after UI merge)
3.x (live tier), then 4.x (CI), then 5.x (QA wiring).
