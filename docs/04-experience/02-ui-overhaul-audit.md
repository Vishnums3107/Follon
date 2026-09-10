# UI overhaul audit — preflight & foundation

**Status: Baseline and visual design system complete (2026-09-07). All 12 workspaces populated and regression verified.
Responsive, accessibility, and signal-color remediation complete (2026-09-10) — see "Verification performed" below.**

## Scope and method

This is the required architecture and source audit for the desktop evidence
dashboard. The institutional visual design system is codified in
`04-visual-design-system.md`. The frontend features 12 distinct evidence
workspaces, all populated by verified immutable artifacts with zero synthetic
mocking in the production path.

## What exists today

- `apps/desktop` is a React 19 + TypeScript application built with Vite and
  packaged by Tauri v2. `server.py` serves the browser bundle and a versioned,
  loopback-only read-only API.
- The app has twelve (12) evidence workspaces: Command Center, Research Lab, Strategy
  Studio, Marketplace, News, Backtest Explorer, Execution Blotter, Risk Cockpit, Portfolio,
  Replay & Incidents, Journal, and Administration. It indexes and renders local,
  immutable evidence artifacts across all 12 Enduring Capabilities (DUR-01 through DUR-12).
- Workspace selection is client-side via `/#workspace/<workspace-id>` (with
  direct `/workspace/<workspace-id>` server support). The React shell mounts
  fixed DOM targets, then a fail-closed TypeScript controller renders typed
  workspace projections into those targets.
- The presentation layer reads only `/api/v1/status`, `/api/v1/features`,
  `/api/v1/evidence`, `/api/v1/evidence/<name>`, and `/api/v1/workspaces`.
  Parsed inputs are validated against the existing read-only contracts before
  rendering. The desktop contains no trading, broker, credential, approval, or
  order-control action.

## Reusable foundations

- The workspace registry, typed renderers, evidence parsing, feature catalog,
  and API boundary can remain intact during a UI-only overhaul.
- The current CSS already has token-like colour, spacing, typography, button,
  input, badge, card, and table primitives. These should be consolidated and
  evolved rather than bypassed with per-screen styling.
- Workspace renderers already use semantic tables, headings, labelled controls,
  and responsive `data-label` values for tables; these are a good basis for an
  accessible, data-dense design system.

## Structural constraints and risk areas

- **Read-only is a product invariant.** Redesign must not introduce a control
  that implies or performs order submission, risk approval, kill switching,
  configuration, credentials, or other trading mutation.
- **Stable DOM targets are contractual.** `main.ts`, the browser-module
  contract, and the evidence contract rely on the shell's existing target IDs.
  Any shell refactor must update these together and retain the same API
  behaviour.
- **Evidence remains authoritative.** UI code must not infer healthy state from
  a missing or invalid record, silently coerce values, use random data, or
  replace fixed-point evidence with lossy browser calculations.
- **The working tree is already materially modified.** Desktop shell, styles,
  workspace renderers, server, and tests contain changes that predate this
  audit. They are treated as in-progress user work and must be preserved during
  the redesign.
- **Responsive quality was unverified until 2026-09-10** — now measured and
  fixed; see below.

## Verification performed (2026-09-10)

Real, headless-browser measurement (not visual guessing) against the built
`web-dist` production bundle and the local generated evidence set, for all 12
workspaces at 1440px, 1024px, and 390px:

- **Horizontal overflow** (`document.documentElement.scrollWidth` vs
  `clientWidth`): initially failing on **all 12 workspaces at 390px** (desktop
  1440px and tablet 1024px were already clean). Root causes, all fixed:
  1. An unconditional `.nav-pages { display: flex; }` rule declared later in
     `styles.css` silently beat the `@media (max-width: 768px) { .header-nav
     { display: none; } }` mobile rule on equal CSS specificity, so the full
     9-tab pillar navigation never actually collapsed on narrow viewports.
     Fixed with a higher-specificity, order-independent selector
     (`.site-header .header-nav.nav-pages`).
  2. The mobile table-to-card reflow (`.f-table tbody td`) never reset the
     desktop `.f-table td { white-space: nowrap; }` rule, so any cell with a
     value longer than one line forced its card — and the whole page — wider
     than the viewport. Fixed with `white-space: normal; overflow-wrap:
     anywhere;` in the mobile rule.
  3. Every cell already carried a `data-label` attribute (its column header)
     but no CSS ever rendered it, so the mobile card view showed bare values
     with no visible label. Added `content: attr(data-label)` via `::before`,
     which also completes the "table-to-card reflow using `data-label`"
     behavior the visual design system already specified.
  - Re-measured after each fix: **0 of 36 (workspace × breakpoint) checks now
    overflow.**
- **Keyboard navigation**: tabbed through the skip link, brand link, all 9
  pillar tabs, the command palette trigger, the new signal-mode toggle, and
  the workspace sidebar. Every stop was reachable, in a logical order, with a
  visible 2-3px focus outline (`:focus-visible`); no keyboard trap found.
- **Console/network health**: zero console errors or failed requests across
  all measured pages once the `web-dist` bundle was rebuilt from source (see
  [master-plan conformance audit, item 23](../06-delivery/14-master-plan-conformance-audit.md)
  for the separate fabricated-evidence findings caught during this same pass).

Not yet done: a full manual visual walkthrough of long-value/overflow edge
cases beyond what the automated checks above cover, and design sign-off on
the resulting mobile card layout.

## Open product decision — resolved 2026-09-10

The visual design system (`04-visual-design-system.md`, §Signal vs. Monochrome
Display Rules) already specifies: green/red reserved for signed financial
figures and buy/sell badges by default, with a monochrome preference that
augments or replaces color with a directional glyph. This is now implemented:
`.f-text-buy`/`.f-text-sell` always render a `▲`/`▼` glyph (so meaning never
depends on color alone, in either mode — a stricter reading of WCAG 1.4.1 than
the doc's literal text, which only mandated the glyph for monochrome mode),
and a header toggle (`#signal-mode-toggle`) lets a viewer remove the green/red
hue entirely, persisted per-browser in `localStorage`. Also fixed during this
pass: `--color-signal-buy`, `--color-signal-sell`, `--color-accent`, and
`--color-ruby` only cleared WCAG AA 4.5:1 against `--color-bg-base`, not
against the `--color-surface-1`/`-2` card backgrounds they are actually
rendered on as text; all four were relightened to clear 4.5:1 against every
surface (see the corrected contrast table in `04-visual-design-system.md`).
