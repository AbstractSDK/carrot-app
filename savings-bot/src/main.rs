use abstract_app::abstract_core::app::{AppConfigResponse, BaseQueryMsg};
use cosmos_sdk_proto::cosmwasm::wasm::v1::{
    query_client::QueryClient, QueryContractsByCodeRequest,
};
use cw_orch::daemon::sender::{Sender, SenderOptions};
use cw_orch::daemon::Wallet;
use cw_orch::prelude::*;
use cw_orch::state::ChainState;
use cw_orch::{
    anyhow,
    daemon::{networks::OSMO_5, Daemon},
    environment::TxHandler,
    tokio::runtime::Runtime,
};
use dotenv::dotenv;
use log::{log, Level};
use tonic::transport::Channel;

use app::msg::{AppExecuteMsg, AppQueryMsg, AvailableRewardsResponse, ExecuteMsg, QueryMsg};

async fn fetch_contracts(channel: Channel, code_id: u64) -> anyhow::Result<Vec<String>> {
    let mut cw_querier = QueryClient::new(channel);
    let contract_addrs = cw_querier
        .contracts_by_code(QueryContractsByCodeRequest {
            code_id,
            // TODO: pagination
            pagination: None,
        })
        .await?
        .into_inner()
        .contracts;
    log!(Level::Info, "Savings addrs: {contract_addrs:?}");
    anyhow::Ok(contract_addrs)
}

fn update_sender_to_grant(daemon: &mut Daemon, contract_addr: &Addr) -> anyhow::Result<()> {
    // Get config of an app to get proxy address(granter)
    let app_config: AppConfigResponse =
        daemon.query(&QueryMsg::Base(BaseQueryMsg::BaseConfig {}), &contract_addr)?;

    // TODO: check if grant is indeed given
    let authz_granter = app_config.proxy_address;
    daemon.set_sender(Wallet::new(Sender::new_with_options(
        &daemon.state(),
        sender_options_constructor(authz_granter.to_string()),
    )?));
    Ok(())
}

fn autocompound(daemon: &mut Daemon, contract_addrs: Vec<String>) -> anyhow::Result<()> {
    for contract in contract_addrs {
        let addr = Addr::unchecked(contract);
        // TODO: Should look into different query to see the cooldown
        let available_rewards: AvailableRewardsResponse =
            daemon.query(&QueryMsg::from(AppQueryMsg::AvailableRewards {}), &addr)?;
        // If not empty - autocompound
        if !available_rewards.available_rewards.is_empty() {
            // Update sender on daemon to use grant of contract
            let sender_update_result = update_sender_to_grant(daemon, &addr);

            // Execute autocompound, if we have grant(s)
            if sender_update_result.is_ok() {
                daemon.execute(
                    &ExecuteMsg::from(AppExecuteMsg::Autocompound {}),
                    &[],
                    &addr,
                )?;
            }
        }
    }

    // We can try to batch it, but it could be PIAS to not gas overflow
    // daemon.daemon.sender.commit_tx(msgs, memo)
    Ok(())
}

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();

    let rt = Runtime::new()?;
    let mut daemon = Daemon::builder()
        .handle(rt.handle())
        .chain(OSMO_5)
        .build()?;
    let code_id = 4583; // TODO: daemon.state().get_code_id(app::contract::APP_ID)?;

    // Get all contracts
    let contract_addrs = rt
        .handle()
        .block_on(fetch_contracts(daemon.channel(), code_id))?;

    // Autocompound
    autocompound(&mut daemon, contract_addrs)?;
    Ok(())
}

// TODO: remove when constructor to SenderOptions added
fn sender_options_constructor(granter: String) -> SenderOptions {
    let mut sender_options = SenderOptions::default();
    sender_options.authz_granter = Some(granter);
    sender_options
}
