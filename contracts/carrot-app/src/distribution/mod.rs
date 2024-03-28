/// This module handles the strategy distribution. It handles the following cases
/// 1. A user deposits some funds.
/// This modules dispatches the funds into the different strategies according to current status and target strats
pub mod deposit;

/// 2. A user want to withdraw some funds
/// This module withdraws a share of the funds deposited inside the registered strategies
pub mod withdraw;

/// 3. A user wants to claim their rewards and autocompound
/// This module compute the available rewards and withdraws the rewards from the registered strategies
pub mod rewards;

/// 4. Some queries are needed on certain structures for abstraction purposes
pub mod query;

/// 4. Some queries are needed on certain structures for abstraction purposes
pub mod rebalance;
