//! Operator-attested corporate actions applied to a PAPER account (delivery state E8.4b).

use follon_domain::Decimal;
use follon_market_data::CorporateAction;

/// A corporate action an operator applies to the PAPER account.
///
/// The broker applies a split or pays a dividend on its own books; this request
/// brings the OMS's independent books along, and reconciliation then shows
/// whether the two agree. `held_quantity` is the position the action was stated
/// against, as the broker's notice gives it. The service refuses the request
/// unless the account holds exactly that, so an action is never applied to a
/// position that changed after the notice was read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperCorporateAction {
    /// The split or cash dividend, in the contract replay and backtests use.
    pub action: CorporateAction,
    /// The signed position in the action's instrument the action applies to.
    pub held_quantity: Decimal,
    /// The authenticated operator applying it.
    pub applied_by: String,
    /// Canonical UTC time it is applied, no earlier than it took effect.
    pub applied_at: String,
}

/// What applying one corporate action did to the PAPER account, as the journal
/// records it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperCorporateActionReceipt {
    /// The action applied.
    pub action: CorporateAction,
    /// The authenticated operator who applied it.
    pub applied_by: String,
    /// Canonical UTC time it was applied.
    pub applied_at: String,
    /// The signed position in the instrument before the action.
    pub quantity_before: Decimal,
    /// The signed position after it: scaled by a split, unchanged by a dividend.
    pub quantity_after: Decimal,
    /// The cash the action moved: a dividend received on a long position or
    /// paid on a short one, and nothing for a split.
    pub cash_delta: Decimal,
}
