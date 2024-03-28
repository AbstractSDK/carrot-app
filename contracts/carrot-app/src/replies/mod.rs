mod after_swaps;
mod osmosis;

pub const OSMOSIS_CREATE_POSITION_REPLY_ID: u64 = 1;
pub const OSMOSIS_ADD_TO_POSITION_REPLY_ID: u64 = 2;

pub const REPLY_AFTER_SWAPS_STEP: u64 = 3;

pub use after_swaps::after_swap_reply;
pub use osmosis::add_to_position::add_to_position_reply;
pub use osmosis::create_position::create_position_reply;
