use abstract_app::objects::AnsAsset;
use abstract_sdk::AbstractResponse;
use cosmwasm_std::{DepsMut, Env, Reply};

use crate::{
    contract::{App, AppResult},
    helpers::{add_funds, get_proxy_balance},
    state::{TEMP_CURRENT_ASSET, TEMP_DEPOSIT_ASSETS},
};

pub fn after_swap_reply(deps: DepsMut, _env: Env, app: App, _reply: Reply) -> AppResult {
    let coins_before = TEMP_CURRENT_ASSET.load(deps.storage)?;
    let current_coins = get_proxy_balance(deps.as_ref(), &coins_before.name, &app)?;

    // We just update the coins to deposit after the swap
    if current_coins > coins_before.amount {
        TEMP_DEPOSIT_ASSETS.update(deps.storage, |f| {
            add_funds(
                f,
                AnsAsset::new(coins_before.name, current_coins - coins_before.amount),
            )
        })?;
    }

    Ok(app.response("after_swap_reply"))
}
