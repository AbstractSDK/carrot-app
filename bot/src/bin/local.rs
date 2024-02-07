use cw_orch::anyhow;
use dotenv::dotenv;
use savings_bot::cron_main;

fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();

    cron_main()
}
