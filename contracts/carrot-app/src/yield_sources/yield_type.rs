use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, CosmosMsg, Deps, SubMsg, Uint128};

use crate::{
    check::{Checked, Unchecked},
    contract::{App, AppResult},
};

use super::{mars::MarsDepositParams, osmosis_cl_pool::ConcentratedPoolParamsBase, ShareType};

// This however is not checkable by itself, because the check also depends on the asset share distribution
#[cw_serde]
pub enum YieldTypeBase<T> {
    ConcentratedLiquidityPool(ConcentratedPoolParamsBase<T>),
    /// For Mars, you just need to deposit in the RedBank
    /// You need to indicate the denom of the funds you want to deposit
    Mars(MarsDepositParams),
}

pub type YieldTypeUnchecked = YieldTypeBase<Unchecked>;
pub type YieldType = YieldTypeBase<Checked>;

impl YieldTypeImplementation for YieldType {
    fn deposit(&self, deps: Deps, funds: Vec<Coin>, app: &App) -> AppResult<Vec<SubMsg>> {
        if funds.is_empty() {
            return Ok(vec![]);
        }
        self.inner().deposit(deps, funds, app)
    }

    fn withdraw(
        &self,
        deps: Deps,
        amount: Option<Uint128>,
        app: &App,
    ) -> AppResult<Vec<CosmosMsg>> {
        self.inner().withdraw(deps, amount, app)
    }

    fn withdraw_rewards(&self, deps: Deps, app: &App) -> AppResult<(Vec<Coin>, Vec<CosmosMsg>)> {
        self.inner().withdraw_rewards(deps, app)
    }

    fn user_deposit(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>> {
        Ok(self.inner().user_deposit(deps, app).unwrap_or_default())
    }

    fn user_rewards(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>> {
        Ok(self.inner().user_rewards(deps, app).unwrap_or_default())
    }

    fn user_liquidity(&self, deps: Deps, app: &App) -> AppResult<Uint128> {
        Ok(self.inner().user_liquidity(deps, app).unwrap_or_default())
    }

    /// Indicate the default funds allocation
    /// This is specifically useful for auto-compound as we're not able to input target amounts
    /// CL pools use that to know the best funds deposit ratio
    /// Mars doesn't use that, because the share is fixed to 1
    fn share_type(&self) -> ShareType {
        self.inner().share_type()
    }
}

impl YieldType {
    fn inner(&self) -> &dyn YieldTypeImplementation {
        match self {
            YieldType::ConcentratedLiquidityPool(params) => params,
            YieldType::Mars(params) => params,
        }
    }
}

pub trait YieldTypeImplementation {
    fn deposit(&self, deps: Deps, funds: Vec<Coin>, app: &App) -> AppResult<Vec<SubMsg>>;

    fn withdraw(&self, deps: Deps, amount: Option<Uint128>, app: &App)
        -> AppResult<Vec<CosmosMsg>>;

    fn withdraw_rewards(&self, deps: Deps, app: &App) -> AppResult<(Vec<Coin>, Vec<CosmosMsg>)>;

    fn user_deposit(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>>;

    fn user_rewards(&self, deps: Deps, app: &App) -> AppResult<Vec<Coin>>;

    fn user_liquidity(&self, deps: Deps, app: &App) -> AppResult<Uint128>;

    /// Indicate the default funds allocation
    /// This is specifically useful for auto-compound as we're not able to input target amounts
    /// CL pools use that to know the best funds deposit ratio
    /// Mars doesn't use that, because the share is fixed to 1
    fn share_type(&self) -> ShareType;
}
