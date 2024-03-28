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
};
use abstract_app::traits::AbstractNameService;

use self::yield_type::YieldType;

/// A yield sources has the following elements
/// A vector of tokens that NEED to be deposited inside the yield source with a repartition of tokens
/// A type that allows routing to the right smart-contract integration internally
#[cw_serde]
pub struct YieldSource {
    /// This id (denom, share)
    pub asset_distribution: Vec<ExpectedToken>,
    pub ty: YieldType,
}

impl YieldSource {
    pub fn check(&self, deps: Deps, app: &App) -> AppResult<()> {
        // First we check the share sums the 100
        let share_sum: Decimal = self.asset_distribution.iter().map(|e| e.share).sum();
        ensure!(
            close_to(Decimal::one(), share_sum),
            AppError::InvalidStrategySum { share_sum }
        );
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

        // Then we check every yield strategy underneath
        match &self.ty {
            YieldType::ConcentratedLiquidityPool(params) => {
                // A valid CL pool strategy is for 2 assets
                ensure_eq!(
                    self.asset_distribution.len(),
                    2,
                    AppError::InvalidStrategy {}
                );
                params.check(deps)?;
            }
            YieldType::Mars(denom) => {
                // We verify there is only one element in the shares vector
                ensure_eq!(
                    self.asset_distribution.len(),
                    1,
                    AppError::InvalidStrategy {}
                );
                // We verify the first element correspond to the mars deposit denom
                ensure_eq!(
                    &self.asset_distribution[0].denom,
                    denom,
                    AppError::InvalidStrategy {}
                );
            }
        }

        Ok(())
    }

    pub fn all_denoms(&self) -> Vec<String> {
        self.asset_distribution
            .iter()
            .map(|e| e.denom.clone())
            .collect()
    }
}

#[cw_serde]
pub struct ExpectedToken {
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

// Related to balance strategies
#[cw_serde]
pub struct BalanceStrategy(pub Vec<BalanceStrategyElement>);

#[cw_serde]
pub struct BalanceStrategyElement {
    pub yield_source: YieldSource,
    pub share: Decimal,
}
impl BalanceStrategyElement {
    pub fn check(&self, deps: Deps, app: &App) -> AppResult<()> {
        self.yield_source.check(deps, app)
    }
}

impl BalanceStrategy {
    pub fn check(&self, deps: Deps, app: &App) -> AppResult<()> {
        // First we check the share sums the 100
        let share_sum: Decimal = self.0.iter().map(|e| e.share).sum();
        ensure!(
            close_to(Decimal::one(), share_sum),
            AppError::InvalidStrategySum { share_sum }
        );
        ensure!(!self.0.is_empty(), AppError::InvalidEmptyStrategy {});

        // Then we check every yield strategy underneath
        for yield_source in &self.0 {
            yield_source.check(deps, app)?;
        }

        Ok(())
    }

    pub fn all_denoms(&self) -> Vec<String> {
        self.0
            .clone()
            .iter()
            .flat_map(|s| s.yield_source.all_denoms())
            .collect()
    }
}
