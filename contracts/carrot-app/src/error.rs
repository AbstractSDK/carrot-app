use abstract_app::sdk::AbstractSdkError;
use abstract_app::AppError as AbstractAppError;
use abstract_app::{objects::ans_host::AnsHostError, std::AbstractError};
use cosmwasm_std::{Coin, ConversionOverflowError, Decimal, StdError};
use cw_asset::{AssetError, AssetInfo};
use cw_controllers::AdminError;
use cw_utils::ParseReplyError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum AppError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Abstract(#[from] AbstractError),

    #[error("{0}")]
    AbstractSdk(#[from] AbstractSdkError),

    #[error("{0}")]
    Asset(#[from] AssetError),

    #[error("{0}")]
    Admin(#[from] AdminError),

    #[error("{0}")]
    DappError(#[from] AbstractAppError),

    #[error("{0}")]
    AnsHost(#[from] AnsHostError),

    #[error("{0}")]
    ParseReplyError(#[from] ParseReplyError),

    #[error("{0}")]
    ConversionOverflowError(#[from] ConversionOverflowError),

    #[error(transparent)]
    ProstDecodeError(#[from] prost::DecodeError),

    #[error(transparent)]
    CoinsError(#[from] cosmwasm_std::CoinsError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Wrong denom deposited, expected exactly {expected}, got {got:?}")]
    DepositError { expected: AssetInfo, got: Vec<Coin> },

    #[error("Wrong asset info stored, expected Native")]
    WrongAssetInfo {},

    #[error("No position registered in contract, please create a position !")]
    NoPosition {},

    #[error("No swap fund to swap assets into each other")]
    NoSwapPossibility {},

    #[error("No top level account owner.")]
    NoTopLevelAccount {},

    #[error("No rewards for autocompound")]
    NoRewards {},

    #[error("Reward configuration error: {0}")]
    RewardConfigError(String),

    #[error("Position already exists. Please withdraw all funds before creating a new position")]
    PositionExists {},

    #[error("Operation exceeds max spread limit, price: {price}")]
    MaxSpreadAssertion { price: Decimal },

    #[error("Carrot app does not accept funds")]
    RedundantFunds {},

    #[error("Not enough liquidity for withdraw to asset")]
    NotEnoughForWithdrawTo {},
}
