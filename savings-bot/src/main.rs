use abstract_app::abstract_core::app::{AppConfigResponse, BaseQueryMsg};
use app::msg::{AppExecuteMsg, AppQueryMsg, AvailableRewardsResponse, ExecuteMsg, QueryMsg};
use cosmos_sdk_proto::cosmwasm::wasm::v1::{
    query_client::QueryClient, QueryContractsByCodeRequest,
};
use cw_orch::{
    anyhow,
    daemon::{networks::OSMO_5, queriers::Authz, Daemon},
    environment::TxHandler,
    prelude::*,
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

// TODO: are we using wasm execute grant here?
const MSG_TYPE_URL: &str = "";

fn daemon_with_savings_authz(daemon: &Daemon, contract_addr: &Addr) -> anyhow::Result<Daemon> {
    // Get config of an app to get proxy address(granter)
    let app_config: AppConfigResponse =
        daemon.query(&QueryMsg::Base(BaseQueryMsg::BaseConfig {}), contract_addr)?;

    // Check if grant is indeed given
    let authz_granter = app_config.proxy_address;
    let authz_querier: Authz = daemon.query_client();
    let grantee = daemon.sender().to_string();
    let grants = daemon.rt_handle.block_on(async {
        authz_querier
            .grants(
                authz_granter.to_string(),
                grantee,
                MSG_TYPE_URL.to_string(),
                None,
            )
            .await
    })?;
    if grants.grants.is_empty() {
        return Err(anyhow::anyhow!("Missing required grant"));
    }

    Ok(daemon.with_authz_granter(authz_granter))
}

fn autocompound(daemon: &mut Daemon, contract_addrs: Vec<String>) -> anyhow::Result<()> {
    for contract in contract_addrs {
        let addr = Addr::unchecked(contract);
        // TODO: Should look into different query to see the cooldown
        let available_rewards: AvailableRewardsResponse =
            daemon.query(&QueryMsg::from(AppQueryMsg::AvailableRewards {}), &addr)?;
        // If not empty - autocompound
        if !available_rewards.available_rewards.is_empty() {
            // Execute autocompound, if we have grant(s)
            if let Ok(daemon) = daemon_with_savings_authz(daemon, &addr) {
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
