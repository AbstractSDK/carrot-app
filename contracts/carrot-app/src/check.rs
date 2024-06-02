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

    use cosmwasm_std::{ensure, Decimal, Deps};

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
                reward_percent: value.reward_percent,
                _phantom: PhantomData,
            }
        }
    }

    impl AutocompoundRewardsConfigUnchecked {
        pub fn check(
            self,
            _deps: Deps,
            _app: &App,
            _dex_name: &str,
        ) -> AppResult<AutocompoundRewardsConfig> {
            ensure!(
                self.reward_percent <= Decimal::one(),
                AppError::RewardConfigError("reward percents should be lower than 100%".to_owned())
            );
            Ok(AutocompoundRewardsConfig {
                reward_percent: self.reward_percent,
                _phantom: PhantomData,
            })
        }
    }

    impl From<Config> for ConfigUnchecked {
        fn from(value: Config) -> Self {
            Self {
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
            yield_type::{YieldParamsBase, YieldType, YieldTypeUnchecked},
            Strategy, StrategyElement, StrategyElementUnchecked, StrategyUnchecked, YieldSource,
            YieldSourceUnchecked,
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
                    position_cache: self.position_cache,
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
                    YieldParamsBase::ConcentratedLiquidityPool(params) => {
                        YieldParamsBase::ConcentratedLiquidityPool(ConcentratedPoolParamsBase {
                            pool_id: params.pool_id,
                            lower_tick: params.lower_tick,
                            upper_tick: params.upper_tick,
                            position_id: params.position_id,
                            _phantom: std::marker::PhantomData,
                            position_cache: params.position_cache,
                        })
                    }
                    YieldParamsBase::Mars(params) => YieldParamsBase::Mars(params),
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
                    params: value.params.into(),
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

                let params = match self.params {
                    YieldParamsBase::ConcentratedLiquidityPool(params) => {
                        // A valid CL pool strategy is for 2 assets
                        ensure_eq!(
                            self.asset_distribution.len(),
                            2,
                            AppError::InvalidStrategy {}
                        );
                        YieldParamsBase::ConcentratedLiquidityPool(params.check(deps, app)?)
                    }
                    YieldParamsBase::Mars(params) => {
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
                        YieldParamsBase::Mars(params.check(deps, app)?)
                    }
                };

                Ok(YieldSource {
                    asset_distribution: self.asset_distribution,
                    params,
                })
            }
        }
    }

    mod strategy {
        use super::*;

        impl From<StrategyElement> for StrategyElementUnchecked {
            fn from(value: StrategyElement) -> Self {
                Self {
                    yield_source: value.yield_source.into(),
                    share: value.share,
                }
            }
        }
        impl Checkable for StrategyElementUnchecked {
            type CheckOutput = StrategyElement;
            fn check(self, deps: Deps, app: &App) -> AppResult<StrategyElement> {
                let yield_source = self.yield_source.check(deps, app)?;
                Ok(StrategyElement {
                    yield_source,
                    share: self.share,
                })
            }
        }

        impl From<Strategy> for StrategyUnchecked {
            fn from(value: Strategy) -> Self {
                Self(value.0.into_iter().map(Into::into).collect())
            }
        }

        impl Checkable for StrategyUnchecked {
            type CheckOutput = Strategy;
            fn check(self, deps: Deps, app: &App) -> AppResult<Strategy> {
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
