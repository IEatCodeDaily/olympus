/**
 * RightPanel — View-owned right sidebar (tabbed).
 * Tabs: Overview / Outline / Settings / Browser / Diff / Git / AI.
 *
 * This is View-owned layout — rendered by SessionsView, not by Pages.
 */

import React from "react";
import { Icon, type IconName } from "../../../components/Icon";
import type { Message, Session } from "../../../types";
import { fmtTime, tokenFmt, isDiffResult, parseDiff } from "../helpers";

export type RsTab = "overview" | "outline" | "settings" | "browser" | "diff" | "git" | "ai";

export function RightPanel({
  width,
  tab,
  onTabChange,
  session,
  totalTokens,
  artifacts,
  messages,
}: {
  width?: number;
  tab: RsTab;
  onTabChange: (t: RsTab) => void;
  session: Session | undefined;
  totalTokens: number;
  artifacts: Array<{ path: string; status: "new" | "modified" }>;
  messages: Message[];
}) {
  const tabs: Array<{ id: RsTab; icon: IconName; title: string }> = [
    { id: "overview", icon: "layout-grid", title: "Overview" },
    { id: "outline", icon: "list", title: "Outline" },
    { id: "settings", icon: "gear", title: "Settings" },
    { id: "browser", icon: "globe", title: "Browser" },
    { id: "diff", icon: "git-compare", title: "Diff" },
    { id: "git", icon: "git-branch", title: "Git" },
    { id: "ai", icon: "sparkles", title: "AI" },
  ];

  const diffs = React.useMemo(() => {
    const out: Array<{ path: string; result: string }> = [];
    for (const m of messages) {
      if (!m.toolCalls) continue;
      for (const tc of m.toolCalls) {
        if (isDiffResult(tc) && tc.result) {
          const args = tc.args as Record<string, unknown> | null;
          const path =
            typeof args === "object" && args && typeof args.path === "string"
              ? args.path
              : tc.name;
          out.push({ path, result: tc.result });
        }
      }
    }
    return out;
  }, [messages]);

  return (
    <aside className="rsidebar" style={width ? { width } : undefined}>
      <div className="rs-tabbar">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            className={`rs-tab${tab === t.id ? " on" : ""}`}
            title={t.title}
            onClick={() => onTabChange(t.id)}
          >
            <Icon name={t.icon} size={13} />
          </button>
        ))}
      </div>

      {tab === "overview" && (
        <div className="rsv on" data-rsv="overview">
          <div className="rs-sec">
            <div className="kv">
              <span className="k">AGENT</span>
              <span className="v">{session?.agent ?? "—"}</span>
            </div>
            <div className="kv">
              <span className="k">NODE</span>
              <span className="v">{session?.node ?? "local"}</span>
            </div>
            <div className="kv">
              <span className="k">MODEL</span>
              <span className="v">{session?.model ?? "—"}</span>
            </div>
            <div className="kv">
              <span className="k">STARTED</span>
              <span className="v">
                {session ? fmtTime(session.startedAt) : "—"}
              </span>
            </div>
          </div>
          <div className="rs-sec">
            <div className="stats">
              <div className="stat">
                <span className="v">{tokenFmt(totalTokens)}</span>
                <span className="l">TOKENS</span>
              </div>
              <div className="stat">
                <span className="v">{messages.length}</span>
                <span className="l">MSGS</span>
              </div>
            </div>
          </div>
          {artifacts.length > 0 && (
            <div className="arts">
              <div className="art-head">
                <span className="l">ARTIFACTS</span>
                <span className="l">{artifacts.length}</span>
              </div>
              {artifacts.map((a) => (
                <div key={a.path} className="art">
                  <Icon name="file" size={12} />
                  <span className="nm">{a.path.split("/").pop()}</span>
                  {a.status === "new" ? (
                    <span className="tag" style={{ color: "var(--silver)", background: "var(--silver-wash)" }}>
                      new
                    </span>
                  ) : (
                    <span className="tag" style={{ color: "var(--green)", background: "var(--green-wash)" }}>
                      M
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {tab === "outline" && (
        <div className="rsv on" data-rsv="outline">
          <div className="rs-sec">
            <div className="gk" style={{ marginBottom: 4 }}>transcript</div>
            <div className="empty-state-msg" style={{ padding: "6px 0", fontSize: 12 }}>
              Coming soon…
            </div>
          </div>
        </div>
      )}

      {tab === "settings" && (
        <div className="rsv on" data-rsv="settings">
          <div className="rs-sec">
            <div className="gk">session settings</div>
            <div className="grow">
              <span style={{ fontSize: 12 }}>Auto-approve tools</span>
              <span className="gsw on"><i /></span>
            </div>
            <div className="grow">
              <span style={{ fontSize: 12 }}>Extended thinking</span>
              <span className="gsw"><i /></span>
            </div>
            <div className="grow">
              <span style={{ fontSize: 12 }}>Notify on finish</span>
              <span className="gsw on"><i /></span>
            </div>
          </div>
          <div className="rs-sec">
            <div className="kv">
              <span className="k">CONTEXT</span>
              <span className="v">
                {tokenFmt(session?.inputTokens)} / {tokenFmt(session?.inputTokens ? session.inputTokens * 4 : null)}
              </span>
            </div>
            <div className="kv">
              <span className="k">SOURCE</span>
              <span className="v">{session?.source ?? "—"}</span>
            </div>
          </div>
        </div>
      )}

      {tab === "browser" && (
        <div className="rsv on" data-rsv="browser">
          <div className="rs-sec">
            <div
              style={{
                display: "flex", alignItems: "center", gap: 7, padding: "4px 9px",
                background: "var(--elev)", border: "1px solid var(--border)",
                borderRadius: "var(--radius-full)", fontFamily: "var(--font-mono)",
                fontSize: 10, color: "var(--dim)",
              }}
            >
              <Icon name="globe" size={12} />
              <span>localhost:5173</span>
            </div>
            <div
              style={{
                height: 200, border: "1px solid var(--border)", borderRadius: "var(--radius)",
                background: "repeating-linear-gradient(45deg,var(--elev),var(--elev) 8px,var(--chrome) 8px,var(--chrome) 16px)",
                display: "flex", alignItems: "center", justifyContent: "center",
              }}
            >
              <span style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--faint)" }}>
                app preview
              </span>
            </div>
          </div>
        </div>
      )}

      {tab === "diff" && (
        <div className="rsv on" data-rsv="diff">
          {diffs.length === 0 ? (
            <div className="rs-sec">
              <div className="gk" style={{ padding: "6px 0" }}>No diffs yet</div>
            </div>
          ) : (
            diffs.map((d, i) => (
              <div key={i} className="rs-sec" style={{ gap: 6 }}>
                <div className="kv">
                  <span className="k">{d.path.split("/").pop()}</span>
                  <span className="v" style={{ color: "var(--green)" }}>M</span>
                </div>
                <div
                  style={{
                    background: "var(--elev)", border: "1px solid var(--border)",
                    borderRadius: "var(--radius)", padding: "8px 10px", overflow: "auto", maxHeight: 220,
                  }}
                >
                  {parseDiff(d.result).map((l, j) => (
                    <div
                      key={j}
                      className={`diffln${l.type === "add" ? " add" : l.type === "del" ? " del" : ""}`}
                      style={l.type === "hdr" ? { color: "var(--dim)", opacity: 0.7 } : undefined}
                    >
                      {l.text}
                    </div>
                  ))}
                </div>
              </div>
            ))
          )}
        </div>
      )}

      {tab === "git" && (
        <div className="rsv on" data-rsv="git">
          <div className="rs-sec">
            <div className="gk">git context</div>
            <div className="empty-state-msg" style={{ padding: "6px 0", fontSize: 12 }}>
              Coming soon…
            </div>
          </div>
          {artifacts.length > 0 && (
            <div className="rs-sec">
              <div className="gk">changed files</div>
              {artifacts.map((a) => (
                <div key={a.path} className="art">
                  <Icon name="file" size={12} />
                  <span className="nm">{a.path.split("/").pop()}</span>
                  <span
                    className="tag"
                    style={a.status === "new"
                      ? { color: "var(--silver)", background: "var(--silver-wash)" }
                      : { color: "var(--green)", background: "var(--green-wash)" }}
                  >
                    {a.status === "new" ? "A" : "M"}
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {tab === "ai" && (
        <div className="rsv on" data-rsv="ai">
          <div className="rs-sec">
            <div className="gk">AI suggestions</div>
            <div style={{ fontSize: 12, color: "var(--dim)", lineHeight: "var(--lh-relaxed)" }}>
              AI suggestions appear here during active sessions.
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}
