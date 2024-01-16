// Taken from https://github.com/quasar-finance/quasar/tree/v1.0.7-cl/smart-contracts/contracts/cl-vault

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Uint128};

#[cw_serde]
pub enum ExecuteMsg {
    ExactDeposit {
        recipient: Option<String>,
    },
    Redeem {
        recipient: Option<String>,
        amount: Uint128,
    },
    VaultExtension(VaultMessage),
}

#[cw_serde]
pub enum VaultMessage {
    ClaimRewards {},
}

#[cw_serde]
pub enum QueryMsg {
    TotalAssets {},
    VaultExtension(VaultQuery),
}

#[cw_serde]
#[derive(QueryResponses)]
#[query_responses(nested)]
pub enum VaultQuery {
    Balances(BalancesQuery),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum BalancesQuery {
    #[returns(UserSharesBalanceResponse)]
    UserSharesBalance { user: String },
}

#[cw_serde]
pub struct TotalAssetsResponse {
    pub token0: Coin,
    pub token1: Coin,
}

#[cw_serde]
pub struct UserSharesBalanceResponse {
    pub balance: Uint128,
}
