/**
 * Shared status utilities for kanban rendering.
 */

import type { CardStatus } from "../../../types";

/** Map CardStatus to gtag color class. */
export function statusBadgeClass(status: CardStatus): string {
  switch (status) {
    case "todo":
      return "";
    case "assigned":
    case "claimed":
      return "ok";
    case "blocked":
      return "warn";
    case "done":
      return "ok";
    default:
      return "";
  }
}
