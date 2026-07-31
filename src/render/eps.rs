//! EPS çizim desteği.
//!
//! # Örnek
//!
//! ```
//! use qrcode::QrCode;
//! use qrcode::render::eps;
//!
//! let code = QrCode::new(b"Hello").unwrap();
//! let eps = code.render::<eps::Color>().build();
//! println!("{eps}");

#![cfg(feature = "eps")]

use alloc::format;
use alloc::string::String;
use core::fmt::Write;

use crate::render::{Canvas as RenderCanvas, Pixel};
use crate::types::Color as ModuleColor;

/// Bir EPS rengi (`[R, G, B]`).
///
/// Her değer 0.0 ile 1.0 aralığında olmalıdır.
#[derive(Copy, Clone, Default, PartialEq, PartialOrd)]
pub struct Color(pub [f64; 3]);

impl Pixel for Color {
    type Canvas = Canvas;
    type Image = String;

    fn default_color(color: ModuleColor) -> Self {
        Self(color.select(Default::default(), [1.0; 3]))
    }
}

#[doc(hidden)]
pub struct Canvas {
    eps: String,
    height: u32,
}

impl RenderCanvas for Canvas {
    type Pixel = Color;
    type Image = String;

    fn new(width: u32, height: u32, dark_pixel: Color, light_pixel: Color) -> Self {
        Self {
            eps: format!(
                concat!(
                    "%!PS-Adobe-3.0 EPSF-3.0\n",
                    "%%BoundingBox: 0 0 {w} {h}\n",
                    "%%Pages: 1\n",
                    "%%EndComments\n",
                    "gsave\n",
                    "{bgr} {bgg} {bgb} setrgbcolor\n",
                    "0 0 {w} {h} rectfill\n",
                    "grestore\n",
                    "{fgr} {fgg} {fgb} setrgbcolor\n"
                ),
                w = width,
                h = height,
                fgr = dark_pixel.0[0],
                fgg = dark_pixel.0[1],
                fgb = dark_pixel.0[2],
                bgr = light_pixel.0[0],
                bgg = light_pixel.0[1],
                bgb = light_pixel.0[2],
            ),
            height,
        }
    }

    fn draw_dark_pixel(&mut self, x: u32, y: u32) {
        self.draw_dark_rect(x, y, 1, 1);
    }

    fn draw_dark_rect(&mut self, left: u32, top: u32, width: u32, height: u32) {
        // PostScript'in başlangıç noktası sol alttadır ve `rectfill` sol alt köşeyi
        // alır; bu yüzden yalnızca üst kenarın değil dikdörtgenin tamamının
        // çevrilmesi gerekir, aksi hâlde görüntü `height` kadar yukarı kayar.
        let bottom = self.height - top - height;
        // Bir `String`'e yazmak hatasızdır.
        let _ = writeln!(self.eps, "{left} {bottom} {width} {height} rectfill");
    }

    fn into_image(mut self) -> String {
        self.eps.push_str("%%EOF");
        self.eps
    }
}
