use abstract_app::objects::namespace::ABSTRACT_NAMESPACE;
use abstract_client::Namespace;
use cw_orch::{
    anyhow,
    daemon::{networks::OSMOSIS_1, Daemon, DaemonBuilder},
    tokio::runtime::Runtime,
};
use dotenv::dotenv;

use carrot_app::AppInterface;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let chain = OSMOSIS_1;
    let rt = Runtime::new()?;
    let daemon = DaemonBuilder::new(chain).handle(rt.handle()).build()?;

    let abstr = abstract_client::AbstractClient::new(daemon)?;

    let publisher = abstr
        .fetch_or_build_account(Namespace::new(ABSTRACT_NAMESPACE)?, |builder| {
            builder.namespace(Namespace::new(ABSTRACT_NAMESPACE).unwrap())
        })?
        .publisher()?;

    publisher.publish_app::<AppInterface<Daemon>>()?;
    abstr.registry().approve_any_abstract_modules()?;
    Ok(())
}
