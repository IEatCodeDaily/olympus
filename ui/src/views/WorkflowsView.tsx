import { useEffect, useMemo, useState } from "react";
import { Badge, EmptyState, PageHeader, PlaceholderBadge, StatPill } from "../components/shell";
import type { Workflow, WorkflowRun, WorkflowRunStatus, WorkflowsResponse } from "../types";

const BASE = import.meta.env.VITE_API_BASE as string;
const TOKEN = import.meta.env.VITE_API_TOKEN as string;
const STATUS_FILTERS: Array<WorkflowRunStatus | "all"> = ["all", "running", "done", "failed"];

function authHeaders(): Record<string, string> {
  return { Authorization: `Bearer ${TOKEN}` };
}

async function fetchWorkflows(): Promise<WorkflowsResponse> {
  const response = await fetch(`${BASE}/api/workflows`, { headers: authHeaders() });
  if (!response.ok) throw new Error(`workflows ${response.status}`);
  return response.json() as Promise<WorkflowsResponse>;
}

export default function WorkflowsView() {
  const [data, setData] = useState<WorkflowsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedWorkflowId, setSelectedWorkflowId] = useState<string | null>(null);
  const [statusFilter, setStatusFilter] = useState<WorkflowRunStatus | "all">("all");

  useEffect(() => {
    let cancelled = false;

    setLoading(true);
    fetchWorkflows()
      .then((next) => {
        if (cancelled) return;
        setData(next);
        setSelectedWorkflowId((current) => current ?? next.workflows[0]?.id ?? null);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : "Failed to load workflows.");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const workflows = data?.workflows ?? [];
  const runs = data?.runs ?? [];

  useEffect(() => {
    if (!workflows.length) {
      setSelectedWorkflowId(null);
      return;
    }
    if (!selectedWorkflowId || !workflows.some((workflow) => workflow.id === selectedWorkflowId)) {
      setSelectedWorkflowId(workflows[0].id);
    }
  }, [selectedWorkflowId, workflows]);

  const selectedWorkflow = workflows.find((workflow) => workflow.id === selectedWorkflowId) ?? null;
  const workflowRuns = useMemo(() => {
    if (!selectedWorkflow) return [];
    return runs
      .filter((run) => run.workflowId === selectedWorkflow.id)
      .filter((run) => statusFilter === "all" || run.status === statusFilter)
      .sort((a, b) => b.startedAt - a.startedAt);
  }, [runs, selectedWorkflow, statusFilter]);

  const latestRun = workflowRuns[0] ?? null;

  return (
    <div className="view-scroll workflows-view">
      <PageHeader
        title="Workflows"
        subtitle="Durable workflow graphs with resumable runs, operator history, and step state."
        actions={<PlaceholderBadge epic="Epic H" />}
      />

      <div className="board-stats">
        <StatPill label="workflows" value={String(workflows.length || "—")} />
        <StatPill label="runs" value={String(runs.length || "—")} />
        <StatPill label="active" value={String(runs.filter((run) => run.status === "running").length || "—")} />
      </div>

      {loading ? (
        <WorkflowSkeleton />
      ) : error ? (
        <EmptyState title="Workflow view unavailable" message={error} />
      ) : !workflows.length ? (
        <EmptyState
          title="No workflows yet"
          message="Durable workflow graphs will land here once Epic H starts returning live workflow definitions and run history."
        />
      ) : (
        <div className="workflows-layout">
          <aside className="workflow-list-panel">
            <div className="workflow-panel-head">
              <div>
                <div className="workflow-panel-kicker">catalog</div>
                <h2 className="workflow-panel-title">Workflow definitions</h2>
              </div>
            </div>

            <menu className="workflow-list" aria-label="Workflow definitions">
              {workflows.map((workflow) => {
                const runCount = runs.filter((run) => run.workflowId === workflow.id).length;
                const activeCount = runs.filter(
                  (run) => run.workflowId === workflow.id && run.status === "running"
                ).length;

                return (
                  <button type="button"
                    key={workflow.id}
                    className={`workflow-list-item ${workflow.id === selectedWorkflow?.id ? "selected" : ""}`}
                    onClick={() => setSelectedWorkflowId(workflow.id)}
                  >
                    <div className="workflow-list-row">
                      <span className="workflow-list-name">{workflow.name}</span>
                      {activeCount > 0 ? <Badge kind="running">{activeCount} live</Badge> : <Badge>{runCount} runs</Badge>}
                    </div>
                    <p className="workflow-list-description">{workflow.description}</p>
                    <div className="workflow-list-meta">
                      <span>{workflow.stepCount} steps</span>
                      <span>{runCount} total runs</span>
                    </div>
                  </button>
                );
              })}
            </menu>
          </aside>

          <section className="workflow-detail-panel">
            {selectedWorkflow ? (
              <>
                <div className="workflow-panel-head workflow-detail-head">
                  <div>
                    <div className="workflow-panel-kicker">selected workflow</div>
                    <h2 className="workflow-panel-title">{selectedWorkflow.name}</h2>
                    <p className="workflow-panel-copy">{selectedWorkflow.description}</p>
                  </div>

                  <div className="workflow-filters" aria-label="Run status filters">
                    {STATUS_FILTERS.map((filter) => (
                      <button type="button"
                        key={filter}
                        className={`workflow-filter ${statusFilter === filter ? "active" : ""}`}
                        onClick={() => setStatusFilter(filter)}
                      >
                        {filter}
                      </button>
                    ))}
                  </div>
                </div>

                {latestRun ? (
                  <div className="workflow-latest-run">
                    <div className="workflow-latest-run-head">
                      <div>
                        <div className="workflow-panel-kicker">latest run</div>
                        <div className="workflow-run-id">{latestRun.id}</div>
                      </div>
                      <div className="workflow-run-summary">
                        <WorkflowStatusBadge status={latestRun.status} />
                        <span>{formatTime(latestRun.startedAt)}</span>
                      </div>
                    </div>
                    <StepLane run={latestRun} />
                  </div>
                ) : (
                  <EmptyState
                    title="No runs match this filter"
                    message="Try another status filter to inspect earlier workflow executions."
                  />
                )}

                <div className="workflow-history">
                  <div className="workflow-panel-kicker">run history</div>
                  <div className="workflow-history-list">
                    {workflowRuns.map((run) => (
                      <article key={run.id} className="workflow-run-card">
                        <div className="workflow-run-card-head">
                          <div>
                            <div className="workflow-run-id">{run.id}</div>
                            <div className="workflow-run-time">Started {formatTime(run.startedAt)}</div>
                          </div>
                          <WorkflowStatusBadge status={run.status} />
                        </div>

                        <div className="workflow-run-steps">
                          {run.steps.map((step) => (
                            <div key={step.id} className={`workflow-step-chip workflow-step-${step.status}`}>
                              <span className="workflow-step-chip-dot" />
                              <span>{step.label}</span>
                            </div>
                          ))}
                        </div>
                      </article>
                    ))}
                  </div>
                </div>
              </>
            ) : null}
          </section>
        </div>
      )}
    </div>
  );
}

function WorkflowStatusBadge({ status }: { status: WorkflowRunStatus }) {
  const kind = status === "failed" ? "failed" : status === "done" ? "done" : "running";
  return <Badge kind={kind}>{status}</Badge>;
}

function StepLane({ run }: { run: WorkflowRun }) {
  return (
    <div className="workflow-step-lane" aria-label={`Step flow for ${run.id}`}>
      {run.steps.map((step, index) => (
        <div key={step.id} className="workflow-step-node-wrap">
          <div className={`workflow-step-node workflow-step-${step.status}`}>
            <div className="workflow-step-node-index">{index + 1}</div>
            <div className="workflow-step-node-label">{step.label}</div>
            <div className="workflow-step-node-status">{step.status}</div>
          </div>
          {index < run.steps.length - 1 ? <div className="workflow-step-connector" aria-hidden="true" /> : null}
        </div>
      ))}
    </div>
  );
}

function WorkflowSkeleton() {
  return (
    <div className="workflows-layout workflow-skeleton" aria-label="Loading workflows">
      <div className="workflow-list-panel">
        {Array.from({ length: 3 }).map((_, index) => (
          <div key={index} className="workflow-skeleton-card">
            <div className="skel-line" style={{ width: `${54 + index * 10}%` }} />
            <div className="skel-line skel-line-sm" style={{ width: `${72 - index * 8}%` }} />
            <div className="skel-line skel-line-sm" style={{ width: `${48 + index * 6}%` }} />
          </div>
        ))}
      </div>
      <div className="workflow-detail-panel">
        <div className="workflow-skeleton-card workflow-skeleton-card-lg">
          <div className="skel-line" style={{ width: "26%" }} />
          <div className="skel-line" style={{ width: "58%" }} />
          <div className="workflow-skeleton-steps">
            {Array.from({ length: 4 }).map((_, index) => (
              <div key={index} className="workflow-skeleton-step" />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

function formatTime(epochSeconds: number): string {
  const deltaMinutes = Math.max(1, Math.round((Date.now() / 1000 - epochSeconds) / 60));
  if (deltaMinutes < 60) return `${deltaMinutes}m ago`;
  const deltaHours = Math.round(deltaMinutes / 60);
  if (deltaHours < 24) return `${deltaHours}h ago`;
  const deltaDays = Math.round(deltaHours / 24);
  return `${deltaDays}d ago`;
}
