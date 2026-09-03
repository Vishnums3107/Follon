//! Broker statement ingestion and offline reconciliation.
//!
//! Parses standard broker statements (e.g., CSV Activity Flex Queries) and
//! reconciles them against internal ledger balances and positions.

use crate::{AccountingError, Currency, MarginPosition, MultiCurrencyLedger};
use follon_domain::Decimal;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::str::FromStr;

/// Reconciled discrepancy found between internal records and broker statements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconciliationIncident {
    /// Internal ledger cash does not match broker statement cash.
    CashMismatch {
        /// Account and currency.
        currency: Currency,
        /// Internal ledger balance.
        internal_balance: Decimal,
        /// Broker reported balance.
        broker_balance: Decimal,
    },
    /// Internal position quantity does not match broker statement.
    PositionMismatch {
        /// Canonical instrument identity.
        instrument_id: String,
        /// Internal projected quantity.
        internal_quantity: Decimal,
        /// Broker reported quantity.
        broker_quantity: Decimal,
    },
}

/// Raw parsed statement cash balance.
#[derive(Debug, Deserialize)]
pub struct StatementCash {
    /// Statement currency.
    pub currency: String,
    /// Exact cash balance.
    pub balance: String,
}

/// Raw parsed statement position.
#[derive(Debug, Deserialize)]
pub struct StatementPosition {
    /// Canonical instrument identity (normalized from broker ticker).
    pub instrument_id: String,
    /// Exact held quantity.
    pub quantity: String,
}

/// A parsed end-of-day broker statement.
#[derive(Debug, Default)]
pub struct BrokerStatement {
    /// Exact cash balances.
    pub cash: Vec<StatementCash>,
    /// Exact held positions.
    pub positions: Vec<StatementPosition>,
}

impl BrokerStatement {
    /// Parses a simple combined CSV containing statement records.
    /// Rows must have `type`, `currency_or_instrument`, and `value`.
    pub fn from_csv(csv_data: &str) -> Result<Self, AccountingError> {
        let mut rdr = csv::Reader::from_reader(csv_data.as_bytes());
        let mut statement = BrokerStatement::default();

        for result in rdr.records() {
            let record = result.map_err(|e| AccountingError(format!("CSV parse error: {}", e)))?;
            if record.len() < 3 {
                continue;
            }
            let record_type = &record[0];
            let identifier = &record[1];
            let value = &record[2];

            match record_type {
                "CASH" => {
                    statement.cash.push(StatementCash {
                        currency: identifier.to_owned(),
                        balance: value.to_owned(),
                    });
                }
                "POSITION" => {
                    statement.positions.push(StatementPosition {
                        instrument_id: identifier.to_owned(),
                        quantity: value.to_owned(),
                    });
                }
                _ => continue,
            }
        }
        Ok(statement)
    }
}

/// Reconciles an internal ledger against a broker statement, returning incidents.
pub fn reconcile_statement(
    ledger: &MultiCurrencyLedger,
    internal_positions: &[MarginPosition],
    statement: &BrokerStatement,
    cash_account_id: &str,
) -> Result<Vec<ReconciliationIncident>, AccountingError> {
    let mut incidents = Vec::new();

    // Reconcile cash
    let mut broker_cash = BTreeMap::new();
    for stmt_cash in &statement.cash {
        let currency = Currency::new(&stmt_cash.currency)?;
        let balance = Decimal::from_str(&stmt_cash.balance)?;
        broker_cash.insert(currency, balance);
    }

    for (currency, broker_balance) in &broker_cash {
        let internal_balance = ledger.balance(cash_account_id, currency);
        if internal_balance != *broker_balance {
            incidents.push(ReconciliationIncident::CashMismatch {
                currency: currency.clone(),
                internal_balance,
                broker_balance: *broker_balance,
            });
        }
    }

    // Check for internal cash not present in broker statement (assuming 0 in broker)
    for ((account_id, currency), internal_balance) in ledger.balances() {
        if account_id == cash_account_id
            && !broker_cash.contains_key(currency)
            && *internal_balance != Decimal::ZERO
        {
            incidents.push(ReconciliationIncident::CashMismatch {
                currency: currency.clone(),
                internal_balance: *internal_balance,
                broker_balance: Decimal::ZERO,
            });
        }
    }

    // Reconcile positions
    let mut broker_positions = BTreeMap::new();
    for stmt_pos in &statement.positions {
        let quantity = Decimal::from_str(&stmt_pos.quantity)?;
        broker_positions.insert(stmt_pos.instrument_id.clone(), quantity);
    }

    for internal_pos in internal_positions {
        let broker_qty = broker_positions
            .remove(&internal_pos.instrument_id)
            .unwrap_or(Decimal::ZERO);

        if internal_pos.quantity != broker_qty {
            incidents.push(ReconciliationIncident::PositionMismatch {
                instrument_id: internal_pos.instrument_id.clone(),
                internal_quantity: internal_pos.quantity,
                broker_quantity: broker_qty,
            });
        }
    }

    // Check for broker positions not present internally
    for (instrument_id, broker_qty) in broker_positions {
        if broker_qty != Decimal::ZERO {
            incidents.push(ReconciliationIncident::PositionMismatch {
                instrument_id,
                internal_quantity: Decimal::ZERO,
                broker_quantity: broker_qty,
            });
        }
    }

    Ok(incidents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JournalLine, JournalTransaction};

    #[test]
    fn reconciles_matching_statement_without_incidents() {
        let usd = Currency::new("USD").unwrap();
        let mut ledger = MultiCurrencyLedger::default();

        let tx = JournalTransaction {
            transaction_id: "tx-1".to_owned(),
            tenant_id: "tenant-1".to_owned(),
            reference_id: "ref-1".to_owned(),
            lines: vec![
                JournalLine {
                    account_id: "cash.broker".to_owned(),
                    currency: usd.clone(),
                    debit: Decimal::from_str("15000").unwrap(),
                    credit: Decimal::ZERO,
                },
                JournalLine {
                    account_id: "equity".to_owned(),
                    currency: usd.clone(),
                    debit: Decimal::ZERO,
                    credit: Decimal::from_str("15000").unwrap(),
                },
            ],
        };
        ledger.post(&tx).unwrap();

        let positions = vec![MarginPosition {
            instrument_id: "AAPL".to_owned(),
            asset_class: "equity".to_owned(),
            currency: usd.clone(),
            quantity: Decimal::from_str("100").unwrap(),
            mark_price: Decimal::from_str("150").unwrap(),
            multiplier: Decimal::from_str("1").unwrap(),
        }];

        let csv = "type,currency_or_instrument,value\nCASH,USD,15000\nPOSITION,AAPL,100\n";
        let statement = BrokerStatement::from_csv(csv).unwrap();

        let incidents =
            reconcile_statement(&ledger, &positions, &statement, "cash.broker").unwrap();
        assert!(incidents.is_empty());
    }

    #[test]
    fn detects_mismatches_and_missing_records() {
        let usd = Currency::new("USD").unwrap();
        let ledger = MultiCurrencyLedger::default(); // Internal cash 0
        let positions = vec![]; // Internal positions 0

        // Broker says we have cash and positions
        let csv = "type,currency_or_instrument,value\nCASH,USD,5000\nPOSITION,MSFT,50\n";
        let statement = BrokerStatement::from_csv(csv).unwrap();

        let incidents =
            reconcile_statement(&ledger, &positions, &statement, "cash.broker").unwrap();
        assert_eq!(incidents.len(), 2);

        assert!(incidents.contains(&ReconciliationIncident::CashMismatch {
            currency: usd,
            internal_balance: Decimal::ZERO,
            broker_balance: Decimal::from_str("5000").unwrap(),
        }));
    }
}
