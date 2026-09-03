import {
  EvidenceEvent,
  parseEvidenceLog,
  parseLiveMonitoringDashboard,
  parseOptionsDashboard,
  parseOperationsDashboard,
  parsePaperDashboard,
  renderEvidence,
  renderLiveMonitoringDashboard,
  renderOptionsDashboard,
  renderOperationsDashboard,
  renderPaperDashboard,
} from "./evidence.js";
import {
  FeatureDefinition,
  SystemStatus,
  isFeatureDefinitions,
  isSystemStatus,
  renderCoverageSummary,
  renderFeatureCatalog,
  renderGenericArtifact,
  renderSystemStatus,
} from "./catalog.js";
import {
  EvidenceArtifact,
  WorkspaceSnapshot,
  parseWorkspaceSnapshot,
  renderWorkspace,
} from "./workspaces.js";

type WorkspaceDefinition = Readonly<{
  id: string;
  title: string;
  description: string;
  features: readonly string[];
}>;

const isTauriRuntime = window.location.protocol === "tauri:" ||
  window.location.hostname === "tauri.localhost";
const apiOrigin = isTauriRuntime ? "http://127.0.0.1:8080" : window.location.origin;

function apiUrl(path: string): string {
  if (!path.startsWith("/api/v1/")) {
    throw new Error("API paths must use the versioned evidence boundary.");
  }
  return new URL(path, apiOrigin).toString();
}

const WORKSPACES: readonly WorkspaceDefinition[] = [
  { id: "command-center", title: "Command Center", description: "System health, acceptance gates, and evidence from every implemented Follon capability.", features: ["market-data", "replay", "research", "paper", "controlled-live", "operations", "options", "commercial", "execution-risk", "accounting", "identity", "platform"] },
  { id: "research-lab", title: "Research Lab", description: "Historical datasets, normalized market data, experiment records, reports, and deterministic options analysis.", features: ["market-data", "research", "options"] },
  { id: "news-cockpit", title: "News Cockpit", description: "Validated headline and sentiment evidence, their deterministic signal values, and linked risk decisions.", features: ["news", "research", "execution-risk"] },
  { id: "strategy-studio", title: "Strategy Studio", description: "Strategy bundle, worker runtime, configuration, replay, and reproducibility identities without browser-side code execution.", features: ["research", "replay"] },
  { id: "backtest-explorer", title: "Backtest Explorer", description: "Completed backtest artifacts, event trails, reports, manifests, trades, and repeatability evidence.", features: ["research", "replay"] },
  { id: "execution-blotter", title: "Execution Blotter", description: "Intent, EMS plan, risk decision, order lifecycle, fill, rejection, replacement, and reconciliation evidence across simulation, PAPER, and controlled LIVE.", features: ["replay", "paper", "controlled-live", "execution-risk"] },
  { id: "risk-cockpit", title: "Risk Cockpit", description: "Portfolio-wide exposure, limits, drawdown, margin, Greeks, alerts, kill-switch state, unknown orders, and reconciliation health.", features: ["paper", "controlled-live", "operations", "execution-risk"] },
  { id: "portfolio", title: "Portfolio", description: "Positions, multi-currency accounting, margin, attribution, P&L, options scenarios, and cross-environment reconciliation.", features: ["replay", "paper", "operations", "options", "execution-risk", "accounting"] },
  { id: "replay-incidents", title: "Replay & Incidents", description: "Canonical causal events, recovery journals, reconciliation incidents, and deterministic reconstruction evidence.", features: ["replay", "paper", "controlled-live", "operations"] },
  { id: "journal", title: "Journal", description: "Append-only PAPER, controlled-LIVE, accounting, operations, and commercial decisions with audit-chain evidence.", features: ["replay", "paper", "controlled-live", "operations", "commercial", "accounting", "platform"] },
  { id: "administration", title: "Administration", description: "Customer IAM, provisioning, entitlement, privacy, retention, release-signature, persistence, and self-host readiness evidence.", features: ["commercial", "identity", "platform"] },
];

const root = document.querySelector<HTMLElement>("#evidence");
if (root === null) {
  throw new Error("evidence root is missing");
}
const fileInput = document.querySelector<HTMLInputElement>("#event-log-file");
const status = document.querySelector<HTMLElement>("#load-status");
const serverEvidence = document.querySelector<HTMLSelectElement>("#server-evidence");
const refreshEvidence = document.querySelector<HTMLButtonElement>("#refresh-evidence");
const featureFilter = document.querySelector<HTMLSelectElement>("#feature-filter");
const featureCatalog = document.querySelector<HTMLElement>("#feature-catalog");
const systemOverview = document.querySelector<HTMLElement>("#system-overview");
const refreshSystem = document.querySelector<HTMLButtonElement>("#refresh-system");
const artifactMeta = document.querySelector<HTMLElement>("#artifact-meta");
const downloadArtifact = document.querySelector<HTMLAnchorElement>("#download-artifact");
const coverageSummary = document.querySelector<HTMLElement>("#coverage-summary");
const artifactSearch = document.querySelector<HTMLInputElement>("#artifact-search");
const workspaceButtons = Array.from(document.querySelectorAll<HTMLButtonElement>("[data-workspace]"));
const workspaceDetail = document.querySelector<HTMLElement>("#workspace-detail");
const workspaceDetailTitle = document.querySelector<HTMLElement>("#workspace-detail-title");
const workspaceDetailCopy = document.querySelector<HTMLElement>("#workspace-detail-copy");
const workspaceArtifactCount = document.querySelector<HTMLElement>("#workspace-artifact-count");
const workspaceFeatureBadges = document.querySelector<HTMLElement>("#workspace-feature-badges");
const workspaceCapabilities = document.querySelector<HTMLElement>("#workspace-capabilities");
const workspaceEvidence = document.querySelector<HTMLElement>("#workspace-evidence");
const workspaceSummary = document.querySelector<HTMLElement>("#workspace-summary");
const workspaceCanvas = document.querySelector<HTMLElement>("#workspace-canvas");
const refreshWorkspace = document.querySelector<HTMLButtonElement>("#refresh-workspace");
if (fileInput === null || status === null || serverEvidence === null || refreshEvidence === null ||
    featureFilter === null || featureCatalog === null || systemOverview === null || refreshSystem === null ||
    artifactMeta === null || downloadArtifact === null || coverageSummary === null || artifactSearch === null ||
    workspaceButtons.length === 0 || workspaceDetail === null || workspaceDetailTitle === null ||
    workspaceDetailCopy === null || workspaceArtifactCount === null || workspaceFeatureBadges === null ||
    workspaceCapabilities === null || workspaceEvidence === null || workspaceSummary === null || workspaceCanvas === null ||
    refreshWorkspace === null) {
  throw new Error("desktop evidence controls are missing");
}
const evidenceRoot = root;
const statusElement = status;
const serverEvidenceSelect = serverEvidence;
const refreshEvidenceButton = refreshEvidence;
const featureFilterSelect = featureFilter;
const featureCatalogRoot = featureCatalog;
const systemOverviewRoot = systemOverview;
const refreshSystemButton = refreshSystem;
const artifactMetaElement = artifactMeta;
const downloadArtifactLink = downloadArtifact;
const coverageSummaryRoot = coverageSummary;
const artifactSearchInput = artifactSearch;
const workspaceDetailRoot = workspaceDetail;
const workspaceDetailTitleElement = workspaceDetailTitle;
const workspaceDetailCopyElement = workspaceDetailCopy;
const workspaceArtifactCountElement = workspaceArtifactCount;
const workspaceFeatureBadgesRoot = workspaceFeatureBadges;
const workspaceCapabilitiesRoot = workspaceCapabilities;
const workspaceEvidenceRoot = workspaceEvidence;
const workspaceSummaryRoot = workspaceSummary;
const workspaceCanvasRoot = workspaceCanvas;
const refreshWorkspaceButton = refreshWorkspace;

let events: EvidenceEvent[] = [];
let evidenceFiles: EvidenceArtifact[] = [];
let featureDefinitions: FeatureDefinition[] = [];
let workspaceSnapshot: WorkspaceSnapshot | null = null;
let currentSystemStatus: SystemStatus | null = null;
let currentWorkspaceId = "command-center";
renderEvidence(evidenceRoot, events);

function setStatus(message: string, state: "idle" | "success" | "error" = "idle"): void {
  statusElement.textContent = message;
  statusElement.dataset.state = state;
}

function renderContents(contents: string, source: string): void {
  try {
    try {
      const dashboard = parseOptionsDashboard(contents);
      events = [];
      renderOptionsDashboard(evidenceRoot, dashboard);
      setStatus(`Loaded deterministic options evidence from ${source}.`, "success");
    } catch {
      try {
        const dashboard = parseOperationsDashboard(contents);
        events = [];
        renderOperationsDashboard(evidenceRoot, dashboard);
        setStatus(`Loaded operations workbench evidence from ${source}.`, "success");
      } catch {
        try {
          const dashboard = parseLiveMonitoringDashboard(contents);
          events = [];
          renderLiveMonitoringDashboard(evidenceRoot, dashboard);
          setStatus(`Loaded controlled-live monitoring state from ${source}.`, "success");
        } catch {
          try {
            const dashboard = parsePaperDashboard(contents);
            events = [];
            renderPaperDashboard(evidenceRoot, dashboard);
            setStatus(`Loaded PAPER operations state from ${source}.`, "success");
          } catch {
            try {
              events = parseEvidenceLog(contents);
              renderEvidence(evidenceRoot, events);
              setStatus(`Loaded ${events.length} immutable events from ${source}.`, "success");
            } catch {
              events = [];
              setStatus(renderGenericArtifact(evidenceRoot, contents, source), "success");
            }
          }
        }
      }
    }
  } catch (error) {
    setStatus(error instanceof Error ? error.message : "Unable to load event evidence.", "error");
    renderEvidence(evidenceRoot, []);
  }
}

async function loadServerEvidence(name: string): Promise<void> {
  if (name.length === 0) {
    return;
  }
  setStatus(`Loading ${name}…`);
  try {
    const response = await fetch(apiUrl(`/api/v1/evidence/${encodeURIComponent(name)}`), { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`Unable to load ${name} (HTTP ${response.status}).`);
    }
    const artifact = evidenceFiles.find((candidate) => candidate.name === name);
    if (artifact !== undefined) {
      artifactMetaElement.textContent = `${artifact.kind} · ${formatBytes(artifact.bytes)} · ${formatTimestamp(artifact.modified_at)}`;
    } else {
      artifactMetaElement.textContent = name;
    }
    downloadArtifactLink.href = apiUrl(`/api/v1/evidence/${encodeURIComponent(name)}?download=1`);
    downloadArtifactLink.hidden = false;
    if (Array.from(serverEvidenceSelect.options).some((option) => option.value === name)) {
      serverEvidenceSelect.value = name;
    }
    renderContents(await response.text(), name);
  } catch (error) {
    setStatus(error instanceof Error ? error.message : "Unable to load server evidence.", "error");
    renderEvidence(evidenceRoot, []);
  }
}

async function refreshServerEvidence(autoLoad: boolean): Promise<void> {
  serverEvidenceSelect.disabled = true;
  refreshEvidenceButton.disabled = true;
  setStatus("Checking the local evidence folder…");
  try {
    const response = await fetch(apiUrl("/api/v1/evidence"), { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`Unable to list local evidence (HTTP ${response.status}).`);
    }
    const files: unknown = await response.json();
    if (!Array.isArray(files) || !files.every(isEvidenceFile)) {
      throw new Error("The local evidence index has an invalid response.");
    }
    evidenceFiles = [...files];
    updateFeatureCatalog();
    await populateEvidenceSelect(autoLoad);
  } catch (error) {
    serverEvidenceSelect.replaceChildren(new Option("Evidence listing is unavailable", ""));
    setStatus(error instanceof Error ? error.message : "Unable to list local evidence.", "error");
  } finally {
    refreshEvidenceButton.disabled = false;
  }
}

async function populateEvidenceSelect(autoLoad: boolean): Promise<void> {
  const selectedFeature = featureFilterSelect.value;
  const query = artifactSearchInput.value.trim().toLocaleLowerCase();
  const featureFiles = selectedFeature === "all"
    ? evidenceFiles
    : evidenceFiles.filter((file) => file.feature === selectedFeature);
  const visibleFiles = query.length === 0
    ? featureFiles
    : featureFiles.filter((file) => `${file.name} ${file.kind} ${file.feature}`.toLocaleLowerCase().includes(query));
  serverEvidenceSelect.replaceChildren();
  if (visibleFiles.length === 0) {
    serverEvidenceSelect.append(new Option("No compatible evidence files found", ""));
    serverEvidenceSelect.disabled = true;
    setStatus(selectedFeature === "all"
      ? "No supported evidence is available yet. Run a Follon workflow, then select Refresh."
      : query.length > 0
        ? "No artifacts match this feature and search query."
        : "No local artifacts are available for this feature yet.");
    renderEvidence(evidenceRoot, []);
    artifactMetaElement.textContent = "No artifact selected";
    downloadArtifactLink.hidden = true;
    return;
  }
  for (const file of visibleFiles) {
    serverEvidenceSelect.append(new Option(`${file.kind} · ${file.name} · ${formatBytes(file.bytes)}`, file.name));
  }
  serverEvidenceSelect.disabled = false;
  if (autoLoad) {
    await loadServerEvidence(visibleFiles[0].name);
  } else {
    setStatus(`Found ${visibleFiles.length} local artifact${visibleFiles.length === 1 ? "" : "s"}.`, "success");
  }
}

function isEvidenceFile(value: unknown): value is EvidenceArtifact {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.name === "string" && typeof candidate.bytes === "number" &&
    typeof candidate.modified_at === "string" && typeof candidate.feature === "string" &&
    typeof candidate.kind === "string" && typeof candidate.format === "string";
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString();
}

async function loadFeatureDefinitions(): Promise<void> {
  try {
    const response = await fetch(apiUrl("/api/v1/features"), { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`Unable to load feature catalog (HTTP ${response.status}).`);
    }
    const value: unknown = await response.json();
    if (!isFeatureDefinitions(value)) {
      throw new Error("The feature catalog has an invalid response.");
    }
    featureDefinitions = value;
    featureFilterSelect.replaceChildren(new Option("All features", "all"));
    for (const feature of featureDefinitions) {
      featureFilterSelect.append(new Option(feature.title, feature.id));
    }
    updateFeatureCatalog();
  } catch (error) {
    featureCatalogRoot.replaceChildren();
    const message = document.createElement("p");
    message.className = "inline-error";
    message.textContent = error instanceof Error ? error.message : "Unable to load feature catalog.";
    featureCatalogRoot.append(message);
  }
}

function updateFeatureCatalog(): void {
  const counts = new Map<string, number>();
  for (const file of evidenceFiles) {
    counts.set(file.feature, (counts.get(file.feature) ?? 0) + 1);
  }
  renderCoverageSummary(coverageSummaryRoot, featureDefinitions, counts);
  renderFeatureCatalog(featureCatalogRoot, featureDefinitions, counts, (featureId) => {
    featureFilterSelect.value = featureId;
    void populateEvidenceSelect(true);
    document.querySelector("#artifacts")?.scrollIntoView({ behavior: "smooth", block: "start" });
  });
}

async function refreshSystemStatus(): Promise<void> {
  refreshSystemButton.disabled = true;
  try {
    const response = await fetch(apiUrl("/api/v1/status"), { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`Unable to load system health (HTTP ${response.status}).`);
    }
    const value: unknown = await response.json();
    if (!isSystemStatus(value)) {
      throw new Error("The system health response is invalid.");
    }
    currentSystemStatus = value;
    renderSystemStatus(systemOverviewRoot, value);
    renderCurrentWorkspace();
  } catch (error) {
    systemOverviewRoot.replaceChildren();
    const message = document.createElement("p");
    message.className = "inline-error";
    message.textContent = error instanceof Error ? error.message : "Unable to load system health.";
    systemOverviewRoot.append(message);
  } finally {
    refreshSystemButton.disabled = false;
  }
}

async function refreshWorkspaceSnapshot(): Promise<void> {
  const response = await fetch(apiUrl("/api/v1/workspaces"), { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`Unable to load integrated workspaces (HTTP ${response.status}).`);
  }
  const value: unknown = await response.json();
  workspaceSnapshot = parseWorkspaceSnapshot(value);
  renderCurrentWorkspace();
}

function renderCurrentWorkspace(): void {
  if (workspaceSnapshot === null) {
    return;
  }
  const workspace = WORKSPACES.find((candidate) => candidate.id === currentWorkspaceId) ?? WORKSPACES[0];
  if (workspace === undefined) {
    return;
  }
  renderWorkspace(workspaceSummaryRoot, workspaceCanvasRoot, workspace.id, workspaceSnapshot, {
    status: currentSystemStatus,
    features: featureDefinitions,
    artifacts: evidenceFiles,
    workspaceFeatures: workspace.features,
    onOpenArtifact: (name) => {
      void loadServerEvidence(name);
      document.querySelector("#artifacts")?.scrollIntoView({ behavior: "smooth", block: "start" });
    },
  });
}

function workspaceRoute(): string {
  const hashPrefix = "#workspace/";
  if (window.location.hash.startsWith(hashPrefix)) {
    return decodeWorkspaceId(window.location.hash.slice(hashPrefix.length));
  }
  const pathPrefix = "/workspace/";
  return window.location.pathname.startsWith(pathPrefix)
    ? decodeWorkspaceId(window.location.pathname.slice(pathPrefix.length))
    : "command-center";
}

function decodeWorkspaceId(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return "command-center";
  }
}

async function openWorkspace(
  workspaceId: string,
  options: Readonly<{ scroll: boolean; history: boolean }>,
): Promise<void> {
  const workspace = WORKSPACES.find((candidate) => candidate.id === workspaceId) ?? WORKSPACES[0];
  if (workspace === undefined) {
    return;
  }
  const definitions = featureDefinitions.filter((feature) => workspace.features.includes(feature.id));
  const artifacts = evidenceFiles.filter((artifact) => workspace.features.includes(artifact.feature));
  workspaceDetailTitleElement.textContent = workspace.title;
  workspaceDetailCopyElement.textContent = workspace.description;
  workspaceArtifactCountElement.textContent = `${artifacts.length} artifact${artifacts.length === 1 ? "" : "s"} available`;
  workspaceFeatureBadgesRoot.replaceChildren();
  for (const feature of definitions) {
    const badge = document.createElement("span");
    badge.className = `workspace-badge feature-${feature.state}`;
    badge.textContent = feature.title;
    workspaceFeatureBadgesRoot.append(badge);
  }

  workspaceCapabilitiesRoot.replaceChildren();
  const capabilities = [...new Set(definitions.flatMap((feature) => feature.capabilities))];
  for (const capability of capabilities) {
    const item = document.createElement("li");
    item.textContent = capability;
    workspaceCapabilitiesRoot.append(item);
  }

  workspaceEvidenceRoot.replaceChildren();
  if (artifacts.length === 0) {
    const empty = document.createElement("p");
    empty.className = "workspace-empty";
    empty.textContent = "No local evidence is available for this workspace yet.";
    workspaceEvidenceRoot.append(empty);
  } else {
    for (const artifact of artifacts.slice(0, 12)) {
      const button = document.createElement("button");
      button.className = "workspace-evidence-item";
      button.type = "button";
      const title = document.createElement("span");
      title.textContent = artifact.name;
      const detail = document.createElement("small");
      detail.textContent = `${artifact.kind} · ${formatBytes(artifact.bytes)}`;
      button.append(title, detail);
      button.addEventListener("click", () => {
        void loadServerEvidence(artifact.name);
        document.querySelector("#artifacts")?.scrollIntoView({ behavior: "smooth", block: "start" });
      });
      workspaceEvidenceRoot.append(button);
    }
  }

  for (const button of workspaceButtons) {
    button.classList.toggle("workspace-active", button.dataset.workspace === workspace.id);
    button.setAttribute("aria-pressed", String(button.dataset.workspace === workspace.id));
  }
  const singleFeature = workspace.features.length === 1 ? workspace.features[0] : "all";
  featureFilterSelect.value = singleFeature ?? "all";
  artifactSearchInput.value = "";
  await populateEvidenceSelect(false);
  currentWorkspaceId = workspace.id;
  renderCurrentWorkspace();
  if (artifacts[0] !== undefined) {
    await loadServerEvidence(artifacts[0].name);
  }
  if (options.history) {
    window.history.pushState({ workspace: workspace.id }, "", `/#workspace/${encodeURIComponent(workspace.id)}`);
  }
  if (options.scroll) {
    workspaceDetailRoot.scrollIntoView({ behavior: "smooth", block: "start" });
  }
}

async function initializeDashboard(): Promise<void> {
  await Promise.all([loadFeatureDefinitions(), refreshSystemStatus()]);
  await refreshServerEvidence(true);
  try {
    await refreshWorkspaceSnapshot();
  } catch (error) {
    workspaceCanvasRoot.replaceChildren();
    const message = document.createElement("p");
    message.className = "inline-error";
    message.textContent = error instanceof Error ? error.message : "Unable to load integrated workspaces.";
    workspaceCanvasRoot.append(message);
  }
  await openWorkspace(workspaceRoute(), { scroll: false, history: false });
}

async function refreshIntegratedWorkspace(): Promise<void> {
  refreshWorkspaceButton.disabled = true;
  try {
    await Promise.all([refreshSystemStatus(), refreshServerEvidence(false)]);
    await refreshWorkspaceSnapshot();
    await openWorkspace(currentWorkspaceId, { scroll: false, history: false });
  } catch (error) {
    const message = document.createElement("p");
    message.className = "inline-error";
    message.textContent = error instanceof Error ? error.message : "Unable to refresh the integrated workspace.";
    workspaceCanvasRoot.replaceChildren(message);
  } finally {
    refreshWorkspaceButton.disabled = false;
  }
}

fileInput.addEventListener("change", async () => {
  const file = fileInput.files?.item(0);
  if (file === null || file === undefined) {
    return;
  }
  artifactMetaElement.textContent = `Local upload · ${formatBytes(file.size)}`;
  downloadArtifactLink.hidden = true;
  renderContents(await file.text(), file.name);
});

serverEvidenceSelect.addEventListener("change", () => { void loadServerEvidence(serverEvidenceSelect.value); });
refreshEvidenceButton.addEventListener("click", () => { void refreshServerEvidence(true); });
featureFilterSelect.addEventListener("change", () => { void populateEvidenceSelect(true); });
artifactSearchInput.addEventListener("input", () => { void populateEvidenceSelect(false); });
refreshSystemButton.addEventListener("click", () => { void refreshSystemStatus(); });
refreshWorkspaceButton.addEventListener("click", () => { void refreshIntegratedWorkspace(); });
for (const button of workspaceButtons) {
  button.addEventListener("click", () => {
    void openWorkspace(button.dataset.workspace ?? "command-center", { scroll: true, history: true });
  });
}
window.addEventListener("popstate", () => {
  void openWorkspace(workspaceRoute(), { scroll: false, history: false });
});
window.addEventListener("hashchange", () => {
  void openWorkspace(workspaceRoute(), { scroll: false, history: false });
});
void initializeDashboard();

// A production desktop host supplies an authenticated server-owned stream only
// when an explicit stream URL is configured. The UI never creates state changes.
const streamParameter = new URLSearchParams(window.location.search).get("stream");
if (streamParameter !== null) {
  let streamUrl: URL | null = null;
  try {
    const candidate = new URL(streamParameter, window.location.origin);
    const expectedProtocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    if (candidate.origin === window.location.origin && candidate.protocol === expectedProtocol) {
      streamUrl = candidate;
    }
  } catch {
    // A status message below explains why the optional stream was ignored.
  }
  if (streamUrl === null) {
    setStatus("Ignored an invalid evidence stream URL. Streams must use this dashboard's origin and transport.", "error");
  } else {
    const stream = new WebSocket(streamUrl);
    stream.addEventListener("message", (message) => {
      try {
        const payload = String(message.data);
        try {
          const dashboard = parseOptionsDashboard(payload);
          events = [];
          renderOptionsDashboard(evidenceRoot, dashboard);
        } catch {
          try {
            const dashboard = parseOperationsDashboard(payload);
            events = [];
            renderOperationsDashboard(evidenceRoot, dashboard);
          } catch {
            try {
              const dashboard = parseLiveMonitoringDashboard(payload);
              events = [];
              renderLiveMonitoringDashboard(evidenceRoot, dashboard);
            } catch {
              try {
                const dashboard = parsePaperDashboard(payload);
                events = [];
                renderPaperDashboard(evidenceRoot, dashboard);
              } catch {
                const next = parseEvidenceLog(`${payload}\n`)[0];
                events = [...events, next];
                renderEvidence(evidenceRoot, events);
              }
            }
          }
        }
      } catch {
        setStatus("Received an invalid evidence event from the configured stream.", "error");
      }
    });
    stream.addEventListener("error", () => {
      setStatus("Configured evidence stream is unavailable; no trading controls are present.", "error");
    });
  }
}
