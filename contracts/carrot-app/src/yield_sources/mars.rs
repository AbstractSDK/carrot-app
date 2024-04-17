use crate::ans_assets::AnsAssets;
use crate::contract::{App, AppResult};
use crate::error::AppError;
use abstract_app::objects::AnsAsset;
use abstract_app::objects::AssetEntry;
use abstract_app::traits::AccountIdentification;
use abstract_money_market_adapter::msg::MoneyMarketQueryMsg;
use abstract_money_market_adapter::MoneyMarketInterface;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{CosmosMsg, Deps, SubMsg, Uint128};

use abstract_money_market_standard::query::MoneyMarketAnsQuery;

use super::yield_type::YieldTypeImplementation;
use super::ShareType;

pub const MARS_MONEY_MARKET: &str = "mars";

#[cw_serde]
pub struct MarsDepositParams {
    /// This should stay a denom because that's a parameter that's accepted by Mars when depositing/withdrawing
    pub asset: AssetEntry,
}

impl YieldTypeImplementation for MarsDepositParams {
    fn deposit(&self, deps: Deps, funds: AnsAssets, app: &App) -> AppResult<Vec<SubMsg>> {
        Ok(vec![SubMsg::new(
            app.ans_money_market(deps, MARS_MONEY_MARKET.to_string())
                .deposit(AnsAsset::new(
                    self.asset.clone(),
                    funds
                        .into_iter()
                        .next()
                        .ok_or(AppError::NoDeposit {})?
                        .amount,
                ))?,
        )])
    }

    fn withdraw(
        &self,
        deps: Deps,
        amount: Option<Uint128>,
        app: &App,
    ) -> AppResult<Vec<CosmosMsg>> {
        let amount = if let Some(amount) = amount {
            amount
        } else {
            self.user_deposit(deps, app)?
                .into_iter()
                .next()
                .ok_or(AppError::NoDeposit {})?
                .amount
        };

        Ok(vec![app
            .ans_money_market(deps, MARS_MONEY_MARKET.to_string())
            .withdraw(AnsAsset::new(self.asset.clone(), amount))?])
    }

    fn withdraw_rewards(&self, _deps: Deps, _app: &App) -> AppResult<(AnsAssets, Vec<CosmosMsg>)> {
        // Mars doesn't have rewards, it's automatically auto-compounded
        Ok((AnsAssets::default(), vec![]))
    }

    fn user_deposit(&self, deps: Deps, app: &App) -> AppResult<AnsAssets> {
        let user = app.account_base(deps)?.proxy;

        let deposit: Uint128 = app
            .ans_money_market(deps, MARS_MONEY_MARKET.to_string())
            .query(MoneyMarketQueryMsg::MoneyMarketAnsQuery {
                query: MoneyMarketAnsQuery::UserDeposit {
                    user: user.to_string(),
                    asset: self.asset.clone(),
                },
                money_market: MARS_MONEY_MARKET.to_string(),
            })?;

        Ok(vec![AnsAsset::new(self.asset.clone(), deposit.u128())].try_into()?)
    }

    fn user_rewards(&self, _deps: Deps, _app: &App) -> AppResult<AnsAssets> {
        // No rewards, because mars is already auto-compounding

        Ok(AnsAssets::default())
    }

    fn user_liquidity(&self, deps: Deps, app: &App) -> AppResult<Uint128> {
        Ok(self
            .user_deposit(deps, app)?
            .into_iter()
            .next()
            .map(|d| d.amount)
            .unwrap_or(Uint128::zero()))
    }

    fn share_type(&self) -> super::ShareType {
        ShareType::Fixed
    }
}
