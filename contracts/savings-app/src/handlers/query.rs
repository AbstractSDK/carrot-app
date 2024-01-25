use abstract_core::objects::AnsAsset;
use abstract_dex_adapter::DexInterface;
use cosmwasm_std::{to_json_binary, Binary, Coin, Decimal, Deps, Env};
use osmosis_std::try_proto_to_cosmwasm_coins;

use crate::{
    contract::{App, AppResult},
    msg::{AppQueryMsg, AssetsBalanceResponse, AvailableRewardsResponse, PositionResponse},
    state::{get_osmosis_position, Config, CONFIG, POSITION},
};

pub fn query_handler(deps: Deps, _env: Env, app: &App, msg: AppQueryMsg) -> AppResult<Binary> {
    match msg {
        AppQueryMsg::Balance {} => to_json_binary(&query_balance(deps, app)?),
        AppQueryMsg::AvailableRewards {} => to_json_binary(&query_rewards(deps, app)?),
        AppQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AppQueryMsg::Position {} => to_json_binary(&query_position(deps)?),
    }
    .map_err(Into::into)
}

fn query_position(deps: Deps) -> AppResult<PositionResponse> {
    let position = POSITION.may_load(deps.storage)?;

    Ok(PositionResponse { position })
}

fn query_config(deps: Deps) -> AppResult<Config> {
    Ok(CONFIG.load(deps.storage)?)
}
fn query_balance(deps: Deps, _app: &App) -> AppResult<AssetsBalanceResponse> {
    let pool = get_osmosis_position(deps)?;

    let balances = try_proto_to_cosmwasm_coins(vec![pool.asset0.unwrap(), pool.asset1.unwrap()])?;
    let liquidity = pool.position.unwrap().liquidity.replace('.', "");
    Ok(AssetsBalanceResponse {
        balances,
        liquidity,
    })
}

fn query_rewards(deps: Deps, _app: &App) -> AppResult<AvailableRewardsResponse> {
    let pool = get_osmosis_position(deps)?;

    let mut rewards = cosmwasm_std::Coins::default();
    for coin in try_proto_to_cosmwasm_coins(pool.claimable_incentives)? {
        rewards.add(coin)?;
    }

    for coin in try_proto_to_cosmwasm_coins(pool.claimable_spread_rewards)? {
        rewards.add(coin)?;
    }

    Ok(AvailableRewardsResponse {
        available_rewards: rewards.into(),
    })
}

pub fn query_price(deps: Deps, funds: &[Coin], app: &App) -> AppResult<Decimal> {
    let config = CONFIG.load(deps.storage)?;

    let amount0 = funds
        .iter()
        .find(|c| c.denom == config.pool_config.token0)
        .map(|c| c.amount)
        .unwrap_or_default();
    let amount1 = funds
        .iter()
        .find(|c| c.denom == config.pool_config.token1)
        .map(|c| c.amount)
        .unwrap_or_default();

    // We take the biggest amount and simulate a swap for the corresponding asset
    let price = if amount0 > amount1 {
        let simulation_result = app.dex(deps, config.exchange).simulate_swap(
            AnsAsset::new(config.pool_config.asset0, amount0),
            config.pool_config.asset1,
        )?;

        Decimal::from_ratio(amount0, simulation_result.return_amount)
    } else {
        let simulation_result = app.dex(deps, config.exchange).simulate_swap(
            AnsAsset::new(config.pool_config.asset1, amount1),
            config.pool_config.asset0,
        )?;

        Decimal::from_ratio(simulation_result.return_amount, amount1)
    };

    Ok(price)
}
