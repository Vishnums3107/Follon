# UI overhaul task list

## Current phase

- [x] Audit source architecture, route ownership, data boundary, existing
  workspace catalogue, and current working-tree constraints.
- [x] Capture and inspect the desktop/tablet/mobile state in a browser
  (2026-09-10: automated overflow/keyboard/console measurement across all 12
  workspaces × 1440/1024/390px; see `02-ui-overhaul-audit.md`).
- [x] Complete the visual, interaction, and accessibility baseline audit
  (2026-09-10: horizontal-overflow, keyboard-navigation, and WCAG AA contrast
  checks; three real mobile CSS bugs found and fixed).
- [x] Confirm the signal-colour versus monochrome-display preference
  (2026-09-10: implemented exactly as specified in
  `04-visual-design-system.md` — always-on directional glyph plus a
  persisted monochrome toggle).

## Design and build sequence

- [x] Publish the visual design system: palette, typography, spacing,
  breakpoints, component inventory, motion rules, focus treatment, and signal
  rules (documented in `04-visual-design-system.md`).
- [x] Implement the shared UI primitives without changing the read-only API or
  workspace data contracts (implemented in `apps/desktop/styles.css`).
- [x] Rebuild and verify Command Center.
- [x] Rebuild and verify Portfolio and Execution Blotter.
- [x] Rebuild and verify the remaining workspaces.
- [x] Verify populated, empty, error, and long-data states at every breakpoint.
- [x] Run focused desktop contracts, type checking, accessibility/keyboard
  checks, console review, and final before/after evidence comparison.
- [x] Publish a short walkthrough of changes, preserved boundaries, and open
  decisions.
