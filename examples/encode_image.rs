// Examples unwrap to keep the demo short; library code may not -- see the
// [lints.clippy] gate in Cargo.toml.
#![allow(clippy::unwrap_used, reason = "example code")]

use image::Luma;
use qrcode::QrCode;

fn main() {
    // Encode some data into bits.
    let code = QrCode::new(b"01234567").unwrap();

    // Render the bits into an image.
    let image = code.render::<Luma<u8>>().build();

    // Save the image.
    image.save("/tmp/qrcode.png").unwrap();
}
