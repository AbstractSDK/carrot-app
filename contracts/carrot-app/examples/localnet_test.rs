use abstract_app::objects::{
    module::ModuleInfo, namespace::ABSTRACT_NAMESPACE, AccountId, AnsAsset, AssetEntry,
};
use abstract_client::{Application, Namespace};
use abstract_dex_adapter::{interface::DexAdapter, DEX_ADAPTER_ID};
use abstract_interface::{Abstract, VCQueryFns};
use abstract_sdk::core::ans_host::QueryMsgFns;
use cosmwasm_std::{coins, Decimal, Uint128, Uint64};
use cw_orch::{
    anyhow,
    daemon::{networks::LOCAL_OSMO, Daemon, DaemonBuilder},
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use carrot_app::AppExecuteMsgFns;
use localnet_install::{five_strategy, four_strategy, three_strategy, USER_NAMESPACE};

mod localnet_install;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let mut chain = LOCAL_OSMO;
    chain.grpc_urls = &["http://localhost:9090"];
    chain.chain_id = "osmosis-1";

    let rt = Runtime::new()?;
    let daemon = DaemonBuilder::default()
        .chain(chain)
        .handle(rt.handle())
        .build()?;

    let client = abstract_client::AbstractClient::new(daemon.clone())?;

    // Verify modules exist
    let account = client
        .account_builder()
        .install_on_sub_account(false)
        .namespace(USER_NAMESPACE.try_into()?)
        .build()?;

    let carrot: Application<Daemon, carrot_app::AppInterface<Daemon>> = account.application()?;

    daemon.rt_handle.block_on(
        daemon
            .daemon
            .sender
            .bank_send(account.proxy()?.as_str(), coins(10_000, "uosmo")),
    )?;

    // carrot.deposit(coins(10_000, "uosmo"), None)?;
    carrot.deposit(vec![AnsAsset::new("uosmo", 10_000u128)], None)?;

    carrot.update_strategy(vec![AnsAsset::new("uosmo", 10_000u128)], five_strategy())?;
    carrot.withdraw(None)?;
    carrot.deposit(vec![AnsAsset::new("uosmo", 10_000u128)], None)?;

    Ok(())
}
