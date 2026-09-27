/**
 * Pure combination-ticket logic, kept free of React and Tauri so it can be
 * regression-tested directly. The native host re-validates everything here;
 * this module exists so an operator sees a precise reason before anything is
 * sent, never to decide on the host's behalf.
 */

export type ComboSide = "BUY" | "SELL";
export type ComboPriceLimitKind = "MAXIMUM_DEBIT" | "MINIMUM_CREDIT";
export type ComboTimeInForce = "DAY" | "GTC";

export type ComboLegDraft = Readonly<{
  instrumentId: string;
  side: ComboSide;
  ratio: string;
  limitPrice: string;
  referencePrice: string;
  referenceObservedAt: string;
}>;

export type ComboDraft = Readonly<{
  accountId: string;
  intentId: string;
  correlationId: string;
  createdAt: string;
  comboQuantity: string;
  priceLimitKind: ComboPriceLimitKind;
  priceLimit: string;
  timeInForce: ComboTimeInForce;
  rationale: string;
  legs: readonly ComboLegDraft[];
}>;

export type ComboLegPayload = Readonly<{
  instrumentId: string;
  side: ComboSide;
  ratio: number;
  limitPrice: string;
  referencePrice: string;
  referenceObservedAt: string;
}>;

/** Exactly the `ComboOrderIntent` IPC shape; the host denies unknown fields. */
export type ComboPayload = Readonly<{
  intentId: string;
  accountId: string;
  strategyId: string;
  correlationId: string;
  legs: readonly ComboLegPayload[];
  comboQuantity: string;
  priceLimitKind: ComboPriceLimitKind;
  priceLimit: string;
  timeInForce: ComboTimeInForce;
  rationale: string;
  createdAt: string;
  strategyVersion: string;
  configurationVersion: string;
  environment: "PAPER";
}>;

export type ComboReceipt = Readonly<{
  command: "SUBMIT_COMBO";
  requestId: string;
  status: string;
  orderId: string | null;
  message: string;
}>;

export const COMBO_MIN_LEGS = 2;
export const COMBO_MAX_LEGS = 16;
const MAX_RATIO = 10_000;
const SCALE = 100_000_000n;
const CANONICAL_ID = /^[a-z0-9._-]+$/u;
const POSITIVE_DECIMAL = /^(?:\d+(?:\.\d{1,8})?|\.\d{1,8})$/u;
const CANONICAL_UTC = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/u;

export function emptyLeg(side: ComboSide): ComboLegDraft {
  return { instrumentId: "", side, ratio: "1", limitPrice: "", referencePrice: "", referenceObservedAt: "" };
}

/** Parses a positive fixed-point decimal into 1e-8 units. Never uses binary floating point. */
export function parseFixed(value: string): bigint | null {
  const text = value.trim();
  if (!POSITIVE_DECIMAL.test(text)) return null;
  const [whole = "", fraction = ""] = text.split(".");
  const units = BigInt(whole === "" ? "0" : whole) * SCALE + BigInt(fraction.padEnd(8, "0"));
  return units > 0n ? units : null;
}

export function formatFixed(units: bigint): string {
  const negative = units < 0n;
  const magnitude = negative ? -units : units;
  const fraction = (magnitude % SCALE).toString().padStart(8, "0");
  return `${negative ? "-" : ""}${magnitude / SCALE}.${fraction}`;
}

function isCanonicalUtc(value: string): boolean {
  if (!CANONICAL_UTC.test(value)) return false;
  const parsed = new Date(value);
  return !Number.isNaN(parsed.getTime()) && parsed.toISOString().replace(/\.\d{3}Z$/u, "Z") === value;
}

/**
 * Signed net price of one unit, from each leg's limit or reference price.
 * Positive is a debit, negative a credit, matching the domain convention.
 * `null` whenever any leg lacks a usable value: an unpriced leg is never
 * filled in from anything else.
 */
export function netPrice(legs: readonly ComboLegDraft[], field: "limitPrice" | "referencePrice"): string | null {
  let net = 0n;
  for (const leg of legs) {
    const price = parseFixed(leg[field]);
    const ratio = /^\d+$/u.test(leg.ratio.trim()) ? BigInt(leg.ratio.trim()) : 0n;
    if (price === null || ratio <= 0n) return null;
    net += (leg.side === "BUY" ? 1n : -1n) * price * ratio;
  }
  return legs.length === 0 ? null : formatFixed(net);
}

export type BuildResult =
  | Readonly<{ ok: true; payload: ComboPayload }>
  | Readonly<{ ok: false; error: string }>;

export function buildComboPayload(draft: ComboDraft): BuildResult {
  const fail = (error: string): BuildResult => ({ ok: false, error });
  for (const [name, value] of [
    ["Account ID", draft.accountId],
    ["Intent ID", draft.intentId],
    ["Correlation ID", draft.correlationId],
  ] as const) {
    if (!CANONICAL_ID.test(value.trim())) return fail(`${name} must be a canonical lower-case ID.`);
  }
  if (!isCanonicalUtc(draft.createdAt.trim())) return fail("Created at must be canonical second-precision UTC.");
  if (draft.legs.length < COMBO_MIN_LEGS || draft.legs.length > COMBO_MAX_LEGS) {
    return fail(`A combination needs ${COMBO_MIN_LEGS} to ${COMBO_MAX_LEGS} legs; a single leg belongs on the order ticket.`);
  }
  const seen = new Set<string>();
  const legs: ComboLegPayload[] = [];
  for (const [index, leg] of draft.legs.entries()) {
    const label = `Leg ${index + 1}`;
    const instrumentId = leg.instrumentId.trim().toLowerCase();
    if (!CANONICAL_ID.test(instrumentId)) return fail(`${label}: instrument must be a canonical ID.`);
    if (seen.has(instrumentId)) return fail(`${label}: every leg must reference a distinct instrument.`);
    seen.add(instrumentId);
    const ratioText = leg.ratio.trim();
    const ratio = /^\d+$/u.test(ratioText) ? Number(ratioText) : Number.NaN;
    if (!Number.isInteger(ratio) || ratio < 1 || ratio > MAX_RATIO) return fail(`${label}: ratio must be a whole number from 1 to ${MAX_RATIO}.`);
    if (parseFixed(leg.limitPrice) === null) return fail(`${label}: limit price must be a positive decimal with at most eight places.`);
    if (parseFixed(leg.referencePrice) === null) {
      return fail(`${label}: enter the price you are observing for this leg right now; no leg is ever priced for you.`);
    }
    if (!isCanonicalUtc(leg.referenceObservedAt.trim())) return fail(`${label}: observation time must be canonical second-precision UTC.`);
    legs.push({
      instrumentId,
      side: leg.side,
      ratio,
      limitPrice: leg.limitPrice.trim(),
      referencePrice: leg.referencePrice.trim(),
      referenceObservedAt: leg.referenceObservedAt.trim(),
    });
  }
  const units = parseFixed(draft.comboQuantity);
  if (units === null || units % SCALE !== 0n) return fail("Units must be a positive whole number; a combination fills in whole units only.");
  if (parseFixed(draft.priceLimit) === null) return fail("The net price limit must be a positive decimal.");
  const rationale = draft.rationale.trim();
  if (rationale === "" || rationale.length > 1024) return fail("A rationale of at most 1024 characters is required.");
  return {
    ok: true,
    payload: {
      intentId: draft.intentId.trim(),
      accountId: draft.accountId.trim(),
      strategyId: "desktop.manual",
      correlationId: draft.correlationId.trim(),
      legs,
      comboQuantity: draft.comboQuantity.trim(),
      priceLimitKind: draft.priceLimitKind,
      priceLimit: draft.priceLimit.trim(),
      timeInForce: draft.timeInForce,
      rationale,
      createdAt: draft.createdAt.trim(),
      strategyVersion: "desktop.manual.v1",
      configurationVersion: "risk.v1",
      environment: "PAPER",
    },
  };
}

const SUBMIT_STATUSES: ReadonlySet<string> = new Set([
  "ACCEPTED_FOR_RISK", "RISK_REJECTED", "PENDING_SUBMIT", "ACKNOWLEDGED",
  "PARTIALLY_FILLED", "FILLED", "REJECTED", "EXPIRED", "UNKNOWN",
]);

export function isComboReceipt(value: unknown, requestId: string): value is ComboReceipt {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const receipt = value as Record<string, unknown>;
  return receipt.command === "SUBMIT_COMBO" && receipt.requestId === requestId &&
    typeof receipt.status === "string" && SUBMIT_STATUSES.has(receipt.status) &&
    (receipt.orderId === null || (typeof receipt.orderId === "string" && CANONICAL_ID.test(receipt.orderId))) &&
    typeof receipt.message === "string" && receipt.message.length > 0;
}

export function isCancelableComboStatus(status: string): boolean {
  return status === "ACKNOWLEDGED" || status === "PARTIALLY_FILLED";
}
