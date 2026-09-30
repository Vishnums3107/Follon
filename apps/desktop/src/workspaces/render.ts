// Workspace dispatch: mounts the renderer for the selected workspace.

import { renderAdministration } from "./administration.js";
import { renderBacktestExplorer } from "./backtest-explorer.js";
import { renderCommandCenter } from "./command-center.js";
import { renderExecutionBlotter } from "./execution-blotter.js";
import { renderJournal } from "./journal.js";
import { renderMarketplace } from "./marketplace.js";
import { renderNewsCockpit } from "./news-cockpit.js";
import { renderEmpty } from "./panels.js";
import { renderPortfolio } from "./portfolio.js";
import { renderReplayAndIncidents } from "./replay-incidents.js";
import { renderResearchLab } from "./research-lab.js";
import { renderRiskCockpit } from "./risk-cockpit.js";
import { renderStrategyStudio } from "./strategy-studio.js";
import { mountedTickets } from "./ticket-mounts.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

export function renderWorkspace(
  summaryRoot: HTMLElement,
  canvasRoot: HTMLElement,
  workspaceId: string,
  snapshot: WorkspaceSnapshot,
  context: WorkspaceContext,
): void {
  mountedTickets.order?.unmount();
  mountedTickets.order = undefined;
  mountedTickets.combo?.unmount();
  mountedTickets.combo = undefined;
  summaryRoot.replaceChildren();
  canvasRoot.replaceChildren();
  switch (workspaceId) {
    case "command-center":
      renderCommandCenter(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "research-lab":
      renderResearchLab(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "news-cockpit":
      renderNewsCockpit(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "strategy-studio":
      renderStrategyStudio(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "marketplace":
      renderMarketplace(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "backtest-explorer":
      renderBacktestExplorer(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "execution-blotter":
      renderExecutionBlotter(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "risk-cockpit":
      renderRiskCockpit(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "portfolio":
      renderPortfolio(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "replay-incidents":
      renderReplayAndIncidents(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "journal":
      renderJournal(summaryRoot, canvasRoot, snapshot, context);
      break;
    case "administration":
      renderAdministration(summaryRoot, canvasRoot, snapshot, context);
      break;
    default:
      renderEmpty(canvasRoot, "Unknown workspace", "Choose a workspace from the navigation.");
  }
}
