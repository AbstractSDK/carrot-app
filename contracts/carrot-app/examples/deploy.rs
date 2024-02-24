use abstract_app::objects::{module::ModuleInfo, namespace::ABSTRACT_NAMESPACE};
use abstract_client::Namespace;
use abstract_interface::VCExecFns;
use cw_orch::{
    anyhow,
    daemon::{networks::OSMOSIS_1, Daemon, DaemonBuilder},
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use carrot_app::{contract::APP_ID, AppInterface};

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let chain = OSMOSIS_1;
    let rt = Runtime::new()?;
    let daemon = DaemonBuilder::default()
        .chain(chain)
        .handle(rt.handle())
        .build()?;

    let abstr = abstract_client::AbstractClient::new(daemon)?;

    let publisher = abstr
        .publisher_builder(Namespace::new(ABSTRACT_NAMESPACE)?)
        .build()?;

    publisher.publish_app::<AppInterface<Daemon>>()?;
    abstr.version_control().approve_any_abstract_modules()?;
    Ok(())
}
