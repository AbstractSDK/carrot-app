use cosmwasm_schema::cw_serde;
use cosmwasm_std::Deps;

use crate::contract::{App, AppResult};

#[cw_serde]
pub struct Checked;
#[cw_serde]
pub struct Unchecked;

pub trait Checkable {
    type CheckOutput;
    fn check(self, deps: Deps, app: &App) -> AppResult<Self::CheckOutput>;
}

mod config {
    use std::marker::PhantomData;

    use abstract_app::{
        abstract_sdk::Resolve, objects::DexAssetPairing, traits::AbstractNameService,
    };
    use cosmwasm_std::{ensure, Deps};

    use crate::{
        autocompound::{
            AutocompoundConfigBase, AutocompoundRewardsConfig, AutocompoundRewardsConfigUnchecked,
        },
        contract::{App, AppResult},
        error::AppError,
        state::{Config, ConfigUnchecked},
    };

    use super::Checkable;
    impl From<AutocompoundRewardsConfig> for AutocompoundRewardsConfigUnchecked {
        fn from(value: AutocompoundRewardsConfig) -> Self {
            Self {
                gas_asset: value.gas_asset,
                swap_asset: value.swap_asset,
                reward: value.reward,
                min_gas_balance: value.min_gas_balance,
                max_gas_balance: value.max_gas_balance,
                _phantom: PhantomData,
            }
        }
    }

    impl AutocompoundRewardsConfigUnchecked {
        pub fn check(
            self,
            deps: Deps,
            app: &App,
            dex_name: &str,
        ) -> AppResult<AutocompoundRewardsConfig> {
            ensure!(
                self.reward <= self.min_gas_balance,
                AppError::RewardConfigError(
                    "reward should be lower or equal to the min_gas_balance".to_owned()
                )
            );
            ensure!(
                self.max_gas_balance > self.min_gas_balance,
                AppError::RewardConfigError(
                    "max_gas_balance has to be bigger than min_gas_balance".to_owned()
                )
            );

            // Check swap asset has pairing into gas asset
            DexAssetPairing::new(self.gas_asset.clone(), self.swap_asset.clone(), dex_name)
                .resolve(&deps.querier, app.name_service(deps).host())?;

            Ok(AutocompoundRewardsConfig {
                gas_asset: self.gas_asset,
                swap_asset: self.swap_asset,
                reward: self.reward,
                min_gas_balance: self.min_gas_balance,
                max_gas_balance: self.max_gas_balance,
                _phantom: PhantomData,
            })
        }
    }

    impl From<Config> for ConfigUnchecked {
        fn from(value: Config) -> Self {
            Self {
                balance_strategy: value.balance_strategy.into(),
                autocompound_config: value.autocompound_config.into(),
                dex: value.dex,
            }
        }
    }

    impl Checkable for ConfigUnchecked {
        type CheckOutput = Config;

        fn check(
            self,
            deps: cosmwasm_std::Deps,
            app: &crate::contract::App,
        ) -> crate::contract::AppResult<Self::CheckOutput> {
            Ok(Config {
                balance_strategy: self.balance_strategy.check(deps, app)?,
                autocompound_config: AutocompoundConfigBase {
                    cooldown_seconds: self.autocompound_config.cooldown_seconds,
                    rewards: self
                        .autocompound_config
                        .rewards
                        .check(deps, app, &self.dex)?,
                },
                dex: self.dex,
            })
        }
    }
}

mod yield_sources {
    use std::marker::PhantomData;

    use cosmwasm_std::{ensure, ensure_eq, Decimal, Deps};
    use cw_asset::AssetInfo;
    use osmosis_std::types::osmosis::{
        concentratedliquidity::v1beta1::Pool, poolmanager::v1beta1::PoolmanagerQuerier,
    };

    use crate::{
        contract::{App, AppResult},
        error::AppError,
        helpers::close_to,
        yield_sources::{
            osmosis_cl_pool::{
                ConcentratedPoolParams, ConcentratedPoolParamsBase, ConcentratedPoolParamsUnchecked,
            },
            yield_type::{YieldType, YieldTypeBase, YieldTypeUnchecked},
            BalanceStrategy, BalanceStrategyElement, BalanceStrategyElementUnchecked,
            BalanceStrategyUnchecked, YieldSource, YieldSourceUnchecked,
        },
    };

    use super::Checkable;

    mod params {
        use crate::yield_sources::mars::MarsDepositParams;

        use super::*;
        impl Checkable for ConcentratedPoolParamsUnchecked {
            type CheckOutput = ConcentratedPoolParams;
            fn check(self, deps: Deps, _app: &App) -> AppResult<ConcentratedPoolParams> {
                let _pool: Pool = PoolmanagerQuerier::new(&deps.querier)
                    .pool(self.pool_id)
                    .map_err(|_| AppError::PoolNotFound {})?
                    .pool
                    .ok_or(AppError::PoolNotFound {})?
                    .try_into()?;
                Ok(ConcentratedPoolParams {
                    pool_id: self.pool_id,
                    lower_tick: self.lower_tick,
                    upper_tick: self.upper_tick,
                    position_id: self.position_id,
                    _phantom: PhantomData,
                })
            }
        }

        impl Checkable for MarsDepositParams {
            type CheckOutput = MarsDepositParams;

            fn check(self, _deps: Deps, _app: &App) -> AppResult<Self::CheckOutput> {
                Ok(self)
            }
        }
    }
    mod yield_type {
        use super::*;

        impl From<YieldType> for YieldTypeUnchecked {
            fn from(value: YieldType) -> Self {
                match value {
                    YieldTypeBase::ConcentratedLiquidityPool(params) => {
                        YieldTypeBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                            pool_id: params.pool_id,
                            lower_tick: params.lower_tick,
                            upper_tick: params.upper_tick,
                            position_id: params.position_id,
                            _phantom: std::marker::PhantomData,
                        })
                    }
                    YieldTypeBase::Mars(params) => YieldTypeBase::Mars(params),
                }
            }
        }
    }
    mod yield_source {
        use super::*;
        use abstract_app::traits::AbstractNameService;

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
    }

    mod balance_strategy {
        use super::*;

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
    }
}
