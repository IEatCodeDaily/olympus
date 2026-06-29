// WorkflowsView — durable workflow graphs (roadmap U5). Backend /api/workflows
// lands with Epic H (Sayiir). Placeholder shows list + run-history layout.
import { PageHeader, EmptyState, PlaceholderBadge } from "../components/shell";

export default function WorkflowsView() {
  return (
    <div className="view-scroll">
      <PageHeader
        title="Workflows"
        subtitle="Durable, resumable agent workflows (e.g. the code-review loop)"
        actions={<PlaceholderBadge epic="Epic H (Sayiir)" />}
      />
      <EmptyState
        title="No workflows yet"
        message="Durable workflow graphs — define a multi-step agent process (coder → reviewer → validator → merge) that survives restarts. Lands with Epic H."
        cta={<button className="btn-primary" disabled>+ New workflow</button>}
      />
    </div>
  );
}
