use abstract_app::objects::AssetEntry;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Decimal, Uint256, Uint64};
use cw_asset::AssetBase;

use crate::{contract::App, state::AutocompoundRewardsConfig};

// This is used for type safety and re-exporting the contract endpoint structs.
abstract_app::app_msg_types!(App, AppExecuteMsg, AppQueryMsg);

/// App instantiate message
#[cosmwasm_schema::cw_serde]
pub struct AppInstantiateMsg {
    /// Id of the pool used to get rewards
    pub pool_id: u64,
    /// Seconds to wait before autocompound is incentivized.
    pub autocompound_cooldown_seconds: Uint64,
    /// Configuration of rewards to the address who helped to execute autocompound
    pub autocompound_rewards_config: AutocompoundRewardsConfig,
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
    // Slippage
    pub max_spread: Option<Decimal>,
    pub belief_price0: Option<Decimal>,
    pub belief_price1: Option<Decimal>,
}

/// App execute messages
#[cosmwasm_schema::cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::ExecuteFns))]
pub enum AppExecuteMsg {
    /// Update autocompound settings
    UpdateConfig {
        autocompound_cooldown_seconds: Option<Uint64>,
        autocompound_rewards_config: Option<AutocompoundRewardsConfig>,
    },
    /// Create the initial liquidity position
    CreatePosition(CreatePositionMessage),
    /// Deposit funds onto the app
    Deposit {
        funds: Vec<Coin>,
        max_spread: Option<Decimal>,
        belief_price0: Option<Decimal>,
        belief_price1: Option<Decimal>,
    },
    /// Withdraw liquidity available on the position
    Withdraw {
        /// Liquidity amount, withdraws everything when omitted
        amount: Option<Uint256>,
        /// When provided, will swap withdrawn liquidity to single asset
        swap_to: Option<SwapToAsset>,
    },
    /// Auto-compounds the pool rewards into the pool
    Autocompound {},
}

/// App query messages
#[cosmwasm_schema::cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::QueryFns))]
#[derive(QueryResponses)]
pub enum AppQueryMsg {
    /// Get the config of the carrot app
    #[returns(crate::state::Config)]
    Config {},
    /// Get the balance and liquidity of the position
    #[returns(AssetsBalanceResponse)]
    Balance {},
    /// Get the position id
    #[returns(PositionResponse)]
    Position {},
    /// Get the status of the compounding logic of the application and pool rewards
    /// Returns [`CompoundStatusResponse`]
    #[returns(CompoundStatusResponse)]
    CompoundStatus {},
}

#[cosmwasm_schema::cw_serde]
pub struct AppMigrateMsg {}

#[cosmwasm_schema::cw_serde]
pub struct BalanceResponse {
    pub balance: Vec<Coin>,
}

#[cw_serde]
pub struct AssetsBalanceResponse {
    pub balances: Vec<Coin>,
    pub liquidity: String,
}

#[cw_serde]
pub struct PositionResponse {
    pub position_id: Option<u64>,
}

#[cw_serde]
pub struct CompoundStatusResponse {
    pub status: CompoundStatus,
    pub autocompound_reward: AssetBase<String>,
    /// Wether user have enough balance to reward or can swap to get enough
    pub autocompound_reward_available: bool,
    pub spread_rewards: Vec<Coin>,
    pub incentives: Vec<Coin>,
}

#[cw_serde]
/// Wether contract is ready for the compound
pub enum CompoundStatus {
    /// Contract is ready for the compound
    Ready {},
    /// How much seconds left for the next compound
    Cooldown(Uint64),
    /// No open position right now
    NoPosition {},
    /// Position exists in state, but errors on query to the pool
    PositionNotAvailable(u64),
}

impl CompoundStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready {})
    }
}

#[cw_serde]
pub struct SwapToAsset {
    pub to_asset: AssetEntry,
    pub max_spread: Option<Decimal>,
}
