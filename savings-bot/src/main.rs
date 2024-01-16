use cosmos_sdk_proto::cosmwasm::wasm::v1::{
    query_client::QueryClient, QueryContractsByCodeRequest,
};
use cw_orch::daemon::sender::Sender;
use cw_orch::prelude::*;
use cw_orch::{
    anyhow,
    daemon::{networks::OSMO_5, Daemon},
    environment::TxHandler,
    tokio::runtime::Runtime,
};
use dotenv::dotenv;
use log::{log, Level};
use tonic::transport::Channel;

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

fn autocompound(daemon: &Daemon, contract_addrs: Vec<String>) -> anyhow::Result<()> {
    for contract in contract_addrs {
        let addr = Addr::unchecked(contract);
        use app::msg::*;
        // TODO: Should look into different query to see the cooldown
        let available_rewards: AvailableRewardsResponse =
        daemon.query(&QueryMsg::from(AppQueryMsg::AvailableRewards {}), &addr)?;
        // If not empty - autocompound
        if !available_rewards.available_rewards.is_empty() {
        // TODO:  Daemon set authZ
            daemon.execute(
                &ExecuteMsg::from(AppExecuteMsg::Autocompound {}),
                &[],
                &addr,
            )?;
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
    let daemon = Daemon::builder()
        .handle(rt.handle())
        .chain(OSMO_5)
        .build()?;
    let code_id = 4583; // TODO: daemon.state().get_code_id(app::contract::APP_ID)?;

    // Get all contracts
    let contract_addrs = rt
        .handle()
        .block_on(fetch_contracts(daemon.channel(), code_id))?;

    // Autocompound
    autocompound(&daemon, contract_addrs)?;
    Ok(())
}
