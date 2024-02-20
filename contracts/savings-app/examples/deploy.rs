use abstract_app::objects::namespace::ABSTRACT_NAMESPACE;
use abstract_client::Namespace;
use cw_orch::{
    anyhow,
    daemon::{networks::OSMO_5, Daemon, DaemonBuilder},
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use savings_app::AppInterface;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let chain = OSMO_5;
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
    Ok(())
}
