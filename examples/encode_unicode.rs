// Örnekler gösterimi kısa tutmak için unwrap kullanır; kütüphane kodu kullanamaz --
// bkz. Cargo.toml'daki [lints.clippy] kapısı.
#![allow(clippy::unwrap_used, reason = "örnek kod")]

use qrcode::QrCode;
use qrcode::render::unicode;

fn main() {
    let code = QrCode::new(b"Hello").unwrap();
    let string = code.render::<unicode::Dense1x2>().quiet_zone(false).build();
    println!("{string}");
}
