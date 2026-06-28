# ACP wire spike: `hermes acp`

Date: 2026-06-28

Scope: drove the real `hermes acp` subprocess over stdio with a throwaway `HERMES_HOME` (`/tmp/hermes-acp-spike-50hy6cv3`). The live profile/state DB was not used. Full raw frame captures are written next to this note:

- `docs/reviews/acp-wire-spike-capture.json` — initialize, `session/new`, `session/prompt`, `session/set_model`, concurrent `/steer`, `session/cancel`.
- `docs/reviews/acp-wire-spike-resume-capture.json` — fresh process `session/resume` against the throwaway ACP session.

## Verdict

ACP is viable for the Olympus bridge, but only as the real Agent Client Protocol method set. There is no ACP `steer` method and no generic ACP `slash` method. Slash commands are prompt text sent through `session/prompt`.

Transport is newline-delimited JSON-RPC 2.0 over stdio. The Python ACP connection reads stdout with `readline()` and writes one JSON object per line; no `Content-Length` framing was observed or required.

## Method table

| Capability | JSON-RPC method | Params shape | Result / notification | Verified |
|---|---|---|---|---|
| handshake | `initialize` | `{ protocolVersion, clientCapabilities, clientInfo }` | response advertises `agentCapabilities`, `agentInfo`, `authMethods`, `protocolVersion` | yes |
| new session | `session/new` | `{ cwd, mcpServers }` | response includes `sessionId`, `_meta.hermes.sessionProvenance`, `models`, `modes`; then `session/update` advertises slash commands | yes |
| prompt | `session/prompt` | `{ sessionId, prompt: [{ type: "text", text }], messageId? }` | streams `session/update` chunks; final response has `stopReason` and optional `usage` | yes |
| model switch | `session/set_model` | `{ sessionId, modelId }` | `{}` response; no model-state notification observed | yes |
| mid-turn steer | `session/prompt` with text `/steer ...` while another prompt is running | same as prompt | immediate streamed ack, then active turn can consume steering | yes |
| cancel | `session/cancel` | notification `{ sessionId }` (no `id`) | active prompt response returns `stopReason: "cancelled"` | yes |
| resume | `session/resume` | `{ sessionId, cwd, mcpServers }` | replays prior transcript via in-call `session/update`, then returns model/mode state | yes, ACP-owned only |
| slash commands | none | slash text must be sent as `session/prompt` text | advertised via `available_commands_update` | yes |

Generated method map source: `venv/lib/python3.11/site-packages/acp/meta.py` contains `session/cancel`, `session/close`, `session/fork`, `session/list`, `session/load`, `session/new`, `session/prompt`, `session/resume`, `session/set_config_option`, `session/set_mode`, and `session/set_model`. It does not contain `steer` or `slash`.

Hermes source checks:

- `acp_adapter/server.py:1352-1363` intercepts slash text inside `session/prompt`.
- `acp_adapter/server.py:1721-1741` dispatches slash commands including `steer`.
- `acp_adapter/session.py:488-490` refuses to restore rows whose `source != "acp"`.
- `hermes acp --help` has no `--resume`; resume is an ACP method call, not a CLI flag.

## Observed frames

Session ID from this run: `2651c325-3bea-426a-a94f-89a3987e6398`.

### 1. Initialize / handshake

Client request:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":true,"writeTextFile":true}},"clientInfo":{"name":"olympus-acp-wire-spike","version":"0.1.0"}}}
```

Agent response:

```json
{"jsonrpc":"2.0","id":1,"result":{"agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true},"sessionCapabilities":{"fork":{},"list":{},"resume":{}}},"agentInfo":{"name":"hermes-agent","version":"0.17.0"},"authMethods":[{"description":"Authenticate Hermes using the currently configured zai runtime credentials.","id":"zai","name":"zai runtime credentials"},{"args":["--setup"],"description":"Open Hermes' interactive model/provider setup in a terminal. Use this when Hermes has not been configured on this machine yet.","id":"hermes-setup","name":"Configure Hermes provider","type":"terminal"}],"protocolVersion":1}}
```

### 2. `session/new`

Client request:

```json
{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/home/rpw/olympus","mcpServers":[]}}
```

Agent response shape, with the full frame in `acp-wire-spike-capture.json`:

```json
{"jsonrpc":"2.0","id":2,"result":{"_meta":{"hermes":{"sessionProvenance":{"acpSessionId":"2651c325-3bea-426a-a94f-89a3987e6398","currentHermesSessionId":"2651c325-3bea-426a-a94f-89a3987e6398","rootHermesSessionId":"2651c325-3bea-426a-a94f-89a3987e6398","parentHermesSessionId":null,"sessionKind":"root","compressionDepth":0}}},"models":{"currentModelId":"zai:glm-4.5"},"modes":{"currentModeId":"default"},"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398"}}
```

Post-response command advertisement notification:

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"availableCommands":[{"description":"List available commands","name":"help"},{"description":"Show current model and provider, or switch models","input":{"hint":"model name to switch to"},"name":"model"},{"description":"List available tools with descriptions","name":"tools"},{"description":"Show conversation message counts by role","name":"context"},{"description":"Clear conversation history","name":"reset"},{"description":"Compress conversation context","name":"compact"},{"description":"Inject guidance into the currently running agent turn","input":{"hint":"guidance for the active turn"},"name":"steer"},{"description":"Queue a prompt to run after the current turn finishes","input":{"hint":"prompt to run next"},"name":"queue"},{"description":"Show Hermes version","name":"version"}],"sessionUpdate":"available_commands_update"}}}
```

### 3. `session/prompt`: `say exactly PONG`

Client request:

```json
{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","prompt":[{"type":"text","text":"say exactly PONG"}],"messageId":"f18f3cfc-7a92-4e79-b893-895a0039d257"}}
```

Streamed notifications contained PONG as two chunks:

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":{"text":"P","type":"text"},"sessionUpdate":"agent_message_chunk"}}}
```

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":{"text":"ONG","type":"text"},"sessionUpdate":"agent_message_chunk"}}}
```

Final response:

```json
{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn","usage":{"cachedReadTokens":2834,"inputTokens":16394,"outputTokens":7,"thoughtTokens":0,"totalTokens":16401}}}
```

Assertion: concatenated stream text contained `PONG`.

### 4. `session/set_model`

Client request, setting the current advertised model back onto the session:

```json
{"jsonrpc":"2.0","id":4,"method":"session/set_model","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","modelId":"zai:glm-4.5"}}
```

Agent response:

```json
{"jsonrpc":"2.0","id":4,"result":{}}
```

No `session/update` notification was observed for this model switch. Source agrees: `set_session_model` returns `SetSessionModelResponse()` after persisting the session; it does not send a model-state notification.

### 5. Concurrent `/steer` via `session/prompt`

Long-running prompt request:

```json
{"jsonrpc":"2.0","id":5,"method":"session/prompt","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","prompt":[{"type":"text","text":"Use the terminal tool to run: python3 -c \"import time; time.sleep(8); print(\\\"SLEPT\\\")\". After the command finishes, answer with a short sentence. If any steering arrives while you are working, obey it."}],"messageId":"c232e924-58af-47d1-9c01-f9f535461844"}}
```

Concurrent steer request sent while the first prompt was active:

```json
{"jsonrpc":"2.0","id":6,"method":"session/prompt","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","prompt":[{"type":"text","text":"/steer In the final answer include the marker STEER_OK."}],"messageId":"4b554dbe-7eed-456e-89c5-0224ede3944b"}}
```

Immediate streamed acknowledgement:

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":{"text":"⏩ Steer queued for the active turn: In the final answer include the marker STEER_OK.","type":"text"},"sessionUpdate":"agent_message_chunk"}}}
```

Steer request final response:

```json
{"jsonrpc":"2.0","id":6,"result":{"stopReason":"end_turn"}}
```

The active turn then ran the terminal tool and included the steering marker. Representative tool notifications:

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":[{"content":{"text":"$ python3 -c \"import time; time.sleep(8); print(\\\"SLEPT\\\")\"","type":"text"},"type":"content"}],"kind":"execute","locations":[],"title":"terminal: python3 -c \"import time; time.sleep(8); print(\\\"SLEPT\\\")\"","toolCallId":"tc-14facd0d75ed","sessionUpdate":"tool_call"}}}
```

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":[{"content":{"text":"terminal result\n- **output:** SLEPT\n- **exit_code:** 0","type":"text"},"type":"content"}],"kind":"execute","status":"completed","toolCallId":"tc-14facd0d75ed","sessionUpdate":"tool_call_update"}}}
```

The final assistant text streamed `STEER_OK` in chunks:

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":{"text":" STE","type":"text"},"sessionUpdate":"agent_message_chunk"}}}
```

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":{"text":"ER","type":"text"},"sessionUpdate":"agent_message_chunk"}}}
```

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":{"text":"_OK","type":"text"},"sessionUpdate":"agent_message_chunk"}}}
```

Long prompt final response:

```json
{"jsonrpc":"2.0","id":5,"result":{"stopReason":"end_turn","usage":{"cachedReadTokens":32509,"inputTokens":32997,"outputTokens":275,"thoughtTokens":0,"totalTokens":33272}}}
```

Observation: `/steer` works only as prompt text, and only if it lands while the turn is active. In this run it did influence the final output.

### 6. `session/cancel`

Prompt request to cancel:

```json
{"jsonrpc":"2.0","id":7,"method":"session/prompt","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","prompt":[{"type":"text","text":"Use the terminal tool to run: python3 -c \"import time; time.sleep(20); print(\\\"DONE\\\")\". Then write one paragraph. This turn is expected to be cancelled."}],"messageId":"71ef9709-ac13-4388-84fe-156be4cf16b7"}}
```

Cancel notification:

```json
{"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398"}}
```

Prompt response after cancel:

```json
{"jsonrpc":"2.0","id":7,"result":{"stopReason":"cancelled","usage":{"cachedReadTokens":32509,"inputTokens":32997,"outputTokens":275,"thoughtTokens":0,"totalTokens":33272}}}
```

`session/cancel` is a JSON-RPC notification. There is no response to the cancel frame itself; the active prompt response carries cancellation.

### 7. `session/resume`

A new `hermes acp` subprocess was started against the same throwaway `HERMES_HOME`, then resumed the ACP-owned session.

Client request:

```json
{"jsonrpc":"2.0","id":2,"method":"session/resume","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","cwd":"/home/rpw/olympus","mcpServers":[]}}
```

History replay happened before the response via `session/update`, for example:

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":{"text":"say exactly PONG","type":"text"},"sessionUpdate":"user_message_chunk"}}}
```

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"2651c325-3bea-426a-a94f-89a3987e6398","update":{"content":{"text":"PONG","type":"text"},"sessionUpdate":"agent_message_chunk"}}}
```

Then the response returned current session state:

```json
{"jsonrpc":"2.0","id":2,"result":{"_meta":{"hermes":{"sessionProvenance":{"acpSessionId":"2651c325-3bea-426a-a94f-89a3987e6398","currentHermesSessionId":"2651c325-3bea-426a-a94f-89a3987e6398","rootHermesSessionId":"2651c325-3bea-426a-a94f-89a3987e6398","parentHermesSessionId":null,"sessionKind":"root","compressionDepth":0}}},"models":{"currentModelId":"zai:glm-4.5"},"modes":{"currentModeId":"default"}}}
```

Important limitation: `session/resume` exists, but Hermes `SessionManager._restore` only restores persisted rows where `source == "acp"`. A non-ACP external row is treated as not restorable by ACP. For Olympus, cross-channel continuation must fork/copy into an ACP-owned session through a Hermes-owned invariant-preserving path; do not expect `session/resume` to attach directly to arbitrary `telegram`, `cli`, `cron`, `discord`, etc. sessions.

## Surprises vs the prior documented contract

1. `steer` is not an ACP method. The bridge must send `/steer ...` as `session/prompt` text.
2. There is no generic `slash` ACP method. Slash commands are advertised by `session/update` and invoked as prompt text.
3. `session/cancel` is a notification, not a request/response pair.
4. `session/set_model` returns `{}` and did not emit a model update notification in this run.
5. `session/resume` replays history through `session/update` before returning, but only for ACP-owned stored sessions.
6. `hermes acp` has no `--resume` CLI flag. Resume is wire-level JSON-RPC only.
