/**
 * Composer — the chat input bar.
 *
 * The model configuration is a single pill: ( agent_icon | model | thinking ).
 * Agent is LOCKED from session creation — only model + thinking are configurable
 * here. One popup controls model + thinking level.
 */

import React, { useState, useEffect, useRef } from "react";
import { Icon, providerLogoIcon } from "../../../components/Icon";
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

  // The agent is locked from the session — find it to get provider + icon
  const lockedAgent = agents.find(
    (a) => a.id === sessionAgent || (sessionAgent == null && a.isDefault),
  );
  const agentIcon = providerLogoIcon(lockedAgent?.provider);
  const agentName = lockedAgent?.id ?? sessionAgent ?? "agent";

  const [popupOpen, setPopupOpen] = useState(false);
  const [thinking, setThinking] = useState<ThinkingLevel>(loadThinking);
  const [selectedModel, setSelectedModel] = useState<string>(
    sessionModel ?? lockedAgent?.model ?? models[0]?.id ?? "",
  );
  const popupRef = useRef<HTMLDivElement>(null);

  // Sync when session data arrives
  useEffect(() => {
    if (sessionModel) setSelectedModel(sessionModel);
  }, [sessionModel]);

  // Close popup on outside click
  useEffect(() => {
    if (!popupOpen) return;
    const handler = (e: MouseEvent) => {
      if (popupRef.current && !popupRef.current.contains(e.target as Node)) {
        setPopupOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [popupOpen]);

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
          placeholder="Type a message…"
          value={text}
          onChange={onTextChange}
          onKeyDown={onKeyDown}
          autoFocus
        />
        <div className="comp-bar">
          <div className="comp-l">
            <button type="button" className="modelpill" title="Access mode">
              <Icon name="shield" size={12} />
              <span className="nm">Full access</span>
            </button>
          </div>
          <div className="comp-r">
            {/* Locked agent chip — harness is fixed at session creation */}
            <span className="modelpill locked" title={`Agent locked: ${agentName} (${lockedAgent?.provider ?? "—"})`}>
              <Icon name={agentIcon} size={13} />
              <span className="nm">{agentName}</span>
              <Icon name="round" size={9} />
            </span>

            {/* Model + thinking picker — editable */}
            <div className="selwrap" ref={popupRef} style={{ position: "relative" }}>
              <button
                type="button"
                className="modelpill"
                title="Model & thinking"
                onClick={() => setPopupOpen((v) => !v)}
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

              {popupOpen && (
                <div className="menu selpop" style={{ display: "flex" }}>
                  {/* Model selector */}
                  <div className="gk" style={{ padding: "5px 8px 2px" }}>model</div>
                  {models.map((m) => (
                    <button
                      key={m.id}
                      type="button"
                      className={`mi${selectedModel === m.id ? " on" : ""}`}
                      onClick={() => {
                        setSelectedModel(m.id);
                        setPopupOpen(false);
                      }}
                    >
                      <span>{m.id}</span>
                      {selectedModel === m.id && <span className="mk2">✓</span>}
                    </button>
                  ))}

                  <div className="cp-div" />

                  {/* Thinking level */}
                  <div className="gk" style={{ padding: "5px 8px 2px" }}>thinking</div>
                  {(["off", "low", "medium", "high"] as ThinkingLevel[]).map((lvl) => (
                    <button
                      key={lvl}
                      type="button"
                      className={`mi${thinking === lvl ? " on" : ""}`}
                      onClick={() => {
                        setThink(lvl);
                        setPopupOpen(false);
                      }}
                    >
                      <span>{lvl === "off" ? "Off" : lvl.charAt(0).toUpperCase() + lvl.slice(1)}</span>
                      {thinking === lvl && <span className="mk2">✓</span>}
                    </button>
                  ))}
                </div>
              )}
            </div>

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
