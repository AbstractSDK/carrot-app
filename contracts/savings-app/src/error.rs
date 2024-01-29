use abstract_app::abstract_core::AbstractError;
use abstract_app::abstract_sdk::AbstractSdkError;
use abstract_app::AppError as AbstractAppError;
use cosmwasm_std::{Coin, StdError};
use cw_asset::{AssetError, AssetInfo};
use cw_controllers::AdminError;
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

    #[error(transparent)]
    ProstDecodeError(#[from] prost::DecodeError),

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
}
