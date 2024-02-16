mod bot;

use std::time::Duration;

use abstract_app::objects::module::{ModuleInfo, ModuleVersion};
use abstract_client::AbstractClient;
pub use bot::Bot;
use cw_orch::{
    anyhow,
    daemon::{networks::OSMO_5, Daemon},
    tokio::runtime::Runtime,
};
use savings_app::contract::{APP_ID, APP_VERSION};

/// entrypoint for the bot
pub fn cron_main() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let daemon = Daemon::builder()
        .handle(rt.handle())
        .chain(OSMO_5)
        .build()?;

    let abstr = AbstractClient::new(daemon.clone())?;
    let module_info =
        ModuleInfo::from_id(APP_ID, ModuleVersion::Version(APP_VERSION.parse().unwrap()))?;

    let mut bot = Bot::new(
        abstr,
        module_info,
        Duration::from_secs(10),
        Duration::from_secs(10),
    );

    // Run long-running autocompound job.
    loop {
        // You can edit retries with CW_ORCH_MAX_TX_QUERY_RETRIES
        bot.fetch_contracts()?;
        bot.autocompound();
    }
}
