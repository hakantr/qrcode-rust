// Örnekler gösterimi kısa tutmak için unwrap kullanır; kütüphane kodu kullanamaz --
// bkz. Cargo.toml'daki [lints.clippy] kapısı.
#![allow(clippy::unwrap_used, reason = "örnek kod")]

use qrcode::QrCode;
use qrcode::render::pic;

fn main() {
    let code = QrCode::new(b"01234567").unwrap();
    let image = code.render::<pic::Color>().min_dimensions(1, 1).build();
    println!("{image}");
}
