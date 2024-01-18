use std::collections::HashSet;

use crate::contract::{App, AppResult};
use crate::error::AppError;
use crate::msg::{AppExecuteMsg, ExecuteMsg};
use crate::state::CONFIG;
use abstract_core::ans_host::{AssetPairingFilter, AssetPairingMapEntry};
use abstract_core::objects::{AnsAsset, AssetEntry};
use abstract_dex_adapter::api::Dex;
use abstract_dex_adapter::msg::OfferAsset;
use abstract_dex_adapter::DexInterface;
use abstract_sdk::features::{AbstractNameService, AbstractResponse, AccountIdentification};
use cosmwasm_std::{
    to_json_binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, QueryRequest,
    Response, StdError, Uint128, WasmMsg, WasmQuery,
};
use cw_asset::AssetList;

use crate::cl_vault::msg::ExtensionExecuteMsg;
use crate::cl_vault::msg::ExtensionQueryMsg;
use crate::cl_vault::query::{TotalAssetsResponse, UserSharesBalanceResponse};
use crate::cl_vault::{self, msg::UserBalanceQueryMsg};

use super::query::{query_balances, ContractBalances};
const MAX_SPREAD_PERCENT: u64 = 20;

pub fn execute_handler(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    app: App,
    msg: AppExecuteMsg,
) -> AppResult {
    match msg {
        AppExecuteMsg::Deposit {} => deposit(deps, env, info, app),
        AppExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, Some(amount), app),
        AppExecuteMsg::WithdrawAll {} => withdraw(deps, env, info, None, app),
        AppExecuteMsg::Autocompound {} => autocompound(deps, env, info, app),
        AppExecuteMsg::InternalSwapAll {} => internal_swap_correct_amount(deps, env, info, app),
        AppExecuteMsg::InternalDepositAll {} => {
            if info.sender != env.contract.address {
                return Err(AppError::Unauthorized {});
            }
            internal_deposit_all(deps.as_ref(), env, info, app)
        }
    }
}

fn deposit(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Only the authorized addresses (admin ?) can deposit
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;
    let msg_swap = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::InternalSwapAll {}))?,
        funds: vec![],
    });

    let msg_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::InternalDepositAll {}))?,
        funds: vec![],
    });

    Ok(app
        .response("deposit")
        .add_message(msg_swap)
        .add_message(msg_deposit))
}

fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Option<Uint128>,
    app: App,
) -> AppResult {
    // Only the authorized addresses (admin ?) can withdraw
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;

    let (withdraw_msg, withdraw_amount) = _inner_withdraw(deps, &env, amount, &app)?;

    Ok(app
        .response("withdraw")
        .add_attribute("withdraw_amount", withdraw_amount)
        .add_message(withdraw_msg))
}

fn autocompound(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    // Only the authorized addresses (admin ?) can auto-compound
    app.admin.assert_admin(deps.as_ref(), &info.sender)?;
    let config = CONFIG.load(deps.storage)?;

    let msg_claim = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.quasar_pool.to_string(),
        msg: to_json_binary(&cl_vault::msg::ExecuteMsg::VaultExtension(
            ExtensionExecuteMsg::ClaimRewards {},
        ))?,
        funds: vec![],
    });

    let msg_swap = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::InternalSwapAll {}))?,
        funds: vec![],
    });

    let msg_deposit = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::Module(AppExecuteMsg::InternalDepositAll {}))?,
        funds: vec![],
    });

    Ok(app
        .response("auto-compound")
        .add_message(msg_claim)
        .add_message(msg_swap)
        .add_message(msg_deposit))
}

fn internal_deposit_all(deps: Deps, env: Env, info: MessageInfo, app: App) -> AppResult<Response> {
    if info.sender != env.contract.address {
        return Err(AppError::Unauthorized {});
    }
    let config = CONFIG.load(deps.storage)?;

    // We just want to query the token0 and token1
    let all_quasar_assets: TotalAssetsResponse = deps
        .querier
        .query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.quasar_pool.to_string(),
            msg: to_json_binary(&crate::cl_vault::msg::QueryMsg::TotalAssets {})?,
        }))
        .map_err(|_| StdError::generic_err("Failed to get TotalAssets2"))?;

    // After the swap we can deposit the exact amount of tokens inside the quasar pool
    let funds = query_balances(
        deps,
        &env,
        &all_quasar_assets.token0.denom,
        &all_quasar_assets.token1.denom,
    )
    .map_err(|_| StdError::generic_err("Failed to get self balance 2"))?;

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.quasar_pool.to_string(),
        msg: to_json_binary(&crate::cl_vault::msg::ExecuteMsg::ExactDeposit { recipient: None })?,
        funds: vec![
            Coin {
                denom: all_quasar_assets.token0.denom,
                amount: funds.token0,
            },
            Coin {
                denom: all_quasar_assets.token1.denom,
                amount: funds.token1,
            },
        ],
    });

    Ok(app.response("deposit_all").add_message(msg))
}

fn internal_swap_correct_amount(deps: DepsMut, env: Env, info: MessageInfo, app: App) -> AppResult {
    if info.sender != env.contract.address {
        return Err(AppError::Unauthorized {});
    }
    let config = CONFIG.load(deps.storage)?;
    let ans = app.name_service(deps.as_ref());

    // First we query the pool to know the ratio we can provide liquidity at :
    let all_quasar_assets: TotalAssetsResponse = deps
        .querier
        .query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: config.quasar_pool.to_string(),
            msg: to_json_binary(&crate::cl_vault::msg::QueryMsg::TotalAssets {})?,
        }))
        .map_err(|_| StdError::generic_err("Failed to query TotalAssets"))?;

    let token0 = all_quasar_assets.token0;
    let token1 = all_quasar_assets.token1;
    let quasar_asset_entries =
        ans.query(&AssetList::from(&vec![token0.clone(), token1.clone()]).to_vec())?;
    let asset_pairing_resp: Vec<AssetPairingMapEntry> = ans.pool_list(
        Some(AssetPairingFilter {
            asset_pair: Some((
                quasar_asset_entries[0].name.clone(),
                quasar_asset_entries[1].name.clone(),
            )),
            dex: None,
        }),
        None,
        None,
    )?;

    let exchange_strs: HashSet<&str> = config.exchanges.iter().map(AsRef::as_ref).collect();
    let pair = asset_pairing_resp
        .into_iter()
        .find(|(pair, refs)| !refs.is_empty() && exchange_strs.contains(pair.dex()))
        .ok_or(AppError::NoSwapPossibility {})?
        .0;

    let dex_name = pair.dex();

    let ratio = Decimal::from_ratio(token0.amount, token1.amount);

    // Then we do swaps to get the right ratio of liquidity to provide

    // We query the pool to swap on:
    let balances = query_balances(deps.as_ref(), &env, &token0.denom, &token1.denom)
        .map_err(|_| StdError::generic_err("Failed to query contract balance"))?;

    let funds = ContractBalances {
        token0: AnsAsset {
            name: quasar_asset_entries[0].name.clone(),
            amount: balances.token0,
        },
        token1: AnsAsset {
            name: quasar_asset_entries[1].name.clone(),
            amount: balances.token1,
        },
    };

    let price = get_price_for(
        quasar_asset_entries[0].clone(),
        quasar_asset_entries[1].name.clone(),
        &app.dex(deps.as_ref(), dex_name.to_string()),
    )
    .map_err(|_| {
        StdError::generic_err(format!(
            "Failed to get price for assets: {:?}, {:?}",
            quasar_asset_entries[0], quasar_asset_entries[1].name
        ))
    })?;

    let (offer_asset, ask_asset) = get_swap_for_ratio(funds, price, ratio)?;

    // We swap the right amount of funds
    let dex = app.dex(deps.as_ref(), pair.dex().to_owned());
    let trigger_swap_msg = dex.swap(
        offer_asset.clone(),
        ask_asset.clone(),
        Some(Decimal::percent(MAX_SPREAD_PERCENT)),
        None,
    )?;

    Ok(app.response("swap_all").add_message(trigger_swap_msg))
}

fn _inner_withdraw(
    deps: DepsMut,
    env: &Env,
    amount: Option<Uint128>,
    app: &App,
) -> AppResult<(CosmosMsg, Uint128)> {
    let config = CONFIG.load(deps.storage)?;

    let liquidity_amount = if let Some(amount) = amount {
        amount
    } else {
        let user_shares: UserSharesBalanceResponse = deps.querier.query_wasm_smart(
            config.quasar_pool.to_string(),
            &cl_vault::msg::QueryMsg::VaultExtension(ExtensionQueryMsg::Balances(
                UserBalanceQueryMsg::UserSharesBalance {
                    user: env.contract.address.to_string(),
                },
            )),
        )?;

        user_shares.balance
    };

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.quasar_pool.to_string(),
        msg: to_json_binary(&cl_vault::msg::ExecuteMsg::Redeem {
            recipient: Some(app.account_base(deps.as_ref())?.proxy.to_string()),
            amount: liquidity_amount,
        })?,
        funds: vec![],
    });
    Ok((msg, liquidity_amount))
}

fn get_price_for(token0: OfferAsset, token1: AssetEntry, dex: &Dex<App>) -> AppResult<Decimal> {
    let swap_response = dex.simulate_swap(token0.clone(), token1)?;

    Ok(Decimal::from_ratio(
        swap_response.return_amount,
        token0.amount,
    ))
}

fn get_swap_for_ratio(
    funds: ContractBalances<AnsAsset>,
    price: Decimal,
    ratio: Decimal,
) -> AppResult<(OfferAsset, AssetEntry)> {
    // ratio is considered non-zero and is expected token0/token1

    if ratio * funds.token1.amount <= funds.token0.amount {
        // if ratio <= token0/token1, we need to get more token1. So if x is amount of token0 we offer, we want (token0 - x)/(token1 + px) = r,
        // p is the price (number of a1 per p0)
        let offer_amount =
            Decimal::from_ratio(funds.token0.amount - ratio * funds.token1.amount, 1u128)
                / (Decimal::one() + price * ratio)
                * Uint128::one();

        Ok((
            OfferAsset {
                name: funds.token0.name,
                amount: offer_amount,
            },
            funds.token1.name,
        ))
    } else {
        // else, if ratio < token0/token1, we need to get more token0. So if x is amount of token0 we want to get, we want (token0 + x)/(token1 - px) = r,
        // p is the price (number of a1 per p0)
        let offer_amount =
            Decimal::from_ratio(ratio * funds.token1.amount - funds.token0.amount, 1u128)
                / (Decimal::one() + price * ratio)
                * price
                * Uint128::one();
        Ok((
            OfferAsset {
                name: funds.token1.name,
                amount: offer_amount,
            },
            funds.token0.name,
        ))
    }
}
