# Follon Repository Capability and Readiness Assessment

Revision 2 prepared for the project owner by Codex on 27 September 2026

## 1 What the project can do today

**Follon is usable for local trading research, replay of tested inputs, and simulated PAPER order operations. It is not currently an integrated broker-connected trading product.** The desktop, gRPC PAPER route, and PAPER CLI compose local model adapters. A real IBKR PAPER bridge exists, but connecting it to those applications requires implementation work. Controlled LIVE has safety and adapter interfaces without a concrete vendor transport.

The strongest demonstrated workflow runs supplied historical bars through strategy, risk, simulated execution, accounting, and attributable result artifacts. The native desktop can submit single orders and combinations, cancel orders, and close positions through the in-process PAPER Risk/OMS route. Its fills use operator-supplied prices rather than a live market feed. Browser evidence views remain read-only.

| Question | Evidence-based answer |
| --- | --- |
| Can it backtest a strategy? | Yes, through a funded, long-only primary runner with supported built-in or trusted Python strategy code. Strategy quality and simulation realism require separate validation. |
| Can it place orders from the desktop? | Yes, in the configured local PAPER model. This does not transmit them to IBKR. |
| Can the supplied apps trade at a real broker? | No such composition is implemented. The PAPER bridge is a library/process component with a narrower single-order interface. |
| Can it trade live capital? | No operational vendor transport or retained live acceptance establishes that capability. |
| Does it contain a profitable strategy? | No verified edge is established. The example is a functional demonstration. |
| Is it ready as a production service? | Release verification, integration, operating proof, user validation, permissions, and independent security remain incomplete. |
| What is it worth? | Sale price and company valuation are undetermined. The earlier replacement-cost range is withdrawn as a project estimate. |

The reviewed revision is **4adb0909430d723c800fb1a2fd4bddb8aaddd792**, the merge of PR 30. Remote main still points to it. The existing seven-suite local measurement passed, while the latest main foundation CI run passed three jobs and failed three. This revision adds focused source reviews, 88 passing test invocations, and an acceptance-boundary experiment, explained in Sections 9 and 11.

The next step is one reliable research-to-broker-PAPER workflow followed by retained operating evidence. A larger feature list does not resolve the integration and assurance gaps.

<!-- PAGE -->

## 2 Corrections from the first assessment

This revision supersedes the first report. These are documentation corrections; the implementation gaps have not been fixed by this update.

| Corrected claim | What source inspection establishes |
| --- | --- |
| Broker PAPER is a current app configuration option | Desktop, API, and CLI use models. Connecting the real bridge requires application integration. |
| Python strategies cannot access adapters or credentials | Their protocol does not supply them. Arbitrary Python still has the operating-system user's filesystem/network access. A child process is not a security sandbox. |
| Advanced accounting enables short or leveraged strategy backtests | The primary runner is funded and long-only. Advanced accounting is a separate model attached after the main run succeeds. |
| Cross-instrument decisions merely need suitable marks | An intent for another instrument is expressly rejected. Separate instrument valuation is a different capability. |
| Replay never models price improvement | The OHLC limit model can fill better than a limit. Venue-specific improvement, midpoint rules, and queue behavior are absent. |
| All journals reject symbolic links | PAPER/LIVE opens and specific admin paths do. Operations journals lack the same guard; path-swap races remain. |
| Acceptance tools prove independent review and clean sessions | They check declared strings, internal hashes, and accepted-subject counts. They do not authenticate reviewers or inspect broker evidence. |
| 29 advanced categories lack any computation anywhere | This pipeline computes two of the 32 advanced artifact contracts. It does not publish real computed artifacts for the other 30. Related algorithms may exist. |
| USD 106,000 to 345,000 is a supported rebuild estimate | It came from unvalidated 6-to-18 engineer-month assumptions. Code size and wage statistics cannot establish project effort or value. |

Backtest artifacts contain a **declared engine version**, not a measured complete engine-build digest. A normal backtest seed is provenance metadata; it does not automatically seed custom Python code. Passing tests prove named cases, not every trading state or hostile-input scenario.

Section 8 explains a checked accounting difference that is intentional: the basic ledger includes buy fees in cost basis, while the advanced account reports trading P&L before separately attributed charges. Their net equity agrees in the demonstration. This is not an identified accounting defect.

<!-- PAGE -->

## 3 Scope and repository structure

The review checked tracked-file inventory, source implementations, application composition, contracts, named tests, stored scans, and GitHub job evidence. Additional read-only reviews checked research, execution, and readiness claims within this engineering task. They are not an external security certification or a line-by-line audit of every file.

| Inventory at the reviewed commit | Count or observation |
| --- | --- |
| Git-tracked files | 511 |
| Root Rust workspace | 21 members, including 17 core modules |
| Native desktop | Separate Rust workspace using Tauri v2 |
| Operator CLI programs | 13 binary targets |
| JSON Schema files | 77 |
| Rust source | 86 files, 81,411 lines |
| Python source | 42 files, 10,842 lines |
| TypeScript and TSX | 12 files, 10,570 lines |
| React navigation | 12 workspaces |
| GitHub repository | Public; 0 stars and 0 forks when rechecked |

Counts include tests, comments, and blank lines. They measure maintenance surface, not quality, work hours, or worth. The report is uncommitted and is not included in the tracked-file count.

Rust owns domain values, risk, execution, accounting, replay, PAPER, LIVE, evidence, and identity primitives. Python supplies strategy/storage workflows and the official IBKR PAPER bridge. React/TypeScript renders views and tickets; Tauri routes native commands. PostgreSQL has a persistence adapter and migrations. The gRPC service exposes selected planning and command functions rather than a complete broker deployment.

The intended flow is **market input to strategy intent to risk to OMS to execution to accounting to evidence to UI**. In supplied runnable applications, execution uses model adapters. A supported strategy submits a declarative intent; its protocol does not grant a broker handle.

The active admitted release scope is US-equities replay-to-PAPER. Broader FX, options, margin, commercial, and controlled-LIVE code must be assessed at its actual layer. An implemented model or schema does not add an operational asset class, venue, or customer service.

Sources: [workspace](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/Cargo.toml#L1), [CLI targets](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/cli/Cargo.toml#L35), [React navigation](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/desktop/src/app-shell.tsx#L9), and [repository guide](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/docs/02-architecture/03-repository-guide.md).

<!-- PAGE -->

## 4 Research and replay capabilities

| Capability | Usable behavior and precise limit |
| --- | --- |
| Historical data | Import supplied canonical CSV bars, build bars from normalized trades, and validate ordering, sessions, and reference data. No operational live-data acquisition service is established. |
| Quote repair | Repair interior sequence gaps from supplied recovery records, refuse contradictions, and retain residual gaps/hashes. It does not request vendor replay or record a live quote stream. |
| Strategy execution | Built-in example or trusted Python worker. Each bar callback may return an intent only for its bar's instrument; another-instrument intent is rejected. |
| Primary account | Funded and long-only. The primary ledger refuses insufficient cash; both portfolio and ledger refuse sales exceeding held quantity. |
| Multiple instruments | Interleaved data and separate latest observed closing marks. No synchronized, freshness-checked cross-instrument decision facility. |
| Risk and fills | Configured quantity/notional, price, tick/lot, calendar, and kill-switch checks; model latency, partial fills, spread, slippage, and fees. |
| Outputs | Result JSON, NDJSON events, Markdown report, and completion manifest, with input/output hashes and declared provenance. |
| Perturbation research | Implemented counterfactual/adversarial replay modes. This does not cover every custom strategy or market scenario. |

Bar-created orders cannot fill on their generating bar. Later eligible OHLC bars can yield better-than-limit prices. Fills round onto the effective tick grid against the trader before limit checks; grid changes that invalidate working orders stop replay. Queue priority, venue depth, midpoint execution, and actual venue responses to changed increments are absent.

Split/dividend ledger calculations exist. Source inspection identified a parity question: replay/worker position snapshots update from executions, while the strategy interface has no corporate-action update hook. Split-aware behavior across worker state, replay orders, and PAPER accounting requires a focused test. This is a source-inspected gap, not a newly reproduced unsafe outcome.

Artifacts record configuration bytes, normalized data, strategy identity, declared engine version, normal seed metadata, and output hashes. Engine version is a supplied string checked for consistency. Full binary/build provenance requires separately retained build/release evidence.

Sources: [cross-instrument refusal](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/control-plane/src/lib.rs#L2517), [primary ledger](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/backtest/src/lib.rs#L338), [fill model](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/control-plane/src/lib.rs#L2042), [action/mark handling](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/backtest/src/lib.rs#L2240), and [specification construction](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/cli/src/backtest.rs#L444).

<!-- PAGE -->

## 5 Python strategies and portable evidence

The Python worker uses a versioned standard-input/output protocol. It receives normalized market events, identity, explicit replay time, and optionally bounded historical/portfolio/state/metrics services. The parent validates returned intents/context before risk. The protocol supplies no broker credentials or adapter handles.

**Only trusted strategy code should run in the current worker.** The parent launches a normal same-user child process, clears inherited environment, and can set an import root and working directory. It does not deny filesystem/network access or constrain operating-system privileges. Loading a strategy executes arbitrary module code. Worker frame reads lack a timeout and pre-read size cap, leaving hang/resource-exhaustion concerns.

Custom Python may read wall-clock time, randomness, files, or network responses. Normal backtest seed is fingerprint metadata, not an automatic seed in the callback. Implemented perturbations use separate scenario seeds. Fixture reproducibility does not establish determinism for arbitrary user strategies.

| Evidence workflow | Implemented behavior | Remaining qualification |
| --- | --- | --- |
| Bundle identity | Hash declared strategy/SDK sources and runtime identity; reject mismatched announced identity. | Does not prevent undeclared external inputs or hostile execution. |
| Decision reconstruction | Bind persisted event lines to manifest journal hash/configuration; report missing ancestors. | Explains retained chains rather than independently verifying actual market history. |
| Capsule | Package/lock sources, extract, replay, compare, and seal a manifest. | External bars and matching Python required; target is implementation/version/platform. |
| Signature | Verify detached Ed25519 signature against a trusted key. | Local key generation demonstrates cryptography, not external signer identity/custody. |

Capsule replay has no corporate-action input route. Controlled imports and Python `-S` limit dependency resolution; they are not an operating-system sandbox. Test exact-runtime, cross-machine reproduction before advertising broader portability.

Sources: [worker launch/frame reads](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/control-plane/src/lib.rs#L891), [module execution](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/python/strategy-sdk/src/follon_strategy_sdk/worker.py#L440), [reconstruction bindings](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/cli/src/operations.rs#L780), and [capsule constraints](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/cli/README.md#L165).

<!-- PAGE -->

## 6 Desktop and API command boundaries

Native desktop and browser share evidence views but have different authority. REST projections cannot submit orders. Native Tauri commands enter the PAPER risk/OMS service with a model execution adapter rather than a connected broker.

| Surface | Current operations | Restriction |
| --- | --- | --- |
| Browser REST | Read evidence/workspace projections. | No order writes or broker connection. |
| Native Tauri | Single/combo submission, cancel, position close, route status. | No replacement or kill-switch command in this native command list. |
| Native PAPER model | Fill marketable orders immediately and fully at attested prices; retain Risk/OMS/journal effects. | No live feed, matching engine, automatic queue, or external broker. Nonmarketable limits rest. |
| gRPC PAPER route | Authenticated combo submission and PAPER kill-switch operations. | Only IBKR_PAPER_MODEL is accepted by configuration. |
| gRPC planning | Selected execution planning, risk, margin, and health operations. | Planning does not execute a slice/combination at a vendor. |
| gRPC LIVE stop route | Authenticated stop/release with attributed journals. | Adapter refuses broker operations and holds no connection. |

Native requests carry operator-attested price/time. A collar around a caller-supplied price cannot independently establish that it matches the market. This model exercises order controls rather than proving realistic fills.

Configured order/position limits and tick/lot checks are real. Native and gRPC PAPER configurations nevertheless set optional `portfolio_risk` to `None`. Portfolio-wide risk calculations exist in core and CLI but are not composed into these two command routes. The desktop does not enforce every available core risk calculation.

gRPC writes require password plus TOTP sessions and appropriate tenant/role permissions, with operator attribution. Its directory is single-tenant. Native IPC writes do not separately authenticate an operator. Persisted sessions, secret custody/enrollment, certificate lifecycle, centralized write approvals, and deployment review remain production work.

Navigation has 12 workspaces: Command Center, Research Lab, Strategies, Marketplace, Backtest, News, Execution Blotter, Risk Cockpit, Portfolio, Replay and Incidents, Journal, Administration. Navigation does not prove a paid marketplace, live news service, or computed output in every panel.

Sources: [native commands](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/desktop/src-tauri/src/lib.rs#L17), [model scope](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/desktop/src-tauri/src/paper_gateway.rs#L11), [native risk composition](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/desktop/src-tauri/src/paper_gateway.rs#L783), and [API model/risk composition](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/services/trading-api/src/main.rs#L1138).

<!-- PAGE -->

## 7 Broker execution and controlled LIVE

The real PAPER-only Rust adapter delegates to the official Python TWS API bridge, requiring its approved distribution and running TWS/IB Gateway. No application composition substitutes it for the models used by desktop, API, or PAPER CLI. Broker integration is implementation work, not merely supplying credentials.

| Layer | Implemented capability | What is not established |
| --- | --- | --- |
| PAPER kernel | Risk-gated orders, reservations, fills, recovery, reconnect/reconciliation, cancellation, and fault models. | Retained real-broker reliability. |
| Official PAPER bridge | Single MKT/LMT DAY submissions, cancellation, polling, snapshots, reconnect. | General TIF, BAG dispatch, or replacement in the real adapter. |
| Combo kernels | Per-leg validation/accounting and atomic lifecycle in tested PAPER/LIVE model paths. | Official PAPER bridge or live vendor combo execution. |
| Replacement kernel | Price-only changes reducing the original limit risk, with checks/evidence. | Desktop command or implementation in the real PAPER adapter. |
| EMS | TWAP, VWAP, participation, arrival-price, iceberg, algo-wheel, repricing, brackets/trailing stops, baskets, routing models. | Complete connected dispatch of these plans. |
| Controlled LIVE | Approval/activation, canary/shadow states, kill switches, reconciliation, recovery, journals. | Concrete operational vendor transport and capital-bearing acceptance. |

Rust emits `submit_combo`, but the Python dispatch table does not implement it. The real PAPER adapter inherits default replacement refusal. The controlled IBKR LIVE adapter inherits default combo refusal. Kernel/model tests do not prove these vendor operations work.

LIVE transport is an interface; the inspected concrete implementation is a test fake. This is a substantive gap. Four-eyes activation assumes trustworthy supplied identities; application authentication and operational review must establish them.

The broker milestone is to compose real PAPER in one reviewed application with explicit order semantics, fresh market inputs, deadlines, restart/reconciliation behavior, permissions, and broker comparison. Only then can actual PAPER reliability be measured. Model runs are not external PAPER sessions.

Sources: [real PAPER adapter](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/adapters/brokers/ibkr/src/lib.rs#L742), [Python dispatch](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/python/ibkr-gateway/src/follon_ibkr_gateway.py#L100), [DAY mapping](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/python/ibkr-gateway/src/follon_ibkr_gateway.py#L628), [replacement default](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/paper/src/lib.rs#L522), and [LIVE transport](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/adapters/brokers/ibkr/src/lib.rs#L977).

<!-- PAGE -->

## 8 Accounting and wider models

Economic values use fixed-point domain decimals. Accounting implements balanced journals, FIFO long/short lots, FX conversion, margin, financing, and read-only account aggregation. Options provide European analytics, implied volatility/Greeks, frozen-chain scenarios, settlement, and declared-export reconciliation. FX uses supplied snapshots. These are model capabilities with source/tests, not operational feeds, custody, tax filing, or broker support.

Advanced backtest economics are computed after the funded long-only runner succeeds. Short/margin model tests do not prove a short or leveraged strategy runs through the current primary route. Read-only aggregation does not authorize capital allocation or cross-account transfers.

**The two sample P&L conventions are intentional.** The primary ledger adds buy fees to acquisition cost. The advanced account records average trading price before fees and charges separately. Comparing raw unrealized fields without this distinction incorrectly suggests a mismatch.

| Repeated SPY example | Primary ledger | Advanced account |
| --- | --- | --- |
| One-share basis | USD 100.10 cost including fee | USD 100.00 trading price |
| Cash after buy | USD 99,899.90 | Same cash in valuation |
| Market value | USD 100.00 | USD 100.00 |
| Unrealized component | USD -0.10 | USD 0.00 before charges |
| Execution fee | USD 0.10 in basis | USD 0.10 separately reported |
| Net equity | USD 99,999.90 | USD 99,999.90 |

Performance net P&L is USD -0.10 against USD 100,000 starting equity. This two-bar flat-price, one-trade fixture is a functional demonstration, not meaningful investment performance or full accounting validation.

European option outputs use bounded fixed-point approximations. Reproducibility does not remove model/calibration assumptions. No American-option model or operational options trading is inferred from those analytics.

Sources: [fee-inclusive basis](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/backtest/src/lib.rs#L239), [advanced convention](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/backtest/src/lib.rs#L699), [attachment after primary run](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/cli/src/backtest.rs#L518), [accounting](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/accounting/src/lib.rs), and [option model](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/options/src/lib.rs#L1).

<!-- PAGE -->

## 9 Verification evidence and CI status

The full local measurement at **16:46:26 UTC** remains evidence for unchanged source. Seven suites passed: 521 root Rust tests/3 ignored, 30 native tests, 55 Python tests, root formatting/Clippy, desktop evidence checks, server contracts. Ignored tests require disposable PostgreSQL. Separate native formatting/Clippy, frontend typecheck/build, dependency audits, and SBOM generation passed in the preceding review.

This deeper pass ran **56 control-plane, 17 backtest, and 15 security/tool tests**, all passing. These 88 invocations overlap existing suites and are not added to earlier totals. They include cross-instrument refusal, same-bar fill prevention, tick changes, split ledger economics, and acceptance structure. Passing them does not resolve source-inspected limits.

| Latest main foundation job | Actual outcome and scope |
| --- | --- |
| Rust | Formatting, Clippy, root all-target tests passed. |
| PostgreSQL integration | Its configured ignored tests passed on GitHub; no fresh local deployment is inferred. |
| Security | Executed secret/dependency/SBOM checks passed. Conditional PR dependency-review was skipped on main push. |
| Python and contracts | Six official-backend tests failed because ibapi was missing. Later storage/server-contract steps were skipped. |
| Desktop | Frontend/build/native formatting passed. Clippy failed on missing glib-2.0 via pkg-config; native tests were skipped. |
| SAST | Failed on historical Nginx Host example in an audit document. Actual config uses Host dashboard:8080. |

Latest main run **36334101143**, exact reviewed commit, still fails when rechecked. PR run **36333055826** also failed with an additional security-job failure; it is separate. Successful Dependabot workflows do not make foundation CI green.

Retained local DAST at **16:49:11 UTC** reports **85 passes, 0 failures**, `independent: false`. It used local test HTTP/gRPC processes, not a deployed broker service. It was not rerun in this deeper pass. Root/native Rust audits exited 0; native retained six unmaintained-crate warnings and one Glib unsoundness warning. npm reported zero vulnerabilities. The retained SBOM has 332 locked or declared components.

These are dated results, not complete security assurance. The debug fixture risk benchmark p99 of 28 microseconds across 100 samples does not establish production latency, availability, or recovery targets. The seven-suite tool is not the full set of release/security checks.

Sources: [main run](https://github.com/Vishnums3107/Follon/actions/runs/36334101143), [workflow](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/.github/workflows/ci.yml), [status suites](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/tools/session_status.py#L78), and [actual Nginx configuration](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/infra/nginx.dashboard.conf#L40).

<!-- PAGE -->

## 10 Security and evidence custody

Meaningful controls include gRPC authentication/role checks, TOTP replay refusal/lockout, tenant checks, transport validation, fixed-point intents, conflicting-output refusal, and journal recovery verification. Their operational limits matter.

| Area | Source-backed limit | Consequence |
| --- | --- | --- |
| Strategy execution | Same-user process; blocking, unbounded frame reads. | Trusted code only; isolation/deadlines/limits before hostile strategies. |
| Identity | Native writes lack separate auth; gRPC lacks complete persisted identity deployment. | Local command validity does not prove a named authorized operator. |
| Acceptance/promotion | Declared IDs and caller-supplied status. | No independent authentication or operating proof. |
| Local journals | Hash chains/fsync; administrator can rewrite files. | Need trusted retained anchors/copies and custody controls. |
| Symbolic links | Specific PAPER/LIVE/admin guards; operations lacks the same check. | Inconsistent handling plus check-before-open races. |
| PostgreSQL events | RLS/constraints; reviewed migration lacks write-once UPDATE/DELETE denial. | No protection claim against database administrator rewriting evidence. |
| Dependencies | Glib unsoundness and six unmaintained crates remain allowed warnings. | Reachability/platform disposition still required. |

Current local “immutable” workflows mean append-only APIs and helpers that refuse conflicting publication. They are not write-once media or proof of origin. A complete hash chain is recomputable. Independently retained trusted hashes/signatures help when signer identity, custody, and storage privileges are established.

Operations journal opening lacks the PAPER/LIVE symlink guard. Its reader treats a nonexisting path as empty, and a dangling link appears absent through that check. This is a source-inspected local filesystem issue; no remote exploit was established. Release key generation can also leave a newly written private key after refusal of its trusted-key output.

Independent deployment security, protected directory secrets, enrollment/certificates, recovery drills, and signer management remain unfinished. Docker was unavailable during the preceding review; no fresh local Compose deployment/restore is claimed.

Sources: [operations journal paths](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/operations/src/lib.rs#L1526), [PAPER link guard](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/paper/src/lib.rs#L2396), [publication helper](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/apps/cli/src/lib.rs#L14), and [event migration](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/adapters/persistence/postgres/migrations/0001_operating_system.sql#L8).

<!-- PAGE -->

## 11 Acceptance and promotion limitations

The retained acceptance JSON has **zero verified records**, `all_gates_eligible: false`. “Verified” here means structurally/hash-valid ledger records, not independently authenticated broker evidence. Zero records do not prove no undocumented activity exists.

| Gate | Recorded count | Distinction |
| --- | --- | --- |
| PAPER sessions | 0 of 30 | Accepted unique subject IDs; clean-session proof requires review. |
| LIVE sessions | 0 of 60 | No operational vendor transport or retained capital session. |
| Design partners | 0 of 5 | No qualifying records retained. |
| Broker options | 0 of 1 | Model tests do not qualify as a broker export. |
| Customers | 0 of 1 in tool | Plan separately requires 10 professionals or 3 organizations. |

The tool validates exact fields, timestamps, hash syntax/continuity, distinct declared observer/reviewer strings, and accepted subjects. It does not authenticate people, locate/re-hash source artifacts, inspect broker reconciliation, or calculate cleanliness. A later rejected record does not remove an earlier accepted subject.

**A controlled synthetic experiment confirmed these limits.** An invented but structurally valid customer record with an arbitrary source hash made the isolated customer gate eligible, without a source artifact or customer. A later rejection left it eligible. The production acceptance subcheck separately accepted a minimal caller-authored JSON with schema 1 and `all_gates_eligible: true`. These are isolated review fixtures, not operational acceptance evidence.

No full release promotion ran. The full tool does verify signed releases; that cryptographic check is real. Its acceptance subcheck nevertheless trusts supplied status, while requester/approver checks require different canonical strings without authenticating separate people or binding reviewed broker evidence to a release.

The project therefore has acceptance bookkeeping and release-verification primitives rather than complete independently enforced production gating. Improve reviewer authentication, source verification, release/environment binding, rejection/correction policy, and session criteria. Retain original discrepancies and external approvals rather than treating hash validity as proof of truth.

Independent security, legal/data/broker permissions, signer custody, recovery, and customer validation remain separate. This report makes no compliance conclusion or production promotion.

Sources: [validation/counting](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/tools/acceptance_evidence.py#L74), [promotion subcheck](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/tools/release_promotion_gate.py#L74), and [roadmap gates](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/docs/06-delivery/03-roadmap-and-gates.md).

<!-- PAGE -->

## 12 Advanced panels and generated evidence

The fixture builder validates 32 hand-authored advanced schema examples. These test contracts, not operating results. The reviewed pipeline deliberately leaves the examples in tests rather than publishing them into desktop evidence.

For this specific group of 32 contracts, real computations are published for **two**: decision reconstruction and strategy capsule manifest. Computed artifacts are not published for the other **30**. Ordinary outputs such as backtests, orders, and option analytics are outside this particular count.

Data-rights/semantics certification accepts a caller-supplied parity score. This pipeline does not measure that score. Publishing an operator-entered value would not establish measured parity.

| Publication gap examples | Requirement before treating them as operating capability |
| --- | --- |
| Scanner, exposure graph, event calendar | Retained attributable computation over declared inputs. |
| Champion/challenger, robustness, research jobs | Actual evaluation/job workflow and measured outputs. |
| News revision timeline, knowledge snapshot | Point-in-time inputs/history and real processing. |
| Assistant, coach, diagnosis | Implemented workflow/computation with inspectable inputs. |
| Fund statement, allocation, lineage | Defined accounting/authority/lineage and generated evidence. |
| Qualification, sandbox preview, workspace snapshot | Actual measurement, isolation/permission behavior, or snapshot producer. |

A panel can have a parser, renderer, related algorithms, and empty state while its exact producer is missing. This review confirms a pipeline publication gap, not universal absence of all related algorithms.

User-facing panels should remain empty until their producer runs. Filled examples do not establish feed rights, research progress, broker qualification, or sandbox security. Earlier fabricated-data remediation makes that distinction important.

Select one producer needed by an observed workflow and verify its contract, provenance, failure behavior, and rendering. Completing every panel is not required for the first bounded broker-PAPER case and will not close external gates.

Sources: [fixture builder](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/tools/build_advanced_evidence_fixtures.py#L906), [fixture exclusion/reconstruction](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/tools/generate_pipeline_evidence.py#L480), [capsule publication](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/tools/generate_pipeline_evidence.py#L521), and [parity input](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/core/market-data/src/rights.rs#L87).

<!-- PAGE -->

## 13 Impact and product usefulness

Measured results are repeatability and verification outcomes. Greater confidence, faster investigation, fewer trading mistakes, customer time savings, and reduced losses are possible benefits; they have not been measured. Correct infrastructure can execute an unprofitable strategy.

The two retained SPY runs remain byte-identical across JSON, NDJSON events, report, and manifest when rechecked. The example contains **two bars, one trade, USD -0.10 net P&L**, and a USD 0.10 fee. Its event SHA-256 is **8634ac518d096bc0118cc9ebb95ff357c783b8a80d94c03617adf25aa82cf722**. It is a small functional demonstration, not multi-year strategy evidence or production performance.

| Mechanism | Possible benefit | Measure next |
| --- | --- | --- |
| Replay/causal chains | Reproduce decisions and compare changes. | Real incident reproduction rate/time and missing inputs. |
| Risk/order validation | Refuse configured violations. | Correct/incorrect refusals and broker discrepancy prevention. |
| UNKNOWN/reconciliation | Expose uncertain, duplicated, or late evidence. | Unresolved discrepancies, duplicate effects, recovery time. |
| Fixed-point accounting | Trace cash, positions, charges, financing. | Broker agreement with conventions and rounding explained. |
| Native PAPER controls | Practice a bounded workflow. | Unaided completion, task time, operator errors, support. |
| Signed capsules/releases | Verify content against a trusted key. | Matching-runtime reproduction and actual signer custody. |

An individual developer/trader can use Follon as a research laboratory and order-control simulator. Professional use could benefit from auditable decisions and recovery if a connected workflow works repeatedly. A customer service additionally needs installation, onboarding, support, data permissions, reliability, and willingness-to-pay observations.

The main present product risk is confusing model adapters with brokers, parsers with computations, child processes with security sandboxes, or internal hash validity with external truth. The updated report/status correct those interpretations.

No predictive edge, verified return, or causal reduction in user losses is established. Measure one real incident-reconstruction or reviewed PAPER reconciliation task against a stated baseline. Do not use feature count as impact.

The retained 28-microsecond p99 is a local debug fixture over 100 samples/25 warmups. It does not prove the production 5-ms objective, 99.9-percent availability, or recovery objectives.

<!-- PAGE -->

## 14 Commercial position and current worth

**Sale value and company valuation are undetermined.** The engineering assets are real, but the repository does not supply verified revenue, margins, retention, buyer offers, comparable deals, or an independent operating appraisal. Unknown is not zero; code size is not a price.

The first report's USD 106,000 to 345,000 replacement range assumed 6 to 18 engineer-months, wage/overhead, and revalidation multipliers. Arithmetic did not establish effort: no work breakdown, time record, quote, or measured comparable supported it. **This revision withdraws the range as a project-specific estimate, including its INR conversion.** It should not be quoted as worth or a supported rebuild budget.

| Value question | Current evidence | Needed for a credible number |
| --- | --- | --- |
| Historical cost | Not established. | Time/labor records, invoices, attributable costs. |
| Replacement cost | No validated scope/effort estimate. | Deliverable, task breakdown, platform assumptions, comparable quotes. |
| Asset sale price | No offer or comparable transaction. | Rights/provenance review, buyer diligence, tested handover, offers. |
| Company value | No verified operating financials. | Revenue/margins/retention, customer risk, cash flow, financing context. |
| Strategic utility | Demonstrated research and simulated order controls. | Repeated user outcomes and reviewed broker-PAPER history. |

The current BLS page still reports a US developer median of USD 135,980 for May 2025. That is labor-market context, not Follon's value or evidence of required effort. AI assistance, reuse, local rates, scope, and assurance can change costs substantially. A wage alone cannot justify a range. [BLS source](https://www.bls.gov/ooh/computer-and-information-technology/software-developers.htm).

Root MIT licence and Cargo/native Apache-2.0 metadata conflict. Resolve notices/contributor provenance before rights or distribution claims. Public code availability limits exclusivity assumptions; expertise, support, deployments, operating history, and customers may become stronger assets.

Alternatives already provide research/backtests. LEAN is a public Apache-2.0 algorithmic engine, and QuantConnect documents an established research workflow. Follon's potential differentiation is a useful local evidence/recovery workflow. That remains a product hypothesis. [LEAN](https://github.com/QuantConnect/Lean), [QuantConnect research guide](https://www.quantconnect.com/docs/v2/writing-algorithms/key-concepts/research-guide).

Test whether a defined user completes a useful task unaided and returns to use the platform. Retained usage, broker comparison, and willingness to pay can support future appraisal; unused features cannot supply them.

<!-- PAGE -->

## 15 Priorities and completion evidence

| Order | Action | Completion evidence |
| --- | --- | --- |
| 1 | Repair CI prerequisites and historical SAST match. | All six foundation jobs execute required steps and pass on reviewed commit. |
| 2 | Integrate real PAPER into one bounded application workflow. | Supported order/TIF/cancel semantics, Gateway logs, attributable reconciliation/restart results. |
| 3 | Make acceptance review trustworthy. | Authenticated reviewers, re-hashed artifacts, release/environment binding, criteria/rejection policy. |
| 4 | Close relevant safety gaps. | Worker deadlines/limits/trusted-code policy; native auth; portfolio-risk composition; journal handling. |
| 5 | Verify accounting/state parity and installation. | Corporate-action worker/order/ledger test; P&L conventions; clean install and recovery evidence. |
| 6 | Retain real PAPER history and observe user tasks. | 30 qualifying sessions; five distinct design partners complete normal workflows unaided, with measured outcomes. |
| Before production/live | Independent security, permissions, signer/secrets, recovery, and appropriate vendor integration. | Named reviewers, reports, warning dispositions, explicit approvals. |

Select/record an approved official IBKR API distribution/version for CI; the bridge has no concrete locked ibapi version. Correct its stale no-ibapi test instructions. Install supported Linux Tauri prerequisites. Treat the historical SAST example precisely while retaining real configuration scanning.

Exclude unsupported vendor combos, replacement, and TIF unless implemented/reviewed. Keep attested-price simulation clearly distinct from broker execution. Define cleanliness, reconnect, unresolved UNKNOWN, and rejected-record policy before counting sessions.

PAPER acceptance requires elapsed operating time and real records. Thirty model runs do not supply it. Two strings entered by one person do not supply independent approval. Reviewers must inspect what the local tools do not verify.

PAPER and LIVE modules are large, roughly 10,463 and 9,353 lines including tests. Refactor around a concrete safety/ownership need with parity checks, rather than a broad rewrite alongside initial broker integration.

This update changes documentation/assessment only. It does not implement the broker, CI, security, acceptance, or corporate-action parity work. Wider commercial/LIVE scope remains gated.

<!-- PAGE -->

## 16 Evidence record and review boundaries

Clickable repository references pin the inspected commit. Updated local documents are newer and uncommitted. Local artifacts are not assumed published on GitHub.

| Retained evidence | Content |
| --- | --- |
| var/follon-delivery-status.json | Seven-suite result at 16:46:26 UTC, actual exits/ignored counts. |
| var/follon-acceptance-status.json | Zero structurally verified records; all counted gates ineligible. |
| Assessment evidence/github-main-jobs.json and log | Main job/step evidence for run 36334101143. |
| Assessment evidence/dast | Local 85-probe non-independent scan at 16:49:11 UTC. |
| Assessment evidence/repeat-a and repeat-b | Four identical output types; two bars/one trade. |
| Assessment evidence/risk-benchmark.json | Debug fixture; configured observed_at is 30 August despite 27 September command execution. |
| Assessment evidence/follon-sbom.cdx.json | CycloneDX 1.6; 332 locked or declared components. |
| Assessment review-v2/inventory.json | Recomputed files, workspace, source lines, CLI/schema counts. |
| Assessment review-v2/targeted-validation.json | 56 replay/control-plane, 17 backtest, 15 security passes, recorded 17:19:10 UTC. |
| Assessment review-v2/gate-boundary-experiment.json | Isolated synthetic subcheck results; no actual customer, session, or full promotion. |

“Assessment” means `var/reports/project-assessment-2026-09-27`. Synthetic review records must not be imported into operational acceptance.

The review identifies source-inspected gaps separately from tested counterexamples. It establishes neither exhaustive coverage nor absence of all defects, production performance, external penetration testing, profitable strategy, compliance, or adoption. Full guarantees require additional evidence.

Entry points: [source tree](https://github.com/Vishnums3107/Follon/tree/4adb0909430d723c800fb1a2fd4bddb8aaddd792), [PR 30](https://github.com/Vishnums3107/Follon/pull/30), [main CI](https://github.com/Vishnums3107/Follon/actions/runs/36334101143), [DAST scanner](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/tools/dast_scan.py), and [licence](https://github.com/Vishnums3107/Follon/blob/4adb0909430d723c800fb1a2fd4bddb8aaddd792/LICENSE).

The supported conclusion is a substantial research/model-order platform with meaningful controls and remaining integration/assurance work. Its value becomes easier to assess after connected PAPER, independently reviewed evidence, and observed repeat use.
