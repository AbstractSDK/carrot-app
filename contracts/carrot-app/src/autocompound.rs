use std::marker::PhantomData;

use abstract_sdk::{Execution, ExecutorMsg, TransferInterface};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Decimal, Deps, Env, MessageInfo, Storage, Timestamp, Uint64};

use crate::check::{Checked, Unchecked};
use crate::contract::App;
use crate::contract::AppResult;
use crate::msg::CompoundStatus;
use crate::state::{Config, AUTOCOMPOUND_STATE, CONFIG};

pub type AutocompoundConfig = AutocompoundConfigBase<Checked>;
pub type AutocompoundConfigUnchecked = AutocompoundConfigBase<Unchecked>;

/// General auto-compound parameters.
/// Includes the cool down and the technical funds config
#[cw_serde]
pub struct AutocompoundConfigBase<T> {
    /// Seconds to wait before autocompound is incentivized.
    /// Allows the user to configure when the auto-compound happens
    pub cooldown_seconds: Uint64,
    /// Configuration of rewards to the address who helped to execute autocompound
    pub rewards: AutocompoundRewardsConfigBase<T>,
}

impl From<AutocompoundConfig> for AutocompoundConfigUnchecked {
    fn from(value: AutocompoundConfig) -> Self {
        Self {
            cooldown_seconds: value.cooldown_seconds,
            rewards: value.rewards.into(),
        }
    }
}

/// Configuration on how rewards should be distributed
/// to the address who helped to execute autocompound
#[cw_serde]
pub struct AutocompoundRewardsConfigBase<T> {
    /// Percentage of the withdraw, rewards that will be sent to the auto-compounder
    pub reward_percent: Decimal,
    pub _phantom: PhantomData<T>,
}

pub type AutocompoundRewardsConfigUnchecked = AutocompoundRewardsConfigBase<Unchecked>;
pub type AutocompoundRewardsConfig = AutocompoundRewardsConfigBase<Checked>;

/// Autocompound related methods
impl Config {
    pub fn get_executor_reward_messages(
        &self,
        deps: Deps,
        env: &Env,
        info: MessageInfo,
        rewards: &[Coin],
        app: &App,
    ) -> AppResult<ExecutorRewards> {
        let config = CONFIG.load(deps.storage)?;
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
                let funds: Vec<Coin> = rewards
                    .iter()
                    .flat_map(|a| {
                        let reward_amount =
                            a.amount * config.autocompound_config.rewards.reward_percent;

                        Some(Coin::new(reward_amount.into(), a.denom.clone()))
                    })
                    .collect();
                ExecutorRewards {
                    funds: funds.clone(),
                    msg: Some(
                        app.executor(deps)
                            .execute(vec![app.bank(deps).transfer(funds, &info.sender)?])?,
                    ),
                }
            } else {
                ExecutorRewards {
                    funds: vec![],
                    msg: None,
                }
            },
        )
    }
}

pub struct ExecutorRewards {
    pub funds: Vec<Coin>,
    pub msg: Option<ExecutorMsg>,
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
