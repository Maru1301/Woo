# ADR 0002: Render commit graph rows with SVG

Status: accepted for M7.

Woo already virtualizes history rows. The Rust graph engine sends semantic lane geometry for each loaded commit; the frontend renders only the visible rows and overscan. A small SVG within each row shares the row's vertical position and click target with its commit metadata. This keeps selection and scrolling in one surface and makes edge styling straightforward.

Canvas could reduce DOM elements for very dense visible graphs, but it would require an imperative redraw loop, viewport alignment, and separate hit testing. M7's pure Rust benchmark kept the slowest measured 100-row layout page near 1 ms; a normal virtualized viewport has fewer than 20 SVGs. There is no measured need for Canvas or WebGL. The renderer remains separate from `GraphRow`, so it can be replaced if real scrolling measurements show a bottleneck.

The graph gutter grows to 12 visible lanes, then clips distant geometry and shows the hidden-lane count. The complete topology stays in Rust data and the opaque pagination cursor. A future graph-width interaction may expose those clipped lanes if real repositories require it.
