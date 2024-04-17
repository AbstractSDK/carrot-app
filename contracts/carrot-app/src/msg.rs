use abstract_app::objects::AssetEntry;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{wasm_execute, Coin, CosmosMsg, Decimal, Env, Uint128, Uint64};
use cw_asset::AssetBase;

use crate::{
    contract::{App, AppResult},
    distribution::deposit::OneDepositStrategy,
    state::ConfigUnchecked,
    yield_sources::{
        yield_type::{YieldType, YieldTypeUnchecked},
        AssetShare, StrategyElementUnchecked, StrategyUnchecked,
    },
};

// This is used for type safety and re-exporting the contract endpoint structs.
abstract_app::app_msg_types!(App, AppExecuteMsg, AppQueryMsg);

/// App instantiate message
#[cosmwasm_schema::cw_serde]
pub struct AppInstantiateMsg {
    /// Future app configuration
    pub config: ConfigUnchecked,
    /// Future app strategy
    pub strategy: StrategyUnchecked,
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
        /// We might not use a vector but use a (usize, Vec<AssetShare>) instead to avoid having to pass a full vector everytime
        yield_sources_params: Option<Vec<Option<Vec<AssetShare>>>>,
    },
    /// Partial withdraw of the funds available on the app
    /// If amount is omitted, withdraws everything that is on the app
    Withdraw {
        value: Option<Uint128>,
        swap_to: Option<AssetEntry>,
    },
    /// Auto-compounds the pool rewards into the pool
    Autocompound {},
    /// Rebalances all investments according to a new balance strategy
    UpdateStrategy {
        funds: Vec<Coin>,
        strategy: StrategyUnchecked,
    },

    /// Only called by the contract internally   
    Internal(InternalExecuteMsg),
}

#[cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::ExecuteFns))]
#[cfg_attr(feature = "interface", impl_into(ExecuteMsg))]
pub enum InternalExecuteMsg {
    DepositOneStrategy {
        swap_strategy: OneDepositStrategy,
        yield_index: usize,
        yield_type: YieldType,
    },
    /// Execute one Deposit Swap Step
    ExecuteOneDepositSwapStep {
        asset_in: Coin,
        denom_out: String,
        expected_amount: Uint128,
    },
    /// Finalize the deposit after all swaps are executed
    FinalizeDeposit {
        yield_index: usize,
        yield_type: YieldType,
    },
}
impl From<InternalExecuteMsg>
    for abstract_app::abstract_core::base::ExecuteMsg<
        abstract_app::abstract_core::app::BaseExecuteMsg,
        AppExecuteMsg,
    >
{
    fn from(value: InternalExecuteMsg) -> Self {
        Self::Module(AppExecuteMsg::Internal(value))
    }
}

impl InternalExecuteMsg {
    pub fn to_cosmos_msg(&self, env: &Env) -> AppResult<CosmosMsg> {
        Ok(wasm_execute(
            env.contract.address.clone(),
            &ExecuteMsg::Module(AppExecuteMsg::Internal(self.clone())),
            vec![],
        )?
        .into())
    }
}

/// App query messages
#[cosmwasm_schema::cw_serde]
#[cfg_attr(feature = "interface", derive(cw_orch::QueryFns))]
#[cfg_attr(feature = "interface", impl_into(QueryMsg))]
#[derive(QueryResponses)]
pub enum AppQueryMsg {
    #[returns(ConfigUnchecked)]
    Config {},
    #[returns(AssetsBalanceResponse)]
    Balance {},
    #[returns(PositionsResponse)]
    Positions {},
    /// Get the claimable rewards that the position has accumulated.
    /// Returns [`AvailableRewardsResponse`]
    #[returns(AvailableRewardsResponse)]
    AvailableRewards {},
    /// Get the status of the compounding logic of the application
    /// Returns [`CompoundStatusResponse`]
    #[returns(CompoundStatusResponse)]
    CompoundStatus {},
    /// Returns the current strategy as stored in the application
    /// Returns [`StrategyResponse`]
    #[returns(StrategyResponse)]
    Strategy {},
    /// Returns the current funds distribution between all the strategies
    /// Returns [`StrategyResponse`]
    #[returns(StrategyResponse)]
    StrategyStatus {},

    // **** Simulation Endpoints *****/
    // **** These allow to preview what will happen under the hood for each operation inside the Carrot App *****/
    // Their arguments match the arguments of the corresponding Execute Endpoint
    #[returns(DepositPreviewResponse)]
    DepositPreview {
        funds: Vec<Coin>,
        yield_sources_params: Option<Vec<Option<Vec<AssetShare>>>>,
    },
    #[returns(WithdrawPreviewResponse)]
    WithdrawPreview { amount: Option<Uint128> },

    /// Returns a preview of the rebalance distribution
    /// Returns [`RebalancePreviewResponse`]
    #[returns(UpdateStrategyPreviewResponse)]
    UpdateStrategyPreview {
        funds: Vec<Coin>,
        strategy: StrategyUnchecked,
    },
}

#[cosmwasm_schema::cw_serde]
pub struct AppMigrateMsg {}

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
    pub strategy: StrategyUnchecked,
}

#[cw_serde]
pub struct PositionsResponse {
    pub positions: Vec<PositionResponse>,
}

#[cw_serde]
pub struct PositionResponse {
    pub params: YieldTypeUnchecked,
    pub balance: AssetsBalanceResponse,
    pub liquidity: Uint128,
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
pub struct DepositPreviewResponse {
    pub withdraw: Vec<(StrategyElementUnchecked, Decimal)>,
    pub deposit: Vec<InternalExecuteMsg>,
}

#[cw_serde]
pub struct WithdrawPreviewResponse {
    /// Share of the total deposit that will be withdrawn from the app
    pub share: Decimal,
    pub funds: Vec<Coin>,
    pub msgs: Vec<CosmosMsg>,
}

#[cw_serde]
pub struct UpdateStrategyPreviewResponse {
    pub withdraw: Vec<(StrategyElementUnchecked, Decimal)>,
    pub deposit: Vec<InternalExecuteMsg>,
}
