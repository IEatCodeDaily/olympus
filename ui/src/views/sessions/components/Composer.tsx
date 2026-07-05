/**
 * Composer — the chat input bar.
 *
 * Layout:
 *   [ textarea …………………………………………………………………… ]
 *   [ (+)                          agent-icon · model · thinking · send ] (idle)
 *   [ running on <node>                        ← auxiliary, below the bar
 *
 * Two modes:
 * - IDLE (no turn running): textarea = prompt, send button sends the message.
 * - RUNNING (turn in flight): the send button becomes a STOP button (square).
 *   Typing into the textarea + Enter injects a STEER (interrupt) into the
 *   running turn instead of starting a new one. A small hint above the bar
 *   shows "steer running turn" so the user knows what Enter will do.
 */

import React, { useState, useEffect, useRef } from "react";
import { Icon } from "../../../components/Icon";
import { BrandIcon, agentBrand } from "../../../components/BrandIcons";
import { useAgents, useModels } from "../../../hooks/queries";

const THINKING_KEY = "olympus-thinking";

type ThinkingLevel = "off" | "low" | "medium" | "high";

function loadThinking(): ThinkingLevel {
  try {
    const v = localStorage.getItem(THINKING_KEY);
    return (v as ThinkingLevel) ?? "off";
  } catch {
    return "off";
  }
}

function saveThinking(v: ThinkingLevel) {
  try {
    localStorage.setItem(THINKING_KEY, v);
  } catch {
    // ignore
  }
}

export function Composer({
  text,
  onTextChange,
  onKeyDown,
  onSend,
  onStop,
  onSteer,
  sending,
  sessionModel,
  sessionAgent,
  sessionNode,
}: {
  text: string;
  onTextChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onSend: (model?: string, thinking?: string) => void;
  onStop: () => void;
  onSteer: (text: string) => void;
  sending: boolean;
  sessionModel: string | null;
  sessionAgent: string | null;
  sessionNode: string | null;
}) {
  const { data: agentsData } = useAgents();
  const agents = agentsData?.agents ?? [];

  // The agent is locked from the session — find it for icon + provider.
  const lockedAgent = agents.find(
    (a) => a.id === sessionAgent || (sessionAgent == null && a.isDefault),
  );
  const agentIcon = agentBrand(lockedAgent?.kind, lockedAgent?.provider);
  const agentName = lockedAgent?.id ?? sessionAgent ?? "agent";
  // The main in-process node reports as "local"; show it as "olympus".
  const nodeLabel = !sessionNode || sessionNode === "local" ? "olympus" : sessionNode;

  // Models are AGENT-SPECIFIC: only what this agent's provider can serve.
  const { data: modelsData } = useModels(lockedAgent?.id ?? sessionAgent);
  const models = modelsData?.models ?? [];

  const [modelOpen, setModelOpen] = useState(false);
  const [plusOpen, setPlusOpen] = useState(false);
  const [thinking, setThinking] = useState<ThinkingLevel>(loadThinking);
  const [selectedModel, setSelectedModel] = useState<string>(
    sessionModel ?? lockedAgent?.model ?? "",
  );
  const modelRef = useRef<HTMLDivElement>(null);
  const plusRef = useRef<HTMLDivElement>(null);

  // Keep the selected model valid for the agent: prefer session model, else the
  // agent's default, else the first model the provider offers.
  useEffect(() => {
    if (sessionModel) {
      setSelectedModel(sessionModel);
      return;
    }
    const valid = models.some((m) => m.id === selectedModel);
    if (!valid) {
      setSelectedModel(lockedAgent?.model ?? models[0]?.id ?? "");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionModel, lockedAgent?.id, models.length]);

  // Close popups on outside click.
  useEffect(() => {
    if (!modelOpen && !plusOpen) return;
    const handler = (e: MouseEvent) => {
      if (modelOpen && modelRef.current && !modelRef.current.contains(e.target as Node)) {
        setModelOpen(false);
      }
      if (plusOpen && plusRef.current && !plusRef.current.contains(e.target as Node)) {
        setPlusOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [modelOpen, plusOpen]);

  const setThink = (v: ThinkingLevel) => {
    setThinking(v);
    saveThinking(v);
  };

  const thinkingLabel =
    thinking === "off" ? "" : thinking.charAt(0).toUpperCase() + thinking.slice(1);
  const modelLabel = selectedModel || lockedAgent?.model || "auto";

  return (
    <div className="composer">
      <div className="comp-box">
        <textarea
          rows={1}
          className="composer-input"
          placeholder={sending ? "Steer the running turn…" : "Type a message…"}
          value={text}
          onChange={onTextChange}
          onKeyDown={onKeyDown}
          autoFocus
        />
        <div className="comp-bar">
          {/* LEFT: + menu — attachments, mentions, etc. */}
          <div className="comp-l">
            <div className="selwrap" ref={plusRef} style={{ position: "relative" }}>
              <button
                type="button"
                className="plusbtn"
                title="Attach, mention…"
                aria-label="Add attachment or mention"
                onClick={() => setPlusOpen((v) => !v)}
              >
                <Icon name="plus" size={16} />
              </button>
              {plusOpen && (
                <div className="menu pluspop" style={{ display: "flex" }}>
                  <button type="button" className="mi" onClick={() => setPlusOpen(false)}>
                    <Icon name="paperclip" size={13} />
                    <span>Attach file</span>
                  </button>
                  <button type="button" className="mi" onClick={() => setPlusOpen(false)}>
                    <Icon name="at" size={13} />
                    <span>Mention session</span>
                  </button>
                  <button type="button" className="mi" onClick={() => setPlusOpen(false)}>
                    <Icon name="file" size={13} />
                    <span>Reference file</span>
                  </button>
                </div>
              )}
            </div>
          </div>

          {/* RIGHT: model selector + thinking + send/stop */}
          <div className="comp-r">
            <div className="selwrap" ref={modelRef} style={{ position: "relative" }}>
              <button
                type="button"
                className="modelpill"
                title="Model & thinking"
                onClick={() => setModelOpen((v) => !v)}
              >
                <span className="nm">{modelLabel}</span>
                {thinkingLabel && (
                  <>
                    <span className="psep" />
                    <span className="nm">{thinkingLabel}</span>
                  </>
                )}
                <Icon name="chevron-down" size={10} />
              </button>

              {modelOpen && (
                <div className="menu selpop" style={{ display: "flex" }}>
                  <div className="gk" style={{ padding: "5px 8px 2px" }}>
                    model · {lockedAgent?.provider ?? "—"}
                  </div>
                  {models.length === 0 && (
                    <div className="gk" style={{ padding: "4px 8px", opacity: 0.6 }}>
                      no models for this provider
                    </div>
                  )}
                  {models.map((m) => (
                    <button
                      key={m.id}
                      type="button"
                      className={`mi${selectedModel === m.id ? " on" : ""}`}
                      onClick={() => {
                        setSelectedModel(m.id);
                        setModelOpen(false);
                      }}
                    >
                      <span>{m.id}</span>
                      {selectedModel === m.id && <span className="mk2">✓</span>}
                    </button>
                  ))}

                  <div className="cp-div" />

                  <div className="gk" style={{ padding: "5px 8px 2px" }}>thinking</div>
                  {(["off", "low", "medium", "high"] as ThinkingLevel[]).map((lvl) => (
                    <button
                      key={lvl}
                      type="button"
                      className={`mi${thinking === lvl ? " on" : ""}`}
                      onClick={() => {
                        setThink(lvl);
                        setModelOpen(false);
                      }}
                    >
                      <span>{lvl === "off" ? "Off" : lvl.charAt(0).toUpperCase() + lvl.slice(1)}</span>
                      {thinking === lvl && <span className="mk2">✓</span>}
                    </button>
                  ))}
                </div>
              )}
            </div>

            {sending ? (
              <button
                type="button"
                className="send stop"
                onClick={onStop}
                title="Stop the running turn"
                aria-label="Stop"
              >
                <Icon name="stop" size={14} />
              </button>
            ) : (
              <button
                type="button"
                className="send"
                onClick={() => onSend(selectedModel, thinking === "off" ? undefined : thinking)}
                disabled={!text.trim()}
                title="Send"
              >
                <Icon name="arrow-up" size={14} />
              </button>
            )}
          </div>
        </div>
      </div>

      {/* Meta row — node · agent. OUTSIDE the composer box, below it, but
          inside .composer so it stays bounded to the composer's width. */}
      <div className="comp-meta">
        {sending ? (
          <span className="cm-item cm-steer" title="Steer the running turn">
            <Icon name="zap" size={11} />
            <span>steer running turn</span>
          </span>
        ) : (
          <>
            <span className="cm-item" title={`Running on node: ${nodeLabel}`}>
              <Icon name="server" size={11} />
              <span>{nodeLabel}</span>
            </span>
            <span className="cm-dot" />
            <span
              className="cm-item"
              title={`Agent: ${agentName} (${lockedAgent?.provider ?? "—"}) — locked for this session`}
            >
              <BrandIcon name={agentIcon} size={12} />
              <span>{agentName}</span>
            </span>
            {thinking !== "off" && (
              <>
                <span className="cm-dot" />
                <span className="cm-item" title={`Thinking level: ${thinkingLabel}`}>
                  <Icon name="brain" size={11} />
                  <span>{thinkingLabel}</span>
                </span>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
