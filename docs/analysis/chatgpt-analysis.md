**Vishnu, I recommend using NautilusTrader as the trading engine and continuing Follon as the product built around it.** For your commercial goal, I would prioritize this over continuing to develop Follon’s entire execution engine independently.

By “fine-tuning Nautilus,” think **configuring, integrating, extending, and thoroughly testing it**.

Nautilus already provides shared backtest/live components, order lifecycle management, portfolio state, risk checks, and execution reconciliation. These overlap with significant work that Follon still needs to connect and validate. My engineering judgment is that reusing this foundation would let you spend more effort on features customers experience and pay for. [Architecture](https://nautilustrader.io/docs/latest/concepts/architecture/), [Live execution](https://nautilustrader.io/docs/latest/concepts/live/)

The division should be:

| Area                                                                                | Recommended direction                                                              |
| ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| Market-data handling, order lifecycle, execution, and backtest runtime              | Integrate NautilusTrader                                                           |
| Follon desktop, dashboards, onboarding, and customer workflows                      | Keep and improve                                                                   |
| Reproducible research, simulation-versus-broker comparisons, incident investigation | Develop as Follon’s distinctive capabilities                                       |
| Authentication, account permissions, approval policies, and operational controls    | Implement around the actual execution path                                         |
| Existing Follon risk/accounting code                                                | Validate and retain useful functions; remove overlapping authority after migration |

**Nautilus should be the sole owner of executable order state for each migrated account.** Follon can request actions and display results, but two engines must not independently manage the same orders.

There is still meaningful robustness work. Nautilus’s documentation explicitly describes persistence occurring independently of venue transport, and runtime queues that do not automatically apply backpressure. Your integration therefore needs deliberate durable-command handling, overload controls, monitoring, and tested recovery. Installing Nautilus alone does not complete those requirements. [Operational boundaries](https://nautilustrader.io/docs/latest/concepts/live/)

I would also **start with a pinned upstream version and small extensions**. A heavily modified private fork creates continuing work to merge fixes and upgrades. Modify the engine only when a measured requirement cannot be met through its supported interfaces.

The next step should be a **ten-working-day PAPER integration trial**:

1. Connect one Follon workflow to Nautilus using one supported broker and strategy.
2. Demonstrate submission, rejection, partial fills, cancellation, and position reporting.
3. Test duplicate requests, disconnects, process restarts, stale data, and uncertain order outcomes.
4. Compare the resulting orders, positions, cash, and evidence against expected results.
5. Migrate that workflow if it passes and demonstrably reduces the remaining implementation work.

Keep the existing Follon path available during this trial. That preserves your investment while making the engine decision concrete.

**Your development advantage should come from making trading research and operations easier to understand, reproduce, and control.** Nautilus can supply much of the execution foundation; Follon can supply the integrated experience, evidence, and customer value that make the finished product worth buying.
**Follon is worth building further. The current version is a substantial engineering alpha, but I would not sell it as a finished live-trading product yet.** Its strongest commercial opportunity is a workstation for reproducible strategy research, explainable risk, and broker reconciliation.

I completed the repository review and drafted a **21-page product and engineering blueprint**:

[](sandbox:/workspace/scratch/399d623232be/Follon_Product_Assessment_and_Engineering_Blueprint.docx)

It contains source-linked findings, competitor mapping, proposed architecture, twelve implementation tickets with acceptance criteria, release gates, pricing experiments, and a twelve-week delivery plan.

**What the implementation actually supports**

I reviewed commit `cb710e7`. Locally, **46 Python tests and 10 desktop check scripts passed**, along with TypeScript checks and desktop web builds. Rust verification relied on the repository’s CI because Cargo was unavailable locally.

The Rust core, fixed-point types, order lifecycle, event-journal primitives, Python SDK, and desktop evidence views provide a useful foundation. However, several gaps directly affect whether customers can trust the product:

| Finding                                                                                                                                                  | Commercial consequence                                                                                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The native desktop initializes an unavailable trading gateway; its command boundary currently accepts PAPER only.                                        | Visible trading controls do not establish a connected execution product. [Source](https://github.com/Vishnums3107/Follon/blob/cb710e763d3d7d9fb282fb60db9928d6372b9284/apps/desktop/src-tauri/src/trading.rs)                 |
| CI for the reviewed revision fails PostgreSQL integration and native desktop setup.                                                                      | Persistence and native packaging need repair before a supported release. [CI run](https://github.com/Vishnums3107/Follon/actions/runs/34505586766)                                                                            |
| The generic portfolio-risk evaluator aggregates native-currency exposures and appends candidate exposure without calculating the resulting net position. | Cross-currency portfolios, closing trades, and concentration calculations need corrected semantics. [Source](https://github.com/Vishnums3107/Follon/blob/cb710e763d3d7d9fb282fb60db9928d6372b9284/core/risk/src/lib.rs)       |
| The adversarial evaluator accepts caller-supplied pass flags.                                                                                            | A “passed” result does not itself prove the required robustness experiments ran successfully. [Source](https://github.com/Vishnums3107/Follon/blob/cb710e763d3d7d9fb282fb60db9928d6372b9284/core/backtest/src/adversarial.rs) |

For example, a $10,000 position and a €10,000 position cannot simply become “20,000” of portfolio exposure. Similarly, selling an existing position needs separate calculations for the resulting position and the exposure while orders remain unfilled. These are priority correctness issues.

**The product direction I recommend**

Make Follon exceptionally good at answering: **“Why did this strategy make this decision, what risk did it create, and why did the broker result differ from the simulation?”**

Build that around five capabilities:

1. **Reproducible runs:** preserve the exact strategy, dataset, configuration, and software versions.
2. **Simulation-to-broker comparison:** explain differences in fills, fees, slippage, and positions.
3. **Correct pre-trade risk:** include currencies, pending orders, reservations, and projected positions.
4. **Incident replay:** reconstruct disconnects, retries, partial fills, and recovery.
5. **Evidence-based readiness:** derive release verdicts from executed tests and observed sessions.

For existing systems, **NautilusTrader is the closest architectural reference** because of its shared research/live components. **LEAN is a useful reference for realistic simulation models**, including fees, fills, slippage, and buying power. The blueprint maps these patterns onto Follon’s existing modules. [NautilusTrader architecture](https://nautilustrader.io/docs/latest/concepts/architecture/), [LEAN reality models](https://www.quantconnect.com/docs/v2/writing-algorithms/reality-modeling/key-concepts)

I recommend preserving Follon’s core and running a bounded upstream-engine comparison before deciding what to reuse.

**The commercial path**

Start with a proposed **$1,000 assisted research/reconciliation pilot**, after fixing its required workflow: one strategy, one dataset, one supported broker statement, and a reproducible report. Use paid delivery and renewal evidence to establish subscription pricing.

The report separately addresses **$1,000–$1 million account sizes** and a potential **$1 million annual-revenue business**. For illustration, 40 teams at $1,800/month plus 60 individuals at $199/month equals **$1,007,280 ARR**; those are pricing assumptions to validate.

The next milestone is concrete: **a signed, installable research/PAPER release that reproduces a customer’s run, reconciles supported broker evidence, and passes the documented correctness and recovery gates.**
