mod add_to_position;
mod create_position;
mod withdraw_to_asset;

pub const CREATE_POSITION_ID: u64 = 1;
pub const ADD_TO_POSITION_ID: u64 = 2;
pub const WITHDRAW_TO_ASSET_ID: u64 = 3;

pub use add_to_position::add_to_position_reply;
pub use create_position::create_position_reply;
pub use withdraw_to_asset::withdraw_to_asset_reply;
