import {
  EvidenceEvent,
  parseEvidenceLog,
  parsePaperDashboard,
  renderEvidence,
  renderPaperDashboard,
} from "./evidence";

const root = document.querySelector<HTMLElement>("#evidence");
if (root === null) {
  throw new Error("evidence root is missing");
}
const fileInput = document.querySelector<HTMLInputElement>("#event-log-file");
const status = document.querySelector<HTMLElement>("#load-status");
if (fileInput === null || status === null) {
  throw new Error("desktop evidence controls are missing");
}

let events: EvidenceEvent[] = [];
renderEvidence(root, events);

fileInput.addEventListener("change", async () => {
  const file = fileInput.files?.item(0);
  if (file === null || file === undefined) {
    return;
  }
  try {
    const contents = await file.text();
    try {
      const dashboard = parsePaperDashboard(contents);
      events = [];
      renderPaperDashboard(root, dashboard);
      status.textContent = `Loaded read-only PAPER operations state from ${file.name}.`;
    } catch {
      events = parseEvidenceLog(contents);
      renderEvidence(root, events);
      status.textContent = `Loaded ${events.length} immutable events from ${file.name}.`;
    }
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : "Unable to load event evidence.";
    renderEvidence(root, []);
  }
});

// A production desktop host supplies an authenticated server-owned stream only
// when an explicit stream URL is configured. The UI never creates state changes.
const streamParameter = new URLSearchParams(window.location.search).get("stream");
if (streamParameter !== null) {
  const stream = new WebSocket(streamParameter);
  stream.addEventListener("message", (message) => {
    try {
      const payload = String(message.data);
      try {
        const dashboard = parsePaperDashboard(payload);
        events = [];
        renderPaperDashboard(root, dashboard);
      } catch {
        const next = parseEvidenceLog(`${payload}\n`)[0];
        events = [...events, next];
        renderEvidence(root, events);
      }
    } catch {
      status.textContent = "Received an invalid evidence event from the configured stream.";
    }
  });
  stream.addEventListener("error", () => {
    status.textContent = "Configured evidence stream is unavailable; no trading controls are present.";
  });
}
