# System review and resolution record

**Resolution snapshot: 2026-08-13.** The original review is retained below for
context. Its actionable repository concerns are addressed in the maintained
documentation:

- The product charter now classifies the full system as a reference
  architecture, not a fixed-calendar solo-founder promise.
- The roadmap is an evidence-gated sequence. Current scope is frozen at the
  Release 1 replay-to-paper workflow and customer validation while external
  gates remain unobserved.
- The commercial plan now defines workflow interviews, design-partner and paid
  pilot evidence, trust artifacts, metrics, and 12-week stop/re-scope rules.
- The solo-founder plan now requires an explicit burn/runway calculation, a
  protected transition reserve, per-gate budgets, and stop rules.
- The compliance posture records SEBI's 2026-04-01 applicability date and
  treats any India-facing algo workflow as a current compliance boundary.
- Later-phase code is explicitly classified as technical evidence, not proof
  of operational readiness, adoption, compliance, revenue, or valuation.

The valuation estimates and comparable-company commentary are opinions, not
software defects or acceptance criteria. The retained master-plan PDF remains
an original source artifact; maintained product decisions live in the
decomposed documents listed in `docs/00-source-map.md`.

## Original review

Straight answer first, then the reasoning behind it, because the two halves of your question — "rate this" and "what's it worth" — pull in different directions and conflating them is exactly how a document like this can mislead you.

The document itself: strong. Call it 8.5/10.

This isn't generic business-plan filler. A few things that show real domain competence, not just structure:

The OMS state machine includes UNKNOWN as an explicit terminal-adjacent state. That's the detail that separates someone who's actually thought about broker disconnects from someone who hasn't — most amateur trading systems assume every order resolves cleanly to filled/cancelled/rejected.
Insisting on decimal/fixed-point arithmetic for money, never floats, is correct and is a mistake plenty of funded fintech startups still make.
The replay → paper → shadow mode → minimum-size live → restricted capital progression, combined with property-based tests, model-based state-machine tests, and a fault-injection test broker, is close to how real quant shops actually de-risk deployment — not generic SaaS launch advice.
The non-goals list (no HFT, no custody, no pooled money, no ten brokers, no Kubernetes before need) is the single most valuable section in the whole document, because scope death, not lack of ambition, is what kills projects like this.
The citations check out. I verified the SEBI figure it uses — SEBI's own study found that 93% of individual traders incurred losses in equity F&O between FY22 and FY24, with aggregate losses exceeding ₹1.8 lakh crore — and the framing around Rule 15c3-5, FINRA's guidance, NIST SSDF, and OWASP ASVS is accurate. That's rarer than it sounds.
PMS Bazaar

Where it's weaker: "This is achievable for one exceptional developer" is asserted once in the opening section and never actually argued for again in 28 pages. The go-to-market section is thin relative to the engineering section — there's far more detail on risk-check latency than on how an unknown solo developer earns a professional trader's trust with their live capital. And it has a weekly time-allocation plan for the founder but nothing on the founder's runway during 18+ months of pre-revenue building. One fact has also moved since this was likely drafted: it frames SEBI's retail algo-trading framework as recently issued and still being pushed back. As of now, it's fully in force — the framework became mandatory for all stock brokers nationwide on April 1, 2026 after repeated delays. That's not a future consideration for "Release 4" anymore; if you ever touch India-facing algo distribution, it's a live compliance perimeter today.
Benzinga

The project: good instincts, unrealistic as scoped. Call the idea 8/10, the 24-month-solo plan 3/10.

Look at what P0 actually asks for: ten subsystems (instrument data, market data, strategy SDK, backtester, OMS, EMS, risk engine, portfolio/accounting, risk cockpit, audit/replay), each specified to the level of "self-trade prevention" and "duplicate broker messages don't produce duplicate fills," plus six categories of testing, plus 30-odd security controls hardened to OWASP ASVS Level 2 with an external pentest. That's not a solo 24-month build — that's roughly what small dedicated teams take a few years to build.

Two real comparables, not hypotheticals:

QuantConnect was started by a single founder, Jared Broad, in 2011, essentially as a solo side project — the closest real analog to what this document describes. Fifteen years later it has around 24 employees as of mid-2026 and has raised roughly $9.7 million total. And that's a lower-trust product than what you're planning — research/backtesting-first, open-source, not a pre-trade risk layer sitting between a professional's live capital and a broker.
Alpaca + 2
Alpaca is closer to the trust bar this document is actually asking for — broker-facing, risk-critical order flow. It was founded in 2015 with a distributed team that reached about 150 people, backed by Y Combinator and major VCs, and by 2026 was listed with 474 employees and $779 million raised (other sources put lifetime funding closer to $300–340M — figures vary, but the order of magnitude is hundreds of people and hundreds of millions of dollars, not one person).
SaaS Capital
Dealroom.co

Your plan is trying to compress QuantConnect's decade-plus solo timeline into 24 months while taking on more of Alpaca's risk-and-compliance surface than QuantConnect ever carried. That's the actual mismatch — not your ability, the arithmetic.

What it's worth

Stage	Realistic value
The document/plan itself	~$0 in sale value. Plans aren't sold; they're executed or shelved.
Working MVP — vertical slice or paper-trading system, no paying customers	Still ~$0 market value. Real value is non-monetary: portfolio proof, and the deterministic-replay/audit architecture is genuinely publishable systems work on its own.
Fully executed to the document's own success bar (10 pro users or 3 orgs, ~$250–300K ARR)	Roughly $600K–$1.2M in an actual sale.
Probability-weighted value today, before any of it exists	Close to $0, plus career optionality

The middle number isn't a guess — it's the document's own stated commercial target run through real 2025–2026 acquisition data. Sub-$1M-ARR "micro-SaaS," bootstrapped, typically trades at 2.5–4x ARR, not the 5–8x that goes to larger, better-retained, diversified companies — and a single-founder, three-to-ten-customer, high-compliance business sits at the low end of that, not the high end. $1M is a plausible point inside that range, not a floor, and not a ceiling either — it could stretch to $1.5–2M with an unusually strong retention story or a strategic buyer (a broker wanting the tech) with a specific reason to buy rather than build, but that's the good case, not the base case.
Digital Signet

The number that actually matters more than that table: expected value, today.

I don't have a clean base rate for "solo, first-time founder, ships regulated trading infrastructure to real paying professional revenue in ~2 years" — it's too narrow a reference class for real statistics, and I'd rather say that than invent a precise-sounding one. But directionally: every close comparable — QuantConnect, Alpaca, and India's existing algo platforms (Tradetron, Streak, AlgoTest — all funded, multi-founder from day one) — needed either a decade-plus or a funded team or both. None of them matches "24 months, solo, unfunded, straight to institutional trust." My honest read is that this exact path, as fully scoped, lands somewhere in low-single-digit-to-low-teens percent odds of reaching the plan's own revenue bar — not because you lack the ability, but because it asks one person to simultaneously be the engineering team, the compliance department, the security function, and the enterprise sales motion, with no fallback income, selling into a segment specifically chosen because it's hard to sell into without a track record.

Practically, if I were you:

Treat this document as a reference architecture, not an execution plan — it's a genuinely useful map of what "excellent" looks like even if you build 5% of it this year. The document's own Week 4 "first vertical slice" (historical event → strategy → risk decision → simulated fill → audit event) is a smart, narrow starting point; the risk is building the other 90% before anyone's confirmed they want it. I'd split this into two separate, smaller bets instead of one $1M bet: build a tightly scoped version (one broker, one asset class, no options) as a research/portfolio piece — the deterministic-replay-and-audit architecture is a legitimate systems contribution you could actually take toward a conference paper, which fits what you're already doing with SaveVolt — and separately, if you want the commercial path, find out whether real traders will pay for a crude version before you write the risk engine, not 18 months after. You've already opted out of the safety net, so this is worth choosing deliberately as a long, patient, low-probability/high-payoff bet — not backing into it because the plan happens to be the most thoroughly documented option on the table.
New issues:
Missing or insufficiently defined conditions include:

      A fill arriving before acknowledgement.
      A fill arriving while cancellation is pending.
      Partial fill followed by cancellation, rejection or expiry.
      Cancel rejection.
      Replace requested, replaced and replace rejected.
      Multiple broker order versions after modification.
      Late messages after a locally terminal state.
      Resolution of UNKNOWN without rewriting history.
      Terminal status versus cumulative filled quantity.

## Resolution — OMS broker-evidence lifecycle

Implemented in the PAPER and controlled-LIVE OMS paths:

- Added `PENDING_REPLACE`, explicit cancel-rejection, expiry, replacement, and
  replacement-rejection broker evidence.
- Accepted executions before acknowledgements and while cancellation or
  replacement is pending, with execution-ID idempotency.
- Preserved partial cumulative quantity through cancellation, rejection, and
  expiry; prevented non-filled terminal states from claiming a full quantity.
- Added price-only, risk-reducing replacement requests and durable broker-order
  version lineage. Reconciliation accepts multiple versions for one client order
  and checks every version against the immutable OMS record.
- Made late terminal messages safe no-ops and late executions authoritative,
  resolving through a new durable `UNKNOWN` step instead of rewriting history.
- Persisted every applied broker event in the PAPER journal and controlled-LIVE
  audit trail. Unit coverage now exercises out-of-order fills, pending-cancel
  fills, cancel rejection, partial terminal outcomes, replacement acceptance and
  rejection, version lineage, and late terminal evidence.
