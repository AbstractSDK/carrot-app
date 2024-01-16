use crate::msg::AppMigrateMsg;
use crate::{
    error::AppError,
    handlers,
    msg::{AppExecuteMsg, AppInstantiateMsg, AppQueryMsg},
    replies::{self},
};
use abstract_app::AppContract;
use abstract_core::objects::dependency::StaticDependency;
use cosmwasm_std::Response;

/// The version of your app
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The id of the app
pub const APP_ID: &str = "abstract-sdk:savings-app";

/// The type of the result returned by your app's entry points.
pub type AppResult<T = Response> = Result<T, AppError>;

/// The type of the app that is used to build your app and access the Abstract SDK features.
pub type App = AppContract<AppError, AppInstantiateMsg, AppExecuteMsg, AppQueryMsg, AppMigrateMsg>;

const DEX_DEPENDENCY: StaticDependency = StaticDependency::new(
    abstract_dex_adapter::EXCHANGE,
    &[abstract_dex_adapter::contract::CONTRACT_VERSION],
);

const APP: App = App::new(APP_ID, APP_VERSION, None)
    .with_instantiate(handlers::instantiate_handler)
    .with_execute(handlers::execute_handler)
    .with_query(handlers::query_handler)
    .with_migrate(handlers::migrate_handler)
    .with_dependencies(&[DEX_DEPENDENCY]);

// Export handlers
#[cfg(feature = "export")]
abstract_app::export_endpoints!(APP, App);

#[cfg(feature = "interface")]
abstract_app::cw_orch_interface!(APP, App, AppInterface);
