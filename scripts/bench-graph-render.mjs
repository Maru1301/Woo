import { createServer } from "vite";
import React from "react";
import { renderToString } from "react-dom/server";
import { performance } from "node:perf_hooks";

const vite = await createServer({ optimizeDeps: { noDiscovery: true, entries: [] }, server: { middlewareMode: true }, appType: "custom" });
try {
  const { default: GraphRowView, GRAPH_ROW_HEIGHT, graphWidth } = await vite.ssrLoadModule("/src/GraphRowView.tsx");
  const count = 10_000;
  const viewport = 400;
  const overscan = 5;
  const rows = Array.from({ length: count }, (_, index) => ({
    nodeLane: index % 8, laneCount: 8, incoming: index > 0,
    continuations: [0, 1, 2, 3, 4, 5, 6, 7].filter((lane) => lane !== index % 8),
    parentLanes: index === count - 1 ? [] : [index % 8],
  }));
  const samples = [];
  let maxSvg = 0;
  let maxPaths = 0;
  for (let step = 0; step < 100; step++) {
    const top = Math.floor((count * GRAPH_ROW_HEIGHT - viewport) * step / 99);
    const start = Math.max(0, Math.floor(top / GRAPH_ROW_HEIGHT) - overscan);
    const end = Math.min(count, start + Math.ceil(viewport / GRAPH_ROW_HEIGHT) + 10);
    const started = performance.now();
    const html = renderToString(React.createElement(React.Fragment, null,
      ...rows.slice(start, end).map((row) => React.createElement(GraphRowView, { row, width: graphWidth(8), selected: false }))));
    samples.push(performance.now() - started);
    maxSvg = Math.max(maxSvg, (html.match(/<svg /g) ?? []).length);
    maxPaths = Math.max(maxPaths, (html.match(/<path /g) ?? []).length);
  }
  samples.sort((a, b) => a - b);
  const octopus = renderToString(React.createElement(GraphRowView, {
    row: { nodeLane: 0, laneCount: 1000, incoming: false, continuations: [], parentLanes: Array.from({length:1000}, (_, index) => index) },
    width: graphWidth(1000), selected: false,
  }));
  const octopusPaths = (octopus.match(/<path /g) ?? []).length;
  console.log(`positions=100 loaded_rows=${count} max_rendered_svg=${maxSvg} max_rendered_paths=${maxPaths} octopus_1000_parent_paths=${octopusPaths} median_ssr_ms=${samples[50].toFixed(3)} worst_ssr_ms=${samples.at(-1).toFixed(3)}`);
} finally {
  await vite.close();
}
