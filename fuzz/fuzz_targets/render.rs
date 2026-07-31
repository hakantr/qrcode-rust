//! Drives the renderers over arbitrary sizing requests.
//!
//! The sizing arithmetic is where a `u32` overflow used to wrap silently and
//! produce an image of the wrong dimensions. Everything here goes through the
//! `try_` forms, which must report rather than abort.
//!
//! Sizes are drawn from two bands rather than the whole `u32` range. The
//! interesting cases are the small ones, which actually allocate and render,
//! and the extreme ones, which have to be rejected before anything is
//! allocated. The band in between is neither: a request like 526575x1785 is
//! legal, under `MAX_IMAGE_PIXELS`, and simply needs several gigabytes -- the
//! allocator running out there is the caller's business, not a defect, and
//! chasing it would crowd out every other finding.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qrcode::QrCode;
use qrcode::render::Renderer;
use qrcode::types::Color;

/// Keeps a rendered image under a few megabytes.
const MAX_RENDERED_SIDE: u32 = 2048;

#[derive(arbitrary::Arbitrary, Debug)]
struct Dimension {
    /// Picks the band: extremes must be rejected, small values must render.
    extreme: bool,
    value: u32,
}

impl Dimension {
    fn get(&self) -> u32 {
        if self.extreme {
            // Values that must overflow or exceed MAX_IMAGE_PIXELS. Rejection
            // happens before any allocation, so these stay cheap.
            u32::MAX - (self.value % 4)
        } else {
            self.value % MAX_RENDERED_SIDE
        }
    }
}

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    data: Vec<u8>,
    module_width: Dimension,
    module_height: Dimension,
    min_width: Dimension,
    min_height: Dimension,
    max_width: Dimension,
    max_height: Dimension,
    quiet_zone: bool,
    /// Fed straight to `Renderer::try_new` alongside a mismatched buffer, so
    /// the length check and its overflow guard both get exercised.
    raw_modules_count: usize,
}

fuzz_target!(|input: Input| {
    // A renderer built by hand, with a `modules_count` that probably does not
    // match the content length.
    let content = [Color::Dark, Color::Light, Color::Light, Color::Dark];
    match Renderer::<char>::try_new(&content, input.raw_modules_count, 4) {
        Ok(mut renderer) => {
            // try_new only succeeds for modules_count == 2 here.
            assert_eq!(input.raw_modules_count, 2);
            let _ = renderer.module_dimensions(input.module_width.get(), input.module_height.get()).try_build();
        }
        Err(_) => assert_ne!(input.raw_modules_count, 2),
    }

    let Ok(code) = QrCode::new(&input.data) else { return };

    let mut renderer = code.render::<char>();
    let renderer = renderer
        .quiet_zone(input.quiet_zone)
        .module_dimensions(input.module_width.get(), input.module_height.get())
        .min_dimensions(input.min_width.get(), input.min_height.get())
        .max_dimensions(input.max_width.get(), input.max_height.get());

    if let Ok(image) = renderer.try_build() {
        // A built image always has at least the QR code's own modules in it.
        assert!(image.lines().count() > 0);
    }

    // The vector backends do not allocate per pixel, so they are safe to drive
    // at any size the sizing arithmetic accepts.
    let _ = code.render::<qrcode::render::unicode::Dense1x2>().try_build();
    let _ = code.render::<qrcode::render::svg::Color>().try_build();
    let _ = code.render::<qrcode::render::eps::Color>().try_build();
    let _ = code.render::<qrcode::render::pic::Color>().try_build();
});
