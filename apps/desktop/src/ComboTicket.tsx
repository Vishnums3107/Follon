import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  COMBO_MAX_LEGS,
  COMBO_MIN_LEGS,
  buildComboPayload,
  emptyLeg,
  isCancelableComboStatus,
  isComboReceipt,
  netPrice,
  type ComboLegDraft,
  type ComboPriceLimitKind,
  type ComboSide,
  type ComboTimeInForce,
} from "./combo-intent.js";

type CommandRouteStatus = Readonly<{
  routeAvailable: boolean;
  message: string;
}>;

type ComboTicketProps = Readonly<{
  defaultAccountId?: string;
}>;

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

function isCancelReceipt(value: unknown, requestId: string): value is Readonly<{ status: string; message: string }> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const receipt = value as Record<string, unknown>;
  return receipt.command === "CANCEL_ORDER" && receipt.requestId === requestId &&
    typeof receipt.status === "string" && typeof receipt.message === "string" && receipt.message.length > 0;
}

/**
 * Operator ticket for one atomic multi-leg PAPER combination.
 *
 * It submits exactly one `submit_combo_order` request, which the native host
 * routes as one OMS order. Legs are never sent as separate orders, and no leg
 * observation is ever filled in: each leg carries the price the operator is
 * actually observing for it.
 */
export function ComboTicket({ defaultAccountId = "" }: ComboTicketProps): React.JSX.Element {
  const [accountId, setAccountId] = useState(defaultAccountId);
  const [intentId, setIntentId] = useState("");
  const [correlationId, setCorrelationId] = useState("");
  const [createdAt, setCreatedAt] = useState("");
  const [comboQuantity, setComboQuantity] = useState("1");
  const [priceLimitKind, setPriceLimitKind] = useState<ComboPriceLimitKind>("MAXIMUM_DEBIT");
  const [priceLimit, setPriceLimit] = useState("");
  const [timeInForce, setTimeInForce] = useState<ComboTimeInForce>("DAY");
  const [rationale, setRationale] = useState("");
  const [legs, setLegs] = useState<readonly ComboLegDraft[]>(() => [emptyLeg("BUY"), emptyLeg("SELL")]);
  const [workingOrderId, setWorkingOrderId] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [routeStatus, setRouteStatus] = useState<CommandRouteStatus>(() => ({
    routeAvailable: false,
    message: "Combination routing is unavailable outside a configured native Risk/OMS host.",
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

  const updateLeg = (index: number, patch: Partial<ComboLegDraft>): void => {
    setLegs((current) => current.map((leg, position) => (position === index ? { ...leg, ...patch } : leg)));
  };

  const handleGenerateMetadata = (): void => {
    const intent = generatedId("intent.desktop.combo");
    const correlation = generatedId("correlation.desktop.combo");
    if (intent === undefined || correlation === undefined) {
      setStatus("Secure identifier generation is unavailable in this host. Enter canonical IDs manually.");
      return;
    }
    if (!intentId.trim()) setIntentId(intent);
    if (!correlationId.trim()) setCorrelationId(correlation);
    if (!createdAt.trim()) setCreatedAt(canonicalTimestamp());
    // Only a leg whose observed price has been entered is stamped; an
    // unpriced leg stays unpriced.
    const now = canonicalTimestamp();
    setLegs((current) => current.map((leg) => (
      leg.referencePrice.trim() && !leg.referenceObservedAt.trim() ? { ...leg, referenceObservedAt: now } : leg
    )));
    setStatus("Generated canonical IDs and stamped observation times for priced legs; existing values were preserved.");
  };

  const handleSubmit = async (): Promise<void> => {
    if (!routeStatus.routeAvailable || !isNativeHost()) {
      setStatus(routeStatus.message);
      return;
    }
    const built = buildComboPayload({
      accountId, intentId, correlationId, createdAt, comboQuantity, priceLimitKind, priceLimit, timeInForce, rationale, legs,
    });
    if (!built.ok) {
      setStatus(built.error);
      return;
    }
    setSubmitting(true);
    setStatus("Routing the combination to Risk/OMS as one atomic order…");
    try {
      const requestId = built.payload.intentId;
      const receipt = await invoke<unknown>("submit_combo_order", { intent: built.payload });
      if (!isComboReceipt(receipt, requestId)) {
        throw new Error("The native Risk/OMS route returned a receipt that does not match this combination request.");
      }
      setWorkingOrderId(receipt.orderId !== null && isCancelableComboStatus(receipt.status) ? receipt.orderId : "");
      const order = receipt.orderId === null ? "" : ` (${receipt.orderId})`;
      setStatus(`${receipt.status}: ${receipt.message}${order}. The draft remains available until authoritative lifecycle evidence is reviewed.`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  const handleCancel = async (): Promise<void> => {
    if (!routeStatus.routeAvailable || !isNativeHost() || !workingOrderId) {
      setStatus(routeStatus.routeAvailable ? "There is no working combination to cancel." : routeStatus.message);
      return;
    }
    const requestId = generatedId("request.cancel.desktop.combo");
    const cancelCorrelationId = generatedId("correlation.cancel.desktop.combo");
    if (requestId === undefined || cancelCorrelationId === undefined) {
      setStatus("Secure identifier generation is unavailable; cancellation was not sent.");
      return;
    }
    setSubmitting(true);
    setStatus("Routing combination cancellation to OMS…");
    try {
      const receipt = await invoke<unknown>("cancel_order", {
        intent: {
          requestId,
          accountId: accountId.trim(),
          orderId: workingOrderId,
          correlationId: cancelCorrelationId,
          environment: "PAPER",
        },
      });
      if (!isCancelReceipt(receipt, requestId)) {
        throw new Error("The native Risk/OMS route returned a receipt that does not match this cancellation request.");
      }
      if (!isCancelableComboStatus(receipt.status)) setWorkingOrderId("");
      setStatus(`${receipt.status}: ${receipt.message}.`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  };

  const protectedNet = netPrice(legs, "limitPrice");
  const referenceNet = netPrice(legs, "referencePrice");

  return (
    <section className="f-card f-card--elevated">
      <h3>Combination ticket</h3>
      <p>
        Creates one declarative PAPER combination. Every leg executes atomically or none does; the host routes it as a
        single Risk/OMS order and never splits it into separately routed legs.
      </p>
      <p className="order-ticket-status" role="status">{routeStatus.message}</p>
      <div className="order-ticket-grid">
        <label>
          Account ID
          <input className="f-input" value={accountId} onChange={(event) => setAccountId(event.target.value)} placeholder="account.primary" required />
        </label>
        <label>
          Intent ID
          <input className="f-input" value={intentId} onChange={(event) => setIntentId(event.target.value)} placeholder="intent.desktop.combo.001" required />
        </label>
        <label>
          Correlation ID
          <input className="f-input" value={correlationId} onChange={(event) => setCorrelationId(event.target.value)} placeholder="correlation.desktop.combo.001" required />
        </label>
        <label>
          Created at (UTC)
          <input className="f-input" value={createdAt} onChange={(event) => setCreatedAt(event.target.value)} placeholder="2026-09-03T12:30:00Z" required />
        </label>
        <label>
          Units
          <input className="f-input" inputMode="numeric" value={comboQuantity} onChange={(event) => setComboQuantity(event.target.value)} required />
        </label>
        <label>
          Net price protection
          <select className="f-input" value={priceLimitKind} onChange={(event) => setPriceLimitKind(event.target.value as ComboPriceLimitKind)}>
            <option value="MAXIMUM_DEBIT">Maximum debit</option>
            <option value="MINIMUM_CREDIT">Minimum credit</option>
          </select>
        </label>
        <label>
          Net price limit (per unit)
          <input className="f-input" inputMode="decimal" value={priceLimit} onChange={(event) => setPriceLimit(event.target.value)} required />
        </label>
        <label>
          Time in force
          <select className="f-input" value={timeInForce} onChange={(event) => setTimeInForce(event.target.value as ComboTimeInForce)}>
            <option value="DAY">Day</option>
            <option value="GTC">Good until cancelled</option>
          </select>
        </label>
        <label>
          Rationale
          <input className="f-input" value={rationale} onChange={(event) => setRationale(event.target.value)} placeholder="Operator rationale or signal reference" maxLength={1024} required />
        </label>
      </div>
      <table className="f-table">
        <thead>
          <tr>
            <th>Leg</th><th>Instrument ID</th><th>Side</th><th>Ratio</th><th>Limit price</th>
            <th>Observed price</th><th>Observed at (UTC)</th><th />
          </tr>
        </thead>
        <tbody>
          {legs.map((leg, index) => (
            <tr key={index}>
              <td data-label="Leg">{index + 1}</td>
              <td data-label="Instrument ID">
                <input className="f-input" value={leg.instrumentId} onChange={(event) => updateLeg(index, { instrumentId: event.target.value })} placeholder="inst.opt.spy.c500" />
              </td>
              <td data-label="Side">
                <select className="f-input" value={leg.side} onChange={(event) => updateLeg(index, { side: event.target.value as ComboSide })}>
                  <option value="BUY">Buy</option>
                  <option value="SELL">Sell</option>
                </select>
              </td>
              <td data-label="Ratio">
                <input className="f-input" inputMode="numeric" value={leg.ratio} onChange={(event) => updateLeg(index, { ratio: event.target.value })} />
              </td>
              <td data-label="Limit price">
                <input className="f-input" inputMode="decimal" value={leg.limitPrice} onChange={(event) => updateLeg(index, { limitPrice: event.target.value })} />
              </td>
              <td data-label="Observed price">
                <input className="f-input" inputMode="decimal" value={leg.referencePrice} onChange={(event) => updateLeg(index, { referencePrice: event.target.value })} placeholder="Price you observe now" />
              </td>
              <td data-label="Observed at (UTC)">
                <input className="f-input" value={leg.referenceObservedAt} onChange={(event) => updateLeg(index, { referenceObservedAt: event.target.value })} placeholder="2026-09-03T12:30:00Z" />
              </td>
              <td data-label="">
                <button
                  className="f-btn f-btn--ghost"
                  type="button"
                  disabled={submitting || legs.length <= COMBO_MIN_LEGS}
                  onClick={() => setLegs((current) => current.filter((_, position) => position !== index))}
                >
                  Remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="order-ticket-status">
        Protected net from leg limits: {protectedNet ?? "incomplete"} · Net at observed prices: {referenceNet ?? "incomplete"}
        {" "}(positive is a debit, negative a credit). There is no live market-data feed wired into this desktop host:
        Risk/OMS collars each leg against the price you observed for that leg.
      </p>
      <div className="order-ticket-actions">
        <button className="f-btn f-btn--primary" disabled={submitting || !routeStatus.routeAvailable} onClick={() => void handleSubmit()}>
          Submit Combination
        </button>
        <button
          className="f-btn f-btn--ghost"
          type="button"
          disabled={submitting || legs.length >= COMBO_MAX_LEGS}
          onClick={() => setLegs((current) => [...current, emptyLeg("BUY")])}
        >
          Add Leg
        </button>
        <button className="f-btn f-btn--ghost" type="button" disabled={submitting} onClick={handleGenerateMetadata}>
          Generate IDs & Timestamps
        </button>
        <button
          className="f-btn"
          type="button"
          disabled={submitting || !routeStatus.routeAvailable || !workingOrderId}
          onClick={() => void handleCancel()}
        >
          Cancel Combination
        </button>
      </div>
      {status !== null && <p className="order-ticket-status">{status}</p>}
    </section>
  );
}
