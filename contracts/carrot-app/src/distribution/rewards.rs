use abstract_sdk::{AccountAction, Execution, ExecutorMsg};
use cosmwasm_std::Deps;

use crate::{
    ans_assets::AnsAssets,
    contract::{App, AppResult},
    error::AppError,
    yield_sources::{yield_type::YieldTypeImplementation, Strategy},
};

impl Strategy {
    pub fn withdraw_rewards(
        self,
        deps: Deps,
        app: &App,
    ) -> AppResult<(AnsAssets, Vec<ExecutorMsg>)> {
        let mut all_rewards = AnsAssets::default();

        let msgs = self
            .0
            .into_iter()
            .map(|s| {
                let (rewards, raw_msgs) = s.yield_source.params.withdraw_rewards(deps, app)?;

                for asset in rewards {
                    all_rewards.add(asset)?;
                }
                Ok::<_, AppError>(
                    app.executor(deps)
                        .execute(vec![AccountAction::from_vec(raw_msgs)])?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((all_rewards, msgs))
    }
}
