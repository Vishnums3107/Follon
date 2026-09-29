import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

/** Controlled-LIVE activation is not exposed by this desktop ticket. */
type ExecutionEnvironment = "PAPER";
type OrderType = "MARKET" | "LIMIT";
type TimeInForce = "DAY" | "GTC";

type CommandReceipt = Readonly<{
  command: "SUBMIT_ORDER" | "CANCEL_ORDER" | "CLOSE_POSITION";
  requestId: string;
  status:
    | "ACCEPTED_FOR_RISK"
    | "RISK_REJECTED"
    | "PENDING_SUBMIT"
    | "ACKNOWLEDGED"
    | "PARTIALLY_FILLED"
    | "FILLED"
    | "PENDING_CANCEL"
    | "PENDING_REPLACE"
    | "CANCELLED"
    | "REJECTED"
    | "EXPIRED"
    | "UNKNOWN"
    | "PENDING_POSITION_CLOSE";
  orderId: string | null;
  message: string;
}>;

type CommandRouteStatus = Readonly<{
  routeAvailable: boolean;
  message: string;
}>;

type OrderTicketProps = Readonly<{
  defaultAccountId?: string;
  defaultEnvironment?: ExecutionEnvironment;
}>;

const DRAFT_STORAGE_PREFIX = "follon:order_ticket_draft:";

type TicketDraft = Readonly<{
  accountId?: string;
  instrumentId?: string;
  intentId?: string;
  correlationId?: string;
  createdAt?: string;
  quantity?: string;
  orderType?: OrderType;
  limitPrice?: string;
  environment?: ExecutionEnvironment;
  timeInForce?: TimeInForce;
  rationale?: string;
  referencePrice?: string;
  referenceObservedAt?: string;
  cancelOrderId?: string;
}>;

function draftStorageKey(accountId: string, environment: ExecutionEnvironment): string {
  return `${DRAFT_STORAGE_PREFIX}${accountId.trim().toLowerCase() || "unbound"}:${environment.toLowerCase()}`;
}

function loadTicketDraft(accountId: string, environment: ExecutionEnvironment): TicketDraft {
  try {
    const raw = sessionStorage.getItem(draftStorageKey(accountId, environment));
    if (raw !== null) {
      return JSON.parse(raw) as TicketDraft;
    }
  } catch {
    // Fall back to defaults if storage is disabled or corrupted
  }
  return {};
}

function persistTicketDraft(accountId: string, environment: ExecutionEnvironment, draft: TicketDraft): void {
  try {
    sessionStorage.setItem(draftStorageKey(accountId, environment), JSON.stringify(draft));
  } catch {
    // Ignore storage quota or access issues
  }
}

function clearTicketDraft(accountId: string, environment: ExecutionEnvironment): void {
  try {
    sessionStorage.removeItem(draftStorageKey(accountId, environment));
  } catch {
    // Ignore
  }
}

function isNativeHost(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function canonicalTimestamp(): string {
  return new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
}

function generatedId(prefix: string): string | undefined {
  const uuid = globalThis.crypto?.randomUUID?.();
  return uuid === undefined ? undefined : `${prefix}.${uuid.toLowerCase()}`;
}

function isCommandReceipt(
  value: unknown,
  command: CommandReceipt["command"],
  requestId: string,
): value is CommandReceipt {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const receipt = value as Record<string, unknown>;
  const allowedStatuses: Readonly<Record<CommandReceipt["command"], ReadonlySet<string>>> = {
    SUBMIT_ORDER: new Set([
      "ACCEPTED_FOR_RISK", "RISK_REJECTED", "PENDING_SUBMIT", "ACKNOWLEDGED",
      "PARTIALLY_FILLED", "FILLED", "REJECTED", "EXPIRED", "UNKNOWN",
    ]),
    CANCEL_ORDER: new Set(["PENDING_CANCEL", "PARTIALLY_FILLED", "FILLED", "CANCELLED", "UNKNOWN"]),
    CLOSE_POSITION: new Set([
      "ACCEPTED_FOR_RISK", "RISK_REJECTED", "PENDING_SUBMIT", "ACKNOWLEDGED",
      "PARTIALLY_FILLED", "FILLED", "REJECTED", "EXPIRED", "UNKNOWN", "PENDING_POSITION_CLOSE",
    ]),
  };
  return receipt.command === command && receipt.requestId === requestId &&
    typeof receipt.status === "string" && allowedStatuses[command].has(receipt.status) &&
    (receipt.orderId === null || (typeof receipt.orderId === "string" && /^[a-z0-9._-]+$/.test(receipt.orderId))) &&
    typeof receipt.message === "string" && receipt.message.length > 0;
}

function isSubmitReceipt(value: unknown, requestId: string): value is CommandReceipt {
  return isCommandReceipt(value, "SUBMIT_ORDER", requestId);
}

function isCancelableStatus(status: CommandReceipt["status"]): boolean {
  return status === "PENDING_SUBMIT" || status === "ACKNOWLEDGED" || status === "PARTIALLY_FILLED";
}

export function OrderTicket({
  defaultAccountId = "",
  defaultEnvironment = "PAPER",
}: OrderTicketProps): React.JSX.Element {
  const [draft] = useState<TicketDraft>(() => loadTicketDraft(defaultAccountId, defaultEnvironment));
  const [accountId, setAccountId] = useState(draft.accountId ?? defaultAccountId);
  const [instrumentId, setInstrumentId] = useState(draft.instrumentId ?? "");
  const [intentId, setIntentId] = useState(draft.intentId ?? "");
  const [correlationId, setCorrelationId] = useState(draft.correlationId ?? "");
  const [createdAt, setCreatedAt] = useState(draft.createdAt ?? "");
  const [quantity, setQuantity] = useState(draft.quantity ?? "1");
  const [orderType, setOrderType] = useState<OrderType>(draft.orderType ?? "MARKET");
  const [limitPrice, setLimitPrice] = useState(draft.limitPrice ?? "");
  const environment: ExecutionEnvironment = defaultEnvironment;
  const [timeInForce, setTimeInForce] = useState<TimeInForce>(draft.timeInForce ?? "DAY");
  const [rationale, setRationale] = useState(draft.rationale ?? "");
  const [referencePrice, setReferencePrice] = useState(draft.referencePrice ?? "");
  const [referenceObservedAt, setReferenceObservedAt] = useState(draft.referenceObservedAt ?? "");
  const [cancelOrderId, setCancelOrderId] = useState(draft.cancelOrderId ?? "");
  const [status, setStatus] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [routeStatus, setRouteStatus] = useState<CommandRouteStatus>(() => ({
    routeAvailable: false,
    message: "Order routing is unavailable outside a configured native Risk/OMS host.",
  }));

  useEffect(() => {
    if (!isNativeHost()) {
      setRouteStatus({
        routeAvailable: false,
        message: "This browser view is read-only. Open a configured native desktop host to request a Risk/OMS action.",
      });
      return;
    }
    let active = true;
    void invoke<CommandRouteStatus>("trading_command_status")
      .then((next) => {
        if (active) setRouteStatus(next);
      })
      .catch(() => {
        if (active) setRouteStatus({
          routeAvailable: false,
          message: "The native host did not provide a Risk/OMS command capability; no action can be sent.",
        });
      });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    persistTicketDraft(accountId, environment, {
      accountId,
      instrumentId,
      intentId,
      correlationId,
      createdAt,
      quantity,
      orderType,
      limitPrice,
      environment,
      timeInForce,
      rationale,
      referencePrice,
      referenceObservedAt,
      cancelOrderId,
    });
  }, [accountId, instrumentId, intentId, correlationId, createdAt, quantity, orderType, limitPrice, environment, timeInForce, rationale, referencePrice, referenceObservedAt, cancelOrderId]);

  const currentDraft = (): TicketDraft => ({
    accountId,
    instrumentId,
    intentId,
    correlationId,
    createdAt,
    quantity,
    orderType,
    limitPrice,
    environment,
    timeInForce,
    rationale,
    referencePrice,
    referenceObservedAt,
    cancelOrderId,
  });

  const restoreDraft = (nextAccountId: string, nextDraft: TicketDraft): void => {
    setAccountId(nextAccountId);
    setInstrumentId(nextDraft.instrumentId ?? "");
    setIntentId(nextDraft.intentId ?? "");
    setCorrelationId(nextDraft.correlationId ?? "");
    setCreatedAt(nextDraft.createdAt ?? "");
    setQuantity(nextDraft.quantity ?? "1");
    setOrderType(nextDraft.orderType ?? "MARKET");
    setLimitPrice(nextDraft.limitPrice ?? "");
    setTimeInForce(nextDraft.timeInForce ?? "DAY");
    setRationale(nextDraft.rationale ?? "");
    setReferencePrice(nextDraft.referencePrice ?? "");
    setReferenceObservedAt(nextDraft.referenceObservedAt ?? "");
    setCancelOrderId(nextDraft.cancelOrderId ?? "");
  };

  const handleAccountChange = (nextAccountId: string): void => {
    persistTicketDraft(accountId, environment, currentDraft());
    restoreDraft(nextAccountId, loadTicketDraft(nextAccountId, environment));
    setStatus(null);
  };

  const handleClearDraft = (): void => {
    clearTicketDraft(accountId, environment);
    setInstrumentId("");
    setIntentId("");
    setCorrelationId("");
    setCreatedAt("");
    setQuantity("1");
    setOrderType("MARKET");
    setLimitPrice("");
    setTimeInForce("DAY");
    setRationale("");
    setReferencePrice("");
    setReferenceObservedAt("");
    setCancelOrderId("");
    setStatus("Draft inputs cleared.");
  };

  const handleAutofillMetadata = (): void => {
    const intent = generatedId("intent.desktop");
    const correlation = generatedId("correlation.desktop");
    if (intent === undefined || correlation === undefined) {
      setStatus("Secure identifier generation is unavailable in this host. Enter canonical IDs manually.");
      return;
    }
    if (!intentId.trim()) setIntentId(intent);
    if (!correlationId.trim()) setCorrelationId(correlation);
    if (!createdAt.trim()) setCreatedAt(canonicalTimestamp());
    if (!referenceObservedAt.trim()) setReferenceObservedAt(canonicalTimestamp());
    setStatus("Generated stable canonical IDs and timestamps; existing values were preserved.");
  };

  const handleSubmit = async (side: "BUY" | "SELL"): Promise<void> => {
    if (!routeStatus.routeAvailable || !isNativeHost()) {
      setStatus(routeStatus.message);
      return;
    }
    if (!accountId.trim() || !instrumentId.trim() || !intentId.trim() || !correlationId.trim() || !createdAt.trim() || !quantity.trim() || !rationale.trim()) {
      setStatus("Account, instrument, intent ID, correlation ID, creation time, quantity, and rationale are required before Risk/OMS preflight.");
      return;
    }
    if (orderType === "LIMIT" && !limitPrice.trim()) {
      setStatus("A limit price is required for a LIMIT intent.");
      return;
    }
    if (!referencePrice.trim() || !referenceObservedAt.trim()) {
      setStatus(
        "There is no live market-data feed wired into this desktop host: enter the reference price you are actually observing right now, and when you observed it, before Risk/OMS preflight.",
      );
      return;
    }
    setSubmitting(true);
    setStatus(`Routing ${side} intent to Risk/OMS…`);
    try {
      const requestId = intentId.trim();
      const receipt = await invoke<unknown>("submit_order", {
        intent: {
          intentId: requestId,
          accountId: accountId.trim(),
          strategyId: "desktop.manual",
          instrumentId: instrumentId.trim().toLowerCase(),
          correlationId: correlationId.trim(),
          side,
          quantity: quantity.trim(),
          orderType,
          limitPrice: orderType === "LIMIT" ? limitPrice.trim() : null,
          timeInForce,
          rationale: rationale.trim(),
          createdAt: createdAt.trim(),
          strategyVersion: "desktop.manual.v1",
          configurationVersion: "risk.v1",
          environment,
          parentIntentId: null,
          referencePrice: referencePrice.trim(),
          referenceObservedAt: referenceObservedAt.trim(),
        },
      });
      if (!isSubmitReceipt(receipt, requestId)) {
        throw new Error("The native Risk/OMS route returned a receipt that does not match this submit request.");
      }
      const order = receipt.orderId === null ? "" : ` (${receipt.orderId})`;
      setCancelOrderId(
        receipt.orderId !== null && isCancelableStatus(receipt.status) ? receipt.orderId : "",
      );
      setStatus(`${receipt.status}: ${receipt.message}${order}. The draft remains available until authoritative lifecycle evidence is reviewed.`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  const handleCancel = async (): Promise<void> => {
    if (!routeStatus.routeAvailable || !isNativeHost()) {
      setStatus(routeStatus.message);
      return;
    }
    if (!accountId.trim() || !cancelOrderId.trim()) {
      setStatus("Account ID and OMS order ID are required before cancellation.");
      return;
    }
    const requestId = generatedId("request.cancel.desktop");
    const cancelCorrelationId = generatedId("correlation.cancel.desktop");
    if (requestId === undefined || cancelCorrelationId === undefined) {
      setStatus("Secure identifier generation is unavailable; cancellation was not sent.");
      return;
    }
    setSubmitting(true);
    setStatus("Routing cancellation to OMS…");
    try {
      const receipt = await invoke<unknown>("cancel_order", {
        intent: {
          requestId,
          accountId: accountId.trim(),
          orderId: cancelOrderId.trim(),
          correlationId: cancelCorrelationId,
          environment,
        },
      });
      if (!isCommandReceipt(receipt, "CANCEL_ORDER", requestId)) {
        throw new Error("The native Risk/OMS route returned a receipt that does not match this cancellation request.");
      }
      setStatus(`${receipt.status}: ${receipt.message}.`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  const handleClosePosition = async (): Promise<void> => {
    if (!routeStatus.routeAvailable || !isNativeHost()) {
      setStatus(routeStatus.message);
      return;
    }
    if (!accountId.trim() || !instrumentId.trim() || !rationale.trim() || !referencePrice.trim() || !referenceObservedAt.trim()) {
      setStatus("Account, instrument, rationale, reference price, and observation time are required before closing a position.");
      return;
    }
    const requestId = generatedId("request.close.desktop");
    const closeCorrelationId = generatedId("correlation.close.desktop");
    if (requestId === undefined || closeCorrelationId === undefined) {
      setStatus("Secure identifier generation is unavailable; position close was not sent.");
      return;
    }
    setSubmitting(true);
    setStatus("Routing position close through Risk/OMS…");
    try {
      const receipt = await invoke<unknown>("close_position", {
        intent: {
          requestId,
          accountId: accountId.trim(),
          instrumentId: instrumentId.trim().toLowerCase(),
          correlationId: closeCorrelationId,
          environment,
          rationale: rationale.trim(),
          referencePrice: referencePrice.trim(),
          referenceObservedAt: referenceObservedAt.trim(),
        },
      });
      if (!isCommandReceipt(receipt, "CLOSE_POSITION", requestId)) {
        throw new Error("The native Risk/OMS route returned a receipt that does not match this position-close request.");
      }
      setStatus(`${receipt.status}: ${receipt.message}.`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <section className="f-card f-card--elevated">
      <h3>Order ticket</h3>
      <p>Creates a declarative PAPER request only when the native host reports a configured Risk/OMS route.</p>
      <p className="order-ticket-status" role="status">{routeStatus.message}</p>
      <div className="order-ticket-grid">
        <label>
          Account ID
          <input
            className="f-input"
            value={accountId}
            onChange={(event) => handleAccountChange(event.target.value)}
            placeholder="account.primary"
            required
          />
        </label>
        <label>
          Instrument ID
          <input
            className="f-input"
            value={instrumentId}
            onChange={(event) => setInstrumentId(event.target.value)}
            placeholder="inst.us_equity.aapl"
            required
          />
        </label>
        <label>
          Intent ID
          <input
            className="f-input"
            value={intentId}
            onChange={(event) => setIntentId(event.target.value)}
            placeholder="intent.desktop.001"
            required
          />
        </label>
        <label>
          Correlation ID
          <input
            className="f-input"
            value={correlationId}
            onChange={(event) => setCorrelationId(event.target.value)}
            placeholder="correlation.desktop.001"
            required
          />
        </label>
        <label>
          Created at (UTC)
          <input
            className="f-input"
            value={createdAt}
            onChange={(event) => setCreatedAt(event.target.value)}
            placeholder="2026-09-03T12:30:00Z"
            required
          />
        </label>
        <label>
          Quantity
          <input
            className="f-input"
            inputMode="decimal"
            value={quantity}
            onChange={(event) => setQuantity(event.target.value)}
            required
          />
        </label>
        <label>
          Environment
          <select
            className="f-input"
            value={environment}
            disabled={!routeStatus.routeAvailable}
          >
            <option value="PAPER">PAPER</option>
          </select>
        </label>
        <label>
          Order type
          <select
            className="f-input"
            value={orderType}
            onChange={(event) => setOrderType(event.target.value as OrderType)}
          >
            <option value="MARKET">Market</option>
            <option value="LIMIT">Limit</option>
          </select>
        </label>
        <label>
          Time in force
          <select
            className="f-input"
            value={timeInForce}
            onChange={(event) => setTimeInForce(event.target.value as TimeInForce)}
          >
            <option value="DAY">Day</option>
            <option value="GTC">Good until cancelled</option>
          </select>
        </label>
        <label>
          Limit price
          <input
            className="f-input"
            inputMode="decimal"
            value={limitPrice}
            onChange={(event) => setLimitPrice(event.target.value)}
            disabled={orderType !== "LIMIT"}
            required={orderType === "LIMIT"}
          />
        </label>
        <label>
          Rationale
          <input
            className="f-input"
            value={rationale}
            onChange={(event) => setRationale(event.target.value)}
            placeholder="Operator rationale or signal reference"
            maxLength={1024}
            required
          />
        </label>
        <label>
          Reference price
          <input
            className="f-input"
            inputMode="decimal"
            value={referencePrice}
            onChange={(event) => setReferencePrice(event.target.value)}
            placeholder="Price you are observing right now"
            required
          />
        </label>
        <label>
          Reference observed at (UTC)
          <input
            className="f-input"
            value={referenceObservedAt}
            onChange={(event) => setReferenceObservedAt(event.target.value)}
            placeholder="2026-09-03T12:30:00Z"
            required
          />
        </label>
        <label>
          OMS order ID to cancel
          <input
            className="f-input"
            value={cancelOrderId}
            onChange={(event) => setCancelOrderId(event.target.value)}
            placeholder="order-00000001"
          />
        </label>
      </div>
      <p className="order-ticket-status">
        There is no live market-data feed wired into this desktop host: Risk/OMS evaluates the price collar and
        notional limits against the reference price and time you enter above, not an automatic quote.
      </p>
      <div className="order-ticket-actions">
        <button
          className="f-btn f-btn--primary"
          disabled={submitting || !routeStatus.routeAvailable}
          onClick={() => void handleSubmit("BUY")}
        >
          Buy
        </button>
        <button
          className="f-btn"
          disabled={submitting || !routeStatus.routeAvailable}
          onClick={() => void handleSubmit("SELL")}
        >
          Sell
        </button>
        <button
          className="f-btn f-btn--ghost"
          type="button"
          disabled={submitting}
          onClick={handleAutofillMetadata}
        >
          Generate IDs & Timestamp
        </button>
        <button
          className="f-btn f-btn--ghost"
          type="button"
          disabled={submitting}
          onClick={handleClearDraft}
        >
          Clear Draft
        </button>
        <button
          className="f-btn"
          type="button"
          disabled={submitting || !routeStatus.routeAvailable || !cancelOrderId.trim()}
          onClick={() => void handleCancel()}
        >
          Cancel OMS Order
        </button>
        <button
          className="f-btn f-btn--danger"
          type="button"
          disabled={submitting || !routeStatus.routeAvailable}
          onClick={() => void handleClosePosition()}
        >
          Close Position
        </button>
      </div>
      {status !== null && <p className="order-ticket-status">{status}</p>}
    </section>
  );
}
