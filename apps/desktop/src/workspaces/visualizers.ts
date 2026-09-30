// SVG visualizers: causal DAG, attention gauge, factor exposure and options payoff.

import { AttentionBudget, DecisionReconstruction, ExposureGraph, OptionAnalyticsEvidence, OptionsDashboard } from "../evidence/index.js";

const SVG_NS = "http://www.w3.org/2000/svg";

function svgEl<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Readonly<Record<string, string>>): SVGElementTagNameMap[K] {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(attrs)) el.setAttribute(key, value);
  return el;
}

function truncateText(value: string, maxLength: number): string {
  return value.length > maxLength ? `${value.slice(0, maxLength - 1)}…` : value;
}

/** Renders only a real, retained decision-reconstruction causal chain; absent evidence yields no visual. */
export function renderCausalDagVisualizer(recon: DecisionReconstruction | undefined): HTMLElement | undefined {
  if (recon === undefined || recon.causal_chain.length === 0) return undefined;
  const boxWidth = 130;
  const boxHeight = 70;
  const stepX = 170;
  const startX = 35;
  const boxY = 55;
  const nodes = recon.causal_chain;
  const svgWidth = Math.max(400, startX * 2 + (nodes.length - 1) * stepX + boxWidth);
  const accentColors = ["#00D2FF", "#00E676", "#D4AF37"];
  const verified = recon.integrity_status === "VERIFIED";

  const wrap = document.createElement("div");
  wrap.className = "causal-dag-container";

  const title = document.createElement("div");
  title.className = "causal-dag-title";
  const titleLeft = document.createElement("span");
  titleLeft.className = "dag-header-left";
  const titleIcon = document.createElement("span");
  titleIcon.innerHTML = `<span class="luxury-pulse-dot luxury-pulse-dot--cyan" aria-hidden="true"><span class="pulse-ring"></span><span class="pulse-core"></span></span>`;
  const titleText = document.createElement("span");
  titleText.textContent = "Interactive Causal Lineage DAG (Temporal Provenance)";
  titleLeft.append(titleIcon, titleText);
  const titleBadge = document.createElement("span");
  titleBadge.className = `dag-header-badge dag-header-badge--${verified ? "verified" : "warn"}`;
  titleBadge.textContent = recon.integrity_status.replace(/_/g, " ");
  title.append(titleLeft, titleBadge);
  wrap.append(title);

  const svg = svgEl("svg", { class: "dag-svg", viewBox: `0 0 ${svgWidth} 180` });
  const defs = svgEl("defs", {});
  const gradient = svgEl("linearGradient", { id: "dagGlow", x1: "0%", y1: "0%", x2: "100%", y2: "0%" });
  for (const [offset, color, opacity] of [["0%", "#00D2FF", "0.3"], ["50%", "#00E676", "0.8"], ["100%", "#D4AF37", "0.9"]] as const) {
    gradient.append(svgEl("stop", { offset, "stop-color": color, "stop-opacity": opacity }));
  }
  defs.append(gradient);
  svg.append(defs);

  const edgeSet = new Set(recon.edges.map((edge) => `${edge.from_node_id}->${edge.to_node_id}`));
  for (let i = 0; i < nodes.length - 1; i++) {
    if (!edgeSet.has(`${nodes[i].node_id}->${nodes[i + 1].node_id}`)) continue;
    const x1 = startX + i * stepX + boxWidth;
    const x2 = startX + (i + 1) * stepX;
    svg.append(svgEl("path", {
      d: `M ${x1} 90 C ${x1 + 35} 90, ${x2 - 35} 90, ${x2} 90`,
      stroke: "url(#dagGlow)", "stroke-width": "2.5", fill: "none", "stroke-dasharray": "4 2", class: "dag-link-pulse",
    }));
  }

  nodes.forEach((node, index) => {
    const x = startX + index * stepX;
    const accent = accentColors[index % accentColors.length];
    const g = svgEl("g", { transform: `translate(${x}, ${boxY})` });
    g.append(svgEl("rect", { width: String(boxWidth), height: String(boxHeight), rx: "8", fill: "#13161C", stroke: accent, "stroke-width": "1.2" }));
    g.append(svgEl("circle", { cx: "16", cy: "20", r: "4", fill: accent }));
    const typeText = svgEl("text", { x: "26", y: "24", fill: "#A3A8B4", "font-size": "10", "font-family": "'JetBrains Mono', monospace" });
    typeText.textContent = truncateText(node.event_type, 16);
    g.append(typeText);
    const summaryText = svgEl("text", { x: "12", y: "44", fill: "#FFFFFF", "font-size": "11", "font-weight": "600", "font-family": "'Inter', sans-serif" });
    summaryText.textContent = truncateText(node.summary, 18);
    g.append(summaryText);
    const actorText = svgEl("text", { x: "12", y: "60", fill: accent, "font-size": "9", "font-family": "'JetBrains Mono', monospace" });
    actorText.textContent = truncateText(node.actor, 20);
    g.append(actorText);
    const tooltip = svgEl("title", {});
    tooltip.textContent = `${node.node_id} · ${node.event_time}`;
    g.append(tooltip);
    svg.append(g);
  });

  wrap.append(svg);
  return wrap;
}

/** Renders only a real, retained attention-budget record; absent evidence yields no visual. */
export function renderAttentionGauge(budget: AttentionBudget | undefined): HTMLElement | undefined {
  if (budget === undefined) return undefined;
  const loadPct = Math.min(100, Math.max(0, budget.cognitive_load_score_bps / 100));
  const reservePct = (100 - loadPct).toFixed(1);
  const dashoffset = (235.6 * (1 - loadPct / 100)).toFixed(1);

  const wrap = document.createElement("div");
  wrap.className = "attention-gauge-container";
  const svgWrap = document.createElement("div");
  svgWrap.className = "gauge-svg-wrap";
  svgWrap.innerHTML = `
    <svg class="gauge-svg" viewBox="0 0 200 130">
      <defs>
        <linearGradient id="gaugeGradient" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stop-color="#00E676" />
          <stop offset="60%" stop-color="#00D2FF" />
          <stop offset="100%" stop-color="#FF3366" />
        </linearGradient>
      </defs>
      <path d="M 25 115 A 75 75 0 0 1 175 115" fill="none" stroke="#22252B" stroke-width="14" stroke-linecap="round" />
      <path d="M 25 115 A 75 75 0 0 1 175 115" fill="none" stroke="url(#gaugeGradient)" stroke-width="14" stroke-linecap="round"
            stroke-dasharray="235.6" stroke-dashoffset="${dashoffset}" />
      <text x="100" y="85" class="gauge-center-text" fill="#FFFFFF" font-family="'JetBrains Mono', monospace" font-size="22" font-weight="700">${loadPct.toFixed(1)}%</text>
      <text x="100" y="105" class="gauge-center-text" fill="#A3A8B4" font-family="'Inter', sans-serif" font-size="9" font-weight="600" letter-spacing="0.08em">LOAD (${budget.budget_exhausted ? "EXHAUSTED" : "NOMINAL"})</text>
    </svg>
  `;
  wrap.append(svgWrap);

  const cluster = document.createElement("div");
  cluster.className = "gauge-metric-cluster";

  const reserveCard = document.createElement("div");
  reserveCard.className = "metric-card gauge-metric-card";
  reserveCard.innerHTML = `<span class="gauge-metric-label"><span class="luxury-pulse-dot luxury-pulse-dot--emerald"><span class="pulse-ring"></span><span class="pulse-core"></span></span>RESERVE CAPACITY</span>`;
  const reserveValue = document.createElement("span");
  reserveValue.className = "f-text-mono gauge-metric-value gauge-metric-value--emerald";
  reserveValue.textContent = `${reservePct}%`;
  const reserveCaption = document.createElement("small");
  reserveCaption.className = "gauge-metric-caption";
  reserveCaption.textContent = "Attentive margin";
  reserveCard.append(reserveValue, reserveCaption);

  const interruptCard = document.createElement("div");
  interruptCard.className = "metric-card gauge-metric-card";
  interruptCard.innerHTML = `<span class="gauge-metric-label"><span class="luxury-pulse-dot luxury-pulse-dot--cyan"><span class="pulse-ring"></span><span class="pulse-core"></span></span>INTERRUPTIONS</span>`;
  const interruptValue = document.createElement("span");
  interruptValue.className = "f-text-mono gauge-metric-value gauge-metric-value--cyan";
  interruptValue.textContent = `${budget.interruptions_per_hour} / hr`;
  const interruptCaption = document.createElement("small");
  interruptCaption.className = "gauge-metric-caption";
  interruptCaption.textContent = `${budget.active_alarms_count} active / ${budget.suppressed_duplicates_count} suppressed`;
  interruptCard.append(interruptValue, interruptCaption);

  cluster.append(reserveCard, interruptCard);
  wrap.append(cluster);
  return wrap;
}

/** Renders only a real, retained exposure graph; absent evidence yields no visual. */
export function renderFactorExposureBars(graph: ExposureGraph | undefined): HTMLElement | undefined {
  if (graph === undefined || graph.factors.length === 0) return undefined;
  const maxAbsBps = Math.max(...graph.factors.map((factor) => Math.abs(factor.loading_bps)), 1);

  const wrap = document.createElement("div");
  wrap.className = "factor-exposure-container";

  const header = document.createElement("div");
  header.className = "factor-header-row";
  const headerLabel = document.createElement("span");
  headerLabel.className = "factor-header-label";
  const headerIcon = document.createElement("span");
  headerIcon.innerHTML = `<span class="luxury-pulse-dot luxury-pulse-dot--emerald"><span class="pulse-ring"></span><span class="pulse-core"></span></span>`;
  const headerText = document.createElement("span");
  headerText.textContent = "Factor Decomposition & Variance Contribution";
  headerLabel.append(headerIcon, headerText);
  const headerBadge = document.createElement("span");
  headerBadge.className = "factor-header-badge";
  headerBadge.textContent = graph.unreconciled_discrepancy ? "UNRECONCILED DISCREPANCY" : "RECONCILED";
  header.append(headerLabel, headerBadge);
  wrap.append(header);

  for (const factor of graph.factors) {
    const positive = factor.loading_bps >= 0;
    const widthBucket = Math.round((Math.abs(factor.loading_bps) / maxAbsBps) * 50);

    const row = document.createElement("div");
    row.className = "factor-bar-row";
    const name = document.createElement("span");
    name.className = "factor-name";
    name.textContent = factor.factor_name;
    row.append(name);

    const track = document.createElement("div");
    track.className = "factor-track-wrap";
    const zero = document.createElement("div");
    zero.className = "factor-zero-line";
    track.append(zero);
    const fill = document.createElement("div");
    fill.className = `factor-bar-fill factor-bar-fill--${positive ? "positive" : "negative"} f-bar-w-${widthBucket}`;
    track.append(fill);
    row.append(track);

    const value = document.createElement("span");
    value.className = `factor-val ${positive ? "f-text-buy" : "f-text-sell"}`;
    value.textContent = `${positive ? "+" : ""}${factor.loading_bps} bps (${factor.factor_variance_pct})`;
    row.append(value);

    wrap.append(row);
  }
  return wrap;
}

type PayoffLeg = Readonly<{ right: "CALL" | "PUT"; strike: number; premium: number }>;

/** Renders a payoff/convexity sketch computed only from real, retained frozen option-chain analytics. */
export function renderOptionsPayoffVisualizer(options: OptionsDashboard | undefined): HTMLElement | undefined {
  if (options === undefined || options.analytics.length === 0) return undefined;
  const mark = Number(options.chain.underlying_mark);
  if (!Number.isFinite(mark)) return undefined;

  const toLeg = (item: OptionAnalyticsEvidence): PayoffLeg | undefined => {
    const strike = Number(item.strike);
    const premium = Number(item.market_premium);
    if (!Number.isFinite(strike) || !Number.isFinite(premium)) return undefined;
    return { right: item.right, strike, premium };
  };
  const nearestToMark = (candidates: readonly PayoffLeg[]): PayoffLeg | undefined =>
    candidates.reduce<PayoffLeg | undefined>((best, leg) => (best === undefined || Math.abs(leg.strike - mark) < Math.abs(best.strike - mark) ? leg : best), undefined);

  const legs = (["CALL", "PUT"] as const)
    .map((right) => nearestToMark(options.analytics.filter((item) => item.right === right).map(toLeg).filter((leg): leg is PayoffLeg => leg !== undefined)))
    .filter((leg): leg is PayoffLeg => leg !== undefined);
  if (legs.length === 0) return undefined;

  const totalPremium = legs.reduce((sum, leg) => sum + leg.premium, 0);
  const payoffAt = (spot: number): number =>
    legs.reduce((sum, leg) => sum + (leg.right === "CALL" ? Math.max(0, spot - leg.strike) : Math.max(0, leg.strike - spot)), 0) - totalPremium;

  const strikes = legs.map((leg) => leg.strike);
  const low = Math.min(mark, ...strikes) * 0.7;
  const high = Math.max(mark, ...strikes) * 1.3;
  const sampleCount = 40;
  const samples = Array.from({ length: sampleCount + 1 }, (_, i) => {
    const spot = low + ((high - low) * i) / sampleCount;
    return { spot, value: payoffAt(spot) };
  });
  const values = samples.map((sample) => sample.value);
  const minVal = Math.min(...values, 0);
  const maxVal = Math.max(...values, 0);
  const valueRange = maxVal - minVal || 1;

  const chartLeft = 40, chartRight = 560, chartTop = 15, chartBottom = 95;
  const toX = (spot: number) => chartLeft + ((spot - low) / (high - low)) * (chartRight - chartLeft);
  const toY = (value: number) => chartBottom - ((value - minVal) / valueRange) * (chartBottom - chartTop);
  const zeroY = toY(0);
  const pathD = samples.map((sample, i) => `${i === 0 ? "M" : "L"} ${toX(sample.spot).toFixed(1)} ${toY(sample.value).toFixed(1)}`).join(" ");
  const fillD = `${pathD} L ${toX(high).toFixed(1)} ${chartBottom} L ${toX(low).toFixed(1)} ${chartBottom} Z`;
  const legLabel = legs.map((leg) => `${leg.right} K=${leg.strike.toFixed(2)}`).join(" + ");

  const wrap = document.createElement("div");
  wrap.className = "options-payoff-container f-card";

  const header = document.createElement("div");
  header.className = "options-payoff-header-row";
  const headerLabel = document.createElement("span");
  headerLabel.className = "options-payoff-header-label";
  const headerIcon = document.createElement("span");
  headerIcon.innerHTML = `<span class="luxury-pulse-dot luxury-pulse-dot--gold"><span class="pulse-ring"></span><span class="pulse-core"></span></span>`;
  const headerText = document.createElement("span");
  headerText.textContent = "Deterministic Options Payoff & Convexity Profile";
  headerLabel.append(headerIcon, headerText);
  const headerBadge = document.createElement("span");
  headerBadge.className = "options-payoff-header-badge";
  headerBadge.textContent = `${options.chain.underlying_instrument_id} @ ${options.chain.underlying_mark}`;
  header.append(headerLabel, headerBadge);
  wrap.append(header);

  const svg = svgEl("svg", { class: "options-payoff-svg", viewBox: "0 0 600 110" });
  const defs = svgEl("defs", {});
  const gradient = svgEl("linearGradient", { id: "payoffFill", x1: "0%", y1: "0%", x2: "0%", y2: "100%" });
  gradient.append(svgEl("stop", { offset: "0%", "stop-color": "#00E676", "stop-opacity": "0.25" }));
  gradient.append(svgEl("stop", { offset: "100%", "stop-color": "#00E676", "stop-opacity": "0" }));
  defs.append(gradient);
  svg.append(defs);

  svg.append(svgEl("line", { x1: "20", y1: zeroY.toFixed(1), x2: "580", y2: zeroY.toFixed(1), stroke: "rgba(255,255,255,0.15)", "stroke-dasharray": "3 3" }));
  const zeroLabel = svgEl("text", { x: "585", y: (zeroY + 3).toFixed(1), fill: "#A3A8B4", "font-size": "9", "font-family": "'JetBrains Mono', monospace" });
  zeroLabel.textContent = "0 PnL";
  svg.append(zeroLabel);

  legs.forEach((leg, index) => {
    const x = toX(leg.strike);
    svg.append(svgEl("line", { x1: x.toFixed(1), y1: "10", x2: x.toFixed(1), y2: "90", stroke: "rgba(212,175,55,0.4)", "stroke-dasharray": "2 2" }));
    const strikeLabel = svgEl("text", { x: (x + 4).toFixed(1), y: String(25 + index * 11), fill: "#D4AF37", "font-size": "9", "font-family": "'JetBrains Mono', monospace" });
    strikeLabel.textContent = `${leg.right} K = ${leg.strike.toFixed(2)}`;
    svg.append(strikeLabel);
    svg.append(svgEl("circle", { cx: x.toFixed(1), cy: toY(payoffAt(leg.strike)).toFixed(1), r: "4", fill: "#D4AF37" }));
  });

  svg.append(svgEl("path", { d: fillD, fill: "url(#payoffFill)" }));
  svg.append(svgEl("path", { d: pathD, fill: "none", stroke: "#00E676", "stroke-width": "2.5" }));
  wrap.append(svg);

  const caption = document.createElement("p");
  caption.className = "options-payoff-caption";
  caption.textContent = `${legLabel}; combined premium ${totalPremium.toFixed(2)} ${options.chain.currency}; expiry payoff swept ${low.toFixed(2)}–${high.toFixed(2)} ${options.chain.currency}.`;
  wrap.append(caption);

  return wrap;
}
