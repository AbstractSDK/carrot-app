use std::collections::HashSet;

use abstract_app::{
    abstract_core::version_control::ModuleFilter,
    abstract_interface::{Abstract, VCQueryFns},
    objects::{
        module::{ModuleId, ModuleInfo, ModuleStatus},
        namespace::ABSTRACT_NAMESPACE,
    },
};
use app::contract::APP_ID;
use cw_orch::{anyhow, daemon::networks::OSMO_5, prelude::*, tokio::runtime::Runtime};
use dotenv::dotenv;
use savings_bot::{autocompound, fetch_instances};
use std::iter::Extend;
use abstract_client::AbstractClient;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();

    let rt = Runtime::new()?;
    let mut daemon = Daemon::builder()
        .handle(rt.handle())
        .chain(OSMO_5)
        .build()?;

    let abstr = AbstractClient::new(daemon)?;
    let module_id = ModuleId::try_from(APP_ID)?;

    let saving_modules = abstr.version_control().module_list(
        Some(ModuleFilter {
            namespace: Some(ABSTRACT_NAMESPACE.to_string()),
            name: Some("savings-app".to_string()),
            version: None,
            status: Some(ModuleStatus::Registered),
        }),
        None,
        None,
    )?;

    let mut contract_instances_to_check: HashSet<(String, Addr)> = HashSet::new();

    for app_info in saving_modules.modules {
        let code_id = app_info.module.reference.unwrap_app()?;
        let contract_addrs = rt
            .handle()
            .block_on(fetch_instances(daemon.channel(), code_id))?;

        contract_addrs.retain(if let Ok(daemon) = daemon_with_savings_authz(daemon, &address))
        // Add all the entries to the `contract_instances_to_check`
        contract_instances_to_check.extend(
            contract_addrs
                .into_iter()
                .map(|addr| (app_info.module.info.to_string(), Addr::unchecked(addr))),
        );
    }


    // Run long-running autocompound job.
    autocompound(&mut daemon, &mut contract_instances_to_check)?;
    Ok(())
}
