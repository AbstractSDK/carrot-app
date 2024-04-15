use abstract_app::objects::{AnsAsset, AssetEntry};
use abstract_dex_adapter::DexInterface;
use cosmwasm_std::{CosmosMsg, Decimal, Deps, Env};
pub const MAX_SPREAD: Decimal = Decimal::percent(20);
pub const DEFAULT_SLIPPAGE: Decimal = Decimal::permille(5);

use crate::contract::{App, AppResult, OSMOSIS};

pub(crate) fn swap_msg(
    deps: Deps,
    _env: &Env,
    offer_asset: AnsAsset,
    ask_asset: AssetEntry,
    app: &App,
) -> AppResult<Option<CosmosMsg>> {
    // Don't swap if not required
    if offer_asset.amount.is_zero() {
        return Ok(None);
    }

    let dex = app.ans_dex(deps, OSMOSIS.to_string());
    let swap_msg = dex.swap(offer_asset, ask_asset, Some(MAX_SPREAD), None)?;

    Ok(Some(swap_msg))
}
