#![allow(unused)]
use abstract_app::objects::{AccountId, AssetEntry};
use abstract_client::AbstractClient;
use cosmwasm_std::{Coin, Uint128, Uint256, Uint64};
use cw_orch::{
    anyhow,
    daemon::{networks::OSMOSIS_1, Daemon, DaemonBuilder},
    prelude::Stargate,
};
use cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::authz::v1beta1::MsgGrantResponse;
use dotenv::dotenv;

use carrot_app::{
    msg::{AppInstantiateMsg, CreatePositionMessage},
    state::AutocompoundRewardsConfig,
};

pub struct CarrotAppInitData {
    pub pool_id: u64,
    pub lower_tick: i64,
    pub upper_tick: i64,
    pub funds: Vec<Coin>,
    pub denom0: String,
    pub denom1: String,
    pub asset0: Coin,
    pub asset1: Coin,
    pub swap_asset: AssetEntry,
}

const AUTOCOMPOUND_COOLDOWN_SECONDS: u64 = 86400;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let chain = OSMOSIS_1;
    let daemon = DaemonBuilder::new(chain).build()?;

    let client = AbstractClient::new(daemon.clone())?;
    let random_account_id = client.random_account_id()?;

    let savings_app_addr = client.module_instantiate2_address::<carrot_app::AppInterface<Daemon>>(
        &AccountId::local(random_account_id),
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
            gas_asset: AssetEntry::new(utils::REWARD_ASSET),
            swap_asset: app_data.swap_asset,
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
            max_spread: None,
            belief_price0: None,
            belief_price1: None,
        }),
    };
    let create_sub_account_message = utils::create_account_message(&client, init_msg)?;

    msgs.push(create_sub_account_message);
    let _ = daemon.commit_any(msgs, None)?;

    Ok(())
}

mod usdt_usdc {
    use abstract_app::objects::AssetEntry;
    use cosmwasm_std::{Coin, Uint128};

    use crate::{usdc_usdc_ax::USDC_NOBLE_ASSET, CarrotAppInitData};

    const POOL_ID: u64 = 1220;
    const LOWER_TICK: i64 = -100000;
    const UPPER_TICK: i64 = 10000;
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
            swap_asset: AssetEntry::new(USDC_NOBLE_ASSET),
        }
    }
}

mod usdc_usdc_ax {
    use abstract_app::objects::AssetEntry;
    use cosmwasm_std::{Coin, Uint128};

    use crate::CarrotAppInitData;

    pub const USDC_NOBLE: &str =
        "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
    pub const USDC_NOBLE_ASSET: &str = "noble>usdc";
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
            denom0: USDC_NOBLE.to_owned(),
            denom1: USDC_AXL.to_owned(),
            asset0: Coin {
                denom: USDC_NOBLE.to_owned(),
                amount: Uint128::new(asset0_amount),
            },
            asset1: Coin {
                denom: USDC_AXL.to_owned(),
                amount: Uint128::new(asset1_amount),
            },
            swap_asset: AssetEntry::new(USDC_NOBLE_ASSET),
        }
    }
}

mod utils {
    use super::*;

    pub const LOTS: u128 = 100_000_000_000_000;
    pub const REWARD_ASSET: &str = "osmosis>osmo";

    use abstract_app::std::account::ModuleInstallConfig;
    use abstract_app::{
        objects::{
            module::{ModuleInfo, ModuleVersion},
            salt, AccountId,
        },
        std::account,
    };
    use abstract_client::*;
    use abstract_dex_adapter::DEX_ADAPTER_ID;
    use abstract_interface::Abstract;
    use carrot_app::contract::{APP_ID, APP_VERSION};
    use cosmwasm_std::{to_json_binary, to_json_vec};
    use cw_orch::{environment::CwEnv, prelude::*};
    use cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::{
        cosmos::{
            authz::v1beta1::{GenericAuthorization, Grant, MsgGrant},
            bank::v1beta1::SendAuthorization,
        },
        cosmwasm::wasm::v1::{MsgExecuteContract, MsgInstantiateContract2},
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
        let dex_fee_account = client.fetch_account(AccountId::local(0))?;
        let dex_fee_addr = dex_fee_account.address()?.to_string();
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
        let granter = chain.sender_addr().to_string();
        let grantee = savings_app_addr.clone();

        let reward_denom = client
            .name_service()
            .resolve(&AssetEntry::new(REWARD_ASSET))?;

        let mut dex_spend_limit = vec![
        cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: app_data.denom0.to_string(),
            amount: LOTS.to_string(),
        },
        cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: app_data.denom1.to_string(),
            amount: LOTS.to_string(),
        },
        cw_orch_osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: reward_denom.to_string(),
            amount: LOTS.to_string(),
        }];
        dex_spend_limit.sort_unstable_by(|a, b| a.denom.cmp(&b.denom));
        let dex_fee_authorization = Any {
            value: MsgGrant {
                granter: chain.sender_addr().to_string(),
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
        let random_account_id = client.random_account_id()?;
        let salt = salt::generate_instantiate_salt(&AccountId::local(random_account_id));
        let code_id = abstr.account_code_id()?;
        let creator = chain.sender_addr().to_string();

        let account_address = chain.wasm_querier().instantiate2_addr(
            code_id,
            &Addr::unchecked(creator.clone()),
            salt.clone(),
        )?;

        let msg = Any {
            type_url: MsgInstantiateContract2::TYPE_URL.to_owned(),
            value: MsgInstantiateContract2 {
                sender: creator,
                admin: account_address,
                code_id,
                label: "Abstract Account".to_owned(),
                msg: to_json_vec(&account::InstantiateMsg::<Empty> {
                    code_id,
                    owner: GovernanceDetails::Monarchy {
                        monarch: chain.sender_addr().to_string(),
                    },
                    name: Some("bob".to_owned()),
                    description: None,
                    link: None,
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
                    account_id: Some(AccountId::local(random_account_id)),
                    authenticator: None,
                })?,
                funds: vec![],
                salt: salt.to_vec(),
                fix_msg: false,
            }
            .to_proto_bytes(),
        };
        Ok(msg)
    }

    pub fn create_sub_account_message<Chain: CwEnv>(
        client: &AbstractClient<Chain>,
        account: &Account<Chain>,
        init_msg: AppInstantiateMsg,
    ) -> anyhow::Result<Any> {
        let chain = client.environment();
        let random_account_id = client.random_account_id()?;

        let msg = Any {
            type_url: MsgExecuteContract::TYPE_URL.to_owned(),
            value: MsgExecuteContract {
                sender: chain.sender_addr().to_string(),
                contract: account.address()?.to_string(),
                msg: to_json_vec(
                    &abstract_app::std::account::ExecuteMsg::<Empty>::CreateSubAccount {
                        name: Some("deep-adventurous-afternoon".to_owned()),
                        description: None,
                        link: None,
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
                        account_id: Some(random_account_id),
                    },
                )?,
                funds: vec![],
            }
            .to_proto_bytes(),
        };
        Ok(msg)
    }
}
