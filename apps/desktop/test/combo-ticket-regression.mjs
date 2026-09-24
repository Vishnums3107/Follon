import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  buildComboPayload,
  formatFixed,
  isCancelableComboStatus,
  isComboReceipt,
  netPrice,
  parseFixed,
} from "../dist/combo-intent.js";

const appDirectory = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const leg = (instrumentId, side, limitPrice, referencePrice, ratio = "1") => ({
  instrumentId,
  side,
  ratio,
  limitPrice,
  referencePrice,
  referenceObservedAt: "2026-09-03T12:29:59Z",
});

const vertical = (overrides = {}) => ({
  accountId: "acct.desktop.paper.test",
  intentId: "intent.desktop.combo.001",
  correlationId: "correlation.desktop.combo.001",
  createdAt: "2026-09-03T12:30:00Z",
  comboQuantity: "2",
  priceLimitKind: "MAXIMUM_DEBIT",
  priceLimit: "2.6",
  timeInForce: "DAY",
  rationale: "operator vertical",
  legs: [leg("inst.opt.spy.c500", "BUY", "50.5", "50"), leg("inst.opt.spy.c505", "SELL", "47.9", "48")],
  ...overrides,
});

// Fixed-point arithmetic never passes through binary floating point.
assert.equal(parseFixed("0.1") + parseFixed("0.2"), parseFixed("0.3"));
assert.equal(formatFixed(parseFixed("0.1") + parseFixed("0.2")), "0.30000000");
assert.equal(parseFixed("1.123456789"), null, "more than eight places is refused");
assert.equal(parseFixed("0"), null, "zero is not positive");
assert.equal(parseFixed("-1"), null);
assert.equal(parseFixed("1e3"), null);

// Net price uses the domain sign convention: debit positive, credit negative.
assert.equal(netPrice(vertical().legs, "limitPrice"), "2.60000000");
assert.equal(netPrice(vertical().legs, "referencePrice"), "2.00000000");
assert.equal(
  netPrice([leg("inst.a", "SELL", "5", "5", "2"), leg("inst.b", "BUY", "3", "3")], "limitPrice"),
  "-7.00000000",
  "ratios apply and a net credit is negative",
);
// An unobserved leg makes the preview incomplete; it is never priced from its limit.
assert.equal(netPrice([leg("inst.a", "BUY", "5", ""), leg("inst.b", "SELL", "3", "3")], "referencePrice"), null);

// A valid draft becomes exactly the host's ComboOrderIntent shape.
const built = buildComboPayload(vertical());
assert.equal(built.ok, true, built.error);
assert.deepEqual(Object.keys(built.payload).sort(), [
  "accountId", "comboQuantity", "configurationVersion", "correlationId", "createdAt", "environment",
  "intentId", "legs", "priceLimit", "priceLimitKind", "rationale", "strategyId", "strategyVersion", "timeInForce",
]);
assert.equal(built.payload.environment, "PAPER");
assert.equal(built.payload.legs.length, 2);
assert.deepEqual(Object.keys(built.payload.legs[0]).sort(), [
  "instrumentId", "limitPrice", "ratio", "referenceObservedAt", "referencePrice", "side",
]);
assert.equal(built.payload.legs[0].ratio, 1);
assert.equal(built.payload.legs[1].referencePrice, "48", "each leg keeps its own observation");

const refusals = [
  ["single leg", { legs: [vertical().legs[0]] }, /2 to 16 legs/u],
  ["duplicate instrument", { legs: [vertical().legs[0], { ...vertical().legs[1], instrumentId: "inst.opt.spy.c500" }] }, /distinct/u],
  ["fractional units", { comboQuantity: "1.5" }, /whole number/u],
  ["unobserved leg", { legs: [vertical().legs[0], { ...vertical().legs[1], referencePrice: "" }] }, /observing/u],
  ["impossible observation time", { legs: [vertical().legs[0], { ...vertical().legs[1], referenceObservedAt: "2026-02-31T12:00:00Z" }] }, /observation time/u],
  ["zero ratio", { legs: [{ ...vertical().legs[0], ratio: "0" }, vertical().legs[1]] }, /ratio/u],
  ["non-canonical intent", { intentId: "Intent 1" }, /Intent ID/u],
  ["missing rationale", { rationale: "  " }, /rationale/u],
];
for (const [name, overrides, reason] of refusals) {
  const result = buildComboPayload(vertical(overrides));
  assert.equal(result.ok, false, `${name} must be refused before anything is sent`);
  assert.match(result.error, reason, name);
}
assert.equal(buildComboPayload(vertical({ comboQuantity: "3.00000000" })).ok, true, "a zero fraction is a whole unit");

// Receipts must answer this exact request with a submit-combination status.
const receipt = { command: "SUBMIT_COMBO", requestId: "intent.desktop.combo.001", status: "FILLED", orderId: "combo-order-intent.desktop.combo.001", message: "accepted" };
assert.equal(isComboReceipt(receipt, "intent.desktop.combo.001"), true);
assert.equal(isComboReceipt(receipt, "intent.desktop.combo.other"), false);
assert.equal(isComboReceipt({ ...receipt, command: "SUBMIT_ORDER" }, "intent.desktop.combo.001"), false);
assert.equal(isComboReceipt({ ...receipt, status: "PENDING_POSITION_CLOSE" }, "intent.desktop.combo.001"), false);
assert.equal(isComboReceipt({ ...receipt, orderId: "Not Canonical" }, "intent.desktop.combo.001"), false);
assert.equal(isCancelableComboStatus("ACKNOWLEDGED"), true);
assert.equal(isCancelableComboStatus("FILLED"), false);

// The ticket submits one combination command and never routes legs as plain orders.
const comboTicket = await readFile(resolve(appDirectory, "src", "ComboTicket.tsx"), "utf8");
assert.match(comboTicket, /invoke<unknown>\("submit_combo_order", \{ intent: built\.payload \}\)/u);
assert.doesNotMatch(comboTicket, /"submit_order"/u);
assert.doesNotMatch(comboTicket, /"close_position"/u);
assert.doesNotMatch(comboTicket, /LIVE"/u);
assert.match(comboTicket, /invoke<unknown>\("cancel_order"/u);
const tauriHost = await readFile(resolve(appDirectory, "src-tauri", "src", "lib.rs"), "utf8");
assert.match(tauriHost, /trading::submit_combo_order,/u);

console.log("Combination ticket regression passed");
