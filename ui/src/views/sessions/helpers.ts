/**
 * Shared helpers for Sessions view + pages.
 */

export function fmtTime(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

export function timeAgo(ts: number): string {
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

export function tokenFmt(n: number | null | undefined): string {
  if (!n) return "0";
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
}

/** Parse a unified diff string into lines with type annotations. */
export function parseDiff(patch: string): Array<{ type: "add" | "del" | "ctx" | "hdr"; text: string }> {
  return patch.split("\n").map((line) => {
    if (line.startsWith("+++") || line.startsWith("---")) return { type: "hdr", text: line };
    if (line.startsWith("@@")) return { type: "hdr", text: line };
    if (line.startsWith("+")) return { type: "add", text: line };
    if (line.startsWith("-")) return { type: "del", text: line };
    return { type: "ctx", text: line };
  });
}

/** Detect whether a tool call result looks like a unified diff. */
export function isDiffResult(tc: { name: string; result?: string | null }): boolean {
  const name = tc.name.toLowerCase();
  if (name === "patch" || name === "write_file" || name === "edit_file") return true;
  const result = tc.result ?? "";
  return result.includes("@@") && (result.includes("+++") || result.includes("---"));
}
