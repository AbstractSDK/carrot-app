use abstract_app::sdk::{feature_objects::AnsHost, Resolve};
use abstract_app::{objects::DexAssetPairing, std::objects::AssetEntry};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    ensure, Deps, Env, MessageInfo, QuerierWrapper, StdResult, Storage, Timestamp, Uint128, Uint64,
};
use cw_storage_plus::Item;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    ConcentratedliquidityQuerier, FullPositionBreakdown,
};

use crate::msg::SwapToAsset;
use crate::{contract::AppResult, error::AppError, msg::CompoundStatus};

const POSITION: Item<Position> = Item::new("position2");
const LAST_COMPOUND: Item<Timestamp> = Item::new("last_compound");
pub const CONFIG: Item<Config> = Item::new("config2");

#[cw_serde]
struct Position {
    pub position_id: u64,
}

/// Type for handling position created by the carrot app and compound status
#[derive(Clone)]
pub struct CarrotPosition {
    pub id: u64,
    pub position: FullPositionBreakdown,
}

impl CarrotPosition {
    /// Private method
    /// Load position id if it's stored in the state
    fn may_load_id(storage: &dyn Storage) -> StdResult<Option<u64>> {
        let maybe_position = POSITION.may_load(storage)?;

        Ok(maybe_position.map(|position| position.position_id))
    }

    /// Load position, returns `Ok(None)` if no valid position found
    pub fn may_load(deps: Deps) -> StdResult<Option<Self>> {
        if let Some(id) = Self::may_load_id(deps.storage)? {
            if let Some(position) = may_load_osmosis_position(&deps.querier, id) {
                return Ok(Some(Self { id, position }));
            }
        }
        Ok(None)
    }

    /// Load position, errors if no valid position found
    pub fn load(deps: Deps) -> Result<Self, AppError> {
        Self::may_load(deps)?.ok_or(AppError::NoPosition {})
    }

    /// Save position
    pub fn save_position(
        storage: &mut dyn Storage,
        compound_timestamp: &Timestamp,
        position_id: u64,
    ) -> StdResult<()> {
        POSITION.save(storage, &Position { position_id })?;
        LAST_COMPOUND.save(storage, compound_timestamp)?;
        Ok(())
    }

    /// Get the status of compound
    pub fn compound_status(
        deps: Deps,
        env: &Env,
        cooldown_seconds: u64,
    ) -> AppResult<(CompoundStatus, Option<Self>)> {
        let status = match Self::may_load_id(deps.storage)? {
            Some(id) => {
                // If saved position but can't query - return position id
                let Some(position) = may_load_osmosis_position(&deps.querier, id) else {
                    return Ok((CompoundStatus::PositionNotAvailable(id), None));
                };
                let ready_on = LAST_COMPOUND
                    .load(deps.storage)?
                    .plus_seconds(cooldown_seconds);
                if env.block.time >= ready_on {
                    (CompoundStatus::Ready {}, Some(Self { id, position }))
                } else {
                    (
                        CompoundStatus::Cooldown(
                            (ready_on.seconds() - env.block.time.seconds()).into(),
                        ),
                        Some(Self { id, position }),
                    )
                }
            }
            None => (CompoundStatus::NoPosition {}, None),
        };
        Ok(status)
    }
}

// Helper to load osmosis position from id, returns `None` if position by id not found
fn may_load_osmosis_position(
    querier: &QuerierWrapper,
    position_id: u64,
) -> Option<FullPositionBreakdown> {
    ConcentratedliquidityQuerier::new(querier)
        .position_by_id(position_id)
        .map(|position_response| position_response.position.unwrap())
        .ok()
}

// Temp state
pub const TEMP_WITHDRAW_TO_ASSET: Item<SwapToAsset> = Item::new("wta");

#[cw_serde]
pub struct Config {
    pub pool_config: PoolConfig,
    pub autocompound_cooldown_seconds: Uint64,
    pub autocompound_rewards_config: AutocompoundRewardsConfig,
}

/// Configuration on how rewards should be distributed
/// to the address who helped to execute autocompound
#[cw_serde]
pub struct AutocompoundRewardsConfig {
    /// Gas denominator for this chain
    pub gas_asset: AssetEntry,
    /// Denominator of the asset that will be used for swap to the gas asset
    pub swap_asset: AssetEntry,
    /// Reward amount
    pub reward: Uint128,
    /// If gas token balance falls below this bound a swap will be generated
    pub min_gas_balance: Uint128,
    /// Upper bound of gas tokens expected after the swap
    pub max_gas_balance: Uint128,
}

impl AutocompoundRewardsConfig {
    pub fn check(&self, deps: Deps, dex_name: &str, ans_host: &AnsHost) -> AppResult<()> {
        ensure!(
            self.reward <= self.min_gas_balance,
            AppError::RewardConfigError(
                "reward should be lower or equal to the min_gas_balance".to_owned()
            )
        );
        ensure!(
            self.max_gas_balance > self.min_gas_balance,
            AppError::RewardConfigError(
                "max_gas_balance has to be bigger than min_gas_balance".to_owned()
            )
        );

        // Check swap asset has pairing into gas asset
        DexAssetPairing::new(self.gas_asset.clone(), self.swap_asset.clone(), dex_name)
            .resolve(&deps.querier, ans_host)?;

        Ok(())
    }
}

#[cw_serde]
pub struct PoolConfig {
    pub pool_id: u64,
    pub asset0: AssetEntry,
    pub asset1: AssetEntry,
}

pub fn assert_contract(info: &MessageInfo, env: &Env) -> AppResult<()> {
    if info.sender == env.contract.address {
        Ok(())
    } else {
        Err(AppError::Unauthorized {})
    }
}
