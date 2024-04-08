use abstract_sdk::{AccountAction, Execution, ExecutorMsg};
use cosmwasm_std::{Coin, Deps};

use crate::{
    contract::{App, AppResult},
    error::AppError,
    yield_sources::Strategy,
};

impl Strategy {
    pub fn withdraw_rewards(
        self,
        deps: Deps,
        app: &App,
    ) -> AppResult<(Vec<Coin>, Vec<ExecutorMsg>)> {
        let (rewards, msgs): (Vec<Vec<Coin>>, _) = self
            .0
            .into_iter()
            .map(|s| {
                let (rewards, raw_msgs) = s.yield_source.ty.withdraw_rewards(deps, app)?;

                Ok::<_, AppError>((
                    rewards,
                    app.executor(deps)
                        .execute(vec![AccountAction::from_vec(raw_msgs)])?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();

        Ok((rewards.into_iter().flatten().collect(), msgs))
    }
}
