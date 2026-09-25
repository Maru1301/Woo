import type { GraphRow } from "./lib/repository";

export const GRAPH_ROW_HEIGHT = 68;
const LANE_WIDTH = 18;
const MAX_VISIBLE_LANES = 12;
const COLORS = ["#8fbaff", "#f3bd78", "#a5d6a7", "#d8b0f2", "#8cdbd8", "#f2a5b6", "#d5cd87", "#a6bbdf"];

export function graphWidth(lanes: number): number {
  return Math.min(MAX_VISIBLE_LANES, Math.max(1, lanes)) * LANE_WIDTH + 12;
}

export default function GraphRowView({ row, width, selected }: { row: GraphRow; width: number; selected: boolean }) {
  const x = (lane: number) => 10 + lane * LANE_WIDTH;
  const visible = (lane: number) => x(lane) < width - 5;
  const color = (lane: number) => COLORS[lane % COLORS.length];
  const middle = GRAPH_ROW_HEIGHT / 2;
  const hidden = row.laneCount > MAX_VISIBLE_LANES;
  return <span className="graph-cell" style={{ width }} title={hidden ? `${row.laneCount} graph lanes; first ${MAX_VISIBLE_LANES} shown` : undefined}>
    <svg width={width} height={GRAPH_ROW_HEIGHT} viewBox={`0 0 ${width} ${GRAPH_ROW_HEIGHT}`} aria-hidden="true">
      {row.continuations.filter(visible).map((lane) => <path key={`c-${lane}`} d={`M ${x(lane)} 0 L ${x(lane)} ${GRAPH_ROW_HEIGHT}`} stroke={color(lane)} />)}
      {row.incoming && visible(row.nodeLane) && <path d={`M ${x(row.nodeLane)} 0 L ${x(row.nodeLane)} ${middle}`} stroke={color(row.nodeLane)} />}
      {row.parentLanes.filter(visible).map((lane, index) => <path key={`p-${index}`} d={lane === row.nodeLane
        ? `M ${x(row.nodeLane)} ${middle} L ${x(lane)} ${GRAPH_ROW_HEIGHT}`
        : `M ${x(row.nodeLane)} ${middle} C ${x(row.nodeLane)} ${middle + 19}, ${x(lane)} ${GRAPH_ROW_HEIGHT - 19}, ${x(lane)} ${GRAPH_ROW_HEIGHT}`}
        stroke={color(lane)} />)}
      {visible(row.nodeLane) && <circle cx={x(row.nodeLane)} cy={middle} r={selected ? 6 : 4.5} fill={color(row.nodeLane)} stroke="#142033" strokeWidth="1.5" />}
    </svg>
    {hidden && <span className="graph-overflow" aria-hidden="true">+{row.laneCount - MAX_VISIBLE_LANES}</span>}
  </span>;
}
