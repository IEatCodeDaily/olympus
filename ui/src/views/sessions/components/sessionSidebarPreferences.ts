import type { Session } from "../../../types";

export const SESSION_METADATA_FIELDS = ["agent", "model", "node", "source", "messages", "tokens"] as const;
export type SessionMetadataField = typeof SESSION_METADATA_FIELDS[number];

export const DEFAULT_SESSION_METADATA_FIELDS: ReadonlySet<SessionMetadataField> = new Set(["agent", "model"]);

export function toggleSessionMetadataField(
  fields: ReadonlySet<SessionMetadataField>,
  field: SessionMetadataField,
): Set<SessionMetadataField> {
  const next = new Set(fields);
  if (next.has(field)) next.delete(field);
  else next.add(field);
  return next;
}

export function sessionMetadata(
  session: Session,
  fields: ReadonlySet<SessionMetadataField>,
): string[] {
  const values: Partial<Record<SessionMetadataField, string | null>> = {
    agent: session.agent,
    model: session.model,
    node: session.node,
    source: session.source,
    messages: `${session.messageCount} msg`,
    tokens: `${formatCompact((session.inputTokens ?? 0) + (session.outputTokens ?? 0))} tok`,
  };
  return SESSION_METADATA_FIELDS.flatMap((field) => {
    const value = fields.has(field) ? values[field] : null;
    return value ? [value] : [];
  });
}

function formatCompact(value: number): string {
  if (value >= 1_000_000) return `${Math.round(value / 100_000) / 10}m`;
  if (value >= 1_000) return `${Math.round(value / 100) / 10}k`;
  return String(value);
}
