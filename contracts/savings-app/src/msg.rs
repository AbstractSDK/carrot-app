use abstract_dex_adapter::msg::DexName;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Uint128};

use crate::{contract::App, state::Position};

// This is used for type safety and re-exporting the contract endpoint structs.
abstract_app::app_msg_types!(App, AppExecuteMsg, AppQueryMsg);

/// App instantiate message
#[cosmwasm_schema::cw_serde]
pub struct AppInstantiateMsg {
    /// Deposit denomination to accept deposits
    pub deposit_denom: String,
    /// Id of the pool used to get rewards
    pub pool_id: u64,
    /// Dex that we are ok to swap on !
    pub exchanges: Vec<DexName>,
    /// Create position with instantiation.
    /// Will not create position if omitted
    pub create_position: Option<CreatePositionMessage>,
}

#[cosmwasm_schema::cw_serde]
pub struct CreatePositionMessage {
    pub lower_tick: i64,
    pub upper_tick: i64,
    // Funds to use to deposit on the account
    pub funds: Vec<Coin>,
    /// The two next fields indicate the token0/token1 ratio we want to deposit inside the current ticks
    pub asset0: Coin,
    pub asset1: Coin,
}

/// App execute messages
#[cosmwasm_schema::cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::ExecuteFns))]
#[cfg_attr(feature = "interface", impl_into(ExecuteMsg))]
pub enum AppExecuteMsg {
    /// Create the initial liquidity position
    CreatePosition(CreatePositionMessage),
    /// Deposit funds onto the app
    Deposit { funds: Vec<Coin> },
    /// Partial withdraw of the funds available on the app
    Withdraw { amount: Uint128 },
    /// Withdraw everything that is on the app
    WithdrawAll {},
    /// Auto-compounds the pool rewards into the pool
    Autocompound {},
}

/// App query messages
#[cosmwasm_schema::cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::QueryFns))]
#[cfg_attr(feature = "interface", impl_into(QueryMsg))]
#[derive(QueryResponses)]
pub enum AppQueryMsg {
    #[returns(crate::state::Config)]
    Config {},
    #[returns(AssetsBalanceResponse)]
    Balance {},
    #[returns(AvailableRewardsResponse)]
    AvailableRewards {},
    #[returns(PositionResponse)]
    Position {},
}

#[cosmwasm_schema::cw_serde]
pub enum AppMigrateMsg {}

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
    pub liquidity: String,
}

#[cw_serde]
pub struct PositionResponse {
    pub position: Option<Position>,
}
