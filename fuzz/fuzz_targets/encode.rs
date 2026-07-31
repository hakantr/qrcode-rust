//! Drives the encoder over arbitrary payloads and version/ec combinations.
//!
//! The contract under test: for any input, `qrcode` either produces a symbol or
//! returns a `QrError`. It never aborts. Built with `overflow-checks = true`, so
//! arithmetic that would silently wrap in a normal release build fails here.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use qrcode::{EcLevel, QrCode, Version};

#[derive(Arbitrary, Debug)]
enum Level {
    L,
    M,
    Q,
    H,
}

impl From<Level> for EcLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::L => Self::L,
            Level::M => Self::M,
            Level::Q => Self::Q,
            Level::H => Self::H,
        }
    }
}

#[derive(Arbitrary, Debug)]
struct Input {
    /// Deliberately unconstrained: out-of-range numbers must be rejected by the
    /// constructors rather than propagate into a table lookup.
    version_number: i16,
    micro: bool,
    level: Level,
    auto_version: bool,
    data: Vec<u8>,
}

fuzz_target!(|input: Input| {
    let ec_level = EcLevel::from(input.level);

    if input.auto_version {
        // The version-picking path: must fit the data or report DataTooLong.
        if let Ok(code) = QrCode::with_error_correction_level(&input.data, ec_level) {
            assert_eq!(code.to_colors().len(), code.width() * code.width());
            let _ = code.max_allowed_errors();
        }
        return;
    }

    let version = if input.micro { Version::micro(input.version_number) } else { Version::normal(input.version_number) };
    let Ok(version) = version else { return };

    if let Ok(code) = QrCode::with_version(&input.data, version, ec_level) {
        assert_eq!(code.width(), usize::try_from(version.width()).unwrap());
        assert_eq!(code.to_colors().len(), code.width() * code.width());

        // Every in-range coordinate resolves, every out-of-range one reports.
        let width = code.width();
        assert!(code.get(width, 0).is_none());
        assert!(code.get(0, width).is_none());
        assert!(code.get(usize::MAX, usize::MAX).is_none());
        assert_eq!(code.get(0, 0), Some(code[(0, 0)]));
    }
});
