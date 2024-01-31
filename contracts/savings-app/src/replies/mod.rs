mod add_to_position;
mod create_position;

pub const CREATE_POSITION_ID: u64 = 1;
pub const ADD_TO_POSITION_ID: u64 = 2;

pub use add_to_position::add_to_position_reply;
pub use create_position::create_position_reply;
