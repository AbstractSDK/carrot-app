use abstract_dex_adapter::msg::DexName;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Uint128};
use cw_asset::AssetInfoBase;

use crate::contract::App;

// This is used for type safety and re-exporting the contract endpoint structs.
abstract_app::app_msg_types!(App, AppExecuteMsg, AppQueryMsg);

/// App instantiate message
#[cosmwasm_schema::cw_serde]
pub struct AppInstantiateMsg {
    /// Deposit denomination to accept deposits
    pub deposit_denom: String,
    /// Id of the pool used to get rewards
    pub quasar_pool: String,
    /// Dex that we are ok to swap on !
    pub exchanges: Vec<DexName>,
}

/// App execute messages
#[cosmwasm_schema::cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::ExecuteFns))]
#[cfg_attr(feature = "interface", impl_into(ExecuteMsg))]
pub enum AppExecuteMsg {
    /// Deposit funds onto the app
    // #[cfg_attr(feature = "interface", payable)]
    Deposit {},
    /// Partial withdraw of the funds available on the app
    Withdraw { amount: Uint128 },
    /// Withdraw everything that is on the app
    WithdrawAll {},
    /// Auto-compounds the pool rewards into the pool
    Autocompound {},

    /// Internal swap all funds that are owned by the contract to match the current position ratio
    InternalSwapAll {},
    /// Internal Deposit all the funds in the contract
    InternalDepositAll {},
}

/// App query messages
#[cosmwasm_schema::cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::QueryFns))]
#[cfg_attr(feature = "interface", impl_into(QueryMsg))]
#[derive(QueryResponses)]
pub enum AppQueryMsg {
    #[returns(StateResponse)]
    State {},
    #[returns(AssetsBalanceResponse)]
    Balance {},
    #[returns(AvailableRewardsResponse)]
    AvailableRewards {},
}

#[cosmwasm_schema::cw_serde]
pub enum AppMigrateMsg {}

#[cosmwasm_schema::cw_serde]
pub struct StateResponse {
    pub deposit_info: AssetInfoBase<String>,
    pub quasar_pool: String,
    pub exchanges: Vec<DexName>,
}

#[cosmwasm_schema::cw_serde]
pub struct BalanceResponse {
    pub balance: Vec<Coin>,
}
#[cosmwasm_schema::cw_serde]
pub struct AvailableRewardsResponse {
    pub available_rewards: Vec<Coin>,
}

#[cw_serde]
pub struct AssetsBalanceResponse {
    pub balances: Vec<Coin>,
}
