mod bot;
mod bot_args;

pub use bot::Bot;
pub use bot_args::BotArgs;

use abstract_app::objects::module::{ModuleInfo, ModuleVersion};
use abstract_client::AbstractClient;
use carrot_app::contract::{APP_ID, APP_VERSION};
use cw_orch::{
    anyhow,
    daemon::{networks::OSMOSIS_1, Daemon, GrpcChannel},
    tokio::runtime::Runtime,
};

/// entrypoint for the bot
pub fn cron_main(bot_args: BotArgs) -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    let mut chain_info = OSMOSIS_1;
    let grpc_urls = if let Some(grpc_urls) = &bot_args.grps_urls {
        grpc_urls.iter().map(String::as_ref).collect()
    } else {
        chain_info.grpc_urls.to_vec()
    };

    chain_info.grpc_urls = &grpc_urls;
    let daemon = Daemon::builder()
        .handle(rt.handle())
        .chain(chain_info)
        .build()?;

    let abstr = AbstractClient::new(daemon.clone())?;
    let module_info =
        ModuleInfo::from_id(APP_ID, ModuleVersion::Version(APP_VERSION.parse().unwrap()))?;

    let mut bot = Bot::new(
        abstr,
        module_info,
        bot_args.fetch_cooldown,
        bot_args.autocompound_cooldown,
    );

    // Run long-running autocompound job.
    loop {
        // You can edit retries with CW_ORCH_MAX_TX_QUERY_RETRIES
        bot.fetch_contracts()?;
        bot.autocompound();

        // Drop connection
        let mut state = std::sync::Arc::try_unwrap(bot.daemon.daemon.state).unwrap();
        drop(state.grpc_channel);

        // Wait for autocompound duration
        std::thread::sleep(bot.autocompound_cooldown);

        // Reconnect
        state.grpc_channel = {
            // TODO: what do we do on failed connection?
            rt.block_on(GrpcChannel::connect(
                &state.chain_data.apis.grpc,
                state.chain_data.chain_id.as_str(),
            ))?
        };
        bot.daemon.daemon.state = std::sync::Arc::new(state);
    }
}
