// Örnekler gösterimi kısa tutmak için unwrap kullanır; kütüphane kodu kullanamaz --
// bkz. Cargo.toml'daki [lints.clippy] kapısı.
#![allow(clippy::unwrap_used, reason = "örnek kod")]

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
