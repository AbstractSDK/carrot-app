use abstract_app::objects::{module::ModuleVersion, AccountId};
use abstract_interface::{AbstractAccount, InstallConfig, ManagerExecFns};
use abstract_sdk::core::app::BaseMigrateMsg;
use cosmwasm_std::to_json_binary;
use cw_orch::{
    anyhow,
    daemon::{networks::OSMOSIS_1, Daemon, DaemonBuilder},
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use carrot_app::{msg::AppMigrateMsg, AppInterface};

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let mut chain = OSMOSIS_1;
    chain.grpc_urls = &["https://grpc.osmosis.zone:443"];
    let rt = Runtime::new()?;
    let daemon = DaemonBuilder::default()
        .chain(chain)
        .handle(rt.handle())
        .build()?;

    let abstr = abstract_client::AbstractClient::new(daemon)?;

    // Add your account_id here
    let account_id_for_migrate = AccountId::local(todo!());
    let account_to_migrate = abstr.account_from(account_id_for_migrate)?;
    let abstr_account: &AbstractAccount<Daemon> = account_to_migrate.as_ref();
    let mut module_info = AppInterface::<Daemon>::module_info()?;
    module_info.version = ModuleVersion::Version("0.1.0".to_owned());
    abstr_account.manager.upgrade(vec![(
        module_info,
        Some(to_json_binary(&carrot_app::msg::MigrateMsg {
            base: BaseMigrateMsg {},
            module: AppMigrateMsg {},
        })?),
    )])?;
    Ok(())
}
