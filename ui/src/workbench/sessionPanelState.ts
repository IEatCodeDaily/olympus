const states = new Map<string, Record<string, unknown>>();
const PREFIX = "olympus-session-panel:";

function stateFor(sessionId: string): Record<string, unknown> {
  const cached = states.get(sessionId);
  if (cached) return cached;
  try {
    const stored = JSON.parse(localStorage.getItem(`${PREFIX}${sessionId}`) ?? "null") as Record<string, unknown> | null;
    if (stored) {
      states.set(sessionId, stored);
      return stored;
    }
  } catch {
    // Ignore stale or unavailable local state.
  }
  const state = {};
  states.set(sessionId, state);
  return state;
}

export function readSessionPanelState<T>(sessionId: string, key: string, fallback: T): T {
  const value = stateFor(sessionId)[key];
  return value === undefined ? fallback : value as T;
}

export function writeSessionPanelState(sessionId: string, key: string, value: unknown): void {
  const state = stateFor(sessionId);
  state[key] = value;
  try {
    localStorage.setItem(`${PREFIX}${sessionId}`, JSON.stringify(state));
  } catch {
    // In-memory state still keeps view switches isolated.
  }
}
