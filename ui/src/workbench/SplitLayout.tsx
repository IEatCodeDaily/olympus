import { useRef, type KeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import type { GroupNode, LayoutNode, SplitNode } from "./model";

export function SplitLayout<TPayload>({
  root,
  surfaceLabel,
  renderGroup,
  onResize,
}: {
  root: LayoutNode<TPayload>;
  surfaceLabel: string;
  renderGroup: (group: GroupNode<TPayload>) => ReactNode;
  onResize: (splitId: string, ratio: number) => void;
}) {
  return (
    <div className="workbench-split-root">
      <SplitNodeView
        node={root}
        surfaceLabel={surfaceLabel}
        renderGroup={renderGroup}
        onResize={onResize}
      />
    </div>
  );
}

function SplitNodeView<TPayload>({
  node,
  surfaceLabel,
  renderGroup,
  onResize,
}: {
  node: LayoutNode<TPayload>;
  surfaceLabel: string;
  renderGroup: (group: GroupNode<TPayload>) => ReactNode;
  onResize: (splitId: string, ratio: number) => void;
}) {
  if (node.type === "group") return <>{renderGroup(node)}</>;
  return (
    <SplitBranch
      node={node}
      surfaceLabel={surfaceLabel}
      renderGroup={renderGroup}
      onResize={onResize}
    />
  );
}

function SplitBranch<TPayload>({
  node,
  surfaceLabel,
  renderGroup,
  onResize,
}: {
  node: SplitNode<TPayload>;
  surfaceLabel: string;
  renderGroup: (group: GroupNode<TPayload>) => ReactNode;
  onResize: (splitId: string, ratio: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const row = node.axis === "horizontal";
  const resizeLabel = `Resize ${surfaceLabel} ${row ? "left and right" : "top and bottom"}`;

  const beginResize = (event: ReactMouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    const move = (moveEvent: MouseEvent) => {
      const rect = ref.current?.getBoundingClientRect();
      if (!rect) return;
      const raw = row
        ? ((moveEvent.clientX - rect.left) / rect.width) * 100
        : ((moveEvent.clientY - rect.top) / rect.height) * 100;
      onResize(node.id, raw);
    };
    const end = () => {
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", end);
    };
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", end);
  };

  const resizeWithKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    const backward = row ? event.key === "ArrowLeft" : event.key === "ArrowUp";
    const forward = row ? event.key === "ArrowRight" : event.key === "ArrowDown";
    if (!backward && !forward) return;
    event.preventDefault();
    onResize(node.id, node.ratio + (forward ? 5 : -5));
  };

  return (
    <div
      ref={ref}
      className={`workbench-split workbench-split-${node.axis}`}
      data-split-id={node.id}
    >
      <div className="workbench-split-child" style={{ flexBasis: `${node.ratio}%` }}>
        <SplitNodeView node={node.first} surfaceLabel={surfaceLabel} renderGroup={renderGroup} onResize={onResize} />
      </div>
      <div
        className="workbench-separator"
        role="separator"
        aria-label={resizeLabel}
        aria-orientation={row ? "vertical" : "horizontal"}
        aria-valuemin={20}
        aria-valuemax={80}
        aria-valuenow={node.ratio}
        tabIndex={0}
        onMouseDown={beginResize}
        onKeyDown={resizeWithKeyboard}
      />
      <div className="workbench-split-child" style={{ flexBasis: `${100 - node.ratio}%` }}>
        <SplitNodeView node={node.second} surfaceLabel={surfaceLabel} renderGroup={renderGroup} onResize={onResize} />
      </div>
    </div>
  );
}
