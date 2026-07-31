// Examples unwrap to keep the demo short; library code may not -- see the
// [lints.clippy] gate in Cargo.toml.
#![allow(clippy::unwrap_used, reason = "example code")]

use qrcode::QrCode;
use qrcode::render::pic;

fn main() {
    let code = QrCode::new(b"01234567").unwrap();
    let image = code.render::<pic::Color>().min_dimensions(1, 1).build();
    println!("{image}");
}
