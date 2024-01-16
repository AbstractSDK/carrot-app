use abstract_app::AppError as AbstractAppError;
use abstract_core::AbstractError;
use abstract_sdk::AbstractSdkError;
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

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Wrong denom deposited, expected exactly {expected}, got {got:?}")]
    DepositError { expected: AssetInfo, got: Vec<Coin> },

    #[error("Wrong asset info stored, expected Native")]
    WrongAssetInfo {},

    #[error("No position registered in contract, please make a deposit before !")]
    NoPosition {},
}
