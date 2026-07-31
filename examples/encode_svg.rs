// Examples unwrap to keep the demo short; library code may not -- see the
// [lints.clippy] gate in Cargo.toml.
#![allow(clippy::unwrap_used, reason = "example code")]

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode, Version};

fn main() {
    let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
    let image = code
        .render()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#800000"))
        .light_color(svg::Color("#ffff80"))
        .build();
    println!("{image}");
}
