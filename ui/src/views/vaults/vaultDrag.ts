export const VAULT_NOTE_DRAG_TYPE = "application/x-olympus-vault-note";
export const VAULT_TAB_DRAG_TYPE = "application/x-olympus-vault-tab";

export interface VaultNoteDragData {
  path: string;
  title: string;
}

export interface VaultTabDragData {
  paneId: string;
  tabId: string;
}

export function readDragData<T extends object>(dataTransfer: DataTransfer, type: string): T | null {
  const raw = dataTransfer.getData(type);
  if (!raw) return null;
  try {
    const value: unknown = JSON.parse(raw);
    return value && typeof value === "object" ? value as T : null;
  } catch {
    return null;
  }
}