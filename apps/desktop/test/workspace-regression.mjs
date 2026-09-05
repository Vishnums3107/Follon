import assert from "node:assert/strict";
import { sentimentSignalPower, strategyIdentityRows } from "../dist/workspaces.js";

assert.equal(sentimentSignalPower(8500, 9200), "7820 bps");
assert.equal(sentimentSignalPower(-8500, 9200), "7820 bps");
assert.equal(sentimentSignalPower(0, 9200), "0 bps");
assert.equal(sentimentSignalPower(10000, 10000), "10000 bps");
for (const pair of [
  [undefined, 9000], [null, 9000], ["", 9000], ["8500", "9200"],
  [NaN, 9000], [Infinity, 9000], [10001, 9000], [-10001, 9000],
  [8500, undefined], [8500, -1], [8500, 10001], [0.5, 9000],
]) {
  assert.equal(sentimentSignalPower(...pair), "Unavailable", `invalid signal ${pair}`);
}
const run = {
  artifact: "run-a.json",
  specification_fingerprint: "a".repeat(64),
  specification: {
    strategy_bundle_hash: "b".repeat(64), configuration_hash: "c".repeat(64), engine_version: "1",
    dataset: { dataset_id: "spy", dataset_version: "1", content_hash: "d".repeat(64) },
  },
};
const revised = structuredClone(run);
revised.artifact = "run-b.json";
revised.specification_fingerprint = "e".repeat(64);
revised.specification.dataset.dataset_version = "2";
revised.specification.dataset.content_hash = "f".repeat(64);
revised.specification.engine_version = "2";
const identities = strategyIdentityRows({ backtests: [run, structuredClone(run), revised] }, undefined);
assert.equal(identities.length, 2, "duplicate exports collapse but different specifications survive");
assert.ok(identities[0][4].includes("spy / 1"));
assert.ok(identities[1][4].includes("spy / 2"));
assert.equal(identities[1][5], "2");
console.log("Workspace sentiment and reproducibility identity regression tests passed");
