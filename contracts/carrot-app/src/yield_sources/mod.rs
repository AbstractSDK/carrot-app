pub mod mars;
pub mod osmosis_cl_pool;
pub mod yield_type;
use abstract_app::objects::AssetEntry;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Deps};
use cw_asset::AssetInfo;

use crate::{
    check::{Checked, Unchecked},
    contract::{App, AppResult},
    helpers::unwrap_native,
    yield_sources::yield_type::YieldParamsBase,
};
use abstract_app::traits::AbstractNameService;
/// A yield sources has the following elements
/// A vector of tokens that NEED to be deposited inside the yield source with a repartition of tokens
/// A type that allows routing to the right smart-contract integration internally
#[cw_serde]
pub struct YieldSourceBase<T> {
    pub asset_distribution: Vec<AssetShare>,
    pub params: YieldParamsBase<T>,
}

pub type YieldSourceUnchecked = YieldSourceBase<Unchecked>;
pub type YieldSource = YieldSourceBase<Checked>;

impl<T: Clone> YieldSourceBase<T> {
    pub fn all_denoms(&self, deps: Deps, app: &App) -> AppResult<Vec<String>> {
        let ans = app.name_service(deps);

        self.asset_distribution
            .iter()
            .map(|e| {
                let denom = unwrap_native(&ans.query(&e.asset)?)?;
                Ok(denom)
            })
            .collect()
    }
}

/// This is used to express a share of tokens inside a strategy
#[cw_serde]
pub struct AssetShare {
    pub asset: AssetEntry,
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
pub struct StrategyBase<T>(pub Vec<StrategyElementBase<T>>);

pub type StrategyUnchecked = StrategyBase<Unchecked>;
pub type Strategy = StrategyBase<Checked>;

impl Strategy {
    pub fn all_denoms(&self, deps: Deps, app: &App) -> AppResult<Vec<String>> {
        let results = self
            .0
            .clone()
            .iter()
            .map(|s| s.yield_source.all_denoms(deps, app))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(results.into_iter().flatten().collect())
    }
}

#[cw_serde]
pub struct StrategyElementBase<T> {
    pub yield_source: YieldSourceBase<T>,
    pub share: Decimal,
}
impl<T: Clone> From<Vec<StrategyElementBase<T>>> for StrategyBase<T> {
    fn from(value: Vec<StrategyElementBase<T>>) -> Self {
        Self(value)
    }
}

pub type StrategyElementUnchecked = StrategyElementBase<Unchecked>;
pub type StrategyElement = StrategyElementBase<Checked>;
