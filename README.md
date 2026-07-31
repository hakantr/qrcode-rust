qrcode-rust
===========

[![Build status](https://github.com/kennytm/qrcode-rust/workflows/Rust/badge.svg)](https://github.com/kennytm/qrcode-rust/actions?query=workflow%3ARust)
[![crates.io](https://img.shields.io/crates/v/qrcode.svg)](https://crates.io/crates/qrcode)
[![MIT OR Apache 2.0](https://img.shields.io/badge/license-MIT%20%2f%20Apache%202.0-blue.svg)](./LICENSE-APACHE.txt)

QR code and Micro QR code encoder in Rust. [Documentation](https://docs.rs/qrcode).

No input aborts the process: everything the encoder rejects comes back as a
[`QrError`](https://docs.rs/qrcode/latest/qrcode/types/enum.QrError.html), in
release builds as much as in debug ones. See [Panics and errors](#panics-and-errors).

Cargo.toml
----------

```toml
[dependencies]
qrcode = "0.15.0"
```

The default settings will depend on the `image` crate. If you don't need image generation capability, disable the `default-features`:

```toml
[dependencies]
qrcode = { version = "0.15.0", default-features = false, features = ["std"] }
```

Example
-------

## Image generation

```rust
use qrcode::QrCode;
use image::Luma;

fn main() {
    // Encode some data into bits.
    let code = QrCode::new(b"01234567").unwrap();

    // Render the bits into an image.
    let image = code.render::<Luma<u8>>().build();

    // Save the image.
    image.save("/tmp/qrcode.png").unwrap();
}
```

Generates this image:

![Output](src/test_annex_i_qr_as_image.png)

## String generation

```rust
use qrcode::QrCode;

fn main() {
    let code = QrCode::new(b"Hello").unwrap();
    let string = code.render::<char>()
        .dark_color('#')
        .quiet_zone(false)
        .module_dimensions(2, 1)
        .build();
    println!("{string}");
}
```

Generates this output:

```none
##############    ########  ##############
##          ##          ##  ##          ##
##  ######  ##  ##  ##  ##  ##  ######  ##
##  ######  ##  ##  ##      ##  ######  ##
##  ######  ##  ####    ##  ##  ######  ##
##          ##  ####  ##    ##          ##
##############  ##  ##  ##  ##############
                ##  ##
##  ##########    ##  ##    ##########
      ##        ##    ########    ####  ##
    ##########    ####  ##  ####  ######
    ##    ##  ####  ##########    ####
  ######    ##########  ##    ##        ##
                ##      ##    ##  ##
##############    ##  ##  ##    ##  ####
##          ##  ##  ##        ##########
##  ######  ##  ##    ##  ##    ##    ##
##  ######  ##  ####  ##########  ##
##  ######  ##  ####    ##  ####    ##
##          ##    ##  ########  ######
##############  ####    ##      ##    ##
```

## SVG generation

```rust
use qrcode::{QrCode, Version, EcLevel};
use qrcode::render::svg;

fn main() {
    let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
    let image = code.render()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#800000"))
        .light_color(svg::Color("#ffff80"))
        .build();
    println!("{image}");
}
```

Generates this SVG:

[![Output](src/test_annex_i_micro_qr_as_svg.svg)](src/test_annex_i_micro_qr_as_svg.svg)

## Unicode string generation

```rust
use qrcode::QrCode;
use qrcode::render::unicode;

fn main() {
    let code = QrCode::new("mow mow").unwrap();
    let image = code.render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    println!("{image}");
}
```

Generates this output:

```text
█████████████████████████████
█████████████████████████████
████ ▄▄▄▄▄ █ ▀▀▀▄█ ▄▄▄▄▄ ████
████ █   █ █▀ ▀ ▀█ █   █ ████
████ █▄▄▄█ ██▄  ▀█ █▄▄▄█ ████
████▄▄▄▄▄▄▄█ ▀▄▀ █▄▄▄▄▄▄▄████
████▄▀ ▄▀ ▄ █▄█  ▀ ▀█ █▄ ████
████▄██▄▄▀▄▄▀█▄ ██▀▀█▀▄▄▄████
█████▄▄▄█▄▄█  ▀▀▄█▀▀▀▄█▄▄████
████ ▄▄▄▄▄ █   ▄▄██▄ ▄ ▀▀████
████ █   █ █▀▄▄▀▄▄ ▄▄▄▄ ▄████
████ █▄▄▄█ █▄  █▄▀▄▀██▄█▀████
████▄▄▄▄▄▄▄█▄████▄█▄██▄██████
█████████████████████████████
▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
```

## PIC generation

```rust
use qrcode::render::pic;
use qrcode::QrCode;

fn main() {
    let code = QrCode::new(b"01234567").unwrap();
    let image = code
        .render::<pic::Color>()
        .min_dimensions(1, 1)
        .build();
    println!("{image}");
}
```

Generates [PIC](https://en.wikipedia.org/wiki/PIC_(markup_language))
output that renders as follows:

```pic
maxpswid=29;maxpsht=29;movewid=0;moveht=1;boxwid=1;boxht=1
define p { box wid $3 ht $4 fill 1 thickness 0.1 with .nw at $1,-$2 }
box wid maxpswid ht maxpsht with .nw at 0,0
p(4,4,1,1)
p(5,4,1,1)
p(6,4,1,1)
p(7,4,1,1)
p(8,4,1,1)
p(9,4,1,1)
…
```
See [`test_annex_i_micro_qr_as_pic.pic`](src/test_annex_i_micro_qr_as_pic.pic) for a full example.

## EPS generation

```rust
use qrcode::render::eps;
use qrcode::{EcLevel, QrCode, Version};

fn main() {
    let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
    let image = code
        .render()
        .min_dimensions(200, 200)
        .dark_color(eps::Color([0.5, 0.0, 0.0]))
        .light_color(eps::Color([1.0, 1.0, 0.5]))
        .build();
    println!("{image}");
}
```

Generates [EPS](https://en.wikipedia.org/wiki/Encapsulated_PostScript)
output that renders as follows:

```postscript
%!PS-Adobe-3.0 EPSF-3.0
%%BoundingBox: 0 0 204 204
%%Pages: 1
%%EndComments
gsave
1 1 0.5 setrgbcolor
0 0 204 204 rectfill
grestore
0.5 0 0 setrgbcolor
24 180 12 12 rectfill
36 180 12 12 rectfill
48 180 12 12 rectfill
60 180 12 12 rectfill
72 180 12 12 rectfill
84 180 12 12 rectfill
…
```
See [`test_annex_i_micro_qr_as_eps.eps`](src/test_annex_i_micro_qr_as_eps.eps) for a full example.

Panics and errors
-----------------

Every entry point is total with respect to its input: a value the encoder cannot
represent is returned as a `QrError`, never raised as a panic. This holds in
release builds — the numeric conversions used to check only under
`debug_assertions` and wrap silently otherwise, which turned a rejected input
into a corrupt QR code rather than an error.

Three things keep it true:

* `Cargo.toml` denies `clippy::{indexing_slicing, unwrap_used, expect_used,
  panic, unreachable}` for the library, so a new panicking construct has to be
  introduced deliberately with an `#[expect(..., reason = "...")]` explaining why
  it cannot fire.
* `tests/no_panic.rs` sweeps every version, error correction level and mask
  pattern, every byte pair, and coordinates on and past each edge, plus a
  deterministic randomised round.
* `fuzz/` holds `cargo fuzz` targets for the encoding and rendering paths, built
  with `overflow-checks = true`:

  ```sh
  cargo install cargo-fuzz
  cargo +nightly fuzz run encode
  cargo +nightly fuzz run render
  ```

The methods that still panic do so only on a broken caller contract, each has a
`# Panics` section, and each has a checked twin:

| Panicking | Checked |
| --- | --- |
| `code[(x, y)]` | `QrCode::get(x, y) -> Option<Color>` |
| `QrCode::is_functional` | `QrCode::get_functional -> Option<bool>` |
| `Canvas::get` / `get_mut` | `Canvas::get_module` / `get_module_mut` |
| `Canvas::put` | `Canvas::try_put` |
| `Canvas::apply_mask` | `Canvas::try_apply_mask` |
| `Renderer::new` | `Renderer::try_new` |
| `Renderer::build` | `Renderer::try_build` |

Allocation failure remains outside the library's control. `Renderer::try_build`
refuses anything above `render::MAX_IMAGE_PIXELS` rather than handing a saturated
length to the allocator, but that is a bound on the pixel *count*, not on memory:
a request that passes it can still be several gigabytes once multiplied by the
backend's element size. Deciding how large an image is affordable is the
caller's, not the library's.

Upgrading from 0.14
-------------------

`Version` is now validated on construction, which is what makes the per-version
table lookups infallible. The enum variants are gone:

```rust
// 0.14
let version = Version::Normal(5);
let micro = Version::Micro(2);

// 0.15
let version = Version::normal(5)?;
let micro = Version::micro(2)?;
```

`Version::normal` and `Version::micro` return `Err(QrError::InvalidVersion)`
outside 1..=40 and 1..=4 respectively, and `Version::number()` reads the number
back. Matching on a `Version` is no longer possible; use `is_micro()`,
`number()` and `width()`.

The other breaking changes:

* `Version::fetch` takes `&[[T; 4]; VERSION_COUNT]` instead of a slice, so a
  short table is a compile error rather than an index panic. It now also rejects
  a default-valued entry for normal versions, matching what it already did for
  Micro ones.
* `ec::create_error_correction_code` returns `QrResult<Vec<u8>>`; sizes above
  `ec::MAX_EC_CODE_SIZE` are an error rather than an index panic.
* `ec::construct_codewords` validates the length of `rawbits` instead of
  asserting it only in debug builds.
* `Canvas::apply_best_mask` returns `QrResult<Canvas>`.
* `QrError` gained `InvalidMaskPattern`, `CoordinateOutOfRange` and
  `ImageTooLarge`, and implements `core::error::Error` in `no_std` builds too.
* `push_numeric_data`, `push_alphanumeric_data` and `push_kanji_data` return
  `Err(QrError::InvalidCharacter)` for bytes outside their character set. The
  alphanumeric encoder previously encoded them as the digit `0`.
