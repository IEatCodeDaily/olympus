/**
 * SessionsWorkbench — S1 IDE-grade chat view.
 *
 * Layout (matches docs/design/concept/olympus-app-concept.html Sessions view):
 *
 *   ┌──────────────────────────────────────────────────────────────────┐
 *   │ vp-head (title · project badge · live badge · panel toggles)    │
 *   ├──────────────────────────────────────────────────────────────────┤
 *   │ vp-body                                                         │
 *   │   chatcol (flex:1)              │ rz-x │ rsidebar (279px)       │
 *   │     hz-l hover zone (outline)   │      │   rs-tabbar            │
 *   │     hz-r hover zone (context)   │      │   rsv panels (7 tabs)  │
 *   │     transcript                  │      │                        │
 *   │     rz-y                        │      │                        │
 *   │     bpanel                      │      │                        │
 *   ├──────────────────────────────────────────────────────────────────┤
 *   │ composer                                                        │
 *   └──────────────────────────────────────────────────────────────────┘
 *
 * The session sidebar (PINNED / RECENT / OBSERVED groups) lives in AppShell —
 * SessionSidebar was already there.  This file owns the viewport only.
 *
 * Mock-first: types defined in types.ts; fixture labels clearly marked so
 * the implementation flips to real data without layout change.
 */

import React, {
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { useNavigate } from "@tanstack/react-router";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { Icon } from "../components/Icon";
import { useSession, useMessages, useAgents, useModels } from "../hooks/queries";
import { sendMessage, forkSession, onFrame } from "../api";
import type { Message, ServerFrame, ToolCall } from "../types";

// ── Helpers ────────────────────────────────────────────────────────────

function fmtTime(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function tokenFmt(n: number | null | undefined): string {
  if (!n) return "0";
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
}

/** Parse a unified diff string into lines with type annotations. */
function parseDiff(patch: string): Array<{ type: "add" | "del" | "ctx" | "hdr"; text: string }> {
  return patch.split("\n").map((line) => {
    if (line.startsWith("+++") || line.startsWith("---")) return { type: "hdr", text: line };
    if (line.startsWith("@@")) return { type: "hdr", text: line };
    if (line.startsWith("+")) return { type: "add", text: line };
    if (line.startsWith("-")) return { type: "del", text: line };
    return { type: "ctx", text: line };
  });
}

/** Detect whether a tool call result looks like a unified diff. */
function isDiffResult(tc: ToolCall): boolean {
  const name = tc.name.toLowerCase();
  if (name === "patch" || name === "write_file" || name === "edit_file") return true;
  const result = tc.result ?? "";
  return result.includes("@@") && (result.includes("+++") || result.includes("---"));
}

// ── Mock-shaped fixtures (labelled; flip to real data without layout change) ──

/** FIXTURE: session outline items (backend doesn't emit yet). */
const MOCK_OUTLINE: string[] = [
  "The auth gate needs the loopback-origin check…",
  "↳ plan · reorder gate, allowlist, 403",
  "↳ patch · auth.rs",
  "Yes — add the regression test…",
  "↳ test · 43/43 pass · PR #142",
];

/** FIXTURE: session context (todo + git) — backend doesn't emit yet. */
const MOCK_CTX = {
  todos: [
    { done: true, text: "Reorder origin check before token compare" },
    { done: true, text: "Return 403 on origin failure" },
    { done: false, text: "Add regression test — remote origin + valid token" },
  ],
  branch: "fix/auth-gate-order",
  pr: "#142 · open",
};

/** FIXTURE: bottom-panel terminal output. */
const MOCK_TERMINAL_LINES: Array<{ cls: string; text: string }> = [
  { cls: "g", text: "$ cargo test -p control-plane auth::" },
  { cls: "d", text: "   Compiling control-plane v0.3.1" },
  { cls: "g", text: "    Finished test [unoptimized + debuginfo]" },
  { cls: "d", text: "running 6 tests ...... ok" },
  { cls: "a", text: "warning: unused import: `std::env` → auth.rs:3" },
  { cls: "g", text: "test result: ok. 6 passed; 0 failed" },
];

// ── Panel state ────────────────────────────────────────────────────────

type RsTab = "overview" | "outline" | "settings" | "browser" | "diff" | "git" | "ai";
type BpTab = "terminal" | "output" | "debug";

// ── Main component ─────────────────────────────────────────────────────

export function SessionsWorkbench({ sessionId }: { sessionId: string }) {
  const { data: session } = useSession(sessionId);
  const { data: msgData, isLoading } = useMessages(sessionId);
  const navigate = useNavigate();

  // streaming
  const [streamingText, setStreamingText] = useState("");
  const [sending, setSending] = useState(false);
  const [text, setText] = useState("");
  const transcriptRef = useRef<HTMLDivElement>(null);
  const messages = msgData?.messages ?? [];

  // panel state
  const [rsCollapsed, setRsCollapsed] = useState(false);
  const [bpCollapsed, setBpCollapsed] = useState(false);
  const [rsTab, setRsTab] = useState<RsTab>("overview");
  const [bpTab, setBpTab] = useState<BpTab>("terminal");

  // overlay pin state
  const [outlinePinned, setOutlinePinned] = useState(false);
  const [ctxPinned, setCtxPinned] = useState(false);

  // ── WS streaming ────────────────────────────────────────────────────
  useEffect(() => {
    const unsub = onFrame((frame: ServerFrame) => {
      if (frame.kind === "message.delta" && frame.sessionId === sessionId) {
        setStreamingText((prev) => prev + frame.textDelta);
      }
      if (frame.kind === "message.done" && frame.sessionId === sessionId) {
        setStreamingText("");
        setSending(false);
      }
    });
    return unsub;
  }, [sessionId]);

  // ── Auto-scroll ─────────────────────────────────────────────────────
  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [messages.length, streamingText]);

  // ── Send ─────────────────────────────────────────────────────────────
  const handleSend = useCallback(async () => {
    const trimmed = text.trim();
    if (!trimmed || sending) return;
    setSending(true);
    setText("");
    try {
      await sendMessage(sessionId, trimmed);
    } catch {
      setText(trimmed);
      setSending(false);
    }
  }, [text, sending, sessionId]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        void handleSend();
      }
    },
    [handleSend],
  );

  const handleTextareaInput = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      setText(e.target.value);
      const el = e.target;
      el.style.height = "auto";
      el.style.height = Math.min(el.scrollHeight, 150) + "px";
    },
    [],
  );

  // ── Fork ─────────────────────────────────────────────────────────────
  const handleFork = useCallback(async () => {
    try {
      const forked = await forkSession(sessionId);
      if (forked?.id)
        void navigate({
          to: "/sessions/$sessionId",
          params: { sessionId: forked.id },
        });
    } catch {
      // user can retry
    }
  }, [sessionId, navigate]);

  const isObserved = session?.managed === false;

  // Derived artifact list from messages (files touched by patch/write_file calls)
  const artifacts = React.useMemo(() => {
    const seen = new Map<string, "new" | "modified">();
    for (const m of messages) {
      if (!m.toolCalls) continue;
      for (const tc of m.toolCalls) {
        if (tc.name === "patch" || tc.name === "write_file" || tc.name === "edit_file") {
          const args = tc.args as Record<string, unknown> | null;
          const path =
            typeof args === "object" && args && typeof args.path === "string"
              ? args.path
              : null;
          if (!path) continue;
          const isNew = tc.name === "write_file" && !tc.result?.includes("@@");
          seen.set(path, isNew ? "new" : "modified");
        }
      }
    }
    return Array.from(seen.entries()).map(([path, status]) => ({ path, status }));
  }, [messages]);

  // Token totals
  const totalTokens =
    (session?.inputTokens ?? 0) + (session?.outputTokens ?? 0);

  return (
    <div
      className="view on chat-view"
      data-view="sessions"
      data-session-id={sessionId}
      style={{ flexDirection: "column" }}
    >
      {/* ── vp-head ───────────────────────────────────────────────────── */}
      <div className="vp-head">
        <div className="vp-left">
          <button
            type="button"
            className="icobtn"
            style={{ padding: 0 }}
            onClick={() => void navigate({ to: "/" })}
            title="Back"
          >
            <Icon name="chevron-left" />
          </button>
          <span className="vp-title chat-title">{session?.title ?? "Untitled"}</span>
          {session?.agent && (
            <span className="proj-badge">{session.agent.toUpperCase()}</span>
          )}
        </div>
        <div className="vp-right">
          {session?.liveness === "active" && (
            <div className="live chat-live-badge">
              <span className="dot" />
              <span className="lbl">LIVE</span>
            </div>
          )}
          {session?.managed && session?.liveness !== "active" && (
            <span className="gtag ok chat-managed-badge">managed</span>
          )}
          <button
            type="button"
            className="icobtn"
            title="Toggle bottom panel"
            onClick={() => setBpCollapsed((v) => !v)}
          >
            <Icon name="panel-bottom" size={14} />
          </button>
          <button
            type="button"
            className="icobtn"
            title="Toggle right panel"
            onClick={() => setRsCollapsed((v) => !v)}
          >
            <Icon name="panel-right" size={14} />
          </button>
        </div>
      </div>

      {/* ── vp-body ───────────────────────────────────────────────────── */}
      <div className="vp-body">
        {/* ── chatcol ─────────────────────────────────────────────────── */}
        <div className="chatcol">
          {/* Hover zones for outline / context overlays */}
          <div className="hz hz-l" />
          <div className="hz hz-r" />

          {/* Session outline overlay (left edge) */}
          <div
            className={`ovl ovl-l${outlinePinned ? " pinned" : ""}`}
            id="outline"
          >
            <div className="ovl-head">
              <span className="gk">session outline</span>
              <button
                type="button"
                className="icobtn"
                style={{ padding: 2 }}
                title="Keep outline open"
                onClick={() => setOutlinePinned((v) => !v)}
              >
                <Icon name="pin" size={12} />
              </button>
            </div>
            {MOCK_OUTLINE.map((line, i) => (
              <div key={i} className="ovl-it">
                {line}
              </div>
            ))}
          </div>

          {/* Session context overlay (right edge) */}
          <div
            className={`ovl ovl-r${ctxPinned ? " pinned" : ""}`}
            id="ctxpanel"
          >
            <div className="ovl-head">
              <span className="gk">session context</span>
              <button
                type="button"
                className="icobtn"
                style={{ padding: 2 }}
                title="Keep context open"
                onClick={() => setCtxPinned((v) => !v)}
              >
                <Icon name="pin" size={12} />
              </button>
            </div>
            <div className="gk" style={{ padding: "2px 4px" }}>
              todo
            </div>
            {MOCK_CTX.todos.map((t, i) => (
              <div key={i} className={`todo${t.done ? " done" : ""}`}>
                <span className="bx" />
                <span>{t.text}</span>
              </div>
            ))}
            <div className="gk" style={{ padding: "10px 4px 2px" }}>
              git
            </div>
            <div className="kv" style={{ padding: "2px 4px" }}>
              <span className="k">BRANCH</span>
              <span className="v" style={{ fontSize: 11 }}>
                {MOCK_CTX.branch}
              </span>
            </div>
            <div className="kv" style={{ padding: "2px 4px" }}>
              <span className="k">PR</span>
              <span className="v" style={{ fontSize: 11 }}>
                {MOCK_CTX.pr}
              </span>
            </div>
          </div>

          {/* ── Transcript ────────────────────────────────────────────── */}
          <div className="transcript" ref={transcriptRef}>
            <div className="tcol">
              {isLoading && (
                <div className="msg-empty">Loading messages…</div>
              )}
              {!isLoading && messages.length === 0 && !streamingText && (
                <div className="msg-empty">
                  No messages yet. Send a message below.
                </div>
              )}
              {messages.map((m) => (
                <MessageBubble
                  key={`${sessionId}-${m.messageId}`}
                  msg={m}
                  sessionId={sessionId}
                  onFork={handleFork}
                />
              ))}
              {streamingText && (
                <div className="msg-ai">
                  <div className="who">
                    {(session?.agent ?? "assistant").toUpperCase()}
                  </div>
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {streamingText}
                  </ReactMarkdown>
                </div>
              )}
            </div>
          </div>

          {/* ── Bottom panel resize + panel ───────────────────────────── */}
          {!bpCollapsed && (
            <>
              <div className="rz-y" />
              <BottomPanel
                tab={bpTab}
                onTabChange={setBpTab}
                onClose={() => setBpCollapsed(true)}
              />
            </>
          )}
        </div>

        {/* ── Right sidebar resize + panel ──────────────────────────── */}
        {!rsCollapsed && (
          <>
            <div className="rz-x" />
            <RightPanel
              tab={rsTab}
              onTabChange={setRsTab}
              session={session}
              totalTokens={totalTokens}
              artifacts={artifacts}
              messages={messages}
            />
          </>
        )}
      </div>

      {/* ── Composer ─────────────────────────────────────────────────── */}
      {isObserved ? (
        <div className="composer">
          <div className="obsbanner">
            <Icon name="alert" size={14} />
            <span style={{ flex: 1 }}>
              This is an observed hermes-studio session — read-only. Fork it to
              continue from Olympus.
            </span>
            <button type="button" className="btn pri" onClick={handleFork}>
              Fork to continue
            </button>
          </div>
        </div>
      ) : (
        <Composer
          text={text}
          onTextChange={handleTextareaInput}
          onKeyDown={handleKeyDown}
          onSend={handleSend}
          sending={sending}
          sessionModel={session?.model ?? null}
          sessionAgent={session?.agent ?? null}
        />
      )}
    </div>
  );
}

// ── MessageBubble ──────────────────────────────────────────────────────

const MessageBubble = React.memo(function MessageBubble({
  msg,
  sessionId,
  onFork,
}: {
  msg: Message;
  sessionId: string;
  onFork: () => void;
}) {
  const isUser = msg.role === "user";
  const isSystem = msg.role === "system" || msg.role === "session_meta";
  const ts = fmtTime(msg.timestamp);
  const [copied, setCopied] = useState(false);
  const [tcExpanded, setTcExpanded] = useState<Set<number>>(new Set());
  const [reasonExpanded, setReasonExpanded] = useState(false);

  const handleCopy = useCallback(() => {
    void navigator.clipboard?.writeText(msg.content ?? "");
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [msg.content]);

  const toggleTc = useCallback((idx: number) => {
    setTcExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(idx)) next.delete(idx);
      else next.add(idx);
      return next;
    });
  }, []);

  if (isSystem) {
    return (
      <div className="msg-system" data-ts={ts}>
        <span className="gk">{msg.content}</span>
      </div>
    );
  }

  // Tool-result message (role === "tool") renders inline as a card
  if (msg.role === "tool") {
    const tc = msg.toolCalls?.[0] ?? null;
    return (
      <ToolCard
        key={`tc-result-${msg.messageId}`}
        tc={tc ?? { name: msg.toolName ?? "tool", args: null, result: msg.content }}
        idx={0}
        expanded={false}
        onToggle={() => {}}
      />
    );
  }

  return (
    <div className={isUser ? "msg-user" : "msg-ai"} data-ts={ts}>
      {!isUser && (
        <div className="who">
          {msg.role === "assistant" ? "ASSISTANT" : msg.role.toUpperCase()}
        </div>
      )}

      {/* Reasoning block */}
      {msg.reasoning && (
        <div className="reasoning-block">
          <button
            type="button"
            className="reasoning-toggle"
            onClick={() => setReasonExpanded((v) => !v)}
          >
            <Icon name={reasonExpanded ? "chevron-down" : "chevron-right"} size={11} />
            <span className="gk" style={{ fontSize: 10 }}>
              thinking
            </span>
          </button>
          {reasonExpanded && (
            <div className="reasoning-body">{msg.reasoning}</div>
          )}
        </div>
      )}

      {/* Content */}
      {isUser ? (
        <span>{msg.content}</span>
      ) : (
        <ReactMarkdown remarkPlugins={[remarkGfm]}>
          {msg.content ?? ""}
        </ReactMarkdown>
      )}

      {/* Tool calls embedded in assistant message */}
      {msg.toolCalls && msg.toolCalls.length > 0 && (
        <div className="tc-list">
          {msg.toolCalls.map((tc, idx) =>
            isDiffResult(tc) ? (
              <DiffCard key={idx} tc={tc} />
            ) : (
              <ToolCard
                key={idx}
                tc={tc}
                idx={idx}
                expanded={tcExpanded.has(idx)}
                onToggle={toggleTc}
              />
            ),
          )}
        </div>
      )}

      {/* Message actions */}
      <div className="msg-acts">
        <button type="button" onClick={handleCopy} title="Copy">
          <Icon name={copied ? "check" : "copy"} size={12} />
        </button>
        <button
          type="button"
          title="Fork from here"
          onClick={onFork}
        >
          <Icon name="git-branch" size={12} />
        </button>
        <span className="ts">{ts}</span>
      </div>
    </div>
  );
});

// ── ToolCard ───────────────────────────────────────────────────────────

function ToolCard({
  tc,
  idx,
  expanded,
  onToggle,
}: {
  tc: ToolCall;
  idx: number;
  expanded: boolean;
  onToggle: (idx: number) => void;
}) {
  const status = tc.result != null ? "✓" : "…";
  const statusColor =
    tc.result != null ? "var(--green)" : "var(--amber)";

  const argsStr = React.useMemo(() => {
    if (!tc.args) return "";
    if (typeof tc.args === "string") return tc.args;
    try {
      return JSON.stringify(tc.args, null, 2);
    } catch {
      return String(tc.args);
    }
  }, [tc.args]);

  return (
    <div className="toolcard">
      <div
        className="tc-head"
        style={{ cursor: "pointer" }}
        onClick={() => onToggle(idx)}
      >
        <span className="st" style={{ color: statusColor }}>
          {status}
        </span>
        <span className="nm">{tc.label ?? tc.name}</span>
        <span className="sp" />
        <Icon name={expanded ? "chevron-down" : "chevron-right"} size={12} />
      </div>
      {expanded && (
        <div className="tc-body">
          {argsStr && (
            <div className="tc-out" style={{ marginBottom: 6 }}>
              {argsStr}
            </div>
          )}
          {tc.result != null && (
            <div className="tc-out">{tc.result}</div>
          )}
        </div>
      )}
    </div>
  );
}

// ── DiffCard ───────────────────────────────────────────────────────────

function DiffCard({ tc }: { tc: ToolCall }) {
  const result = tc.result ?? "";
  const args = tc.args as Record<string, unknown> | null;
  const filePath =
    typeof args === "object" && args && typeof args.path === "string"
      ? args.path
      : tc.name;
  const lines = parseDiff(result);
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div className="toolcard">
      <div
        className="tc-head"
        style={{ cursor: "pointer" }}
        onClick={() => setCollapsed((v) => !v)}
      >
        <span className="st" style={{ color: "var(--green)" }}>
          ✓
        </span>
        <span className="nm">{tc.name} · {filePath}</span>
        <span className="sp" />
        <Icon name={collapsed ? "chevron-right" : "chevron-down"} size={12} />
      </div>
      {!collapsed && (
        <div className="tc-body">
          <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, marginBottom: 4, color: "var(--faint)" }}>
            {filePath}
          </div>
          {lines.map((l, i) => (
            <div
              key={i}
              className={`diffln${l.type === "add" ? " add" : l.type === "del" ? " del" : ""}`}
              style={l.type === "hdr" ? { color: "var(--dim)", opacity: 0.7 } : undefined}
            >
              {l.text}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Composer ───────────────────────────────────────────────────────────

function Composer({
  text,
  onTextChange,
  onKeyDown,
  onSend,
  sending,
  sessionModel,
  sessionAgent,
}: {
  text: string;
  onTextChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onSend: () => void;
  sending: boolean;
  sessionModel: string | null;
  sessionAgent: string | null;
}) {
  const { data: agentsData } = useAgents();
  const { data: modelsData } = useModels();
  const agents = agentsData?.agents ?? [];
  const models = modelsData?.models ?? [];

  const [pickerOpen, setPickerOpen] = useState(false);
  const [selectedAgent, setSelectedAgent] = useState<string>(
    sessionAgent ?? agents.find((a) => a.isDefault)?.id ?? "default",
  );
  const [selectedModel, setSelectedModel] = useState<string>(
    sessionModel ?? models[0]?.id ?? "",
  );
  const pickerRef = useRef<HTMLDivElement>(null);

  // Update selections if session data arrives
  useEffect(() => {
    if (sessionAgent) setSelectedAgent(sessionAgent);
  }, [sessionAgent]);
  useEffect(() => {
    if (sessionModel) setSelectedModel(sessionModel);
  }, [sessionModel]);

  // Close picker on outside click
  useEffect(() => {
    if (!pickerOpen) return;
    const handler = (e: MouseEvent) => {
      if (
        pickerRef.current &&
        !pickerRef.current.contains(e.target as Node)
      ) {
        setPickerOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [pickerOpen]);

  return (
    <div className="composer">
      <div className="comp-box">
        <textarea
          rows={1}
          className="composer-input"
          placeholder="Type a message…"
          value={text}
          onChange={onTextChange}
          onKeyDown={onKeyDown}
          autoFocus
        />
        <div className="comp-bar">
          <div className="comp-l">
            {/* Access mode pill */}
            <button type="button" className="modelpill" title="Access mode">
              <Icon name="shield" size={12} />
              <span className="nm">Full access</span>
            </button>
          </div>
          <div className="comp-r">
            {/* Agent / model picker */}
            <div className="selwrap" ref={pickerRef} style={{ position: "relative" }}>
              <button
                type="button"
                className="modelpill"
                title="Agent · model"
                onClick={() => setPickerOpen((v) => !v)}
              >
                <Icon name="bot" size={12} />
                <span className="nm">{selectedAgent}</span>
                <span className="psep" />
                <span className="nm">{selectedModel || "auto"}</span>
                <Icon name="chevron-down" size={10} />
              </button>

              {pickerOpen && (
                <div className="menu selpop" style={{ display: "flex" }}>
                  <div className="gk" style={{ padding: "5px 8px 2px" }}>
                    agent
                  </div>
                  {agents.map((a) => (
                    <button
                      key={a.id}
                      type="button"
                      className={`mi sel-agent${selectedAgent === a.id ? " on" : ""}`}
                      onClick={() => {
                        setSelectedAgent(a.id);
                        setPickerOpen(false);
                      }}
                    >
                      <span>{a.id}</span>
                      {selectedAgent === a.id && <span className="mk2">✓</span>}
                    </button>
                  ))}
                  <div className="cp-div" />
                  <div className="gk" style={{ padding: "5px 8px 2px" }}>
                    model
                  </div>
                  {models.map((m) => (
                    <button
                      key={m.id}
                      type="button"
                      className={`mi${selectedModel === m.id ? " on" : ""}`}
                      onClick={() => {
                        setSelectedModel(m.id);
                        setPickerOpen(false);
                      }}
                    >
                      <span>{m.id}</span>
                      {selectedModel === m.id && <span className="mk2">✓</span>}
                    </button>
                  ))}
                </div>
              )}
            </div>

            {sending && <span className="spin" />}
            <span className="comp-hint">↵ send · ⇧↵ newline</span>
            <button
              type="button"
              className="send"
              onClick={onSend}
              disabled={!text.trim() || sending}
              title="Send"
            >
              <Icon name="arrow-up" size={14} />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── BottomPanel ────────────────────────────────────────────────────────

function BottomPanel({
  tab,
  onTabChange,
  onClose,
}: {
  tab: BpTab;
  onTabChange: (t: BpTab) => void;
  onClose: () => void;
}) {
  const tabs: Array<{ id: BpTab; label: string }> = [
    { id: "terminal", label: "Terminal" },
    { id: "output", label: "Output" },
    { id: "debug", label: "Debug" },
  ];

  return (
    <div className="bpanel">
      <div className="bp-tabs">
        <div className="bp-tablist">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              className={`bp-tab${tab === t.id ? " on" : ""}`}
              onClick={() => onTabChange(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="bp-right">
          <button type="button" className="icobtn" title="Clear">
            <Icon name="trash" size={13} />
          </button>
          <button
            type="button"
            className="icobtn"
            title="Close panel"
            onClick={onClose}
          >
            <Icon name="chevron-down" size={13} />
          </button>
        </div>
      </div>
      <div className="bp-body">
        {tab === "terminal" &&
          MOCK_TERMINAL_LINES.map((l, i) => (
            <div key={i} className={`ln ${l.cls}`}>
              {l.text}
            </div>
          ))}
        {tab === "output" && (
          <div className="ln d">No output yet.</div>
        )}
        {tab === "debug" && (
          <div className="ln d">No debug data.</div>
        )}
      </div>
    </div>
  );
}

// ── RightPanel ─────────────────────────────────────────────────────────

function RightPanel({
  tab,
  onTabChange,
  session,
  totalTokens,
  artifacts,
  messages,
}: {
  tab: RsTab;
  onTabChange: (t: RsTab) => void;
  session: import("../types").Session | undefined;
  totalTokens: number;
  artifacts: Array<{ path: string; status: "new" | "modified" }>;
  messages: Message[];
}) {
  const tabs: Array<{ id: RsTab; icon: import("../components/Icon").IconName; title: string }> = [
    { id: "overview", icon: "layout-grid", title: "Overview" },
    { id: "outline", icon: "list", title: "Outline" },
    { id: "settings", icon: "gear", title: "Settings" },
    { id: "browser", icon: "globe", title: "Browser" },
    { id: "diff", icon: "git-compare", title: "Diff" },
    { id: "git", icon: "git-branch", title: "Git" },
    { id: "ai", icon: "sparkles", title: "AI" },
  ];

  // Collect diffs from messages for the diff tab
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
    <aside className="rsidebar">
      {/* Tab bar — pill row */}
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

      {/* Overview */}
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
                    <span
                      className="tag"
                      style={{
                        color: "var(--silver)",
                        background: "var(--silver-wash)",
                      }}
                    >
                      new
                    </span>
                  ) : (
                    <span
                      className="tag"
                      style={{
                        color: "var(--green)",
                        background: "var(--green-wash)",
                      }}
                    >
                      M
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Outline */}
      {tab === "outline" && (
        <div className="rsv on" data-rsv="outline">
          <div className="rs-sec" style={{ gap: 4 }}>
            <div className="gk" style={{ marginBottom: 4 }}>
              transcript
            </div>
            {MOCK_OUTLINE.map((line, i) => (
              <div key={i} className="ovl-it">
                {line}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Settings */}
      {tab === "settings" && (
        <div className="rsv on" data-rsv="settings">
          <div className="rs-sec">
            <div className="gk">session settings</div>
            <div className="grow">
              <span style={{ fontSize: 12 }}>Auto-approve tools</span>
              <span className="gsw on">
                <i />
              </span>
            </div>
            <div className="grow">
              <span style={{ fontSize: 12 }}>Extended thinking</span>
              <span className="gsw">
                <i />
              </span>
            </div>
            <div className="grow">
              <span style={{ fontSize: 12 }}>Notify on finish</span>
              <span className="gsw on">
                <i />
              </span>
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

      {/* Browser preview stub */}
      {tab === "browser" && (
        <div className="rsv on" data-rsv="browser">
          <div className="rs-sec">
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 7,
                padding: "4px 9px",
                background: "var(--elev)",
                border: "1px solid var(--border)",
                borderRadius: "var(--radius-full)",
                fontFamily: "var(--font-mono)",
                fontSize: 10,
                color: "var(--dim)",
              }}
            >
              <Icon name="globe" size={12} />
              <span>localhost:5173</span>
            </div>
            <div
              style={{
                height: 200,
                border: "1px solid var(--border)",
                borderRadius: "var(--radius)",
                background:
                  "repeating-linear-gradient(45deg,var(--elev),var(--elev) 8px,var(--chrome) 8px,var(--chrome) 16px)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <span
                style={{
                  fontFamily: "var(--font-mono)",
                  fontSize: 10,
                  color: "var(--faint)",
                }}
              >
                app preview
              </span>
            </div>
          </div>
        </div>
      )}

      {/* Diff */}
      {tab === "diff" && (
        <div className="rsv on" data-rsv="diff">
          {diffs.length === 0 ? (
            <div className="rs-sec">
              <div className="gk" style={{ padding: "6px 0" }}>
                No diffs yet
              </div>
            </div>
          ) : (
            diffs.map((d, i) => (
              <div key={i} className="rs-sec" style={{ gap: 6 }}>
                <div className="kv">
                  <span className="k">{d.path.split("/").pop()}</span>
                  <span className="v" style={{ color: "var(--green)" }}>
                    M
                  </span>
                </div>
                <div
                  style={{
                    background: "var(--elev)",
                    border: "1px solid var(--border)",
                    borderRadius: "var(--radius)",
                    padding: "8px 10px",
                    overflow: "auto",
                    maxHeight: 220,
                  }}
                >
                  {parseDiff(d.result).map((l, j) => (
                    <div
                      key={j}
                      className={`diffln${l.type === "add" ? " add" : l.type === "del" ? " del" : ""}`}
                      style={
                        l.type === "hdr" ? { color: "var(--dim)", opacity: 0.7 } : undefined
                      }
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

      {/* Git */}
      {tab === "git" && (
        <div className="rsv on" data-rsv="git">
          <div className="rs-sec">
            <div className="kv">
              <span className="k">BRANCH</span>
              <span className="v">{MOCK_CTX.branch}</span>
            </div>
            <div className="kv">
              <span className="k">PR</span>
              <span className="v">{MOCK_CTX.pr}</span>
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
                    style={
                      a.status === "new"
                        ? { color: "var(--silver)", background: "var(--silver-wash)" }
                        : { color: "var(--green)", background: "var(--green-wash)" }
                    }
                  >
                    {a.status === "new" ? "A" : "M"}
                  </span>
                </div>
              ))}
              <div className="dr-actions" style={{ marginTop: 4 }}>
                <button type="button" className="btn pri">
                  Commit
                </button>
                <button type="button" className="btn">
                  Stash
                </button>
              </div>
            </div>
          )}
        </div>
      )}

      {/* AI */}
      {tab === "ai" && (
        <div className="rsv on" data-rsv="ai">
          <div className="rs-sec">
            <div className="gk">AI suggestions</div>
            <div
              style={{
                fontSize: 12,
                color: "var(--dim)",
                lineHeight: "var(--lh-relaxed)",
              }}
            >
              AI suggestions appear here during active sessions.
            </div>
          </div>
        </div>
      )}
    </aside>
  );
}

// ── Empty pane (no session selected) ──────────────────────────────────

export function SessionEmptyPane() {
  return (
    <>
      <div className="gv-head">
        <span className="gv-title">Sessions</span>
      </div>
      <div className="gv-body">
        <div className="empty-state">
          <div className="empty-state-icon">
            <Icon name="message-square" size={32} />
          </div>
          <div className="empty-state-title">Select a session</div>
          <div className="empty-state-msg">
            Choose a session from the sidebar or create a new one to start
            chatting.
          </div>
        </div>
      </div>
    </>
  );
}
