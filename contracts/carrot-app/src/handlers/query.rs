use abstract_app::{
    abstract_core::objects::AnsAsset,
    traits::{AbstractNameService, Resolve},
};
use abstract_dex_adapter::DexInterface;
use cosmwasm_std::{ensure, to_json_binary, Binary, Coin, Decimal, Deps, Env};
use cw_asset::Asset;
use osmosis_std::try_proto_to_cosmwasm_coins;

use crate::{
    contract::{App, AppResult, OSMOSIS},
    error::AppError,
    handlers::swap_helpers::DEFAULT_MAX_SPREAD,
    helpers::{get_balance, get_user},
    msg::{AppQueryMsg, AssetsBalanceResponse, CompoundStatusResponse, PositionResponse},
    state::{CarrotPosition, Config, CONFIG},
};

pub fn query_handler(deps: Deps, env: Env, app: &App, msg: AppQueryMsg) -> AppResult<Binary> {
    match msg {
        AppQueryMsg::Balance {} => to_json_binary(&query_balance(deps, app)?),
        AppQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AppQueryMsg::Position {} => to_json_binary(&query_position(deps)?),
        AppQueryMsg::CompoundStatus {} => to_json_binary(&query_compound_status(deps, env, app)?),
    }
    .map_err(Into::into)
}

/// Gets the status of the compounding logic of the application
/// Accounts for the user's ability to pay for the gas fees of executing the contract.
fn query_compound_status(deps: Deps, env: Env, app: &App) -> AppResult<CompoundStatusResponse> {
    let config = CONFIG.load(deps.storage)?;

    let (status, maybe_carrot_position) =
        CarrotPosition::compound_status(deps, &env, config.autocompound_cooldown_seconds.u64())?;

    let (spread_rewards, incentives) = if let Some(carrot_position) = maybe_carrot_position {
        (
            try_proto_to_cosmwasm_coins(carrot_position.position.claimable_spread_rewards)?,
            try_proto_to_cosmwasm_coins(carrot_position.position.claimable_incentives)?,
        )
    } else {
        (vec![], vec![])
    };

    let gas_denom = config
        .autocompound_rewards_config
        .gas_asset
        .resolve(&deps.querier, &app.ans_host(deps)?)?;

    // Get user gas balance
    let user = get_user(deps, app)?;
    let user_gas_balance = gas_denom.query_balance(&deps.querier, user.clone())?;

    // Get rewards available
    let reward = Asset::new(gas_denom.clone(), config.autocompound_rewards_config.reward);
    let rewards_available = if user_gas_balance >= reward.amount {
        true
    } else {
        // check if can swap
        let rewards_config = config.autocompound_rewards_config;
        let dex = app.ans_dex(deps, OSMOSIS.to_string());

        // Reverse swap to see how many swap coins needed
        let required_gas_coins = reward.amount - user_gas_balance;
        let response = dex.simulate_swap(
            AnsAsset::new(rewards_config.gas_asset, required_gas_coins),
            rewards_config.swap_asset.clone(),
        )?;

        // Check if user has enough of swap coins
        let user_swap_balance = get_balance(rewards_config.swap_asset, deps, user, app)?;
        let required_swap_amount = response.return_amount;

        user_swap_balance > required_swap_amount
    };

    Ok(CompoundStatusResponse {
        status,
        autocompound_reward: reward.into(),
        autocompound_reward_available: rewards_available,
        spread_rewards,
        incentives,
    })
}

fn query_position(deps: Deps) -> AppResult<PositionResponse> {
    let position_id = CarrotPosition::may_load(deps)?.map(|carrot_position| carrot_position.id);
    Ok(PositionResponse { position_id })
}

fn query_config(deps: Deps) -> AppResult<Config> {
    Ok(CONFIG.load(deps.storage)?)
}

fn query_balance(deps: Deps, _app: &App) -> AppResult<AssetsBalanceResponse> {
    let Some(carrot_position) = CarrotPosition::may_load(deps)? else {
        return Ok(AssetsBalanceResponse {
            balances: vec![],
            liquidity: "0".to_string(),
        });
    };

    let balances = try_proto_to_cosmwasm_coins(vec![
        carrot_position.position.asset0.unwrap(),
        carrot_position.position.asset1.unwrap(),
    ])?;
    // Decimal to atomics
    let liquidity = carrot_position
        .position
        .position
        .unwrap()
        .liquidity
        .replace('.', "");
    Ok(AssetsBalanceResponse {
        balances,
        liquidity,
    })
}

pub fn query_price(
    deps: Deps,
    funds: &[Coin],
    app: &App,
    max_spread: Option<Decimal>,
    belief_price0: Option<Decimal>,
    belief_price1: Option<Decimal>,
) -> AppResult<Decimal> {
    let config = CONFIG.load(deps.storage)?;
    let ans_host = app.ans_host(deps)?;

    // We know it's native denom for osmosis pool
    let token0 = config
        .pool_config
        .asset0
        .resolve(&deps.querier, &ans_host)?
        .inner();
    let token1 = config
        .pool_config
        .asset1
        .resolve(&deps.querier, &ans_host)?
        .inner();

    let amount0 = funds
        .iter()
        .find(|c| c.denom == token0)
        .map(|c| c.amount)
        .unwrap_or_default();
    let amount1 = funds
        .iter()
        .find(|c| c.denom == token1)
        .map(|c| c.amount)
        .unwrap_or_default();

    // We take the biggest amount and simulate a swap for the corresponding asset
    let price = if amount0 > amount1 {
        let simulation_result = app.ans_dex(deps, OSMOSIS.to_string()).simulate_swap(
            AnsAsset::new(config.pool_config.asset0, amount0),
            config.pool_config.asset1,
        )?;

        let price = Decimal::from_ratio(amount0, simulation_result.return_amount);
        if let Some(belief_price) = belief_price1 {
            ensure!(
                belief_price.abs_diff(price) <= max_spread.unwrap_or(DEFAULT_MAX_SPREAD),
                AppError::MaxSpreadAssertion { price }
            );
        }
        price
    } else {
        let simulation_result = app.ans_dex(deps, OSMOSIS.to_string()).simulate_swap(
            AnsAsset::new(config.pool_config.asset1, amount1),
            config.pool_config.asset0,
        )?;

        let price = Decimal::from_ratio(simulation_result.return_amount, amount1);
        if let Some(belief_price) = belief_price0 {
            ensure!(
                belief_price.abs_diff(price) <= max_spread.unwrap_or(DEFAULT_MAX_SPREAD),
                AppError::MaxSpreadAssertion { price }
            );
        }
        price
    };

    Ok(price)
}
