#!/usr/bin/env bash
# Bundle a Playwright run into test-evidence/<ts>/:
#   - videos/*.mp4 (webm → h264 so they play in Hermes chat + browsers)
#   - shots/*.png (all screenshots, flattened with test-dir prefix)
#   - contact-sheet.png (grid of all screenshots, via ffmpeg tile filter)
#   - report/index.html (browsable Playwright HTML report)
#   - results.json (machine-readable pass/fail)
#
# Usage: bash scripts/evidence-bundle.sh
# Output: echoes the output directory path on the last line.
set -euo pipefail

cd "$(dirname "$0")/.."

TS="${EVIDENCE_TS:-$(date +%Y%m%d-%H%M%S)}"
OUT="test-evidence/$TS"
mkdir -p "$OUT/videos" "$OUT/shots"

# 1. Videos: webm → mp4 (h264, faststart for web playback)
count_videos=0
find test-results -name 'video.webm' | while read -r v; do
  name=$(basename "$(dirname "$v")")
  ffmpeg -loglevel error -y -i "$v" \
    -c:v libx264 -pix_fmt yuv420p -movflags +faststart \
    "$OUT/videos/$name.mp4" 2>/dev/null || true
done
count_videos=$(find "$OUT/videos" -name '*.mp4' 2>/dev/null | wc -l)

# 2. Screenshots: flatten with test-dir prefix
find test-results -name '*.png' ! -name 'test-failed-*' | while read -r p; do
  dir=$(basename "$(dirname "$p")")
  base=$(basename "$p")
  cp "$p" "$OUT/shots/${dir}-${base}"
done

count_shots=$(find "$OUT/shots" -name '*.png' 2>/dev/null | wc -l)

# 3. Contact sheet (grid of all screenshots)
if [ "$count_shots" -gt 0 ]; then
  # All Playwright viewport screenshots share the same dimensions,
  # so the glob + tile filter works without per-input scaling.
  cols=6
  rows=$(( (count_shots + cols - 1) / cols ))
  ffmpeg -loglevel error -y \
    -pattern_type glob -i "$OUT/shots/*.png" \
    -vf "scale=320:-2,tile=${cols}x${rows}" \
    -frames:v 1 "$OUT/contact-sheet.png" 2>/dev/null || true
fi

# 4. HTML report + JSON results
cp -r playwright-report "$OUT/report" 2>/dev/null || true
cp test-results/results.json "$OUT/" 2>/dev/null || true

# 5. Summary manifest
cat > "$OUT/manifest.txt" <<EOF
Olympus E2E Evidence Bundle
Timestamp: $TS
Videos: $count_videos
Screenshots: $count_shots
Contact sheet: $([ -f "$OUT/contact-sheet.png" ] && echo yes || echo no)
Report: $OUT/report/index.html
EOF

echo "$OUT"
