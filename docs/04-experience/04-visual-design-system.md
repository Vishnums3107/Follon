# Follon Institutional Visual Design System

## 1. Design Philosophy

Follon's presentation layer operates under the core tenet: **Deterministic Evidence Over Visual Speculation**.
The terminal's interface is an institutional workstation designed for quantitative traders, risk officers, and compliance auditors.

- **Read-Only Boundary Invariant**: The interface displays declarative state, verifiable audit trails, and deterministic projections. It never holds trading secrets, executes order actions directly, or allows unauthenticated mutations.
- **Cognitive Clarity & High Data Density**: Maximizes data density without clutter, ensuring risk limits, P&L, position attribution, and system health are immediately scannable.
- **Fail-Closed Presentation**: Missing or malformed data is never silently coerced. State uncertainty remains explicitly flagged as `UNKNOWN` or `DEGRADED`.

---

## 2. Color Palette & Surface Tokens

### Surfaces & Backgrounds
| Token | Value | Role |
| :--- | :--- | :--- |
| `--color-bg-base` | `#0D0E12` | Foundation viewport background |
| `--color-surface-1` | `#16181D` | Primary card and workspace container surface |
| `--color-surface-2` | `#22252B` | Elevated card surfaces and table header backgrounds |
| `--color-surface-3` | `#2A2D35` | Interactive control surfaces and hovered card states |
| `--color-obsidian` | `#08090C` | Deep recessed containers and terminal view backgrounds |
| `--color-glass-surface` | `rgba(19, 21, 26, 0.85)` | Modal backdrops and sticky floating drawers |
| `--color-glass-border` | `rgba(255, 255, 255, 0.08)` | Translucent edge boundary |

### Typography Colors
| Token | Value | Role |
| :--- | :--- | :--- |
| `--color-text-primary` | `#FFFFFF` | Primary headers, active metric values, symbol tickers |
| `--color-text-secondary`| `#A3A8B4` | Body text, labels, secondary metrics |
| `--color-text-muted` | `#9CA3B2` | Inactive table headers, metadata captions, timestamps |

### Institutional Accents & Signals
Contrast is verified against every surface a token is actually rendered as
text on, not `--color-bg-base` alone (2026-09-10 correction: the original
table below checked only `#0D0E12` and missed that `--color-signal-buy`,
`--color-signal-sell`, `--color-accent`, and `--color-ruby` fell to 3.7-4.4:1
— below WCAG AA — as text on `--color-surface-1`/`-2` card backgrounds; each
was relightened until it cleared 4.5:1 everywhere).

| Token | Value | Contrast vs bg-base / surface-1 / surface-2 | Role |
| :--- | :--- | :--- | :--- |
| `--color-accent` | `#478AF7` | 5.7:1 / 5.3:1 / 4.6:1 (WCAG AA) | Primary active route, interactive links, selection rings |
| `--color-accent-hover` | `#60A5FA` | 7.6:1 / 7.0:1 / 6.0:1 (WCAG AAA) | Interactive hover highlight |
| `--color-signal-buy` | `#469E56` | 5.8:1 / 5.3:1 / 4.6:1 (WCAG AA) | Long positions, positive returns, buy signals |
| `--color-signal-sell` | `#EB5D47` | 5.7:1 / 5.2:1 / 4.5:1 (WCAG AA) | Short positions, negative returns, sell signals, hard limits |
| `--color-gold` | `#D4AF37` | 9.2:1 / 8.5:1 / 7.3:1 (WCAG AAA) | Institutional badge accents, verified release certificates |
| `--color-gold-bright` | `#F5E080` | 13.5:1+ (WCAG AAA) | High-emphasis verification badges and status highlights |
| `--color-cyan` | `#00D2FF` | 10.7:1 / 9.9:1 / 8.5:1 (WCAG AAA) | Market quote feeds, live streaming tick indicators |
| `--color-ruby` | `#FF4070` | 5.7:1 / 5.3:1 / 4.6:1 (WCAG AA) | Danger accents, monochrome-mode badge variant |
| `--color-border` | `#2C353F` | 1.6:1 / 1.4:1 / n/a | Structural separators and card outlines (decorative rest-state contrast; `.f-input`/`.f-select` switch to the WCAG-AA `--color-accent` border on focus, so the boundary is never identifiable by rest-state contrast alone) |
| `--color-border-hover` | `#4B5563` | 2.6:1 / 2.4:1 / n/a | Hovered container borders (decorative, same non-sole-cue note as `--color-border`) |

### Signal vs. Monochrome Display Rules
- **Default Mode**: Green (`--color-signal-buy`) and Red (`--color-signal-sell`) are strictly reserved for signed financial figures (returns, P&L, delta exposure) and order side badges (`BUY`/`SELL`).
- **Both modes** (2026-09-10: strengthened from monochrome-only to satisfy WCAG 1.4.1 — color is never the sole cue, even in the default view): `.f-text-buy`/`.f-text-sell` always render a directional glyph via CSS `::before`:
  - Long / Positive: `▲` with bold weight, immediately before the value (which itself carries its own `+`/`-` sign).
  - Short / Negative: `▼` with bold weight.
- **Monochrome Mode**: toggled by the header's `#signal-mode-toggle` button and persisted per-browser in `localStorage` (`follon:signal_mode`). Removes the green/red hue from `.f-text-buy`/`.f-text-sell` (`html[data-signal-mode="mono"]`); the glyph above already carries the direction, so no information is lost.

---

## 3. Typography Stack

- **UI Sans-Serif**: `'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif`
  - Used for titles, navigation, button labels, and system status text.
  - Weights: `400` (Regular), `500` (Medium), `600` (Semi-Bold), `700` (Bold).
- **Tabular Monospace**: `'JetBrains Mono', 'SF Mono', Consolas, Menlo, monospace`
  - Mandatory for all financial values, timestamps, event IDs, SHA-256 digests, and order quantities.
  - Enforced CSS property: `font-variant-numeric: tabular-nums;`.

---

## 4. Spacing, Radii & Grid System

### Spacing Scale
- `--space-1`: `4px` (Tight padding, badge insets)
- `--space-2`: `8px` (Button vertical padding, item gaps)
- `--space-3`: `12px` (Standard component gap, table cell padding)
- `--space-4`: `16px` (Card padding, standard layout gutter)
- `--space-5`: `24px` (Header padding, major section separation)
- `--space-6`: `32px` (Workspace page container margins)

### Border Radii
- `--radius-sm`: `6px` (Buttons, inputs, inline badges)
- `--radius-md`: `10px` (Cards, metric modules, data table wrappers)
- `--radius-lg`: `16px` (Top header bar, modals, floating drawers)

### Responsive Breakpoints
- **Desktop (Wide)**: `>= 1440px` (Full 4-column metric cards, split-pane blotters, persistent drawer).
- **Desktop (Standard)**: `1024px - 1439px` (3-column metric cards, collapsible drawer).
- **Tablet**: `768px - 1023px` (2-column metric cards, sticky horizontal navigation, table scroll).
- **Mobile**: `< 768px` (Single-column layout, table-to-card reflow using `data-label`).

---

## 5. Component Inventory

### 1. Metric Cards (`.metric-card`, `.f-card`)
- Clean bordered cards with `.metric-label` (uppercase muted caption), `.metric-value` (prominent primary text with tabular alignment), and `.metric-detail` (contextual explanation).

### 2. Data Tables (`.f-table-container`, `.f-table`)
- Sticky headers (`top: 0`, `z-index: 10`) with muted uppercase column titles.
- Alternating subtle row highlight on hover (`background: var(--color-surface-2)`).
- Truncated cells with tooltip copy support for SHA-256 hashes and event IDs.

### 3. Badges (`.f-badge`)
- Pill-shaped status indicators (`padding: 2px 8px; border-radius: 9999px;`).
- Variants: `.f-badge--buy`, `.f-badge--sell`, `.f-badge--accent`, `.f-badge--gold`.

### 4. Interactive Controls (`.f-btn`, `.f-input`, `.f-select`)
- Clear focus rings with `--color-accent` (2px solid outline with offset).
- Disabled states with `opacity: 0.5; cursor: not-allowed;`.

### 5. Evidence Inspector (`#evidence-inspector`)
- Slide-over drawer with raw JSON/NDJSON syntax highlighting, content-addressed hash verification, and download capability.

---

## 6. Accessibility & Motion Rules

- **WCAG 2.1 AA Compliance**: All text and foreground signal colors meet minimum 4.5:1 contrast against `--color-bg-base` and both card surfaces (`--color-surface-1`, `--color-surface-2`) — verified 2026-09-10 with a computed relative-luminance check against every token, which is what caught and fixed the four colors that had only been checked against `--color-bg-base`. Color is additionally never the sole cue for signed values: see Signal vs. Monochrome Display Rules above.
- **Focus Management**: All interactive items (`<a>`, `<button>`, `<input>`, `<select>`) possess an unambiguous focus outline for full keyboard accessibility (`Tab`, `Shift+Tab`, `Enter`, `Space`) — verified 2026-09-10 by tabbing through the full Command Center header and sidebar in a live browser; every stop showed a visible outline in a logical order.
- **Reduced Motion**: Respects `@media (prefers-reduced-motion: reduce)` by disabling micro-transitions and transforms.
