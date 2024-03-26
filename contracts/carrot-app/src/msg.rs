use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Uint128, Uint64};
use cw_asset::AssetBase;

use crate::{
    contract::App,
    state::AutocompoundConfig,
    yield_sources::{yield_type::YieldType, BalanceStrategy, ExpectedToken, OneDepositStrategy},
};

// This is used for type safety and re-exporting the contract endpoint structs.
abstract_app::app_msg_types!(App, AppExecuteMsg, AppQueryMsg);

/// App instantiate message
#[cosmwasm_schema::cw_serde]
pub struct AppInstantiateMsg {
    /// Strategy to use to dispatch the deposited funds
    pub balance_strategy: BalanceStrategy,
    /// Configuration of the aut-compounding procedure
    pub autocompound_config: AutocompoundConfig,
    /// Target dex to swap things on
    pub dex: String,
    /// Create position with instantiation.
    /// Will not create position if omitted
    pub deposit: Option<Vec<Coin>>,
}

/// App execute messages
#[cosmwasm_schema::cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::ExecuteFns))]
#[cfg_attr(feature = "interface", impl_into(ExecuteMsg))]
pub enum AppExecuteMsg {
    /// Deposit funds onto the app
    /// Those funds will be distributed between yield sources according to the current strategy
    /// TODO : for now only send stable coins that have the same value as USD
    /// More tokens can be included when the oracle adapter is live
    Deposit {
        funds: Vec<Coin>,
        /// This is additional paramters used to change the funds repartition when doing an additional deposit
        /// This is not used for a first deposit into a strategy that hasn't changed for instance
        /// This is an options because this is not mandatory
        /// The vector then has option inside of it because we might not want to change parameters for all strategies
        /// We might not use a vector but use a (usize, Vec<ExpectedToken>) instead to avoid having to pass a full vector everytime
        yield_sources_params: Option<Vec<Option<Vec<ExpectedToken>>>>,
    },
    /// Partial withdraw of the funds available on the app
    /// If amount is omitted, withdraws everything that is on the app
    Withdraw { amount: Option<Uint128> },
    /// Auto-compounds the pool rewards into the pool
    Autocompound {},
    /// Rebalances all investments according to a new balance strategy
    Rebalance { strategy: BalanceStrategy },

    /// Only called by the contract internally
    DepositOneStrategy {
        swap_strategy: OneDepositStrategy,
        yield_type: YieldType,
        yield_index: usize,
    },
    /// Execute one Deposit Swap Step
    ExecuteOneDepositSwapStep {
        asset_in: Coin,
        denom_out: String,
        expected_amount: Uint128,
    },
    /// Finalize the deposit after all swaps are executed
    FinalizeDeposit {
        yield_type: YieldType,
        yield_index: usize,
    },
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
    /// Get the claimable rewards that the position has accumulated.
    /// Returns [`AvailableRewardsResponse`]
    #[returns(AvailableRewardsResponse)]
    AvailableRewards {},
    /// Get the status of the compounding logic of the application
    /// Returns [`CompoundStatusResponse`]
    #[returns(CompoundStatusResponse)]
    CompoundStatus {},
    /// Returns the current strategy
    /// Returns [`StrategyResponse`]
    #[returns(StrategyResponse)]
    Strategy {},
    /// Returns a preview of the rebalance distribution
    /// Returns [`RebalancePreviewResponse`]
    #[returns(RebalancePreviewResponse)]
    RebalancePreview {},
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
    pub total_value: Uint128,
}

#[cw_serde]
pub struct StrategyResponse {
    pub strategy: BalanceStrategy,
}

#[cw_serde]
pub struct CompoundStatusResponse {
    pub status: CompoundStatus,
    pub reward: AssetBase<String>,
    // Wether user have enough balance to reward or can swap
    pub rewards_available: bool,
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
}

impl CompoundStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready {})
    }
}

#[cw_serde]
pub struct RebalancePreviewResponse {}
