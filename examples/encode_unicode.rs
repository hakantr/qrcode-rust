// Examples unwrap to keep the demo short; library code may not -- see the
// [lints.clippy] gate in Cargo.toml.
#![allow(clippy::unwrap_used, reason = "example code")]

use qrcode::QrCode;
use qrcode::render::unicode;

fn main() {
    let code = QrCode::new(b"Hello").unwrap();
    let string = code.render::<unicode::Dense1x2>().quiet_zone(false).build();
    println!("{string}");
}
