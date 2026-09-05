import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

type ExecutionEnvironment = "PAPER" | "LIVE";
type OrderType = "MARKET" | "LIMIT";
type TimeInForce = "DAY" | "GTC";

type CommandReceipt = Readonly<{
  command: string;
  requestId: string;
  status: string;
  orderId: string | null;
  message: string;
}>;

type OrderTicketProps = Readonly<{
  defaultAccountId?: string;
  defaultEnvironment?: ExecutionEnvironment;
}>;

const DRAFT_STORAGE_KEY = "follon:order_ticket_draft";

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
}>;

function loadTicketDraft(): TicketDraft {
  try {
    const raw = sessionStorage.getItem(DRAFT_STORAGE_KEY);
    if (raw !== null) {
      return JSON.parse(raw) as TicketDraft;
    }
  } catch {
    // Fall back to defaults if storage is disabled or corrupted
  }
  return {};
}

function persistTicketDraft(draft: TicketDraft): void {
  try {
    sessionStorage.setItem(DRAFT_STORAGE_KEY, JSON.stringify(draft));
  } catch {
    // Ignore storage quota or access issues
  }
}

function clearTicketDraft(): void {
  try {
    sessionStorage.removeItem(DRAFT_STORAGE_KEY);
  } catch {
    // Ignore
  }
}

export function OrderTicket({
  defaultAccountId = "",
  defaultEnvironment = "PAPER",
}: OrderTicketProps): React.JSX.Element {
  const [draft] = useState<TicketDraft>(() => loadTicketDraft());
  const [accountId, setAccountId] = useState(draft.accountId ?? defaultAccountId);
  const [instrumentId, setInstrumentId] = useState(draft.instrumentId ?? "");
  const [intentId, setIntentId] = useState(draft.intentId ?? "");
  const [correlationId, setCorrelationId] = useState(draft.correlationId ?? "");
  const [createdAt, setCreatedAt] = useState(draft.createdAt ?? "");
  const [quantity, setQuantity] = useState(draft.quantity ?? "1");
  const [orderType, setOrderType] = useState<OrderType>(draft.orderType ?? "MARKET");
  const [limitPrice, setLimitPrice] = useState(draft.limitPrice ?? "");
  const [environment, setEnvironment] = useState<ExecutionEnvironment>(draft.environment ?? defaultEnvironment);
  const [timeInForce, setTimeInForce] = useState<TimeInForce>(draft.timeInForce ?? "DAY");
  const [rationale, setRationale] = useState(draft.rationale ?? "");
  const [status, setStatus] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    persistTicketDraft({
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
    });
  }, [accountId, instrumentId, intentId, correlationId, createdAt, quantity, orderType, limitPrice, environment, timeInForce, rationale]);

  const handleClearDraft = (): void => {
    clearTicketDraft();
    setAccountId(defaultAccountId);
    setInstrumentId("");
    setIntentId("");
    setCorrelationId("");
    setCreatedAt("");
    setQuantity("1");
    setOrderType("MARKET");
    setLimitPrice("");
    setEnvironment(defaultEnvironment);
    setTimeInForce("DAY");
    setRationale("");
    setStatus("Draft inputs cleared.");
  };

  const handleSubmit = async (side: "BUY" | "SELL"): Promise<void> => {
    setSubmitting(true);
    setStatus(`Routing ${side} intent to Risk/OMS…`);
    try {
      const receipt = await invoke<CommandReceipt>("submit_order", {
        intent: {
          intentId: intentId.trim(),
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
        },
      });
      const order = receipt.orderId === null ? "" : ` (${receipt.orderId})`;
      clearTicketDraft();
      setStatus(`${receipt.status}: ${receipt.message}${order}`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <section className="f-card f-card--elevated">
      <h3>Order ticket</h3>
      <p>Submits a declarative request to the configured Risk/OMS route.</p>
      <div className="order-ticket-grid">
        <label>
          Account ID
          <input
            className="f-input"
            value={accountId}
            onChange={(event) => setAccountId(event.target.value)}
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
            onChange={(event) => setEnvironment(event.target.value as ExecutionEnvironment)}
          >
            <option value="PAPER">PAPER</option>
            <option value="LIVE">LIVE</option>
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
      </div>
      <div className="order-ticket-actions">
        <button
          className="f-btn f-btn--primary"
          disabled={submitting}
          onClick={() => void handleSubmit("BUY")}
        >
          Buy
        </button>
        <button
          className="f-btn"
          disabled={submitting}
          onClick={() => void handleSubmit("SELL")}
        >
          Sell
        </button>
        <button
          className="f-btn f-btn--ghost"
          type="button"
          disabled={submitting}
          onClick={handleClearDraft}
        >
          Clear Draft
        </button>
      </div>
      {status !== null && <p className="order-ticket-status">{status}</p>}
    </section>
  );
}
