# Holly design reference: graphite and tungsten

Static HTML. Open `index.html` in any browser; no server, no network.

- `index.html?mode=connected|pending|busy` : the 1080x760 window in each state. Nav buttons at the top switch states.
- `tokens.html` : colour, type, radii, spacing, depth, colour rule.
- `theme.rs` : the same tokens as Rust constants (pure data, contrast tests). The HTML and `theme.rs` must agree; if they ever differ, `theme.rs` is the source of truth for values, the HTML for layout.
- `screens/*.png` : screenshots at 1x of each state, for quick comparison.
- `fonts/` : Hanken Grotesk 300/400/500/600 latin woff2 plus the SIL OFL license. These are the same files to bundle in the app.

## How to use it

- Layout, sizes, order and states: read the HTML source of `index.html` (inline styles carry the exact px values). It is a reference, not code to port: gpui has no CSS.
- Compare your gpui window to the PNG of the same state, side by side, at 1080x760.
- Things CSS does here that gpui may not: inset shadows, tabular figures, blur glow. The prompt says what to do when gpui lacks one.
- The window in the HTML is demo data (Mic 1 battery 100, level lit 11 and 7, NC on/medium, volume 4, stereo, EQ "Equalization", reverb on/large, schedule 15min, toggles on). Real values come from the ViewModel; many have no verified source yet (see the prompt).

Regenerate: `python3 -I tools/build_reference.py <canvas project dir> <out dir>` (kept outside the repo; the design canvas sources are the origin).
