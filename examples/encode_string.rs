// Örnekler gösterimi kısa tutmak için unwrap kullanır; kütüphane kodu kullanamaz --
// bkz. Cargo.toml'daki [lints.clippy] kapısı.
#![allow(clippy::unwrap_used, reason = "örnek kod")]

use qrcode::QrCode;

fn main() {
    let code = QrCode::new(b"Hello").unwrap();
    let string = code.render::<char>().quiet_zone(false).module_dimensions(2, 1).build();
    println!("{string}");
}
