//! Durable journal record shapes and their conversions to and from domain types.

use follon_domain::{
    validate_canonical_id, validate_utc_timestamp, ComboIntent, OrderIntent, OrderState, OrderType,
    RiskDecision, Side, TimeInForce,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum PersistentJournalRecord {
    V3(PersistentJournalRecordV3),
    V2(PersistentJournalRecordV2),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistentJournalRecordV3 {
    pub(crate) schema_version: u32,
    pub(crate) sequence: u64,
    pub(crate) previous_hash: String,
    pub(crate) state: PersistentPaperState,
    pub(crate) entry_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistentJournalRecordV2 {
    pub(crate) schema_version: u32,
    pub(crate) sequence: u64,
    pub(crate) state: PersistentPaperState,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentPaperState {
    pub(crate) configuration_fingerprint: String,
    pub(crate) account_id: String,
    pub(crate) currency: String,
    pub(crate) cash: String,
    pub(crate) orders: BTreeMap<String, PersistentOrder>,
    pub(crate) risk_evidence: BTreeMap<String, PersistentRiskEvidence>,
    pub(crate) positions: BTreeMap<String, PersistentPosition>,
    pub(crate) execution_ids: Vec<String>,
    pub(crate) active_kill_switches: Vec<String>,
    pub(crate) incidents: BTreeMap<String, PersistentIncident>,
    pub(crate) last_reconciled_at: Option<String>,
    pub(crate) last_reconciliation_clean: Option<bool>,
    pub(crate) paper_days: BTreeMap<String, PersistentPaperDay>,
    pub(crate) next_reconciliation: u64,
    #[serde(default)]
    pub(crate) broker_connected: bool,
    #[serde(default)]
    pub(crate) latest_reconciliation: Option<PersistentReconciliationReport>,
    /// Absent in a document written before this field existed; an empty book
    /// is exactly correct for one, since no fill could have been applied to a
    /// tax-lot ledger that did not yet exist.
    ///
    /// This default makes the *type* tolerant. It does not, on its own, let
    /// `FilePaperJournal::open` read an older journal **file**: that reader
    /// additionally requires every line to re-serialize byte-for-byte, so a
    /// line missing any field the current serializer writes is rejected before
    /// a default can apply. Verified directly rather than assumed. The same
    /// correction applies to every `#[serde(default)]` field below.
    #[serde(default)]
    pub(crate) tax_lots: PersistentTaxLotBook,
    /// Last observed mark per instrument (Decimal-as-string, matching every
    /// other persisted decimal field). Absent in a document written before
    /// this field existed; an empty map is correct for one -- every position
    /// simply falls back to its own average cost until re-quoted. See
    /// `tax_lots` above on what this default does and does not achieve.
    #[serde(default)]
    pub(crate) marks: BTreeMap<String, String>,
    /// Highest observed real equity (Decimal-as-string). `None` on a journal
    /// written before this field existed; `restore()` bootstraps it to the
    /// account's real equity computed from the rest of the just-restored
    /// state, which is the honest value for a peak that was never tracked
    /// before now.
    #[serde(default)]
    pub(crate) peak_equity: Option<String>,
    /// UTC calendar date of the current session-start daily-loss baseline
    /// (Decimal-as-string equity paired below). `None` on a journal written
    /// before this field existed, or before any risk evaluation has ever run.
    #[serde(default)]
    pub(crate) daily_baseline_date: Option<String>,
    /// Equity observed at the first risk evaluation of `daily_baseline_date`
    /// (Decimal-as-string). Present if and only if `daily_baseline_date` is.
    #[serde(default)]
    pub(crate) daily_baseline_equity: Option<String>,
    /// Net signed per-strategy contribution to each instrument
    /// (`instrument_id -> strategy_id -> quantity`, Decimal-as-string).
    /// Missing or absent entries on a journal written before this field
    /// existed are correct as empty: no fill could have been attributed to a
    /// strategy-attribution ledger that did not yet exist.
    #[serde(default)]
    pub(crate) strategy_attribution: BTreeMap<String, BTreeMap<String, String>>,
    /// Atomic multi-leg combination orders. Absent in a document written
    /// before the combination path existed; an empty map is exactly correct
    /// for one, since no combination could have been submitted through a path
    /// that did not yet exist. See `tax_lots` above on what this default does
    /// and does not achieve.
    #[serde(default)]
    pub(crate) combo_orders: BTreeMap<String, PersistentComboOrder>,
    /// Combination risk evidence, including refusals. Empty on an older
    /// journal for the same reason.
    #[serde(default)]
    pub(crate) combo_risk_evidence: BTreeMap<String, PersistentComboRiskEvidence>,
    /// Operator-attributed kill-switch changes (E3.3b). Never written while
    /// empty, so a journal without one re-serializes byte-for-byte.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) kill_switch_operations: Vec<PersistentKillSwitchOperation>,
    /// Operator-attributed order commands (E5.2b). Never written while empty,
    /// so a journal without one re-serializes byte-for-byte.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) order_operations: Vec<PersistentOrderOperation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentKillSwitchOperation {
    pub(crate) scope: String,
    pub(crate) action: String,
    pub(crate) operator: String,
    pub(crate) operated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentOrderOperation {
    pub(crate) order_id: String,
    pub(crate) action: String,
    pub(crate) operator: String,
    pub(crate) operated_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct PersistentTaxLotBook {
    pub(crate) lots: BTreeMap<String, Vec<PersistentTaxLot>>,
    pub(crate) applied_lot_ids: Vec<String>,
    pub(crate) applied_disposal_ids: Vec<String>,
    pub(crate) realized_by_currency: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) short_lots: BTreeMap<String, Vec<PersistentTaxLot>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) applied_short_lot_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) applied_cover_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentTaxLot {
    pub(crate) lot_id: String,
    pub(crate) opened_at: String,
    pub(crate) remaining_quantity: String,
    pub(crate) unit_cost: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentReconciliationReport {
    pub(crate) reconciliation_id: String,
    pub(crate) reconciled_at: String,
    pub(crate) issues: Vec<PersistentReconciliationIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentReconciliationIssue {
    incident_id: String,
    category: String,
    subject: String,
    detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentOrder {
    pub(crate) intent: PersistentIntent,
    pub(crate) state: String,
    pub(crate) broker_order_id: Option<String>,
    #[serde(default)]
    pub(crate) broker_order_versions: Vec<String>,
    #[serde(default)]
    pub(crate) replace_return_state: Option<String>,
    pub(crate) filled_quantity: String,
    pub(crate) market: PersistentMarketData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentMarketData {
    pub(crate) instrument_id: String,
    pub(crate) mark_price: String,
    pub(crate) observed_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentComboOrder {
    pub(crate) intent: PersistentComboIntent,
    pub(crate) state: String,
    pub(crate) broker_order_id: Option<String>,
    #[serde(default)]
    pub(crate) broker_order_versions: Vec<String>,
    /// One persisted mark per leg, reusing the single-instrument observation
    /// shape rather than inventing a second format to migrate later.
    pub(crate) market: Vec<PersistentMarketData>,
    /// Versioned extension; absence retains the exact E1.3a serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) execution_state: Option<PersistentComboExecutionState>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentComboRiskEvidence {
    pub(crate) intent: PersistentComboIntent,
    pub(crate) approved: bool,
    pub(crate) reason_codes: Vec<String>,
    pub(crate) policy_version: String,
    pub(crate) decided_at: String,
    pub(crate) correlation_id: String,
    pub(crate) actor: String,
    pub(crate) evaluated_limits: String,
    pub(crate) market: Vec<PersistentMarketData>,
    /// Versioned extension; absence retains the exact pre-E3.3a serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) submitted_by: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentComboIntent {
    pub(crate) intent_id: String,
    pub(crate) account_id: String,
    pub(crate) strategy_id: String,
    pub(crate) correlation_id: String,
    pub(crate) legs: Vec<PersistentComboLeg>,
    pub(crate) combo_quantity: String,
    /// `MAXIMUM_DEBIT` or `MINIMUM_CREDIT`, matching `ComboPriceLimit::kind`.
    pub(crate) price_limit_kind: String,
    pub(crate) price_limit_amount: String,
    pub(crate) time_in_force: String,
    pub(crate) rationale: String,
    pub(crate) created_at: String,
    pub(crate) strategy_version: String,
    pub(crate) configuration_version: String,
    pub(crate) environment: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentComboLeg {
    instrument_id: String,
    side: String,
    ratio: u32,
    limit_price: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentRiskEvidence {
    pub(crate) intent: PersistentIntent,
    pub(crate) approved: bool,
    pub(crate) reason_codes: Vec<String>,
    pub(crate) policy_version: String,
    pub(crate) decided_at: String,
    pub(crate) correlation_id: String,
    pub(crate) actor: String,
    pub(crate) evaluated_limits: String,
    pub(crate) market: PersistentMarketData,
    /// Versioned extension (E5.2b); absence retains the earlier serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) submitted_by: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentIntent {
    pub(crate) intent_id: String,
    pub(crate) account_id: String,
    pub(crate) strategy_id: String,
    pub(crate) instrument_id: String,
    pub(crate) correlation_id: String,
    pub(crate) side: String,
    pub(crate) quantity: String,
    pub(crate) order_type: String,
    pub(crate) limit_price: Option<String>,
    pub(crate) time_in_force: String,
    pub(crate) rationale: String,
    pub(crate) created_at: String,
    pub(crate) strategy_version: String,
    pub(crate) configuration_version: String,
    pub(crate) environment: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentPosition {
    pub(crate) quantity: String,
    pub(crate) average_cost: String,
    pub(crate) realized_pnl: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PersistentIncident {
    pub(crate) category: String,
    pub(crate) subject: String,
    pub(crate) detail: String,
    pub(crate) explanation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct PersistentPaperDay {
    pub(crate) calendar_id: String,
    pub(crate) session_opens_at: String,
    pub(crate) session_closes_at: String,
    pub(crate) clean: bool,
}

impl From<&OrderIntent> for PersistentIntent {
    fn from(intent: &OrderIntent) -> Self {
        Self {
            intent_id: intent.intent_id.clone(),
            account_id: intent.account_id.clone(),
            strategy_id: intent.strategy_id.clone(),
            instrument_id: intent.instrument_id.clone(),
            correlation_id: intent.correlation_id.clone(),
            side: intent.side.as_str().to_owned(),
            quantity: intent.quantity.to_string(),
            order_type: intent.order_type.as_str().to_owned(),
            limit_price: intent.limit_price.map(|price| price.to_string()),
            time_in_force: intent.time_in_force.as_str().to_owned(),
            rationale: intent.rationale.clone(),
            created_at: intent.created_at.clone(),
            strategy_version: intent.strategy_version.clone(),
            configuration_version: intent.configuration_version.clone(),
            environment: intent.environment.clone(),
        }
    }
}

impl From<&PaperMarketData> for PersistentMarketData {
    fn from(market: &PaperMarketData) -> Self {
        Self {
            instrument_id: market.instrument_id.clone(),
            mark_price: market.mark_price.to_string(),
            observed_at: market.observed_at.clone(),
        }
    }
}

impl TryFrom<PersistentMarketData> for PaperMarketData {
    type Error = PaperError;

    fn try_from(market: PersistentMarketData) -> Result<Self, Self::Error> {
        let market = Self {
            instrument_id: market.instrument_id,
            mark_price: decimal("persisted market mark price", &market.mark_price)?,
            observed_at: market.observed_at,
        };
        market.validate()?;
        Ok(market)
    }
}

impl From<&PaperRiskEvidence> for PersistentRiskEvidence {
    fn from(evidence: &PaperRiskEvidence) -> Self {
        Self {
            intent: PersistentIntent::from(&evidence.intent),
            approved: evidence.decision.approved,
            reason_codes: evidence.decision.reason_codes.clone(),
            policy_version: evidence.decision.policy_version.clone(),
            decided_at: evidence.decision.decided_at.clone(),
            correlation_id: evidence.decision.correlation_id.clone(),
            actor: evidence.decision.actor.clone(),
            evaluated_limits: evidence.decision.evaluated_limits.clone(),
            market: PersistentMarketData::from(&evidence.market),
            submitted_by: evidence.submitted_by.clone(),
        }
    }
}

impl TryFrom<PersistentRiskEvidence> for PaperRiskEvidence {
    type Error = PaperError;

    fn try_from(evidence: PersistentRiskEvidence) -> Result<Self, Self::Error> {
        let intent = OrderIntent::try_from(evidence.intent)?;
        validate_canonical_id("persisted risk correlation_id", &evidence.correlation_id)?;
        validate_utc_timestamp("persisted risk decided_at", &evidence.decided_at)?;
        if evidence.reason_codes.is_empty()
            || evidence.reason_codes.iter().any(|reason| reason.is_empty())
            || evidence.policy_version.is_empty()
            || evidence.actor.is_empty()
            || evidence.evaluated_limits.is_empty()
        {
            return Err(PaperError("persisted risk evidence is invalid".to_owned()));
        }
        if evidence.correlation_id != intent.correlation_id {
            return Err(PaperError(
                "persisted risk correlation does not match its intent".to_owned(),
            ));
        }
        if let Some(operator) = &evidence.submitted_by {
            validate_canonical_id("persisted paper order submitted_by", operator)?;
        }
        let decision = RiskDecision {
            decision_id: format!("paper-risk-{}", intent.intent_id),
            intent_id: intent.intent_id.clone(),
            approved: evidence.approved,
            reason_codes: evidence.reason_codes,
            policy_version: evidence.policy_version,
            decided_at: evidence.decided_at,
            correlation_id: evidence.correlation_id,
            actor: evidence.actor,
            evaluated_limits: evidence.evaluated_limits,
        };
        Ok(Self {
            intent,
            decision,
            market: PaperMarketData::try_from(evidence.market)?,
            submitted_by: evidence.submitted_by,
        })
    }
}

impl From<&ReconciliationReport> for PersistentReconciliationReport {
    fn from(report: &ReconciliationReport) -> Self {
        Self {
            reconciliation_id: report.reconciliation_id.clone(),
            reconciled_at: report.reconciled_at.clone(),
            issues: report
                .issues
                .iter()
                .map(|issue| PersistentReconciliationIssue {
                    incident_id: issue.incident_id.clone(),
                    category: issue.category.clone(),
                    subject: issue.subject.clone(),
                    detail: issue.detail.clone(),
                })
                .collect(),
        }
    }
}

impl TryFrom<PersistentReconciliationReport> for ReconciliationReport {
    type Error = PaperError;

    fn try_from(value: PersistentReconciliationReport) -> Result<Self, Self::Error> {
        validate_canonical_id(
            "persisted paper reconciliation_id",
            &value.reconciliation_id,
        )?;
        validate_utc_timestamp("persisted paper reconciliation time", &value.reconciled_at)?;
        let mut incident_ids = BTreeSet::new();
        let mut issues = Vec::with_capacity(value.issues.len());
        for issue in value.issues {
            validate_canonical_id("persisted paper incident_id", &issue.incident_id)?;
            validate_broker_reason("persisted paper issue category", &issue.category)?;
            validate_broker_reason("persisted paper issue subject", &issue.subject)?;
            validate_broker_reason("persisted paper issue detail", &issue.detail)?;
            if !incident_ids.insert(issue.incident_id.clone()) {
                return Err(PaperError(
                    "persisted paper reconciliation repeats an incident ID".to_owned(),
                ));
            }
            issues.push(ReconciliationIssue {
                incident_id: issue.incident_id,
                category: issue.category,
                subject: issue.subject,
                detail: issue.detail,
            });
        }
        Ok(Self {
            reconciliation_id: value.reconciliation_id,
            reconciled_at: value.reconciled_at,
            issues,
        })
    }
}

impl TryFrom<PersistentIntent> for OrderIntent {
    type Error = PaperError;

    fn try_from(intent: PersistentIntent) -> Result<Self, Self::Error> {
        let result = Self {
            intent_id: intent.intent_id,
            account_id: intent.account_id,
            strategy_id: intent.strategy_id,
            instrument_id: intent.instrument_id,
            correlation_id: intent.correlation_id,
            side: match intent.side.as_str() {
                "BUY" => Side::Buy,
                "SELL" => Side::Sell,
                _ => return Err(PaperError("persisted intent side is invalid".to_owned())),
            },
            quantity: decimal("persisted intent quantity", &intent.quantity)?,
            order_type: match intent.order_type.as_str() {
                "MARKET" => OrderType::Market,
                "LIMIT" => OrderType::Limit,
                _ => {
                    return Err(PaperError(
                        "persisted intent order type is invalid".to_owned(),
                    ))
                }
            },
            limit_price: intent
                .limit_price
                .as_deref()
                .map(|price| decimal("persisted intent limit price", price))
                .transpose()?,
            time_in_force: match intent.time_in_force.as_str() {
                "DAY" => TimeInForce::Day,
                "GTC" => TimeInForce::GoodTilCancelled,
                _ => {
                    return Err(PaperError(
                        "persisted intent time in force is invalid".to_owned(),
                    ))
                }
            },
            rationale: intent.rationale,
            created_at: intent.created_at,
            strategy_version: intent.strategy_version,
            configuration_version: intent.configuration_version,
            environment: intent.environment,
        };
        result.validate()?;
        Ok(result)
    }
}

impl From<&ComboIntent> for PersistentComboIntent {
    fn from(intent: &ComboIntent) -> Self {
        Self {
            intent_id: intent.intent_id.clone(),
            account_id: intent.account_id.clone(),
            strategy_id: intent.strategy_id.clone(),
            correlation_id: intent.correlation_id.clone(),
            legs: intent
                .legs
                .iter()
                .map(|leg| PersistentComboLeg {
                    instrument_id: leg.instrument_id.clone(),
                    side: leg.side.as_str().to_owned(),
                    ratio: leg.ratio,
                    limit_price: leg.limit_price.to_string(),
                })
                .collect(),
            combo_quantity: intent.combo_quantity.to_string(),
            price_limit_kind: intent.price_limit.kind().to_owned(),
            price_limit_amount: intent.price_limit.amount().to_string(),
            time_in_force: intent.time_in_force.as_str().to_owned(),
            rationale: intent.rationale.clone(),
            created_at: intent.created_at.clone(),
            strategy_version: intent.strategy_version.clone(),
            configuration_version: intent.configuration_version.clone(),
            environment: intent.environment.clone(),
        }
    }
}

impl TryFrom<PersistentComboIntent> for ComboIntent {
    type Error = PaperError;

    fn try_from(intent: PersistentComboIntent) -> Result<Self, Self::Error> {
        let amount = decimal("persisted combo price limit", &intent.price_limit_amount)?;
        let mut legs = Vec::with_capacity(intent.legs.len());
        for leg in intent.legs {
            legs.push(follon_domain::ComboIntentLeg {
                instrument_id: leg.instrument_id,
                side: match leg.side.as_str() {
                    "BUY" => Side::Buy,
                    "SELL" => Side::Sell,
                    _ => return Err(PaperError("persisted combo leg side is invalid".to_owned())),
                },
                ratio: leg.ratio,
                limit_price: decimal("persisted combo leg limit price", &leg.limit_price)?,
            });
        }
        let result = Self {
            intent_id: intent.intent_id,
            account_id: intent.account_id,
            strategy_id: intent.strategy_id,
            correlation_id: intent.correlation_id,
            legs,
            combo_quantity: decimal("persisted combo quantity", &intent.combo_quantity)?,
            price_limit: match intent.price_limit_kind.as_str() {
                "MAXIMUM_DEBIT" => follon_domain::ComboPriceLimit::MaximumDebit(amount),
                "MINIMUM_CREDIT" => follon_domain::ComboPriceLimit::MinimumCredit(amount),
                _ => {
                    return Err(PaperError(
                        "persisted combo price limit kind is invalid".to_owned(),
                    ))
                }
            },
            time_in_force: match intent.time_in_force.as_str() {
                "DAY" => TimeInForce::Day,
                "GTC" => TimeInForce::GoodTilCancelled,
                _ => {
                    return Err(PaperError(
                        "persisted combo time in force is invalid".to_owned(),
                    ))
                }
            },
            rationale: intent.rationale,
            created_at: intent.created_at,
            strategy_version: intent.strategy_version,
            configuration_version: intent.configuration_version,
            environment: intent.environment,
        };
        // Re-validated on the way back in, not trusted because it was once
        // written: a journal is a file on disk and may have been edited.
        result.validate()?;
        Ok(result)
    }
}

impl From<&PaperComboRiskEvidence> for PersistentComboRiskEvidence {
    fn from(evidence: &PaperComboRiskEvidence) -> Self {
        Self {
            intent: PersistentComboIntent::from(&evidence.intent),
            approved: evidence.decision.approved,
            reason_codes: evidence.decision.reason_codes.clone(),
            policy_version: evidence.decision.policy_version.clone(),
            decided_at: evidence.decision.decided_at.clone(),
            correlation_id: evidence.decision.correlation_id.clone(),
            actor: evidence.decision.actor.clone(),
            evaluated_limits: evidence.decision.evaluated_limits.clone(),
            market: evidence
                .market
                .marks
                .iter()
                .map(PersistentMarketData::from)
                .collect(),
            submitted_by: evidence.submitted_by.clone(),
        }
    }
}

impl TryFrom<PersistentComboRiskEvidence> for PaperComboRiskEvidence {
    type Error = PaperError;

    fn try_from(evidence: PersistentComboRiskEvidence) -> Result<Self, Self::Error> {
        let intent = ComboIntent::try_from(evidence.intent)?;
        let market = combo_market_from_persisted(evidence.market)?;
        if let Some(operator) = &evidence.submitted_by {
            validate_canonical_id("persisted paper combo submitted_by", operator)?;
        }
        Ok(Self {
            decision: RiskDecision {
                decision_id: format!("paper-combo-risk-{}", intent.intent_id),
                intent_id: intent.intent_id.clone(),
                approved: evidence.approved,
                reason_codes: evidence.reason_codes,
                policy_version: evidence.policy_version,
                decided_at: evidence.decided_at,
                correlation_id: evidence.correlation_id,
                actor: evidence.actor,
                evaluated_limits: evidence.evaluated_limits,
            },
            intent,
            market,
            submitted_by: evidence.submitted_by,
        })
    }
}

pub(crate) fn combo_market_from_persisted(
    marks: Vec<PersistentMarketData>,
) -> Result<PaperComboMarketData, PaperError> {
    let mut restored = Vec::with_capacity(marks.len());
    for mark in marks {
        restored.push(PaperMarketData::try_from(mark)?);
    }
    Ok(PaperComboMarketData { marks: restored })
}

pub(crate) fn parse_order_state(value: &str) -> Result<OrderState, PaperError> {
    match value {
        "CREATED" => Ok(OrderState::Created),
        "PENDING_RISK" => Ok(OrderState::PendingRisk),
        "RISK_REJECTED" => Ok(OrderState::RiskRejected),
        "APPROVED" => Ok(OrderState::Approved),
        "PENDING_SUBMIT" => Ok(OrderState::PendingSubmit),
        "SUBMITTED" => Ok(OrderState::Submitted),
        "ACKNOWLEDGED" => Ok(OrderState::Acknowledged),
        "PARTIALLY_FILLED" => Ok(OrderState::PartiallyFilled),
        "FILLED" => Ok(OrderState::Filled),
        "PENDING_CANCEL" => Ok(OrderState::PendingCancel),
        "PENDING_REPLACE" => Ok(OrderState::PendingReplace),
        "CANCELLED" => Ok(OrderState::Cancelled),
        "REJECTED" => Ok(OrderState::Rejected),
        "EXPIRED" => Ok(OrderState::Expired),
        "UNKNOWN" => Ok(OrderState::Unknown),
        _ => Err(PaperError("persisted OMS state is invalid".to_owned())),
    }
}

pub(crate) fn parse_kill_switch_scope(value: &str) -> Result<KillSwitchScope, PaperError> {
    KillSwitchScope::from_key(value)
        .map_err(|_| PaperError("persisted kill-switch scope is invalid".to_owned()))
}
