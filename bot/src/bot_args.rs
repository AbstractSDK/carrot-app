use clap::Parser;
use humantime::parse_duration;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct BotArgs {
    /// Fetch cooldown
    #[arg(long = "fcd", value_parser = parse_duration, value_name = "DURATION")]
    pub fetch_cooldown: Duration,
    /// Autocompound cooldown
    #[arg(long = "acd", value_parser = parse_duration, value_name = "DURATION")]
    pub autocompound_cooldown: Duration,
    /// Custom grpc urls
    #[arg(long = "grpcs", value_name = "URL")]
    pub grps_urls: Vec<String>,
}
