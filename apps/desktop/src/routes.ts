/**
 * Canonical typed route and workspace registry for the Follon trading terminal.
 *
 * Consolidates navigation definitions across the shell, runtime controller,
 * and test contracts into a single authoritative module.
 */

export type WorkspaceId =
  | "command-center"
  | "marketplace"
  | "research-lab"
  | "news-cockpit"
  | "strategy-studio"
  | "backtest-explorer"
  | "execution-blotter"
  | "risk-cockpit"
  | "portfolio"
  | "replay-incidents"
  | "journal"
  | "administration";

export type WorkspaceGroupLabel = "Monitor" | "Research" | "Operate" | "Govern";

export type WorkspaceDefinition = Readonly<{
  id: WorkspaceId;
  title: string;
  subtitle: string;
  group: WorkspaceGroupLabel;
  description: string;
  features: readonly string[];
}>;

export type WorkspaceLink = Readonly<{
  id: WorkspaceId;
  title: string;
  subtitle: string;
}>;

export type WorkspaceNavGroup = Readonly<{
  label: WorkspaceGroupLabel;
  workspaces: readonly WorkspaceLink[];
}>;

export const DEFAULT_WORKSPACE_ID: WorkspaceId = "command-center";

export const WORKSPACES: readonly WorkspaceDefinition[] = [
  {
    id: "command-center",
    title: "Command Center",
    subtitle: "System and gate health",
    group: "Monitor",
    description: "System health, acceptance gates, and evidence from every implemented Follon capability.",
    features: [
      "market-data", "replay", "research", "paper", "controlled-live",
      "operations", "options", "commercial", "execution-risk", "accounting",
      "identity", "platform",
    ],
  },
  {
    id: "research-lab",
    title: "Research Lab",
    subtitle: "Datasets and experiments",
    group: "Research",
    description: "Historical datasets, normalized market data, experiment records, reports, and deterministic options analysis.",
    features: ["market-data", "research", "options"],
  },
  {
    id: "strategy-studio",
    title: "Strategies",
    subtitle: "Versions and reproducibility",
    group: "Research",
    description: "Strategy bundle, worker runtime, configuration, replay, and reproducibility identities without browser-side code execution.",
    features: ["research", "replay"],
  },
  {
    id: "marketplace",
    title: "Marketplace",
    subtitle: "Discover local research assets",
    group: "Research",
    description: "Discover locally indexed strategies, datasets, and research evidence. Inspect provenance before using an asset; listings are not approval or performance endorsements.",
    features: ["market-data", "research", "replay"],
  },
  {
    id: "backtest-explorer",
    title: "Backtest",
    subtitle: "Runs and comparisons",
    group: "Research",
    description: "Completed backtest artifacts, event trails, reports, manifests, trades, and repeatability evidence.",
    features: ["research", "replay"],
  },
  {
    id: "news-cockpit",
    title: "News",
    subtitle: "Headlines and signal provenance",
    group: "Research",
    description: "Validated headline and sentiment evidence, their deterministic signal values, and linked risk decisions.",
    features: ["news", "research", "execution-risk"],
  },
  {
    id: "execution-blotter",
    title: "Execution Blotter",
    subtitle: "OMS lifecycle evidence",
    group: "Operate",
    description: "Intent, EMS plan, risk decision, order lifecycle, fill, rejection, replacement, and reconciliation evidence across simulation, PAPER, and controlled LIVE.",
    features: ["replay", "paper", "controlled-live", "execution-risk"],
  },
  {
    id: "risk-cockpit",
    title: "Risk Cockpit",
    subtitle: "Limits and alerts",
    group: "Operate",
    description: "Portfolio-wide exposure, limits, drawdown, margin, Greeks, alerts, kill-switch state, unknown orders, and reconciliation health.",
    features: ["paper", "controlled-live", "operations", "execution-risk"],
  },
  {
    id: "portfolio",
    title: "Portfolio",
    subtitle: "Positions and attribution",
    group: "Operate",
    description: "Positions, multi-currency accounting, margin, attribution, P&L, options scenarios, and cross-environment reconciliation.",
    features: ["replay", "paper", "operations", "options", "execution-risk", "accounting"],
  },
  {
    id: "replay-incidents",
    title: "Replay & Incidents",
    subtitle: "Causal reconstruction",
    group: "Operate",
    description: "Canonical causal events, recovery journals, reconciliation incidents, and deterministic reconstruction evidence.",
    features: ["replay", "paper", "controlled-live", "operations"],
  },
  {
    id: "journal",
    title: "Journal",
    subtitle: "Audit chains",
    group: "Operate",
    description: "Append-only PAPER, controlled-LIVE, accounting, operations, and commercial decisions with audit-chain evidence.",
    features: ["replay", "paper", "controlled-live", "operations", "commercial", "accounting", "platform"],
  },
  {
    id: "administration",
    title: "Administration",
    subtitle: "Commercial and deployment",
    group: "Govern",
    description: "Customer IAM, provisioning, entitlement, privacy, retention, release-signature, persistence, and self-host readiness evidence.",
    features: ["commercial", "identity", "platform"],
  },
];

export const WORKSPACE_GROUPS: readonly WorkspaceNavGroup[] = [
  {
    label: "Monitor",
    workspaces: WORKSPACES.filter((w) => w.group === "Monitor").map((w) => ({
      id: w.id,
      title: w.title,
      subtitle: w.subtitle,
    })),
  },
  {
    label: "Research",
    workspaces: WORKSPACES.filter((w) => w.group === "Research").map((w) => ({
      id: w.id,
      title: w.title,
      subtitle: w.subtitle,
    })),
  },
  {
    label: "Operate",
    workspaces: WORKSPACES.filter((w) => w.group === "Operate").map((w) => ({
      id: w.id,
      title: w.title,
      subtitle: w.subtitle,
    })),
  },
  {
    label: "Govern",
    workspaces: WORKSPACES.filter((w) => w.group === "Govern").map((w) => ({
      id: w.id,
      title: w.title,
      subtitle: w.subtitle,
    })),
  },
];

const WORKSPACE_MAP = new Map<WorkspaceId, WorkspaceDefinition>(
  WORKSPACES.map((w) => [w.id, w]),
);

export function isWorkspaceId(value: unknown): value is WorkspaceId {
  return typeof value === "string" && WORKSPACE_MAP.has(value as WorkspaceId);
}

export function getWorkspaceDefinition(id: string): WorkspaceDefinition {
  return WORKSPACE_MAP.get(id as WorkspaceId) ?? WORKSPACE_MAP.get(DEFAULT_WORKSPACE_ID)!;
}

export function workspaceHash(id: WorkspaceId): string {
  return `#workspace/${encodeURIComponent(id)}`;
}

export function workspacePath(id: WorkspaceId): string {
  return `/workspace/${encodeURIComponent(id)}`;
}

export function decodeWorkspaceRoute(hash: string, pathname: string): WorkspaceId {
  const hashPrefix = "#workspace/";
  if (hash.startsWith(hashPrefix)) {
    try {
      const decoded = decodeURIComponent(hash.slice(hashPrefix.length));
      if (isWorkspaceId(decoded)) return decoded;
    } catch {
      return DEFAULT_WORKSPACE_ID;
    }
  }
  const pathPrefix = "/workspace/";
  if (pathname.startsWith(pathPrefix)) {
    try {
      const decoded = decodeURIComponent(pathname.slice(pathPrefix.length));
      if (isWorkspaceId(decoded)) return decoded;
    } catch {
      return DEFAULT_WORKSPACE_ID;
    }
  }
  return DEFAULT_WORKSPACE_ID;
}
