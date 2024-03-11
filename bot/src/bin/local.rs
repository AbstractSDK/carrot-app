use cw_orch::anyhow;
use dotenv::dotenv;
use savings_bot::cron_main;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();

    let bot_args = savings_bot::BotArgs::parse();
    cron_main(bot_args)
}
