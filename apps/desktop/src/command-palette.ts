/**
 * Universal Command & Search Palette (SOLO-02).
 *
 * Provides fast, keyboard-accessible navigation and discovery across
 * workspaces, actions, loaded evidence artifacts, and documentation anchors.
 */

import { WORKSPACES, WorkspaceId, getWorkspaceDefinition } from "./routes.js";
import { EvidenceArtifact } from "./workspaces.js";

export type PaletteAction = Readonly<{
  id: string;
  title: string;
  category: "Workspace" | "Action" | "Evidence";
  subtitle: string;
  badge?: string;
  onSelect: () => void;
}>;

export type CommandPaletteOptions = Readonly<{
  onOpenWorkspace: (workspaceId: WorkspaceId) => void;
  onOpenArtifact: (name: string) => void;
  onRefreshHealth: () => void;
  onRefreshEvidence: () => void;
  getArtifacts: () => readonly EvidenceArtifact[];
}>;

export class CommandPalette {
  private isOpen = false;
  private selectedIndex = 0;
  private currentItems: PaletteAction[] = [];
  private readonly modal: HTMLElement;
  private readonly input: HTMLInputElement;
  private readonly list: HTMLElement;
  private readonly backdrop: HTMLElement;
  private readonly options: CommandPaletteOptions;

  constructor(options: CommandPaletteOptions) {
    this.options = options;

    // Create modal elements
    this.backdrop = document.createElement("div");
    this.backdrop.className = "palette-backdrop";
    this.backdrop.hidden = true;

    this.modal = document.createElement("div");
    this.modal.className = "palette-modal f-card f-card--elevated";
    this.modal.setAttribute("role", "dialog");
    this.modal.setAttribute("aria-modal", "true");
    this.modal.setAttribute("aria-label", "Command & Search Palette");

    const header = document.createElement("div");
    header.className = "palette-header";

    this.input = document.createElement("input");
    this.input.type = "search";
    this.input.className = "palette-input f-input";
    this.input.placeholder = "Type a command, workspace, or artifact… (Esc to close)";
    this.input.setAttribute("aria-autocomplete", "list");
    this.input.setAttribute("aria-controls", "palette-results");

    const kbdHint = document.createElement("span");
    kbdHint.className = "palette-hint";
    kbdHint.innerHTML = "<kbd>↑↓</kbd> navigate <kbd>↵</kbd> select <kbd>esc</kbd> close";

    header.append(this.input, kbdHint);

    this.list = document.createElement("ul");
    this.list.id = "palette-results";
    this.list.className = "palette-list";
    this.list.setAttribute("role", "listbox");

    this.modal.append(header, this.list);
    this.backdrop.append(this.modal);
    document.body.append(this.backdrop);

    this.bindEvents();
  }

  private bindEvents(): void {
    // Global shortcut Ctrl+K or Cmd+K
    window.addEventListener("keydown", (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        this.toggle();
      } else if (event.key === "Escape" && this.isOpen) {
        event.preventDefault();
        this.close();
      }
    });

    this.backdrop.addEventListener("click", (event: MouseEvent) => {
      if (event.target === this.backdrop) {
        this.close();
      }
    });

    this.input.addEventListener("input", () => {
      this.filter(this.input.value);
    });

    this.input.addEventListener("keydown", (event: KeyboardEvent) => {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        this.moveSelection(1);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        this.moveSelection(-1);
      } else if (event.key === "Enter") {
        event.preventDefault();
        this.executeSelected();
      }
    });
  }

  public open(): void {
    this.isOpen = true;
    this.backdrop.hidden = false;
    this.input.value = "";
    this.filter("");
    this.input.focus?.();
  }

  public close(): void {
    this.isOpen = false;
    this.backdrop.hidden = true;
  }

  public toggle(): void {
    if (this.isOpen) {
      this.close();
    } else {
      this.open();
    }
  }

  private buildDefaultItems(): PaletteAction[] {
    const items: PaletteAction[] = [];

    // 1. Workspaces
    for (const ws of WORKSPACES) {
      items.push({
        id: `ws-${ws.id}`,
        title: ws.title,
        category: "Workspace",
        subtitle: `${ws.group} · ${ws.subtitle}`,
        badge: ws.group,
        onSelect: () => this.options.onOpenWorkspace(ws.id),
      });
    }

    // 2. Actions
    items.push({
      id: "act-brief",
      title: "Daily Operating Brief",
      category: "Action",
      subtitle: "Review system freshness, unresolved orders, positions, and gates",
      badge: "Brief",
      onSelect: () => {
        this.options.onOpenWorkspace("command-center");
        document.querySelector("#daily-brief")?.scrollIntoView({ behavior: "smooth", block: "start" });
      },
    });

    items.push({
      id: "act-ticket",
      title: "Draft Order Intent",
      category: "Action",
      subtitle: "Open Order Ticket to submit declarative intent to Risk/OMS",
      badge: "Trading",
      onSelect: () => {
        this.options.onOpenWorkspace("execution-blotter");
        document.querySelector(".order-ticket-grid")?.scrollIntoView({ behavior: "smooth", block: "start" });
      },
    });

    items.push({
      id: "act-refresh-health",
      title: "Refresh System Health",
      category: "Action",
      subtitle: "Query health and gate evaluation status from local server",
      badge: "System",
      onSelect: () => this.options.onRefreshHealth(),
    });

    items.push({
      id: "act-refresh-evidence",
      title: "Refresh Local Evidence Listing",
      category: "Action",
      subtitle: "Index local artifacts from the var directory",
      badge: "Evidence",
      onSelect: () => this.options.onRefreshEvidence(),
    });

    // Signature Connected Experiences & Advanced Capabilities
    items.push({
      id: "act-away-desk",
      title: "Can I Safely Leave the Desk?",
      category: "Action",
      subtitle: "Away-desk readiness check, kill-switch status & escalation policy (SOLO-05/06, EXEC-01)",
      badge: "Experience 5",
      onSelect: () => {
        this.options.onOpenWorkspace("command-center");
        setTimeout(() => document.querySelector("#away-desk-readiness-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-explain-moment",
      title: "Explain This Moment",
      category: "Action",
      subtitle: "Unified temporal reconstruction across market, strategy, risk, OMS & ledger (SOLO-01, RES-03)",
      badge: "Experience 1",
      onSelect: () => {
        this.options.onOpenWorkspace("replay-incidents");
        setTimeout(() => document.querySelector("#explain-moment-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-strategy-invalidation",
      title: "Show What Would Invalidate This Strategy",
      category: "Action",
      subtitle: "Strategy falsification conditions and synthetic stress injection explorer (RES-01/04/05/08, AI-03)",
      badge: "Experience 2",
      onSelect: () => {
        this.options.onOpenWorkspace("strategy-studio");
        setTimeout(() => document.querySelector("#strategy-invalidation-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-joint-correlation",
      title: "Why Are These Strategies Losing Together?",
      category: "Action",
      subtitle: "Joint strategy loss, co-movement, and common factor dependency breakdown (RES-06, DATA-05, RISK-01)",
      badge: "Experience 3",
      onSelect: () => {
        this.options.onOpenWorkspace("risk-cockpit");
        setTimeout(() => document.querySelector("#joint-correlation-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-input-correction",
      title: "What Changes If This Input Is Corrected?",
      category: "Action",
      subtitle: "Trace revised datasets or corrected news to affected experiments and decisions (DATA-01/03)",
      badge: "Experience 4",
      onSelect: () => {
        this.options.onOpenWorkspace("research-lab");
        setTimeout(() => document.querySelector("#input-correction-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-workspace-rebuild",
      title: "Rebuild My Entire Workspace",
      category: "Action",
      subtitle: "Cold-start disaster recovery drills reconstructing application, research & state (LIFE-01..03)",
      badge: "Experience 6",
      onSelect: () => {
        this.options.onOpenWorkspace("administration");
        setTimeout(() => document.querySelector("#workspace-rebuild-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-champion-challenger",
      title: "Champion vs Challenger Shadow Evaluation",
      category: "Action",
      subtitle: "Shadow-evaluate challenger iterations against champions with automated retirement (RES-08)",
      badge: "RES-08",
      onSelect: () => {
        this.options.onOpenWorkspace("strategy-studio");
        setTimeout(() => document.querySelector("#champion-challenger-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-execution-planner",
      title: "Capability-Aware Execution Planner",
      category: "Action",
      subtitle: "Plan algorithmic child slices (TWAP, VWAP, Peg) with venue capability verification (EXEC-04)",
      badge: "EXEC-04",
      onSelect: () => {
        this.options.onOpenWorkspace("execution-blotter");
        setTimeout(() => document.querySelector("#execution-planner-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-ops-assistant",
      title: "Operations Diagnosis Assistant & Runbooks",
      category: "Action",
      subtitle: "Synthesize root-cause diagnosis and parameterized remediation runbooks (AI-05)",
      badge: "AI-05",
      onSelect: () => {
        this.options.onOpenWorkspace("administration");
        setTimeout(() => document.querySelector("#operations-assistant-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-model-eval",
      title: "AI Model Evaluation Console",
      category: "Action",
      subtitle: "Benchmark reasoning latency, determinism, token cost & hallucination rates (AI-06)",
      badge: "AI-06",
      onSelect: () => {
        this.options.onOpenWorkspace("administration");
        setTimeout(() => document.querySelector("#model-evaluation-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-strategy-capsules",
      title: "Portable Strategy Capsules",
      category: "Action",
      subtitle: "Export and inspect self-contained reproducible strategy capsules and manifests (ASSET-04)",
      badge: "ASSET-04",
      onSelect: () => {
        this.options.onOpenWorkspace("marketplace");
        setTimeout(() => document.querySelector("#strategy-capsule-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    items.push({
      id: "act-multi-asset",
      title: "Multi-Asset Roll & Settlement Lifecycle",
      category: "Action",
      subtitle: "Coordinate options rolls, cash/physical assignment, FX hedging, and futures delivery (PORT-02)",
      badge: "PORT-02",
      onSelect: () => {
        this.options.onOpenWorkspace("portfolio");
        setTimeout(() => document.querySelector("#multi-asset-panel")?.scrollIntoView({ behavior: "smooth", block: "start" }), 50);
      },
    });

    // 3. Artifacts
    const artifacts = this.options.getArtifacts();
    for (const art of artifacts.slice(0, 30)) {
      items.push({
        id: `art-${art.name}`,
        title: art.name,
        category: "Evidence",
        subtitle: `${art.kind} · ${art.feature} · ${formatBytes(art.bytes)}`,
        badge: art.feature,
        onSelect: () => this.options.onOpenArtifact(art.name),
      });
    }

    return items;
  }

  private filter(query: string): void {
    const rawItems = this.buildDefaultItems();
    const cleanQuery = query.trim().toLowerCase();

    if (cleanQuery.length === 0) {
      this.currentItems = rawItems;
    } else {
      this.currentItems = rawItems.filter((item) => {
        const text = `${item.title} ${item.subtitle} ${item.category} ${item.badge ?? ""}`.toLowerCase();
        return text.includes(cleanQuery);
      });
    }

    this.selectedIndex = 0;
    this.renderList();
  }

  private moveSelection(delta: number): void {
    if (this.currentItems.length === 0) return;
    this.selectedIndex = (this.selectedIndex + delta + this.currentItems.length) % this.currentItems.length;
    this.renderList();
    const activeEl = this.list.children[this.selectedIndex] as HTMLElement | undefined;
    activeEl?.scrollIntoView({ block: "nearest" });
  }

  private executeSelected(): void {
    const item = this.currentItems[this.selectedIndex];
    if (item !== undefined) {
      this.close();
      item.onSelect();
    }
  }

  private renderList(): void {
    this.list.replaceChildren();

    if (this.currentItems.length === 0) {
      const empty = document.createElement("li");
      empty.className = "palette-empty";
      empty.textContent = "No matching commands, workspaces, or artifacts.";
      this.list.append(empty);
      return;
    }

    this.currentItems.forEach((item, index) => {
      const li = document.createElement("li");
      li.className = `palette-item${index === this.selectedIndex ? " palette-item--active" : ""}`;
      li.setAttribute("role", "option");
      li.setAttribute("aria-selected", String(index === this.selectedIndex));

      const titleSpan = document.createElement("span");
      titleSpan.className = "palette-item-title";
      titleSpan.textContent = item.title;

      const subSpan = document.createElement("span");
      subSpan.className = "palette-item-sub";
      subSpan.textContent = item.subtitle;

      const metaDiv = document.createElement("div");
      metaDiv.className = "palette-item-meta";
      metaDiv.append(titleSpan, subSpan);

      li.append(metaDiv);

      if (item.badge) {
        const badgeSpan = document.createElement("span");
        badgeSpan.className = `palette-item-badge badge-${item.category.toLowerCase()}`;
        badgeSpan.textContent = item.badge;
        li.append(badgeSpan);
      }

      li.addEventListener("click", () => {
        this.close();
        item.onSelect();
      });

      this.list.append(li);
    });
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
