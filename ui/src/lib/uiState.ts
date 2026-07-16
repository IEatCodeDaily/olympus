import { apiFetch } from "../api";

const PREFIX = "olympus-ui-state:";

function key(surface: string): string {
  return `${PREFIX}${surface}`;
}

function readLocal(surface: string): unknown | null {
  try {
    const raw = localStorage.getItem(key(surface));
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

function writeLocal(surface: string, state: unknown): void {
  try {
    localStorage.setItem(key(surface), JSON.stringify(state));
  } catch {
    // local persistence is best effort; keep the UI path synchronous.
  }
}

export function getLocalUiState<T>(surface: string): T | null {
  return readLocal(surface) as T | null;
}

export async function loadWorkspaceState<T>(surface: string): Promise<T | null> {
  const local = getLocalUiState<T>(surface);
  try {
    const res = await apiFetch(`/api/ui-state/${encodeURIComponent(surface)}`);
    if (!res.ok) return local;
    const body = (await res.json()) as { state?: unknown };
    return (body.state ?? body) as T;
  } catch {
    return local;
  }
}

export function saveWorkspaceState(surface: string, state: unknown): void {
  writeLocal(surface, state);
  void apiFetch(`/api/ui-state/${encodeURIComponent(surface)}`, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ state }),
  }).catch(() => undefined);
}
