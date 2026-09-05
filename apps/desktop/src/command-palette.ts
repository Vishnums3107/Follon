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
