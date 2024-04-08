use crate::yield_sources::Strategy;

/// In order to re-balance the strategies, we need in order :
/// 1. Withdraw from the strategies that will be deleted
/// 2. Compute the total value that should land in each strategy
/// 3. Withdraw from strategies that have too much value
/// 4. Deposit all the withdrawn funds into the strategies to match the target.
impl Strategy {}
