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

const POOL_ID: u64 = 1220;
const AUTOCOMPOUND_COOLDOWN_SECONDS: u64 = 86400;
const LOWER_TICK: i64 = 100;
const UPPER_TICK: i64 = 200;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let chain = OSMOSIS_1;
    let rt = Runtime::new()?;
    let daemon = DaemonBuilder::default()
        .chain(chain)
        .handle(rt.handle())
        .build()?;

    // let sender_addr = daemon.sender();
    // panic!("{:?}", sender_addr);

    let client = AbstractClient::new(daemon.clone())?;
    let next_local_account_id = client.next_local_account_id()?;

    let savings_app_addr = client.module_instantiate2_address::<carrot_app::AppInterface<Daemon>>(
        &AccountId::local(next_local_account_id),
    )?;
    let funds = vec![Coin {
        denom: utils::TOKEN1.to_owned(),
        amount: Uint128::new(300_000),
    }];
    let init_msg = AppInstantiateMsg {
        pool_id: POOL_ID,
        // 5 mins
        autocompound_cooldown_seconds: Uint64::new(AUTOCOMPOUND_COOLDOWN_SECONDS),
        autocompound_rewards_config: AutocompoundRewardsConfig {
            gas_denom: utils::REWARD_DENOM.to_owned(),
            swap_denom: utils::TOKEN1.to_owned(),
            reward: Uint128::new(50_000),
            min_gas_balance: Uint128::new(1000000),
            max_gas_balance: Uint128::new(3000000),
        },
        create_position: Some(CreatePositionMessage {
            lower_tick: LOWER_TICK,
            upper_tick: UPPER_TICK,
            funds,
            asset0: Coin {
                denom: utils::TOKEN0.to_owned(),
                amount: Uint128::new(1000137456),
            },
            asset1: Coin {
                denom: utils::TOKEN0.to_owned(),
                amount: Uint128::new(1000000000),
            },
        }),
    };
    // Give all authzs and create subaccount with app in single tx
    let mut msgs = utils::give_authorizations_msgs(&client, savings_app_addr)?;
    let create_sub_account_message = utils::create_account_message(&client, init_msg)?;

    msgs.push(create_sub_account_message);
    let _ = daemon.commit_any::<MsgGrantResponse>(msgs, None)?;

    Ok(())
}

mod utils {

    use std::iter;

    use super::*;

    pub const LOTS: u128 = 100_000_000_000_000;
    // USDT
    pub const TOKEN0: &str = "ibc/4ABBEF4C8926DDDB320AE5188CFD63267ABBCEFC0583E4AE05D6E5AA2401DDAB";
    // USDC
    pub const TOKEN1: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";

    pub const REWARD_DENOM: &str = "uosmo";

    use abstract_app::objects::{module::ModuleInfo, AccountId};
    use abstract_client::*;
    use abstract_dex_adapter::DEX_ADAPTER_ID;
    use abstract_interface::Abstract;
    use abstract_sdk::core::{account_factory, manager::ModuleInstallConfig};
    use carrot_app::contract::APP_ID;
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

    pub fn give_authorizations_msgs<Chain: CwEnv + Stargate>(
        client: &AbstractClient<Chain>,
        savings_app_addr: impl Into<String>,
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
            denom: TOKEN0.to_string(),
            amount: LOTS.to_string(),
        },
        cw_orch::osmosis_test_tube::osmosis_test_tube::osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: TOKEN1.to_string(),
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
                        ModuleInstallConfig::new(ModuleInfo::from_id_latest(DEX_ADAPTER_ID)?, None),
                        ModuleInstallConfig::new(
                            ModuleInfo::from_id_latest(APP_ID)?,
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
