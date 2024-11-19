use carrot_app::AppInterface;
use cw_orch::prelude::*;
use networks::OSMOSIS_1;

#[test]
fn successful_wasm() {
    // Panics if no path to a .wasm file is found
    AppInterface::<MockBech32>::wasm(&OSMOSIS_1.into());
}
