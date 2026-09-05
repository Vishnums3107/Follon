// DOM behavior tests; this small harness does not validate browser layout.
import assert from "node:assert/strict";
import { renderWorkspace } from "../dist/workspaces.js";

class Element {
  constructor(tag) {
    this.tag = tag;
    this.children = [];
    this.value = "";
    this.textContent = "";
    this.hidden = false;
    this.events = {};
    this.style = {};
  }
  append(...items) {
    this.children.push(...items);
    if (this.tag === "select" && this.value === "") this.value = this.children[0]?.value ?? "";
  }
  replaceChildren(...items) { this.children = items; }
  setAttribute(key, value) { this[key] = value; }
  addEventListener(key, callback) { this.events[key] = callback; }
  fire(key) { this.events[key]?.({ preventDefault() {}, stopPropagation() {} }); }
  querySelector(tag) {
    return this.children.find((item) => item.tag === tag) ??
      this.children.map((item) => item.querySelector(tag)).find(Boolean);
  }
  get rows() { return this.children; }
}
globalThis.document = { createElement: (tag) => new Element(tag) };
globalThis.Option = class extends Element {
  constructor(text, value) { super("option"); this.textContent = text; this.value = value; }
};

const snapshot = { datasets: [], backtests: [] };
const summary = new Element("div");
const root = new Element("div");
let opened;
const artifacts = Array.from({ length: 55 }, (_, index) => ({
  name: `Asset ${index}`, feature: index % 2 ? "research" : "replay", kind: "report",
  bytes: 1024, modified_at: "2026-09-05T00:00:00Z",
}));
artifacts.push({ name: "Private", feature: "identity", kind: "report", bytes: 2, modified_at: "" });
renderWorkspace(summary, root, "marketplace", snapshot, { artifacts, onOpenArtifact: (name) => { opened = name; } });
const panel = root.children[0];
const [search, category, count] = panel.children[1].children;
const results = panel.children[2];
const more = panel.children[3];
assert.equal(results.children.length, 24);
assert.equal(count.textContent, "24 of 55 assets");
more.fire("click");
assert.equal(results.children.length, 48);
more.fire("click");
assert.equal(results.children.length, 55);
assert.equal(more.hidden, true);
category.value = "research";
category.fire("change");
assert.equal(results.children.length, 24);
assert.equal(count.textContent, "24 of 27 assets");
search.value = "  ASSET 1 ";
search.fire("input");
assert.equal(count.textContent, "6 of 6 assets");
results.children[0].children[3].fire("click");
assert.equal(opened, "Asset 1");
search.value = "<script>";
search.fire("input");
assert.equal(count.textContent, "0 of 0 assets");
assert.equal(results.children[0].textContent, "No matching assets");
assert.equal(more.hidden, true);
renderWorkspace(summary, root, "marketplace", snapshot, { artifacts: [], onOpenArtifact() {} });
assert.equal(root.children[0].children[2].children[0].textContent, "Your local catalogue is empty");

// Filtering a later page must still open the original source artifact.
const news = Array.from({ length: 45 }, (_, index) => ({
  artifact: `headline-${index}.ndjson`,
  data: { event_type: "news.headline.v1", event_time: "2026-09-05T00:00:00Z",
    payload: { news_id: String(index), headline: `Headline ${index}`, source: "fixture", entity_tickers: ["SPY"] } },
}));
renderWorkspace(summary, root, "news-cockpit", { ...snapshot, events: news }, {
  artifacts: [], onOpenArtifact: (name) => { opened = name; },
});
const headlines = root.children[0];
const [filter, status, previous, next] = headlines.children[1].children;
const rows = headlines.querySelector("tbody").children;
assert.equal(rows.filter((row) => !row.hidden).length, 20);
assert.equal(previous.disabled, true);
next.fire("click");
assert.equal(rows[20].hidden, false);
next.fire("click");
assert.equal(rows.filter((row) => !row.hidden).length, 5);
assert.equal(next.disabled, true);
filter.value = "HEADLINE 41";
filter.fire("input");
assert.equal(status.textContent, "1 of 45 records · Page 1 of 1");
assert.equal(rows[41].hidden, false);
rows[41].fire("click");
assert.equal(opened, "headline-41.ndjson");
filter.value = "not present";
filter.fire("input");
assert.equal(rows.filter((row) => !row.hidden).length, 0);
assert.equal(status.textContent, "0 of 45 records · Page 1 of 1");
console.log("Marketplace and paginated news collection regressions passed");
