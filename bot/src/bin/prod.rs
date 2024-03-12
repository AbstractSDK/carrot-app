//! # Production bin
//! Use the production environment's environment variables to provide the bot with a seed-phrase.
//! `MAIN_MNEMONIC`

use cw_orch::anyhow;
use savings_bot::cron_main;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let bot_args = savings_bot::BotArgs::parse();
    cron_main(bot_args)
}
