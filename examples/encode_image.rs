// Örnekler demoyu kısa tutmak için unwrap kullanır; kütüphane kodu kullanamaz --
// bkz. Cargo.toml'daki [lints.clippy] kapısı.
#![allow(clippy::unwrap_used, reason = "örnek kod")]

use image::Luma;
use qrcode::QrCode;

fn main() {
    // Bir miktar veriyi bitlere kodla.
    let code = QrCode::new(b"01234567").unwrap();

    // Bitleri bir görüntüye çiz.
    let image = code.render::<Luma<u8>>().build();

    // Görüntüyü kaydet.
    image.save("/tmp/qrcode.png").unwrap();
}
