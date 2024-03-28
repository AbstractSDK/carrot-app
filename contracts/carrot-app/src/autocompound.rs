use abstract_app::abstract_sdk::{feature_objects::AnsHost, Resolve};
use abstract_app::objects::AnsAsset;
use abstract_app::{abstract_core::objects::AssetEntry, objects::DexAssetPairing};
use abstract_dex_adapter::DexInterface;
use abstract_sdk::{Execution, TransferInterface};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    ensure, Addr, CosmosMsg, Deps, Env, MessageInfo, Storage, Timestamp, Uint128, Uint64,
};

use crate::contract::App;
use crate::handlers::swap_helpers::swap_msg;
use crate::msg::CompoundStatus;
use crate::state::{Config, AUTOCOMPOUND_STATE};
use crate::{contract::AppResult, error::AppError};

/// General auto-compound parameters.
/// Includes the cool down and the technical funds config
#[cw_serde]
pub struct AutocompoundConfig {
    /// Seconds to wait before autocompound is incentivized.
    /// Allows the user to configure when the auto-compound happens
    pub cooldown_seconds: Uint64,
    /// Configuration of rewards to the address who helped to execute autocompound
    pub rewards: AutocompoundRewardsConfig,
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

/// Autocompound related methods
impl Config {
    pub fn get_executor_reward_messages(
        &self,
        deps: Deps,
        env: &Env,
        info: MessageInfo,
        app: &App,
    ) -> AppResult<Vec<CosmosMsg>> {
        Ok(
            // If called by non-admin and reward cooldown has ended, send rewards to the contract caller.
            if !app.admin.is_admin(deps, &info.sender)?
                && get_autocompound_status(
                    deps.storage,
                    env,
                    self.autocompound_config.cooldown_seconds.u64(),
                )?
                .is_ready()
            {
                self.autocompound_executor_rewards(deps, env, &info.sender, app)?
            } else {
                vec![]
            },
        )
    }
    pub fn autocompound_executor_rewards(
        &self,
        deps: Deps,
        env: &Env,
        executor: &Addr,
        app: &App,
    ) -> AppResult<Vec<CosmosMsg>> {
        let rewards_config = self.autocompound_config.rewards.clone();

        // Get user balance of gas denom
        let user_gas_balance = app.bank(deps).balance(&rewards_config.gas_asset)?.amount;

        let mut rewards_messages = vec![];

        // If not enough gas coins - swap for some amount
        if user_gas_balance < rewards_config.min_gas_balance {
            // Get asset entries
            let dex = app.ans_dex(deps, self.dex.to_string());

            // Do reverse swap to find approximate amount we need to swap
            let need_gas_coins = rewards_config.max_gas_balance - user_gas_balance;
            let simulate_swap_response = dex.simulate_swap(
                AnsAsset::new(rewards_config.gas_asset.clone(), need_gas_coins),
                rewards_config.swap_asset.clone(),
            )?;

            // Get user balance of swap denom
            let user_swap_balance = app.bank(deps).balance(&rewards_config.swap_asset)?.amount;

            // Swap as much as available if not enough for max_gas_balance
            let swap_amount = simulate_swap_response.return_amount.min(user_swap_balance);

            let msgs = swap_msg(
                deps,
                env,
                AnsAsset::new(rewards_config.swap_asset, swap_amount),
                rewards_config.gas_asset.clone(),
                app,
            )?;
            rewards_messages.extend(msgs);
        }

        // We send their reward to the executor
        let msg_send = app.bank(deps).transfer(
            vec![AnsAsset::new(
                rewards_config.gas_asset,
                rewards_config.reward,
            )],
            executor,
        )?;

        rewards_messages.push(app.executor(deps).execute(vec![msg_send])?.into());

        Ok(rewards_messages)
    }
}

#[cw_serde]
pub struct AutocompoundState {
    pub last_compound: Timestamp,
}

pub fn get_autocompound_status(
    storage: &dyn Storage,
    env: &Env,
    cooldown_seconds: u64,
) -> AppResult<CompoundStatus> {
    let position = AUTOCOMPOUND_STATE.may_load(storage)?;
    let status = match position {
        Some(position) => {
            let ready_on = position.last_compound.plus_seconds(cooldown_seconds);
            if env.block.time >= ready_on {
                CompoundStatus::Ready {}
            } else {
                CompoundStatus::Cooldown((env.block.time.seconds() - ready_on.seconds()).into())
            }
        }
        None => CompoundStatus::NoPosition {},
    };
    Ok(status)
}
