/**
 * Composer — the chat input bar.
 *
 * Bug fixes:
 *  - Bug 2: Thinking toggle (localStorage 'olympus-thinking'), persisted.
 *  - Bug 3: Agent label shows id + provider only (e.g. "coding-agent · openai-codex"),
 *    no "ACP/CLI" dual-label.
 *  - Bug 4: Fetches /api/agents (via useAgents hook) — Claude, Codex, etc.
 */

import React, { useState, useEffect, useRef } from "react";
import { Icon } from "../../../components/Icon";
import { useAgents, useModels } from "../../../hooks/queries";
import type { AgentInfo } from "../../../types";

const THINKING_KEY = "olympus-thinking";

function loadThinking(): boolean {
  try {
    return localStorage.getItem(THINKING_KEY) === "true";
  } catch {
    return false;
  }
}

function saveThinking(v: boolean) {
  try {
    localStorage.setItem(THINKING_KEY, String(v));
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
  statusLabel,
  sessionModel,
  sessionAgent,
}: {
  text: string;
  onTextChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onSend: () => void;
  sending: boolean;
  statusLabel: string | null;
  sessionModel: string | null;
  sessionAgent: string | null;
}) {
  const { data: agentsData } = useAgents();
  const { data: modelsData } = useModels();
  const agents = agentsData?.agents ?? [];
  const models = modelsData?.models ?? [];

  const [pickerOpen, setPickerOpen] = useState(false);
  const [thinking, setThinking] = useState(loadThinking);
  const [selectedAgent, setSelectedAgent] = useState<string>(
    sessionAgent ?? agents.find((a) => a.isDefault)?.id ?? "default",
  );
  const [selectedModel, setSelectedModel] = useState<string>(
    sessionModel ?? models[0]?.id ?? "",
  );
  const pickerRef = useRef<HTMLDivElement>(null);

  // Update selections if session data arrives late
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
      if (pickerRef.current && !pickerRef.current.contains(e.target as Node)) {
        setPickerOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [pickerOpen]);

  const toggleThinking = () => {
    const next = !thinking;
    setThinking(next);
    saveThinking(next);
  };

  // Bug 3: show id + provider for the active agent
  const activeAgent: AgentInfo | undefined =
    agents.find((a) => a.id === selectedAgent) ?? agents.find((a) => a.isDefault);
  const agentLabel = activeAgent
    ? `${activeAgent.id}${activeAgent.provider ? " · " + activeAgent.provider : ""}`
    : selectedAgent;

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
            {/* Bug 2: Thinking toggle */}
            <button
              type="button"
              className={`modelpill${thinking ? " on" : ""}`}
              title="Extended thinking"
              onClick={toggleThinking}
              data-thinking={thinking}
            >
              <Icon name="brain" size={12} />
              <span className="nm">Thinking</span>
            </button>
          </div>
          <div className="comp-r">
            {/* Agent / model picker — Bug 3+4 */}
            <div className="selwrap" ref={pickerRef} style={{ position: "relative" }}>
              <button
                type="button"
                className="modelpill"
                title="Agent · model"
                onClick={() => setPickerOpen((v) => !v)}
              >
                <Icon name="bot" size={12} />
                <span className="nm">{agentLabel}</span>
                <span className="psep" />
                <span className="nm">{selectedModel || activeAgent?.model || "auto"}</span>
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
                        if (a.model) setSelectedModel(a.model);
                        setPickerOpen(false);
                      }}
                    >
                      <span>
                        {a.id}
                        {a.provider && (
                          <span style={{ color: "var(--faint)", marginLeft: 6 }}>
                            {a.provider}
                          </span>
                        )}
                      </span>
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
            {statusLabel && (
              <span
                className="comp-hint"
                style={{ color: "var(--amber)", fontStyle: "italic" }}
              >
                {statusLabel}
              </span>
            )}
            {!statusLabel && <span className="comp-hint">↵ send · ⇧↵ newline</span>}
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
