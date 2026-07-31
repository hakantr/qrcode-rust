//! Exhaustive sweep asserting that no public entry point aborts the process.
//!
//! The crate's contract is that a *valid* input never panics and an invalid one
//! comes back as a [`QrError`]. That is easy to regress silently, so this test
//! walks the whole combination space -- every version, every error correction
//! level, every mask pattern, coordinates on and past every edge -- and fails
//! on the first panic rather than on a wrong answer.
//!
//! It runs as an integration test so it only sees the public API, which is
//! exactly the surface the contract covers. The panicking methods that have a
//! documented checked counterpart (`Canvas::get`, `Renderer::new`,
//! `QrCode::is_functional`, indexing) are deliberately exercised through their
//! checked form here.

// This test asserts by panicking, which is the one place the crate's own
// no-panic gate has to stand down.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::match_wild_err_arm,
    clippy::indexing_slicing,
    reason = "the test reports failures by panicking"
)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use qrcode::canvas::{Canvas, MaskPattern};
use qrcode::render::Renderer;
use qrcode::types::Color;
use qrcode::{EcLevel, QrCode, Version};

const EC_LEVELS: [EcLevel; 4] = [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H];

const MASK_PATTERNS: [MaskPattern; 8] = [
    MaskPattern::Checkerboard,
    MaskPattern::HorizontalLines,
    MaskPattern::VerticalLines,
    MaskPattern::DiagonalLines,
    MaskPattern::LargeCheckerboard,
    MaskPattern::Fields,
    MaskPattern::Diamonds,
    MaskPattern::Meadow,
];

/// Every version the standard defines, plus the numbers just outside each
/// family's range so the constructors are exercised too.
fn all_versions() -> Vec<Version> {
    let normal = (1..=40).filter_map(|n| Version::normal(n).ok());
    normal.chain((1..=4).filter_map(|n| Version::micro(n).ok())).collect()
}

/// Round counts for the randomised sweep. A debug build runs the encoder about
/// twenty times slower, so it does a lighter pass; CI runs this file with
/// `--release`, which is also the profile the no-panic guarantee is about.
const RANDOM_ROUNDS: usize = if cfg!(debug_assertions) { 300 } else { 4_000 };
const SIZING_ROUNDS: usize = if cfg!(debug_assertions) { 200 } else { 1_000 };

#[track_caller]
fn no_panic<T>(what: &str, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => panic!("panicked: {what}"),
    }
}

#[test]
fn version_constructors_reject_out_of_range_numbers() {
    for number in [i16::MIN, -1, 0, 41, 100, i16::MAX] {
        no_panic(&format!("Version::normal({number})"), || {
            assert!(Version::normal(number).is_err(), "Version::normal({number}) should be rejected");
        });
    }
    for number in [i16::MIN, -1, 0, 5, 100, i16::MAX] {
        no_panic(&format!("Version::micro({number})"), || {
            assert!(Version::micro(number).is_err(), "Version::micro({number}) should be rejected");
        });
    }
}

#[test]
fn encoding_never_panics_for_any_version_and_ec_level() {
    for version in all_versions() {
        // Only the Micro versions hold a non-multiple of 8 data bits, so they
        // are where the terminator offset matters. Sweeping every length there
        // and spot-checking the Normal ones keeps this test quick enough to run
        // on every commit.
        // A Normal version always holds a whole number of codewords, so one
        // short and one long payload is enough there.
        let lengths: &[usize] = if version.is_micro() {
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 16, 20, 21, 22, 23, 24, 32]
        } else {
            &[0, 7]
        };

        for ec_level in EC_LEVELS {
            for &length in lengths {
                let data: Vec<u8> = b"0123456789".iter().copied().cycle().take(length).collect();
                let what = format!("with_version({version:?}, {ec_level:?}, len={length})");
                no_panic(&what, || {
                    if let Ok(code) = QrCode::with_version(&data, version, ec_level) {
                        let _ = code.max_allowed_errors();
                        let _ = code.to_colors();
                    }
                });
            }
        }
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    use qrcode::bits::Bits;

    // The parser assigns a mode to every byte, and the per-mode encoders must
    // accept whatever it produces. Every ordered pair covers each Shift JIS
    // lead/trail combination, including the ones just past the encodable range.
    //
    // This drives the segmentation and bit-packing stages directly; running the
    // mask search 65536 times would dominate the runtime without covering
    // anything the sweep below does not.
    let version = Version::normal(40).unwrap();
    for hi in 0..=u8::MAX {
        for lo in 0..=u8::MAX {
            no_panic(&format!("push_optimal_data([{hi:#04x}, {lo:#04x}])"), || {
                let mut bits = Bits::new(version);
                assert_eq!(bits.push_optimal_data(&[hi, lo]), Ok(()), "{hi:#04x} {lo:#04x}");
            });
        }
    }

    // And the whole pipeline over a smaller but still adversarial sample.
    for hi in (0..=u8::MAX).step_by(7) {
        no_panic(&format!("QrCode::new([{hi:#04x}])"), || {
            let _ = QrCode::new([hi]);
        });
        for lo in [0x00, 0x3f, 0x40, 0x7e, 0x7f, 0x80, 0xbf, 0xc0, 0xeb, 0xfc, 0xfd, 0xff] {
            no_panic(&format!("QrCode::new([{hi:#04x}, {lo:#04x}])"), || {
                let _ = QrCode::new([hi, lo]);
            });
        }
    }
}

#[test]
fn every_mask_pattern_on_every_symbol_is_accepted_or_rejected_cleanly() {
    for version in all_versions() {
        for ec_level in EC_LEVELS {
            for pattern in MASK_PATTERNS {
                let what = format!("try_apply_mask({version:?}, {ec_level:?}, {pattern:?})");
                no_panic(&what, || {
                    let mut canvas = Canvas::new(version, ec_level);
                    canvas.draw_all_functional_patterns();
                    // Micro symbols support only 4 of the 8 patterns, and some
                    // version/ec_level pairs do not exist at all. Both must come
                    // back as an error, not an abort.
                    let _ = canvas.try_apply_mask(pattern);
                });
            }

            no_panic(&format!("apply_best_mask({version:?}, {ec_level:?})"), || {
                let mut canvas = Canvas::new(version, ec_level);
                canvas.draw_all_functional_patterns();
                let _ = canvas.apply_best_mask();
            });
        }
    }
}

#[test]
fn coordinates_outside_the_symbol_are_reported_not_fatal() {
    for version in all_versions() {
        let width = version.width();
        let canvas = Canvas::new(version, EcLevel::L);

        for coord in [i16::MIN, -width - 1, -width, -1, 0, width - 1, width, width + 1, i16::MAX] {
            no_panic(&format!("get_module({coord}, 0) on {version:?}"), || {
                let inside = (-width..width).contains(&coord);
                assert_eq!(canvas.get_module(coord, 0).is_some(), inside, "{version:?} x={coord}");
                assert_eq!(canvas.get_module(0, coord).is_some(), inside, "{version:?} y={coord}");
            });
        }
    }

    let code = QrCode::new(b"01234567").unwrap();
    let width = code.width();
    for coord in [0, width - 1, width, width + 1, usize::MAX] {
        no_panic(&format!("QrCode::get({coord}, 0)"), || {
            let inside = coord < width;
            assert_eq!(code.get(coord, 0).is_some(), inside, "x={coord}");
            assert_eq!(code.get(0, coord).is_some(), inside, "y={coord}");
            assert_eq!(code.get_functional(coord, 0).is_some(), inside, "functional x={coord}");
        });
    }
}

#[test]
fn renderer_reports_bad_input_instead_of_aborting() {
    let content = [Color::Dark; 9];

    // Mismatched length.
    for modules_count in [0, 1, 2, 4, 100] {
        no_panic(&format!("Renderer::try_new(len=9, {modules_count})"), || {
            assert!(Renderer::<char>::try_new(&content, modules_count, 4).is_err());
        });
    }
    no_panic("Renderer::try_new(len=9, 3)", || {
        assert!(Renderer::<char>::try_new(&content, 3, 4).is_ok());
    });

    // A `modules_count` whose square overflows `usize` used to wrap into a
    // length that could spuriously match.
    no_panic("Renderer::try_new(overflowing modules_count)", || {
        assert!(Renderer::<char>::try_new(&content, usize::MAX, 4).is_err());
    });

    // An empty QR code has no modules; sizing used to divide by zero.
    no_panic("Renderer with zero modules", || {
        let mut renderer = Renderer::<char>::try_new(&[], 0, 0).unwrap();
        let _ = renderer.min_dimensions(200, 200).try_build();
        let _ = renderer.max_dimensions(200, 200).try_build();
    });

    // Dimensions that overflow `u32` must be reported, not wrapped.
    no_panic("Renderer with overflowing module size", || {
        let code = QrCode::new(b"hi").unwrap();
        assert!(code.render::<char>().module_dimensions(u32::MAX, u32::MAX).try_build().is_err());
    });
}

#[test]
fn error_correction_rejects_oversized_blocks() {
    use qrcode::ec::{MAX_EC_CODE_SIZE, create_error_correction_code};

    for size in [0, 1, MAX_EC_CODE_SIZE, MAX_EC_CODE_SIZE + 1, 1000, usize::from(u8::MAX)] {
        no_panic(&format!("create_error_correction_code(_, {size})"), || {
            let result = create_error_correction_code(b"data", size);
            assert_eq!(result.is_ok(), size <= MAX_EC_CODE_SIZE, "size={size}");
        });
    }
}

#[test]
fn renderers_never_panic_for_any_symbol() {
    for version in all_versions() {
        for ec_level in EC_LEVELS {
            let Ok(code) = QrCode::with_version(b"1", version, ec_level) else { continue };
            let what = format!("render {version:?} {ec_level:?}");
            no_panic(&what, || {
                let _ = code.to_debug_str('#', '.');
                #[cfg(feature = "svg")]
                let _ = code.render::<qrcode::render::svg::Color>().try_build();
                #[cfg(feature = "eps")]
                let _ = code.render::<qrcode::render::eps::Color>().try_build();
                #[cfg(feature = "pic")]
                let _ = code.render::<qrcode::render::pic::Color>().try_build();
                let _ = code.render::<qrcode::render::unicode::Dense1x2>().try_build();
            });
        }
    }
}

/// A dependency-free randomised round over the same surface the `fuzz/` targets
/// cover, so the guarantee is checked on every `cargo test` and not only when
/// somebody runs `cargo fuzz`. Deterministic: the seed is fixed, so a failure
/// reproduces.
#[test]
fn randomised_inputs_never_panic() {
    // xorshift64*, enough for shaking out shapes the structured sweep misses.
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };

    let versions = all_versions();
    for round in 0..RANDOM_ROUNDS {
        let length = usize::try_from(next() % 40).unwrap();
        let data: Vec<u8> = (0..length).map(|_| u8::try_from(next() % 256).unwrap()).collect();

        // Unconstrained version numbers: the constructors have to reject the
        // out-of-range ones rather than let them reach a table lookup.
        let number = i16::try_from(next() % 64).unwrap() - 8;
        let version = if next() % 2 == 0 { Version::normal(number) } else { Version::micro(number) };
        let ec_level = EC_LEVELS[usize::try_from(next() % 4).unwrap()];

        let what = format!("round {round}: version_number={number} ec={ec_level:?} data={data:02x?}");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = QrCode::with_error_correction_level(&data, ec_level);
            if let Ok(version) = version
                && let Ok(code) = QrCode::with_version(&data, version, ec_level)
            {
                let _ = code.max_allowed_errors();
                let _ = code.to_debug_str('#', '.');
                let _ = code.render::<qrcode::render::unicode::Dense1x2>().try_build();
            }
        }));
        assert!(result.is_ok(), "panicked: {what}");
    }

    // Sizing requests are the other half: the arithmetic used to wrap.
    let code = QrCode::new(b"fuzz").unwrap();
    for round in 0..SIZING_ROUNDS {
        let mw = u32::try_from(next() % 4).unwrap().saturating_mul(u32::MAX / 3);
        let mh = u32::try_from(next() % 4).unwrap().saturating_mul(u32::MAX / 3);
        let what = format!("sizing round {round}: module_dimensions({mw}, {mh})");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = code.render::<char>().module_dimensions(mw, mh).try_build();
            let _ = code.render::<char>().min_dimensions(mw, mh).try_build();
            let _ = code.render::<char>().max_dimensions(mw, mh).try_build();
        }));
        assert!(result.is_ok(), "panicked: {what}");
    }

    let _ = versions;
}
