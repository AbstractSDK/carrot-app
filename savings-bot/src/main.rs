use abstract_app::{
    abstract_core::app::BaseQueryMsg, objects::nested_admin::TopLevelOwnerResponse,
};
use app::msg::{AppExecuteMsg, AppQueryMsg, AvailableRewardsResponse, ExecuteMsg, QueryMsg};
use cosmos_sdk_proto::{
    cosmwasm::wasm::v1::{query_client::QueryClient, QueryContractsByCodeRequest},
    traits::Message,
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
use osmosis_std::types::{
    cosmos::{
        authz::v1beta1::GenericAuthorization,
        bank::v1beta1::{MsgSend, SendAuthorization},
    },
    osmosis::{
        concentratedliquidity::v1beta1::{
            MsgAddToPosition, MsgCollectIncentives, MsgCollectSpreadRewards, MsgCreatePosition,
            MsgWithdrawPosition,
        },
        gamm::v1beta1::MsgSwapExactAmountIn,
    },
};
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

const AUTHORIZATION_URLS: &[&str] = &[
    MsgCreatePosition::TYPE_URL,
    MsgSwapExactAmountIn::TYPE_URL,
    MsgAddToPosition::TYPE_URL,
    MsgWithdrawPosition::TYPE_URL,
    MsgCollectIncentives::TYPE_URL,
    MsgCollectSpreadRewards::TYPE_URL,
];

fn daemon_with_savings_feegrant(daemon: &Daemon, contract_addr: &Addr) -> anyhow::Result<Daemon> {
    // Get config of an app to get top level owner(granter)
    let tlo: TopLevelOwnerResponse = daemon.query(
        &QueryMsg::Base(BaseQueryMsg::TopLevelOwner {}),
        contract_addr,
    )?;

    // Check if authz is indeed given
    let authz_granter = tlo.address;
    let authz_querier: Authz = daemon.query_client();
    let grantee = contract_addr.to_string();
    let generic_authorizations: Vec<GenericAuthorization> = daemon
        .rt_handle
        .block_on(async {
            authz_querier
                .grants(
                    authz_granter.to_string(),
                    grantee.clone(),
                    GenericAuthorization::TYPE_URL.to_string(),
                    None,
                )
                .await
        })?
        .grants
        .into_iter()
        .map(|grant| GenericAuthorization::decode(&*grant.authorization.unwrap().value))
        .collect::<Result<_, _>>()?;
    // Check all generic authorizations are in place
    for &authorization_url in AUTHORIZATION_URLS {
        if !generic_authorizations.contains(&GenericAuthorization {
            msg: authorization_url.to_owned(),
        }) {
            return Err(anyhow::anyhow!(
                "Missing authorization: {authorization_url}"
            ));
        }
    }

    // Check any of send authorization is in place
    if !generic_authorizations.contains(&GenericAuthorization {
        msg: MsgSend::TYPE_URL.to_owned(),
    }) {
        let send_authorizations = daemon.rt_handle.block_on(async {
            authz_querier
                .grants(
                    authz_granter.to_string(),
                    grantee,
                    SendAuthorization::TYPE_URL.to_string(),
                    None,
                )
                .await
        })?;
        if send_authorizations.grants.is_empty() {
            return Err(anyhow::anyhow!("Missing send authorization"));
        }
    }

    Ok(daemon.with_fee_granter(authz_granter))
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
            // Just output error without crashing, to keep rolling
            match daemon_with_savings_feegrant(daemon, &addr) {
                Ok(daemon) => {
                    let res = daemon.execute(
                        &ExecuteMsg::from(AppExecuteMsg::Autocompound {}),
                        &[],
                        &addr,
                    );
                    if let Err(e) = res {
                        eprintln!("Execution of autocompound failed: {e:?}");
                    }
                }
                Err(e) => {
                    eprintln!("Authz error for {addr}: {e:?}");
                }
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
