use cosmwasm_schema::cw_serde;
use cosmwasm_std::{coins, Coin, CosmosMsg, Deps, Env, SubMsg, Uint128};
use osmosis_std::types::osmosis::{
    concentratedliquidity::v1beta1::Pool, poolmanager::v1beta1::PoolmanagerQuerier,
};

use crate::{
    contract::{App, AppResult},
    error::AppError,
};

use super::{mars, osmosis_cl_pool, ShareType};

/// Denomination of a bank / token-factory / IBC token.
pub type Denom = String;

#[cw_serde]
pub enum YieldType {
    /// For osmosis CL Pools, you need a pool id to do your deposit, and that's all
    ConcentratedLiquidityPool(ConcentratedPoolParams),
    /// For Mars, you just need to deposit in the RedBank
    /// You need to indicate the denom of the funds you want to deposit
    Mars(Denom),
}

impl YieldType {
    pub fn deposit(
        self,
        deps: Deps,
        env: &Env,
        funds: Vec<Coin>,
        app: &App,
    ) -> AppResult<Vec<SubMsg>> {
        if funds.is_empty() {
            return Ok(vec![]);
        }
        match self {
            YieldType::ConcentratedLiquidityPool(params) => {
                osmosis_cl_pool::deposit(deps, env, params, funds, app)
            }
            YieldType::Mars(denom) => mars::deposit(deps, denom, funds[0].amount, app),
        }
    }

    pub fn withdraw(
        self,
        deps: Deps,
        amount: Option<Uint128>,
        app: &App,
    ) -> AppResult<Vec<CosmosMsg>> {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => {
                osmosis_cl_pool::withdraw(deps, amount, app, params)
            }
            YieldType::Mars(denom) => mars::withdraw(deps, denom, amount, app),
        }
    }

    pub fn withdraw_rewards(self, deps: Deps, app: &App) -> AppResult<(Vec<Coin>, Vec<CosmosMsg>)> {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => {
                osmosis_cl_pool::withdraw_rewards(deps, params, app)
            }
            YieldType::Mars(denom) => mars::withdraw_rewards(deps, denom, app),
        }
    }

    pub fn user_deposit(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>> {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => {
                osmosis_cl_pool::user_deposit(deps, app, params.clone())
            }
            YieldType::Mars(denom) => Ok(coins(
                mars::user_deposit(deps, denom.clone(), app)?.into(),
                denom,
            )),
        }
    }

    pub fn user_rewards(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>> {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => {
                osmosis_cl_pool::user_rewards(deps, app, params.clone())
            }
            YieldType::Mars(denom) => mars::user_rewards(deps, denom.clone(), app),
        }
    }

    pub fn user_liquidity(&self, deps: Deps, app: &App) -> AppResult<Uint128> {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => {
                osmosis_cl_pool::user_liquidity(deps, app, params.clone())
            }
            YieldType::Mars(denom) => mars::user_liquidity(deps, denom.clone(), app),
        }
    }

    /// Indicate the default funds allocation
    /// This is specifically useful for auto-compound as we're not able to input target amounts
    /// CL pools use that to know the best funds deposit ratio
    /// Mars doesn't use that, because the share is fixed to 1
    pub fn share_type(&self) -> ShareType {
        match self {
            YieldType::ConcentratedLiquidityPool(_) => ShareType::Dynamic,
            YieldType::Mars(_) => ShareType::Fixed,
        }
    }
}

#[cw_serde]
pub struct ConcentratedPoolParams {
    pub pool_id: u64,
    pub lower_tick: i64,
    pub upper_tick: i64,
    pub position_id: Option<u64>,
}

impl ConcentratedPoolParams {
    pub fn check(&self, deps: Deps) -> AppResult<()> {
        let _pool: Pool = PoolmanagerQuerier::new(&deps.querier)
            .pool(self.pool_id)?
            .pool
            .ok_or(AppError::PoolNotFound {})?
            .try_into()?;
        Ok(())
    }
}
