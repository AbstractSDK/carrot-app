use abstract_sdk::AbstractResponse;
use cosmwasm_std::{coin, DepsMut, Env, Reply};

use crate::{
    contract::{App, AppResult},
    helpers::{add_funds, get_proxy_balance},
    state::{TEMP_CURRENT_COIN, TEMP_DEPOSIT_COINS},
};

pub fn after_swap_reply(deps: DepsMut, _env: Env, app: App, _reply: Reply) -> AppResult {
    let coins_before = TEMP_CURRENT_COIN.load(deps.storage)?;
    let current_coins = get_proxy_balance(deps.as_ref(), &app, coins_before.denom)?;
    // We just update the coins to deposit after the swap
    if current_coins.amount > coins_before.amount {
        TEMP_DEPOSIT_COINS.update(deps.storage, |f| {
            add_funds(
                f,
                coin(
                    (current_coins.amount - coins_before.amount).into(),
                    current_coins.denom,
                ),
            )
        })?;
    }
    deps.api.debug("Swap reply over");

    Ok(app.response("after_swap_reply"))
}
