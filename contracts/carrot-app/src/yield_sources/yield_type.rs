use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, CosmosMsg, Deps, SubMsg, Uint128};

use crate::contract::{App, AppResult};

use super::{mars::MarsDepositParams, osmosis_cl_pool::ConcentratedPoolParams, ShareType};

#[cw_serde]
pub enum YieldType {
    ConcentratedLiquidityPool(ConcentratedPoolParams),
    /// For Mars, you just need to deposit in the RedBank
    /// You need to indicate the denom of the funds you want to deposit
    Mars(MarsDepositParams),
}

impl YieldType {
    pub fn deposit(self, deps: Deps, funds: Vec<Coin>, app: &App) -> AppResult<Vec<SubMsg>> {
        if funds.is_empty() {
            return Ok(vec![]);
        }
        match self {
            YieldType::ConcentratedLiquidityPool(params) => params.deposit(deps, funds, app),
            YieldType::Mars(params) => params.deposit(deps, funds, app),
        }
    }

    pub fn withdraw(
        self,
        deps: Deps,
        amount: Option<Uint128>,
        app: &App,
    ) -> AppResult<Vec<CosmosMsg>> {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => params.withdraw(deps, amount, app),
            YieldType::Mars(params) => params.withdraw(deps, amount, app),
        }
    }

    pub fn withdraw_rewards(self, deps: Deps, app: &App) -> AppResult<(Vec<Coin>, Vec<CosmosMsg>)> {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => params.withdraw_rewards(deps, app),
            YieldType::Mars(params) => params.withdraw_rewards(deps, app),
        }
    }

    pub fn user_deposit(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>> {
        let user_deposit_result = match self {
            YieldType::ConcentratedLiquidityPool(params) => params.user_deposit(deps, app),
            YieldType::Mars(params) => params.user_deposit(deps, app),
        };
        Ok(user_deposit_result.unwrap_or_default())
    }

    pub fn user_rewards(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>> {
        let user_deposit_result = match self {
            YieldType::ConcentratedLiquidityPool(params) => params.user_rewards(deps, app),
            YieldType::Mars(params) => params.user_rewards(deps, app),
        };
        Ok(user_deposit_result.unwrap_or_default())
    }

    pub fn user_liquidity(&self, deps: Deps, app: &App) -> AppResult<Uint128> {
        let user_deposit_result = match self {
            YieldType::ConcentratedLiquidityPool(params) => params.user_liquidity(deps, app),
            YieldType::Mars(params) => params.user_liquidity(deps, app),
        };
        Ok(user_deposit_result.unwrap_or_default())
    }

    /// Indicate the default funds allocation
    /// This is specifically useful for auto-compound as we're not able to input target amounts
    /// CL pools use that to know the best funds deposit ratio
    /// Mars doesn't use that, because the share is fixed to 1
    pub fn share_type(&self) -> ShareType {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => params.share_type(),
            YieldType::Mars(params) => params.share_type(),
        }
    }
}

pub trait YieldTypeImplementation {
    fn deposit(self, deps: Deps, funds: Vec<Coin>, app: &App) -> AppResult<Vec<SubMsg>>;

    fn withdraw(self, deps: Deps, amount: Option<Uint128>, app: &App) -> AppResult<Vec<CosmosMsg>>;

    fn withdraw_rewards(self, deps: Deps, app: &App) -> AppResult<(Vec<Coin>, Vec<CosmosMsg>)>;

    fn user_deposit(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>>;

    fn user_rewards(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>>;

    fn user_liquidity(&self, deps: Deps, app: &App) -> AppResult<Uint128>;

    /// Indicate the default funds allocation
    /// This is specifically useful for auto-compound as we're not able to input target amounts
    /// CL pools use that to know the best funds deposit ratio
    /// Mars doesn't use that, because the share is fixed to 1
    fn share_type(&self) -> ShareType;

    /// Verifies the yield type is valid
    fn check(&self, deps: Deps) -> AppResult<()>;
}
