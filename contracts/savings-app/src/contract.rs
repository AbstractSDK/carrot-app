use abstract_app::abstract_core::objects::dependency::StaticDependency;
use abstract_app::AppContract;
use cosmwasm_std::Response;

use crate::{
    error::AppError,
    handlers,
    msg::{AppExecuteMsg, AppInstantiateMsg, AppMigrateMsg, AppQueryMsg},
    replies::{
        add_to_position_reply, create_position_reply, ADD_TO_POSITION_ID, CREATE_POSITION_ID,
    },
};

/// The version of your app
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The id of the app
pub const APP_ID: &str = "abstract:savings-app";

/// The type of the result returned by your app's entry points.
pub type AppResult<T = Response> = Result<T, AppError>;

/// The type of the app that is used to build your app and access the Abstract SDK features.
pub type App = AppContract<AppError, AppInstantiateMsg, AppExecuteMsg, AppQueryMsg, AppMigrateMsg>;

const DEX_DEPENDENCY: StaticDependency = StaticDependency::new(
    abstract_dex_adapter::DEX_ADAPTER_ID,
    &[abstract_dex_adapter::contract::CONTRACT_VERSION],
);

const APP: App = App::new(APP_ID, APP_VERSION, None)
    .with_instantiate(handlers::instantiate_handler)
    .with_execute(handlers::execute_handler)
    .with_query(handlers::query_handler)
    .with_migrate(handlers::migrate_handler)
    .with_replies(&[
        (CREATE_POSITION_ID, create_position_reply),
        (ADD_TO_POSITION_ID, add_to_position_reply),
    ])
    .with_dependencies(&[DEX_DEPENDENCY]);

// Export handlers
#[cfg(feature = "export")]
abstract_app::export_endpoints!(APP, App);

#[cfg(feature = "interface")]
abstract_app::cw_orch_interface!(APP, App, AppInterface);

#[cfg(feature = "interface")]
impl<Chain: cw_orch::environment::CwEnv> abstract_app::abstract_interface::DependencyCreation
    for crate::AppInterface<Chain>
{
    type DependenciesConfig = cosmwasm_std::Empty;

    fn dependency_install_configs(
        _configuration: Self::DependenciesConfig,
    ) -> Result<
        Vec<abstract_app::abstract_core::manager::ModuleInstallConfig>,
        abstract_app::abstract_interface::AbstractInterfaceError,
    > {
        let dex_dependency_install_configs: Vec<abstract_app::abstract_core::manager::ModuleInstallConfig> =
            <abstract_dex_adapter::interface::DexAdapter<Chain> as abstract_app::abstract_interface::DependencyCreation>::dependency_install_configs(
                cosmwasm_std::Empty {},
            )?;

        let adapter_install_config = abstract_app::abstract_core::manager::ModuleInstallConfig::new(
            abstract_app::abstract_core::objects::module::ModuleInfo::from_id(
                abstract_dex_adapter::DEX_ADAPTER_ID,
                abstract_dex_adapter::contract::CONTRACT_VERSION.into(),
            )?,
            None,
        );
        Ok([dex_dependency_install_configs, vec![adapter_install_config]].concat())
    }
}
