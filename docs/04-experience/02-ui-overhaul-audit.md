# UI overhaul audit — preflight

**Status: baseline capture blocked; no visual implementation has started.**

## Scope and method

This is the required architecture and source audit for the desktop evidence
dashboard. A visual audit at desktop, tablet, and mobile widths must be added
before accepting any presentation changes. The in-app browser was unavailable
on 2026-09-03, so no screenshots or visual-quality claims are recorded here.

## What exists today

- `apps/desktop` is a React 19 + TypeScript application built with Vite and
  packaged by Tauri v2. `server.py` serves the browser bundle and a versioned,
  loopback-only read-only API.
- The app has ten evidence workspaces: Command Center, Research Lab, Strategy
  Studio, Backtest Explorer, Execution Blotter, Risk Cockpit, Portfolio, Replay
  & Incidents, Journal, and Administration. It also indexes and renders local,
  immutable evidence artifacts.
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
- **Responsive quality is unverified.** Source includes tablet and mobile
  breakpoints, including table-to-card conversion, but visual reflow,
  overflow, focus visibility, and console health have not yet been observed.

## Before-state evidence required

When browser capture is available, collect and inspect these before accepting
any visual change:

1. Command Center at 1440px, 1024px, and 390px.
2. Each remaining workspace at the same widths, including populated and empty
   evidence states where available.
3. Keyboard navigation through workspace selection, refresh actions, artifact
   filters, and artifact opening.
4. Error, no-evidence, overflow, and long-value states.

The screenshots must be saved beside the audit record or in a dedicated
evidence folder, and the final comparison must use the same viewport and state.

## Open product decision

Use restrained green/red only for profit/loss and buy/sell signals by default.
Offer a monochrome-display preference that preserves a non-colour indicator;
do not silently remove red/green from high-speed trading signals.
