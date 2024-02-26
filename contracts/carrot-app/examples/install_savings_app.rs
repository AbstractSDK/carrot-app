use abstract_app::objects::AccountId;
use abstract_client::AbstractClient;
use cosmwasm_std::{Coin, Uint128, Uint64};
use cw_orch::{
    anyhow,
    daemon::{networks::OSMOSIS_1, Daemon, DaemonBuilder},
    prelude::Stargate,
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use carrot_app::{
    msg::{AppInstantiateMsg, CreatePositionMessage},
    state::AutocompoundRewardsConfig,
};
use osmosis_std::types::cosmos::authz::v1beta1::MsgGrantResponse;

pub struct CarrotAppInitData {
    pub pool_id: u64,
    pub lower_tick: i64,
    pub upper_tick: i64,
    pub funds: Vec<Coin>,
    pub denom0: String,
    pub denom1: String,
    pub asset0: Coin,
    pub asset1: Coin,
    pub swap_denom: String,
}

const AUTOCOMPOUND_COOLDOWN_SECONDS: u64 = 86400;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let chain = OSMOSIS_1;
    let rt = Runtime::new()?;
    let daemon = DaemonBuilder::default()
        .chain(chain)
        .handle(rt.handle())
        .build()?;

    let client = AbstractClient::new(daemon.clone())?;
    let next_local_account_id = client.next_local_account_id()?;

    let savings_app_addr = client.module_instantiate2_address::<carrot_app::AppInterface<Daemon>>(
        &AccountId::local(next_local_account_id),
    )?;

    let funds = vec![Coin {
        denom: usdc_usdc_ax::USDC_AXL.to_owned(),
        amount: Uint128::new(6_000),
    }];

    let app_data = usdc_usdc_ax::app_data(funds, 100_000_000_000_000, 100_000_000_000_000);

    // Give all authzs and create subaccount with app in single tx
    let mut msgs = utils::give_authorizations_msgs(&client, savings_app_addr, &app_data)?;
    
    let init_msg = AppInstantiateMsg {
        pool_id: app_data.pool_id,
        autocompound_cooldown_seconds: Uint64::new(AUTOCOMPOUND_COOLDOWN_SECONDS),
        autocompound_rewards_config: AutocompoundRewardsConfig {
            gas_denom: utils::REWARD_DENOM.to_owned(),
            swap_denom: app_data.swap_denom.to_owned(),
            reward: Uint128::new(50_000),
            min_gas_balance: Uint128::new(1000000),
            max_gas_balance: Uint128::new(3000000),
        },
        create_position: Some(CreatePositionMessage {
            lower_tick: app_data.lower_tick,
            upper_tick: app_data.upper_tick,
            funds: app_data.funds,
            asset0: app_data.asset0,
            asset1: app_data.asset1,
        }),
    };
    let create_sub_account_message = utils::create_account_message(&client, init_msg)?;

    msgs.push(create_sub_account_message);
    let _ = daemon.commit_any::<MsgGrantResponse>(msgs, None)?;

    Ok(())
}

mod usdt_usdc {
    use cosmwasm_std::{Coin, Uint128};

    use crate::CarrotAppInitData;

    const POOL_ID: u64 = 1220;
    const LOWER_TICK: i64 = 100;
    const UPPER_TICK: i64 = 200;
    // USDT
    pub const TOKEN0: &str = "ibc/4ABBEF4C8926DDDB320AE5188CFD63267ABBCEFC0583E4AE05D6E5AA2401DDAB";
    // USDC
    pub const TOKEN1: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";

    pub fn app_data(
        funds: Vec<Coin>,
        asset0_amount: u128,
        asset1_amount: u128,
    ) -> CarrotAppInitData {
        CarrotAppInitData {
            pool_id: POOL_ID,
            lower_tick: LOWER_TICK,
            upper_tick: UPPER_TICK,
            funds,
            denom0: TOKEN0.to_owned(),
            denom1: TOKEN1.to_owned(),
            asset0: Coin {
                denom: TOKEN0.to_owned(),
                amount: Uint128::new(asset0_amount),
            },
            asset1: Coin {
                denom: TOKEN1.to_owned(),
                amount: Uint128::new(asset1_amount),
            },
            swap_denom: TOKEN1.to_owned(),
        }
    }
}

mod usdc_usdc_ax {
    use cosmwasm_std::{Coin, Uint128};

    use crate::CarrotAppInitData;

    pub const USDC_NOBEL: &str =
        "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
    pub const USDC_AXL: &str =
        "ibc/D189335C6E4A68B513C10AB227BF1C1D38C746766278BA3EEB4FB14124F1D858";
    pub const USDC_AXL_POOL_ID: u64 = 1223;
    const LOWER_TICK: i64 = -3700;
    const UPPER_TICK: i64 = 300;

    pub fn app_data(
        funds: Vec<Coin>,
        asset0_amount: u128,
        asset1_amount: u128,
    ) -> CarrotAppInitData {
        CarrotAppInitData {
            pool_id: USDC_AXL_POOL_ID,
            lower_tick: LOWER_TICK,
            upper_tick: UPPER_TICK,
            funds,
            denom0: USDC_NOBEL.to_owned(),
            denom1: USDC_AXL.to_owned(),
            asset0: Coin {
                denom: USDC_NOBEL.to_owned(),
                amount: Uint128::new(asset0_amount),
            },
            asset1: Coin {
                denom: USDC_AXL.to_owned(),
                amount: Uint128::new(asset1_amount),
            },
            swap_denom: USDC_NOBEL.to_owned(),
        }
    }
}

mod utils {
    use super::*;

    pub const LOTS: u128 = 100_000_000_000_000;
    pub const REWARD_DENOM: &str = "uosmo";

    use abstract_app::objects::{
        module::{ModuleInfo, ModuleVersion},
        AccountId,
    };
    use abstract_client::*;
    use abstract_dex_adapter::DEX_ADAPTER_ID;
    use abstract_interface::Abstract;
    use abstract_sdk::core::{account_factory, manager::ModuleInstallConfig};
    use carrot_app::contract::{APP_ID, APP_VERSION};
    use cosmwasm_std::{to_json_binary, to_json_vec};
    use cw_orch::{environment::CwEnv, prelude::*};
    use osmosis_std::types::{
        cosmos::{
            authz::v1beta1::{GenericAuthorization, Grant, MsgGrant},
            bank::v1beta1::SendAuthorization,
        },
        cosmwasm::wasm::v1::MsgExecuteContract,
        osmosis::{
            concentratedliquidity::v1beta1::{
                MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition,
                MsgWithdrawPosition,
            },
            gamm::v1beta1::MsgSwapExactAmountIn,
        },
    };
    use prost::Message;
    use prost_types::Any;
    use std::iter;

    pub fn give_authorizations_msgs<Chain: CwEnv + Stargate>(
        client: &AbstractClient<Chain>,
        savings_app_addr: impl Into<String>,
        app_data: &CarrotAppInitData,
    ) -> Result<Vec<Any>, anyhow::Error> {
        let dex_fee_account = client.account_from(AccountId::local(0))?;
        let dex_fee_addr = dex_fee_account.proxy()?.to_string();
        let chain = client.environment().clone();

        let authorization_urls = [
            MsgCreatePosition::TYPE_URL,
            MsgSwapExactAmountIn::TYPE_URL,
            MsgAddToPosition::TYPE_URL,
            MsgWithdrawPosition::TYPE_URL,
            MsgCollectIncentives::TYPE_URL,
            MsgCollectSpreadRewards::TYPE_URL,
        ]
        .map(ToOwned::to_owned);
        let savings_app_addr: String = savings_app_addr.into();
        let granter = chain.sender().to_string();
        let grantee = savings_app_addr.clone();

        let dex_spend_limit = vec![
        cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: app_data.denom0.to_string(),
            amount: LOTS.to_string(),
        },
        cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: app_data.denom1.to_string(),
            amount: LOTS.to_string(),
        },
        cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: REWARD_DENOM.to_owned(),
            amount: LOTS.to_string(),
        }];
        let dex_fee_authorization = Any {
            value: MsgGrant {
                granter: chain.sender().to_string(),
                grantee: grantee.clone(),
                grant: Some(Grant {
                    authorization: Some(
                        SendAuthorization {
                            spend_limit: dex_spend_limit,
                            allow_list: vec![dex_fee_addr, savings_app_addr],
                        }
                        .to_any(),
                    ),
                    expiration: None,
                }),
            }
            .encode_to_vec(),
            type_url: MsgGrant::TYPE_URL.to_owned(),
        };

        let msgs: Vec<Any> = authorization_urls
            .into_iter()
            .map(|msg| Any {
                value: MsgGrant {
                    granter: granter.clone(),
                    grantee: grantee.clone(),
                    grant: Some(Grant {
                        authorization: Some(GenericAuthorization { msg }.to_any()),
                        expiration: None,
                    }),
                }
                .encode_to_vec(),
                type_url: MsgGrant::TYPE_URL.to_owned(),
            })
            .chain(iter::once(dex_fee_authorization))
            .collect();
        Ok(msgs)
    }

    pub fn create_account_message<Chain: CwEnv>(
        client: &AbstractClient<Chain>,
        init_msg: AppInstantiateMsg,
    ) -> anyhow::Result<Any> {
        let chain = client.environment();
        let abstr = Abstract::load_from(chain.clone())?;
        let account_factory_addr = abstr.account_factory.addr_str()?;
        let next_local_account_id = client.next_local_account_id()?;

        let msg = Any {
            type_url: MsgExecuteContract::TYPE_URL.to_owned(),
            value: MsgExecuteContract {
                sender: chain.sender().to_string(),
                contract: account_factory_addr.to_string(),
                msg: to_json_vec(&account_factory::ExecuteMsg::CreateAccount {
                    governance: GovernanceDetails::Monarchy {
                        monarch: chain.sender().to_string(),
                    },
                    name: "bob".to_owned(),
                    description: None,
                    link: None,
                    base_asset: None,
                    namespace: None,
                    install_modules: vec![
                        ModuleInstallConfig::new(
                            ModuleInfo::from_id(
                                DEX_ADAPTER_ID,
                                ModuleVersion::Version(
                                    abstract_dex_adapter::contract::CONTRACT_VERSION.to_owned(),
                                ),
                            )?,
                            None,
                        ),
                        ModuleInstallConfig::new(
                            ModuleInfo::from_id(
                                APP_ID,
                                ModuleVersion::Version(APP_VERSION.to_owned()),
                            )?,
                            Some(to_json_binary(&init_msg)?),
                        ),
                    ],
                    account_id: Some(AccountId::local(next_local_account_id)),
                })?,
                funds: vec![],
            }
            .to_proto_bytes(),
        };
        Ok(msg)
    }
}
