#!/usr/bin/env bash
# Bundle a Maestro run into test-evidence/<ts>/:
#   - videos/*.mp4 (when a flow records one; Maestro web recording is beta)
#   - shots/*.png (all screenshots, flattened)
#   - contact-sheet.png (grid of all screenshots, via ffmpeg tile filter)
#   - report-*.xml (machine-readable JUnit pass/fail by viewport tier)
#
# Usage: bash scripts/evidence-bundle.sh
# Output: echoes the output directory path on the last line.
set -euo pipefail

cd "$(dirname "$0")/.."

TS="${EVIDENCE_TS:-$(date +%Y%m%d-%H%M%S)}"
OUT="test-evidence/$TS"
if [[ -n "${MAESTRO_RESULTS_DIR:-}" ]]; then
  RESULTS_DIRS=("$MAESTRO_RESULTS_DIR")
else
  RESULTS_DIRS=(test-results/maestro test-results/maestro-mobile)
fi
mkdir -p "$OUT/videos" "$OUT/shots"

# 1. Videos: Maestro records mp4 directly.
count_videos=0
for results in "${RESULTS_DIRS[@]}"; do
  [[ -d "$results" ]] || continue
  find "$results" -name '*.mp4' | while read -r v; do
    cp "$v" "$OUT/videos/$(basename "$v")"
  done
done
count_videos=$(find "$OUT/videos" -name '*.mp4' 2>/dev/null | wc -l)

# 2. Screenshots: flatten with test-dir prefix
for results in "${RESULTS_DIRS[@]}"; do
  [[ -d "$results" ]] || continue
  tier=$(basename "$results")
  find "$results" -name '*.png' | while read -r p; do
    dir=$(basename "$(dirname "$p")")
    base=$(basename "$p")
    cp "$p" "$OUT/shots/${tier}-${dir}-${base}"
  done
done

count_shots=$(find "$OUT/shots" -name '*.png' 2>/dev/null | wc -l)

# 3. Contact sheet (grid of all screenshots)
if [ "$count_shots" -gt 0 ]; then
  # All Maestro viewport screenshots share the same dimensions,
  # so the glob + tile filter works without per-input scaling.
  cols=6
  rows=$(( (count_shots + cols - 1) / cols ))
  ffmpeg -loglevel error -y \
    -pattern_type glob -i "$OUT/shots/*.png" \
    -vf "scale=320:-2,tile=${cols}x${rows}" \
    -frames:v 1 "$OUT/contact-sheet.png" 2>/dev/null || true
fi

# 4. JUnit reports + command logs
for results in "${RESULTS_DIRS[@]}"; do
  [[ -d "$results" ]] || continue
  tier=$(basename "$results")
  cp "$results/report.xml" "$OUT/report-${tier}.xml" 2>/dev/null || true
  cp "$results/maestro.log" "$OUT/maestro-${tier}.log" 2>/dev/null || true
done

# 5. Summary manifest
cat > "$OUT/manifest.txt" <<EOF
Olympus E2E Evidence Bundle
Timestamp: $TS
Videos: $count_videos
Screenshots: $count_shots
Contact sheet: $([ -f "$OUT/contact-sheet.png" ] && echo yes || echo no)
Reports: $OUT/report-maestro.xml, $OUT/report-maestro-mobile.xml
EOF

echo "$OUT"
