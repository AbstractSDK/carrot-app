pub mod mars;
pub mod osmosis_cl_pool;
pub mod yield_type;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{ensure, ensure_eq, Decimal, Deps};
use cw_asset::AssetInfo;

use crate::{
    contract::{App, AppResult},
    error::AppError,
    helpers::close_to,
    yield_sources::yield_type::YieldTypeBase,
};
use abstract_app::traits::AbstractNameService;

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

impl From<YieldSource> for YieldSourceUnchecked {
    fn from(value: YieldSource) -> Self {
        Self {
            asset_distribution: value.asset_distribution,
            ty: value.ty.into(),
        }
    }
}

impl Checkable for YieldSourceUnchecked {
    type CheckOutput = YieldSource;
    fn check(self, deps: Deps, app: &App) -> AppResult<YieldSource> {
        // First we check the share sums the 100
        let share_sum: Decimal = self.asset_distribution.iter().map(|e| e.share).sum();
        ensure!(
            close_to(Decimal::one(), share_sum),
            AppError::InvalidStrategySum { share_sum }
        );
        // We make sure that assets are associated with this strategy
        ensure!(
            !self.asset_distribution.is_empty(),
            AppError::InvalidEmptyStrategy {}
        );
        // We ensure all deposited tokens exist in ANS
        let all_denoms = self.all_denoms();
        let ans = app.name_service(deps);
        ans.host()
            .query_assets_reverse(
                &deps.querier,
                &all_denoms
                    .iter()
                    .map(|denom| AssetInfo::native(denom.clone()))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| AppError::AssetsNotRegistered(all_denoms))?;

        let ty = match self.ty {
            YieldTypeBase::ConcentratedLiquidityPool(params) => {
                // A valid CL pool strategy is for 2 assets
                ensure_eq!(
                    self.asset_distribution.len(),
                    2,
                    AppError::InvalidStrategy {}
                );
                YieldTypeBase::ConcentratedLiquidityPool(params.check(deps, app)?)
            }
            YieldTypeBase::Mars(params) => {
                // We verify there is only one element in the shares vector
                ensure_eq!(
                    self.asset_distribution.len(),
                    1,
                    AppError::InvalidStrategy {}
                );
                // We verify the first element correspond to the mars deposit denom
                ensure_eq!(
                    self.asset_distribution[0].denom,
                    params.denom,
                    AppError::InvalidStrategy {}
                );
                YieldTypeBase::Mars(params.check(deps, app)?)
            }
        };

        Ok(YieldSource {
            asset_distribution: self.asset_distribution,
            ty,
        })
    }
}

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

#[cw_serde]
pub struct Checked;
#[cw_serde]
pub struct Unchecked;

pub trait Checkable {
    type CheckOutput;
    fn check(self, deps: Deps, app: &App) -> AppResult<Self::CheckOutput>;
}

// This represents a balance strategy
// This object is used for storing the current strategy, retrieving the actual strategy status or expressing a target strategy when depositing
#[cw_serde]
pub struct BalanceStrategyBase<T: Clone>(pub Vec<BalanceStrategyElementBase<T>>);

pub type BalanceStrategyUnchecked = BalanceStrategyBase<Unchecked>;
pub type BalanceStrategy = BalanceStrategyBase<Checked>;

impl From<BalanceStrategy> for BalanceStrategyUnchecked {
    fn from(value: BalanceStrategy) -> Self {
        Self(value.0.into_iter().map(Into::into).collect())
    }
}

impl Checkable for BalanceStrategyUnchecked {
    type CheckOutput = BalanceStrategy;
    fn check(self, deps: Deps, app: &App) -> AppResult<BalanceStrategy> {
        // First we check the share sums the 100
        let share_sum: Decimal = self.0.iter().map(|e| e.share).sum();
        ensure!(
            close_to(Decimal::one(), share_sum),
            AppError::InvalidStrategySum { share_sum }
        );
        ensure!(!self.0.is_empty(), AppError::InvalidEmptyStrategy {});

        // Then we check every yield strategy underneath

        let checked = self
            .0
            .into_iter()
            .map(|yield_source| yield_source.check(deps, app))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(checked.into())
    }
}

impl BalanceStrategy {
    pub fn all_denoms(&self) -> Vec<String> {
        self.0
            .clone()
            .iter()
            .flat_map(|s| s.yield_source.all_denoms())
            .collect()
    }
}

impl<T: Clone> From<Vec<BalanceStrategyElementBase<T>>> for BalanceStrategyBase<T> {
    fn from(value: Vec<BalanceStrategyElementBase<T>>) -> Self {
        Self(value)
    }
}

#[cw_serde]
pub struct BalanceStrategyElementBase<T> {
    pub yield_source: YieldSourceBase<T>,
    pub share: Decimal,
}

pub type BalanceStrategyElementUnchecked = BalanceStrategyElementBase<Unchecked>;
pub type BalanceStrategyElement = BalanceStrategyElementBase<Checked>;

impl From<BalanceStrategyElement> for BalanceStrategyElementUnchecked {
    fn from(value: BalanceStrategyElement) -> Self {
        Self {
            yield_source: value.yield_source.into(),
            share: value.share,
        }
    }
}
impl Checkable for BalanceStrategyElementUnchecked {
    type CheckOutput = BalanceStrategyElement;
    fn check(self, deps: Deps, app: &App) -> AppResult<BalanceStrategyElement> {
        let yield_source = self.yield_source.check(deps, app)?;
        Ok(BalanceStrategyElement {
            yield_source,
            share: self.share,
        })
    }
}
