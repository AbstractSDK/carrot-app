pub mod mars;
pub mod osmosis_cl_pool;
pub mod yield_type;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::Decimal;

use crate::{
    check::{Checked, Unchecked},
    yield_sources::yield_type::YieldTypeBase,
};

/// A yield sources has the following elements
/// A vector of tokens that NEED to be deposited inside the yield source with a repartition of tokens
/// A type that allows routing to the right smart-contract integration internally
#[cw_serde]
pub struct YieldSourceBase<T> {
    pub asset_distribution: Vec<AssetShare>,
    pub ty: YieldTypeBase<T>,
}

pub type YieldSourceUnchecked = YieldSourceBase<Unchecked>;
pub type YieldSource = YieldSourceBase<Checked>;

impl<T: Clone> YieldSourceBase<T> {
    pub fn all_denoms(&self) -> Vec<String> {
        self.asset_distribution
            .iter()
            .map(|e| e.denom.clone())
            .collect()
    }
}

/// This is used to express a share of tokens inside a strategy
#[cw_serde]
pub struct AssetShare {
    pub denom: String,
    pub share: Decimal,
}

#[cw_serde]
pub enum ShareType {
    /// This allows using the current distribution of tokens inside the position to compute the distribution on deposit
    Dynamic,
    /// This forces the position to use the target distribution of tokens when depositing
    Fixed,
}

// This represents a balance strategy
// This object is used for storing the current strategy, retrieving the actual strategy status or expressing a target strategy when depositing
#[cw_serde]
pub struct BalanceStrategyBase<T>(pub Vec<BalanceStrategyElementBase<T>>);

pub type BalanceStrategyUnchecked = BalanceStrategyBase<Unchecked>;
pub type BalanceStrategy = BalanceStrategyBase<Checked>;

impl BalanceStrategy {
    pub fn all_denoms(&self) -> Vec<String> {
        self.0
            .clone()
            .iter()
            .flat_map(|s| s.yield_source.all_denoms())
            .collect()
    }
}

#[cw_serde]
pub struct BalanceStrategyElementBase<T> {
    pub yield_source: YieldSourceBase<T>,
    pub share: Decimal,
}
impl<T: Clone> From<Vec<BalanceStrategyElementBase<T>>> for BalanceStrategyBase<T> {
    fn from(value: Vec<BalanceStrategyElementBase<T>>) -> Self {
        Self(value)
    }
}

pub type BalanceStrategyElementUnchecked = BalanceStrategyElementBase<Unchecked>;
pub type BalanceStrategyElement = BalanceStrategyElementBase<Checked>;
