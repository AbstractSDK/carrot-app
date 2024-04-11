use crate::contract::{App, AppResult};
use abstract_app::traits::AccountIdentification;
use abstract_app::{objects::AnsAsset, traits::AbstractNameService};
use abstract_money_market_adapter::msg::MoneyMarketQueryMsg;
use abstract_money_market_adapter::MoneyMarketInterface;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{coins, Coin, CosmosMsg, Deps, SubMsg, Uint128};
use cw_asset::AssetInfo;

use abstract_money_market_standard::query::MoneyMarketAnsQuery;

use super::yield_type::YieldTypeImplementation;
use super::ShareType;

pub const MARS_MONEY_MARKET: &str = "mars";

#[cw_serde]
pub struct MarsDepositParams {
    pub denom: String,
}

impl YieldTypeImplementation for MarsDepositParams {
    fn deposit(&mut self, deps: Deps, funds: Vec<Coin>, app: &App) -> AppResult<Vec<SubMsg>> {
        let ans = app.name_service(deps);
        let ans_fund = ans.query(&AssetInfo::native(self.denom.clone()))?;

        Ok(vec![SubMsg::new(
            app.ans_money_market(deps, MARS_MONEY_MARKET.to_string())
                .deposit(AnsAsset::new(ans_fund, funds[0].amount))?,
        )])
    }

    fn withdraw(
        &mut self,
        deps: Deps,
        amount: Option<Uint128>,
        app: &App,
    ) -> AppResult<Vec<CosmosMsg>> {
        let ans = app.name_service(deps);

        let amount = if let Some(amount) = amount {
            amount
        } else {
            self.user_deposit(deps, app)?[0].amount
        };

        let ans_fund = ans.query(&AssetInfo::native(self.denom.clone()))?;

        Ok(vec![app
            .ans_money_market(deps, MARS_MONEY_MARKET.to_string())
            .withdraw(AnsAsset::new(ans_fund, amount))?])
    }

    fn withdraw_rewards(
        &mut self,
        _deps: Deps,
        _app: &App,
    ) -> AppResult<(Vec<Coin>, Vec<CosmosMsg>)> {
        // Mars doesn't have rewards, it's automatically auto-compounded
        Ok((vec![], vec![]))
    }

    fn user_deposit(&mut self, deps: Deps, app: &App) -> AppResult<Vec<Coin>> {
        let ans = app.name_service(deps);
        let asset = ans.query(&AssetInfo::native(self.denom.clone()))?;
        let user = app.account_base(deps)?.proxy;

        let deposit: Uint128 = app
            .ans_money_market(deps, MARS_MONEY_MARKET.to_string())
            .query(MoneyMarketQueryMsg::MoneyMarketAnsQuery {
                query: MoneyMarketAnsQuery::UserDeposit {
                    user: user.to_string(),
                    asset,
                },
                money_market: MARS_MONEY_MARKET.to_string(),
            })?;

        Ok(coins(deposit.u128(), self.denom.clone()))
    }

    fn user_rewards(&mut self, _deps: Deps, _app: &App) -> AppResult<Vec<Coin>> {
        // No rewards, because mars is already auto-compounding

        Ok(vec![])
    }

    fn user_liquidity(&mut self, deps: Deps, app: &App) -> AppResult<Uint128> {
        Ok(self.user_deposit(deps, app)?[0].amount)
    }

    fn share_type(&mut self) -> super::ShareType {
        ShareType::Fixed
    }

    // No cache for mars
    fn clear_cache(&mut self) {}
}
