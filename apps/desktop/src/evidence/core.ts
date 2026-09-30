// Evidence events, the canonical NDJSON log parser and shared validation primitives.

export type EvidenceEvent = Readonly<{
  event_id: string;
  event_type: string;
  schema_version: number;
  event_time: string;
  receive_time: string;
  account_id: string | null;
  strategy_id: string | null;
  instrument_id: string | null;
  correlation_id: string;
  causation_id: string | null;
  actor: string;
  source: string;
  payload: Record<string, unknown>;
  software_version: string;
  configuration_version: string;
}>;

/** Parses and validates canonical NDJSON before it is shown as evidence. */
export function parseEvidenceLog(ndjson: string): EvidenceEvent[] {
  const eventIds = new Set<string>();
  const events: EvidenceEvent[] = [];
  for (const [index, line] of ndjson.split(/\r?\n/).filter(Boolean).entries()) {
    let candidate: unknown;
    try {
      candidate = JSON.parse(line);
    } catch {
      throw new Error(`Line ${index + 1} is not valid JSON.`);
    }
    if (!isEvidenceEvent(candidate)) {
      throw new Error(`Line ${index + 1} is not a valid Follon event envelope.`);
    }
    if (eventIds.has(candidate.event_id)) {
      throw new Error(`Line ${index + 1} repeats event ID ${candidate.event_id}.`);
    }
    if (candidate.causation_id !== null && !eventIds.has(candidate.causation_id)) {
      throw new Error(`Line ${index + 1} references an unseen causation event.`);
    }
    eventIds.add(candidate.event_id);
    events.push(candidate);
  }
  if (events.length === 0) {
    throw new Error("The selected event log is empty.");
  }
  return events;
}

function isEvidenceEvent(value: unknown): value is EvidenceEvent {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const envelopeFields = new Set([
    "event_id", "event_type", "schema_version", "event_time", "receive_time",
    "account_id", "strategy_id", "instrument_id", "correlation_id", "causation_id",
    "actor", "source", "payload", "software_version", "configuration_version",
  ]);
  const isCanonicalId = (field: unknown): field is string =>
    typeof field === "string" && /^[a-z0-9._-]+$/.test(field);
  const isNullableCanonicalId = (field: unknown): field is string | null =>
    field === null || isCanonicalId(field);
  const isCanonicalUtcTimestamp = (field: unknown): field is string => {
    if (typeof field !== "string") return false;
    const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?Z$/.exec(field);
    if (match === null) return false;
    const [year, month, day, hour, minute, second] = match.slice(1, 7).map(Number);
    if (year < 1 || month < 1 || month > 12 || day < 1 || hour > 23 || minute > 59 || second > 59) {
      return false;
    }
    const parsed = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
    return parsed.getUTCFullYear() === year && parsed.getUTCMonth() === month - 1 &&
      parsed.getUTCDate() === day && parsed.getUTCHours() === hour &&
      parsed.getUTCMinutes() === minute && parsed.getUTCSeconds() === second;
  };
  return (
    Object.keys(candidate).every((field) => envelopeFields.has(field)) &&
    isCanonicalId(candidate.event_id) &&
    typeof candidate.event_type === "string" &&
    /^([a-z]+\.)+[a-z_]+\.v[1-9][0-9]*$/.test(candidate.event_type) &&
    typeof candidate.schema_version === "number" && Number.isInteger(candidate.schema_version) && candidate.schema_version >= 1 &&
    isCanonicalUtcTimestamp(candidate.event_time) &&
    isCanonicalUtcTimestamp(candidate.receive_time) &&
    isNullableCanonicalId(candidate.account_id) &&
    isNullableCanonicalId(candidate.strategy_id) &&
    isNullableCanonicalId(candidate.instrument_id) &&
    isCanonicalId(candidate.correlation_id) &&
    isNullableCanonicalId(candidate.causation_id) &&
    typeof candidate.actor === "string" && candidate.actor.length > 0 &&
    typeof candidate.source === "string" && candidate.source.length > 0 &&
    candidate.payload !== null &&
    typeof candidate.payload === "object" &&
    !Array.isArray(candidate.payload) &&
    typeof candidate.software_version === "string" && candidate.software_version.length > 0 &&
    typeof candidate.configuration_version === "string" && candidate.configuration_version.length > 0
  );
}

export function hasExactKeys(value: unknown, expected: readonly string[]): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const keys = Object.keys(value);
  return keys.length === expected.length && keys.every((key) => expected.includes(key));
}

export function isCanonicalId(value: unknown): value is string {
  return typeof value === "string" && /^[a-z0-9._-]+$/.test(value);
}

export function isDecimal(value: unknown): value is string {
  return typeof value === "string" && /^-?[0-9]+(?:\.[0-9]{1,8})?$/.test(value);
}

export function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

export function isPositiveInteger(value: unknown): value is number {
  return isNonNegativeInteger(value) && value > 0;
}

export function isHash(value: unknown): value is string {
  return typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
}

export function isUtcTimestamp(value: unknown): value is string {
  return typeof value === "string" && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value);
}
