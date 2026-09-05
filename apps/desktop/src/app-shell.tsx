import { useEffect } from "react";

type WorkspaceLink = Readonly<{
  id: string;
  title: string;
  subtitle: string;
}>;

const WORKSPACE_GROUPS: readonly Readonly<{
  label: string;
  workspaces: readonly WorkspaceLink[];
}>[] = [
  { label: "Monitor", workspaces: [
    { id: "command-center", title: "Command Center", subtitle: "System and gate health" },
  ] },
  { label: "Research", workspaces: [
    { id: "research-lab", title: "Research Lab", subtitle: "Datasets and experiments" },
    { id: "strategy-studio", title: "Strategies", subtitle: "Versions and reproducibility" },
    { id: "marketplace", title: "Marketplace", subtitle: "Discover local research assets" },
    { id: "backtest-explorer", title: "Backtest", subtitle: "Runs and comparisons" },
    { id: "news-cockpit", title: "News", subtitle: "Headlines and signal provenance" },
  ] },
  { label: "Operate", workspaces: [
    { id: "execution-blotter", title: "Execution Blotter", subtitle: "OMS lifecycle evidence" },
    { id: "risk-cockpit", title: "Risk Cockpit", subtitle: "Limits and alerts" },
    { id: "portfolio", title: "Portfolio", subtitle: "Positions and attribution" },
    { id: "replay-incidents", title: "Replay & Incidents", subtitle: "Causal reconstruction" },
    { id: "journal", title: "Journal", subtitle: "Audit chains" },
  ] },
  { label: "Govern", workspaces: [
    { id: "administration", title: "Administration", subtitle: "Commercial and deployment" },
  ] },
];

/** React-owned application shell. The desktop controller is loaded only after
 * every workspace target has mounted. */
export function AppShell(): React.JSX.Element {
  useEffect(() => {
    void import("./main.js").catch((error: unknown) => {
      const status = document.querySelector<HTMLElement>("#load-status");
      if (status !== null) {
        status.textContent = error instanceof Error ? error.message : "Dashboard failed to initialize";
        status.dataset.state = "error";
      }
    });
  }, []);

  return (
    <>
      <a className="skip-link" href="#workspace-detail">Skip to active workspace</a>
      <header className="site-header f-card f-card--elevated">
        <a className="brand" href="/" aria-label="Follon trading terminal home">
          <span className="brand-mark" aria-hidden="true">F</span><span>Follon</span>
        </a>
        <nav className="header-nav" aria-label="Dashboard sections">
          <a href="#system">System</a><a href="#workspaces">Workspaces</a>
          <a href="#capabilities">Capabilities</a><a href="#artifacts">Evidence</a>
        </nav>
        <button id="open-palette" className="palette-trigger" type="button" aria-label="Open command palette (Ctrl+K)">
          <span>Search & Actions</span>
          <kbd className="palette-kbd">Ctrl+K</kbd>
        </button>
        <span className="environment-badge f-badge f-badge--accent">TRADING TERMINAL</span>
      </header>

      <main id="app">
        <section className="hero" aria-labelledby="dashboard-title">
          <p className="eyebrow">Research. Validate. Operate.</p>
          <h1 id="dashboard-title">Your trading workspace.</h1>
          <p className="intro">Discover research, inspect strategies, compare backtests, and follow every decision through risk, execution, and portfolio evidence.</p>
          <div className="safety-note"><strong>Trading boundary</strong><span>Order submit, cancel, and position-close requests use the native desktop command route. Risk, OMS, broker credentials, approvals, and audit recording remain in their owning application boundaries.</span></div>
        </section>

        <section id="workspaces" className="dashboard-section" aria-labelledby="workspaces-title">
          <div className="section-heading">
            <div><p className="eyebrow">Workspace navigator</p><h2 id="workspaces-title">Research to execution</h2></div>
            <p className="section-copy">Twelve connected workspaces. Local evidence stays attributable to its original source.</p>
          </div>
          <div className="workspace-shell">
            <aside className="workspace-sidebar" aria-label="Dashboard workspaces">
              {WORKSPACE_GROUPS.map((group) => (
                <div className="workspace-nav-group" key={group.label}>
                  <p className="workspace-nav-label">{group.label}</p>
                  {group.workspaces.map((workspace) => (
                    <button
                      className={`workspace-card${workspace.id === "command-center" ? " workspace-active" : ""}`}
                      type="button"
                      data-workspace={workspace.id}
                      key={workspace.id}
                    >
                      <span>{workspace.title}</span><small>{workspace.subtitle}</small>
                    </button>
                  ))}
                </div>
              ))}
            </aside>
            <section id="workspace-detail" tabIndex={-1} className="workspace-detail f-card f-card--elevated" aria-live="polite" aria-labelledby="workspace-detail-title">
              <div className="workspace-detail-heading">
                <div><p className="eyebrow">Active workspace</p><h3 id="workspace-detail-title">Command Center</h3></div>
                <div className="workspace-header-actions"><span id="workspace-artifact-count" className="workspace-count f-badge">Loading evidence</span><button id="refresh-workspace" className="f-btn f-btn--primary" type="button">Refresh workspace</button></div>
              </div>
              <p id="workspace-detail-copy" className="workspace-detail-copy">System health, acceptance gates, and evidence from every implemented Follon capability.</p>
              <div id="workspace-feature-badges" className="workspace-badges" />
              <div id="workspace-summary" className="workspace-summary" aria-label="Workspace summary" />
              <div id="workspace-canvas" className="workspace-canvas">
                <div className="workspace-loading"><strong>Loading integrated workspace</strong><span>Reading bounded, immutable system projections…</span></div>
              </div>
              <details className="workspace-capability-drawer">
                <summary>Implementation scope and capabilities</summary>
                <ul id="workspace-capabilities" className="workspace-capabilities" />
                <div id="workspace-evidence" className="workspace-evidence-list" />
              </details>
            </section>
          </div>
        </section>

        <section id="system" className="dashboard-section" aria-labelledby="system-title">
          <div className="section-heading">
            <div><p className="eyebrow">Runtime</p><h2 id="system-title">System health</h2></div>
            <button id="refresh-system" className="f-btn" type="button">Refresh health</button>
          </div>
          <div id="system-overview" className="metric-grid" aria-live="polite" />
        </section>

        <section id="capabilities" className="dashboard-section" aria-labelledby="capabilities-title">
          <div className="section-heading">
            <div><p className="eyebrow">Product map</p><h2 id="capabilities-title">Implemented capabilities</h2></div>
            <p className="section-copy">A gate marked open means code exists, but required real operating or customer evidence has not yet been observed.</p>
          </div>
          <div id="coverage-summary" className="metric-grid coverage-summary" aria-live="polite">
            <article className="metric-card"><p className="metric-label">Coverage</p><p className="metric-value">Loading</p><p className="metric-detail">Reading the implementation catalog</p></article>
          </div>
          <div id="feature-catalog" className="feature-grid" />
        </section>

        <section id="artifacts" className="source-panel" aria-label="Evidence source">
          <div>
            <p className="eyebrow">Evidence explorer</p><h2>Inspect immutable artifacts</h2>
            <p>Browse JSON, NDJSON, Markdown, and CSV evidence recursively indexed from the local <code>var</code> folder. Content is always rendered as inert text or validated structures.</p>
          </div>
          <div className="source-actions">
            <label className="field-label" htmlFor="feature-filter">Feature area</label>
            <select id="feature-filter" className="f-select" aria-label="Filter evidence by feature"><option value="all">All features</option></select>
            <label className="field-label" htmlFor="artifact-search">Search artifacts</label>
            <input id="artifact-search" className="f-input" type="search" placeholder="Name, type, or feature" autoComplete="off" />
            <label className="field-label" htmlFor="server-evidence">Available evidence</label>
            <div className="control-row">
              <select id="server-evidence" className="f-select" aria-label="Available server evidence" disabled><option>Loading local evidence…</option></select>
              <button id="refresh-evidence" className="f-btn" type="button">Refresh</button>
            </div>
            <label className="upload-label" htmlFor="event-log-file">Or choose a local evidence file</label>
            <input id="event-log-file" type="file" accept=".ndjson,.json,.md,.csv,application/x-ndjson,application/json,text/markdown,text/csv" />
          </div>
        </section>
        <p id="load-status" className="status" role="status">Loading local Follon evidence…</p>
        <div className="artifact-toolbar"><span id="artifact-meta">No artifact selected</span><a id="download-artifact" className="download-link" href="#" hidden>Download original</a></div>
        <section id="evidence" aria-live="polite" />
      </main>
      <footer>Follon trading terminal · Native Risk/OMS command route</footer>
    </>
  );
}
