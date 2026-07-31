//! String rendering support.

use crate::cast::As;
use crate::render::{Canvas as RenderCanvas, Pixel};
use crate::types::Color;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Abstraction of an image element.
pub trait Element: Copy {
    /// Obtains the default element color when a module is dark or light.
    fn default_color(color: Color) -> Self;

    /// Returns the number of bytes in `self`.
    fn strlen(self) -> usize;

    /// Appends `self` to the end of the given `string`.
    fn append_to_string(self, string: &mut String);
}

impl Element for char {
    fn default_color(color: Color) -> Self {
        color.select('\u{2588}', ' ')
    }

    fn strlen(self) -> usize {
        self.len_utf8()
    }

    fn append_to_string(self, string: &mut String) {
        string.push(self);
    }
}

impl Element for &str {
    fn default_color(color: Color) -> Self {
        color.select("\u{2588}", " ")
    }

    fn strlen(self) -> usize {
        self.len()
    }

    fn append_to_string(self, string: &mut String) {
        string.push_str(self);
    }
}

#[doc(hidden)]
pub struct Canvas<P: Element> {
    buffer: Vec<P>,
    width: usize,
    dark_pixel: P,
    dark_cap_inc: isize,
    capacity: isize,
}

impl<P: Element> Pixel for P {
    type Canvas = Canvas<Self>;
    type Image = String;

    fn default_unit_size() -> (u32, u32) {
        (1, 1)
    }

    fn default_color(color: Color) -> Self {
        <Self as Element>::default_color(color)
    }
}

impl<P: Element> RenderCanvas for Canvas<P> {
    type Pixel = P;
    type Image = String;

    fn new(width: u32, height: u32, dark_pixel: P, light_pixel: P) -> Self {
        let width = width.as_usize();
        let height = height.as_usize();
        let dark_cap = dark_pixel.strlen();
        let light_cap = light_pixel.strlen();
        Self {
            buffer: vec![light_pixel; width.saturating_mul(height)],
            width,
            dark_pixel,
            // A dark element may be shorter than a light one, so the running
            // adjustment is signed even though the total never goes below zero.
            dark_cap_inc: dark_cap.as_isize() - light_cap.as_isize(),
            // One newline between each pair of rows; an empty image has none,
            // which used to underflow this to -1.
            capacity: light_cap
                .saturating_mul(width)
                .saturating_mul(height)
                .saturating_add(height.saturating_sub(1))
                .as_isize(),
        }
    }

    fn draw_dark_pixel(&mut self, x: u32, y: u32) {
        let x = x.as_usize();
        let y = y.as_usize();
        if let Some(pixel) = self.buffer.get_mut(x + y * self.width) {
            self.capacity += self.dark_cap_inc;
            *pixel = self.dark_pixel;
        }
    }

    fn into_image(self) -> String {
        // `dark_cap_inc` is negative when the dark element is the shorter one,
        // so the running total can dip below zero before the last pixel.
        let mut result = String::with_capacity(self.capacity.max(0).as_usize());
        for (i, pixel) in self.buffer.into_iter().enumerate() {
            if i != 0 && i % self.width == 0 {
                result.push('\n');
            }
            pixel.append_to_string(&mut result);
        }
        result
    }
}

#[test]
fn test_render_to_string() {
    use crate::render::Renderer;

    let colors = &[Color::Dark, Color::Light, Color::Light, Color::Dark];
    let image: String = Renderer::<char>::new(colors, 2, 1).build();
    assert_eq!(&image, "    \n \u{2588}  \n  \u{2588} \n    ");

    let image2 = Renderer::new(colors, 2, 1).light_color("A").dark_color("!B!").module_dimensions(2, 2).build();

    assert_eq!(
        &image2,
        "AAAAAAAA\n\
         AAAAAAAA\n\
         AA!B!!B!AAAA\n\
         AA!B!!B!AAAA\n\
         AAAA!B!!B!AA\n\
         AAAA!B!!B!AA\n\
         AAAAAAAA\n\
         AAAAAAAA"
    );
}
