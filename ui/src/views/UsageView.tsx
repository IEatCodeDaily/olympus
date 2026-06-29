// UsageView — budget / token / subscription spend (roadmap U3). Backend /api/usage
// lands with Epic I. This view is built mock-first against the future contract.
import { useEffect, useMemo, useState } from "react";
import { Badge, EmptyState, PageHeader, PlaceholderBadge, StatPill } from "../components/shell";
import type { UsageRange, UsageResponse, UsageSummary } from "../types";

const BASE = (import.meta.env.VITE_API_BASE as string | undefined) ?? "http://127.0.0.1:8787";
const RANGES: UsageRange[] = ["24h", "7d", "30d"];

async function fetchUsage(range: UsageRange): Promise<UsageResponse> {
  const res = await fetch(`${BASE}/api/usage?range=${range}`);
  if (!res.ok) throw new Error(`usage ${res.status}`);
  return res.json() as Promise<UsageResponse>;
}

function formatTokens(value: number): string {
  return new Intl.NumberFormat("en", {
    notation: value >= 100_000 ? "compact" : "standard",
    maximumFractionDigits: value >= 100_000 ? 1 : 0,
  }).format(value);
}

function formatDollars(value: number): string {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: value < 100 ? 2 : 0,
    maximumFractionDigits: value < 100 ? 2 : 0,
  }).format(value);
}

function formatPercent(summary: UsageSummary): string {
  return `${Math.round((summary.used / summary.subscriptionLimit) * 100)}%`;
}

export default function UsageView() {
  const [range, setRange] = useState<UsageRange>("24h");
  const [data, setData] = useState<UsageResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    fetchUsage(range)
      .then((next) => {
        if (!cancelled) setData(next);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Unable to load usage");
          setData(null);
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [range]);

  const totals = useMemo(() => {
    const summaries = data?.summaries ?? [];
    return summaries.reduce(
      (acc, summary) => ({
        tokens: acc.tokens + summary.tokensIn + summary.tokensOut,
        estCost: acc.estCost + summary.estCost,
      }),
      { tokens: 0, estCost: 0 }
    );
  }, [data]);

  return (
    <div className="view-scroll">
      <PageHeader
        title="Usage"
        subtitle="Token spend, estimated cost, and subscription pressure by model and provider"
        actions={<PlaceholderBadge epic="Epic I" />}
      />

      <div className="usage-toolbar" role="toolbar" aria-label="Usage range selector">
        <div className="usage-range-group">
          {RANGES.map((option) => (
            <button
              key={option}
              className={`usage-range-btn ${range === option ? "active" : ""}`}
              onClick={() => setRange(option)}
              type="button"
            >
              {option}
            </button>
          ))}
        </div>
        <div className="usage-toolbar-note">mock contract until /api/usage lands</div>
      </div>

      <div className="board-stats">
        <StatPill label={`tokens (${range})`} value={loading ? "…" : formatTokens(totals.tokens)} />
        <StatPill label={`est. cost (${range})`} value={loading ? "…" : formatDollars(totals.estCost)} />
        <StatPill label="active models" value={String(data?.summaries.length ?? 0)} />
      </div>

      {loading ? (
        <div className="usage-grid usage-grid-loading" aria-label="Loading usage cards">
          {RANGES.map((slot) => (
            <div key={slot} className="usage-card usage-card-skeleton">
              <div className="usage-skel usage-skel-title" />
              <div className="usage-skel usage-skel-pill" />
              <div className="usage-skel usage-skel-bar" />
              <div className="usage-skel usage-skel-row" />
              <div className="usage-skel usage-skel-row short" />
            </div>
          ))}
        </div>
      ) : error ? (
        <EmptyState
          title="Usage mock failed to load"
          message={`Could not load the mock usage contract: ${error}.`}
        />
      ) : !data || data.summaries.length === 0 ? (
        <EmptyState
          title="No usage yet"
          message="Once requests start flowing, this view will show token spend, estimated cost, and subscription pressure for each model."
        />
      ) : (
        <div className="usage-grid">
          {data.summaries.map((summary) => {
            const percent = Math.min(100, Math.round((summary.used / summary.subscriptionLimit) * 100));
            const tokenTotal = summary.tokensIn + summary.tokensOut;
            const share = totals.tokens > 0 ? Math.round((tokenTotal / totals.tokens) * 100) : 0;

            return (
              <section key={`${summary.provider}-${summary.model}`} className="usage-card">
                <div className="usage-card-head">
                  <div>
                    <div className="usage-model-line">
                      <h2 className="usage-model">{summary.model}</h2>
                      <Badge>{summary.provider}</Badge>
                    </div>
                    <p className="usage-caption">{formatTokens(tokenTotal)} tokens over {data.range}</p>
                  </div>
                  <div className="usage-cost">{formatDollars(summary.estCost)}</div>
                </div>

                <div className="usage-progress-meta">
                  <span>subscription window</span>
                  <span>{formatPercent(summary)}</span>
                </div>
                <div className="usage-bar" aria-hidden="true">
                  <div className="usage-bar-fill" style={{ width: `${percent}%` }} />
                </div>
                <div className="usage-progress-foot">
                  <span>{formatTokens(summary.used)} used</span>
                  <span>{formatTokens(summary.subscriptionLimit)} limit</span>
                </div>

                <div className="usage-metrics">
                  <div className="usage-metric">
                    <span className="usage-metric-label">input</span>
                    <span className="usage-metric-value">{formatTokens(summary.tokensIn)}</span>
                  </div>
                  <div className="usage-metric">
                    <span className="usage-metric-label">output</span>
                    <span className="usage-metric-value">{formatTokens(summary.tokensOut)}</span>
                  </div>
                  <div className="usage-metric">
                    <span className="usage-metric-label">share of total</span>
                    <span className="usage-metric-value">{share}%</span>
                  </div>
                </div>
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}
