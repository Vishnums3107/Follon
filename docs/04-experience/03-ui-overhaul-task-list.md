# UI overhaul task list

## Current phase

- [x] Audit source architecture, route ownership, data boundary, existing
  workspace catalogue, and current working-tree constraints.
- [ ] Capture and inspect the desktop/tablet/mobile before state in a browser.
- [ ] Complete the visual, interaction, and accessibility baseline audit.
- [ ] Confirm the signal-colour versus monochrome-display preference.

## Design and build sequence

- [ ] Publish the visual design system: palette, typography, spacing,
  breakpoints, component inventory, motion rules, focus treatment, and signal
  rules.
- [ ] Implement the shared UI primitives without changing the read-only API or
  workspace data contracts.
- [ ] Rebuild and verify Command Center.
- [ ] Rebuild and verify Portfolio and Execution Blotter.
- [ ] Rebuild and verify the remaining workspaces.
- [ ] Verify populated, empty, error, and long-data states at every breakpoint.
- [ ] Run focused desktop contracts, type checking, accessibility/keyboard
  checks, console review, and final before/after evidence comparison.
- [ ] Publish a short walkthrough of changes, preserved boundaries, and open
  decisions.
