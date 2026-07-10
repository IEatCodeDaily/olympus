// GraphPage — interactive force-directed link graph for a vault.
//
// Nodes = notes (sized by link count, colored by folder).
// Edges = wikilinks between notes.
// Canvas rendering via react-force-graph-2d (handles zoom/pan/drag).
// Click a node → navigates to that note.

import { useRef, useCallback, useMemo } from "react";
import { useNavigate } from "@tanstack/react-router";
import ForceGraph2D from "react-force-graph-2d";
import { useQuery } from "@tanstack/react-query";
import { Icon } from "../../../components/Icon";
import { apiFetch } from "../../../api";

interface GraphNode {
  id: string;
  title: string;
  path: string;
  cid?: string;
  linkCount: number;
  x?: number;
  y?: number;
}

interface GraphEdge {
  source: string;
  target: string;
}

interface VaultGraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export function GraphPage({ vaultId }: { vaultId: string }) {
  const navigate = useNavigate();
  const fgRef = useRef<any>(null);

  const { data, isLoading, error } = useQuery({
    queryKey: ["vaultGraph", vaultId],
    queryFn: async () => {
      const res = await apiFetch(
        `/api/vaults/${vaultId}/graph`,
      );
      if (!res.ok) throw new Error(`graph ${res.status}`);
      return res.json() as Promise<VaultGraphData>;
    },
    enabled: !!vaultId,
    staleTime: 10_000,
  });

  // Transform for react-force-graph: nodes need `id`, edges need source/target as node refs
  const graphData = useMemo(() => {
    if (!data) return { nodes: [], links: [] };
    return {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: data.edges.map((e) => ({ source: e.source, target: e.target })),
    };
  }, [data]);

  const handleNodeClick = useCallback(
    (node: any) => {
      if (node.path) {
        void navigate({
          to: "/vaults/$vaultId",
          params: { vaultId },
          search: { note: node.path },
        });
      }
    },
    [navigate, vaultId],
  );

  // Custom canvas node drawing (colored circles sized by link count)
  const nodePaint = useCallback((node: any, ctx: any, globalScale: number) => {
    const radius = 3 + Math.min(node.linkCount ?? 0, 8);
    const color =
      node.linkCount > 5
        ? "#c9c9c9" // silver for hubs
        : node.linkCount > 0
          ? "#8a8a8e"
          : "#4a4a4e";

    ctx.beginPath();
    ctx.arc(node.x, node.y, radius, 0, 2 * Math.PI);
    ctx.fillStyle = color;
    ctx.fill();

    // Label at high zoom
    if (globalScale > 2) {
      ctx.font = `${10 / globalScale}px IBM Plex Sans`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillStyle = "#c9c9c9";
      ctx.fillText(node.title ?? node.id, node.x, node.y + radius + 6);
    }
  }, []);

  if (!vaultId) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <div className="empty-state-icon"><Icon name="workflow" size={32} /></div>
          <div className="empty-state-title">No vault selected</div>
          <div className="empty-state-msg">Pick a vault to see its graph.</div>
        </div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <span className="gk">Loading graph…</span>
        </div>
      </div>
    );
  }

  if (error || !data || data.nodes.length === 0) {
    return (
      <div className="vault-content">
        <div className="empty-state">
          <div className="empty-state-icon"><Icon name="workflow" size={32} /></div>
          <div className="empty-state-title">No graph data</div>
          <div className="empty-state-msg">
            {data && data.nodes.length === 0
              ? "Create notes with [[wikilinks]] to build the graph."
              : "Could not load the vault graph."}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="vault-content" style={{ padding: 0, height: "100%", overflow: "hidden" }}>
      <div style={{ height: "100%", position: "relative" }}>
        <ForceGraph2D
          ref={fgRef}
          graphData={graphData}
          nodeRelSize={5}
          linkColor={() => "#333336"}
          linkWidth={0.5}
          linkDirectionalArrowLength={3}
          linkDirectionalArrowRelPos={1}
          onNodeClick={handleNodeClick}
          nodeCanvasObject={nodePaint}
          nodeCanvasObjectMode={() => "after"}
          cooldownTicks={100}
          width={800}
          height={600}
        />
        <div style={{
          position: "absolute",
          top: 8,
          left: 8,
          fontSize: 10,
          fontFamily: "var(--font-mono)",
          color: "var(--faint)",
          pointerEvents: "none",
        }}>
          {data.nodes.length} notes · {data.edges.length} links
        </div>
      </div>
    </div>
  );
}
