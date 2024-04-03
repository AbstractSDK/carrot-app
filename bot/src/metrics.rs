use prometheus::{
    Encoder, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry, TextEncoder,
};

use warp::Filter;

pub struct Metrics {
    pub fetch_count: IntCounter,
    pub fetch_instances_count: IntGauge,
    pub autocompounded_count: IntCounterVec,
    pub autocompounded_error_count: IntCounterVec,
    pub contract_instances_to_autocompound: IntGauge,
    // Total value locked by all instance
    pub total_value_locked: IntGauge,
    // The balance of the instance used to calculate the APR
    pub reference_contract_balance: IntGauge,
    // balance of every instance
    pub contract_balance: IntGaugeVec,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        let fetch_count = IntCounter::new(
            "carrot_app_bot_fetch_count",
            "Number of times the bot has fetched the instances",
        )
        .unwrap();
        let fetch_instances_count = IntGauge::new(
            "carrot_app_bot_fetch_instances_count",
            "Number of fetched instances",
        )
        .unwrap();
        let autocompounded_count = IntCounterVec::new(
            Opts::new(
                "carrot_app_bot_autocompounded_count",
                "Number of times contracts have been autocompounded",
            ),
            &["contract_version"],
        )
        .unwrap();
        let autocompounded_error_count = IntCounterVec::new(
            Opts::new(
                "carrot_app_bot_autocompounded_error_count",
                "Number of times autocompounding errored",
            ),
            &["contract_version"],
        )
        .unwrap();
        let contract_instances_to_autocompound = IntGauge::new(
            "carrot_app_bot_contract_instances_to_autocompound",
            "Number of instances that are eligible to be compounded",
        )
        .unwrap();
        let total_value_locked = IntGauge::new(
            "carrot_app_bot_total_value_locked",
            "Total value locked by all carrot instances",
        )
        .unwrap();
        let reference_contract_balance = IntGauge::new(
            "carrot_app_bot_reference_contract_balance",
            "balance of the reference contract to calculate the apr",
        )
        .unwrap();
        let contract_balance = IntGaugeVec::new(
            Opts::new(
                "carrot_app_bot_contract_balance",
                "Funds managed by a specific carrot instance",
            ),
            &["contract_address", "contract_version"],
        )
        .unwrap();
        registry.register(Box::new(fetch_count.clone())).unwrap();
        registry
            .register(Box::new(fetch_instances_count.clone()))
            .unwrap();
        registry
            .register(Box::new(autocompounded_count.clone()))
            .unwrap();
        registry
            .register(Box::new(autocompounded_error_count.clone()))
            .unwrap();
        registry
            .register(Box::new(contract_instances_to_autocompound.clone()))
            .unwrap();
        registry
            .register(Box::new(total_value_locked.clone()))
            .unwrap();
        registry
            .register(Box::new(reference_contract_balance.clone()))
            .unwrap();
        registry
            .register(Box::new(contract_balance.clone()))
            .unwrap();
        Self {
            fetch_count,
            fetch_instances_count,
            autocompounded_count,
            autocompounded_error_count,
            contract_instances_to_autocompound,
            total_value_locked,
            reference_contract_balance,
            contract_balance,
        }
    }
}

pub async fn serve_metrics(registry: prometheus::Registry) {
    let addr: std::net::SocketAddr = "0.0.0.0:8000".parse().unwrap();
    let metric_server = warp::serve(warp::path("metrics").map(move || {
        let metric_families = registry.gather();
        let mut buffer = vec![];
        let encoder = TextEncoder::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        warp::reply::with_header(
            buffer,
            "content-type",
            "text/plain; version=0.0.4; charset=utf-8",
        )
    }));
    metric_server.run(addr).await;
}
