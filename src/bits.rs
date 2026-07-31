//! The `bits` module encodes binary data into raw bits used in a QR code.

// The binary literals in this module's tests are grouped along the QR code bit
// field boundaries (mode indicator, character count, data) rather than in even
// nibbles. That is what makes them checkable against ISO/IEC 18004 by eye, so
// the two literal-formatting lints are deliberately off here.
#![allow(clippy::unusual_byte_groupings, clippy::unreadable_literal)]

use alloc::vec::Vec;
use core::cmp::min;

#[cfg(feature = "bench")]
extern crate test;

use crate::cast::{As, Truncate};
use crate::optimize::{Optimizer, Parser, Segment, total_encoded_len};
use crate::types::{EcLevel, Mode, QrError, QrResult, Version, VersionKind};

//------------------------------------------------------------------------------
//{{{ Bits

/// The `Bits` structure stores the encoded data for a QR code.
pub struct Bits {
    data: Vec<u8>,
    bit_offset: usize,
    version: Version,
}

impl Bits {
    /// Constructs a new, empty bits structure.
    pub const fn new(version: Version) -> Self {
        Self { data: Vec::new(), bit_offset: 0, version }
    }

    /// Pushes an N-bit big-endian integer to the end of the bits.
    ///
    /// Note: It is up to the developer to ensure that `number` really only is
    /// `n` bit in size. Otherwise the excess bits may stomp on the existing
    /// ones.
    fn push_number(&mut self, n: usize, number: u16) {
        debug_assert!(n == 16 || n < 16 && number < (1 << n), "{number} is too big as a {n}-bit number");

        let b = self.bit_offset + n;
        match (self.bit_offset, b) {
            (0, 0..=8) => {
                self.data.push((number << (8 - b)).truncate_as_u8());
            }
            (0, _) => {
                self.data.push((number >> (b - 8)).truncate_as_u8());
                self.data.push((number << (16 - b)).truncate_as_u8());
            }
            // A non-zero `bit_offset` means the last byte is partially filled,
            // so `last_mut()` is always `Some` in the arms below.
            (_, 0..=8) => {
                if let Some(last) = self.data.last_mut() {
                    *last |= (number << (8 - b)).truncate_as_u8();
                }
            }
            (_, 9..=16) => {
                if let Some(last) = self.data.last_mut() {
                    *last |= (number >> (b - 8)).truncate_as_u8();
                }
                self.data.push((number << (16 - b)).truncate_as_u8());
            }
            _ => {
                if let Some(last) = self.data.last_mut() {
                    *last |= (number >> (b - 8)).truncate_as_u8();
                }
                self.data.push((number >> (b - 16)).truncate_as_u8());
                self.data.push((number << (24 - b)).truncate_as_u8());
            }
        }
        self.bit_offset = b & 7;
    }

    /// Pushes an N-bit big-endian integer to the end of the bits, and check
    /// that the number does not overflow the bits.
    ///
    /// Returns `Err(QrError::DataTooLong)` on overflow.
    fn push_number_checked(&mut self, n: usize, number: usize) -> QrResult<()> {
        if n > 16 || number >= (1 << n) {
            Err(QrError::DataTooLong)
        } else {
            self.push_number(n, number.as_u16());
            Ok(())
        }
    }

    /// Reserves `n` extra bits of space for pushing.
    fn reserve(&mut self, n: usize) {
        let extra_bytes = (n + (8 - self.bit_offset) % 8).div_ceil(8);
        self.data.reserve(extra_bytes);
    }

    /// Convert the bits into a bytes vector.
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    /// Total number of bits currently pushed.
    pub fn len(&self) -> usize {
        if self.bit_offset == 0 { self.data.len() * 8 } else { (self.data.len() - 1) * 8 + self.bit_offset }
    }

    /// Whether there are any bits pushed.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The maximum number of bits allowed by the provided QR code version and
    /// error correction level.
    ///
    /// # Errors
    ///
    /// Returns `Err(QrError::InvalidVersion)` if it is not valid to use the
    /// `ec_level` for the given version (e.g. `Version::micro(1)` with
    /// `EcLevel::H`).
    pub fn max_len(&self, ec_level: EcLevel) -> QrResult<usize> {
        self.version.fetch(ec_level, &DATA_LENGTHS)
    }

    /// Version of the QR code.
    pub const fn version(&self) -> Version {
        self.version
    }
}

#[test]
fn test_push_number() {
    use alloc::vec;
    let mut bits = Bits::new(Version::normal(1).unwrap());

    bits.push_number(3, 0b010); // 0:0 .. 0:3
    bits.push_number(3, 0b110); // 0:3 .. 0:6
    bits.push_number(3, 0b101); // 0:6 .. 1:1
    bits.push_number(7, 0b001_1010); // 1:1 .. 2:0
    bits.push_number(4, 0b1100); // 2:0 .. 2:4
    bits.push_number(12, 0b1011_0110_1101); // 2:4 .. 4:0
    bits.push_number(10, 0b01_1001_0001); // 4:0 .. 5:2
    bits.push_number(15, 0b111_0010_1110_0011); // 5:2 .. 7:1

    let bytes = bits.into_bytes();

    assert_eq!(
        bytes,
        vec![
            0b010__110__10, // 90
            0b1__001_1010,  // 154
            0b1100__1011,   // 203
            0b0110_1101,    // 109
            0b01_1001_00,   // 100
            0b01__111_001,  // 121
            0b0_1110_001,   // 113
            0b1__0000000,   // 128
        ]
    );
}

#[cfg(feature = "bench")]
#[bench]
fn bench_push_splitted_bytes(bencher: &mut test::Bencher) {
    bencher.iter(|| {
        let mut bits = Bits::new(Version::normal(40).unwrap());
        bits.push_number(4, 0b0101);
        for _ in 0..1024 {
            bits.push_number(8, 0b10101010);
        }
        bits.into_bytes()
    });
}

//}}}
//------------------------------------------------------------------------------
//{{{ Mode indicator

/// An "extended" mode indicator, includes all indicators supported by QR code
/// beyond those bearing data.
#[derive(Copy, Clone)]
pub enum ExtendedMode {
    /// ECI mode indicator, to introduce an ECI designator.
    Eci,

    /// The normal mode to introduce data.
    Data(Mode),

    /// FNC-1 mode in the first position.
    Fnc1First,

    /// FNC-1 mode in the second position.
    Fnc1Second,

    /// Structured append.
    StructuredAppend,
}

impl Bits {
    /// Push the mode indicator to the end of the bits.
    ///
    /// # Errors
    ///
    /// If the mode is not supported in the provided version, this method
    /// returns `Err(QrError::UnsupportedCharacterSet)`.
    pub fn push_mode_indicator(&mut self, mode: ExtendedMode) -> QrResult<()> {
        let number = match (self.version.kind(), mode) {
            (VersionKind::Micro(1), ExtendedMode::Data(Mode::Numeric)) => return Ok(()),
            (VersionKind::Micro(_), ExtendedMode::Data(Mode::Numeric)) => 0,
            (VersionKind::Micro(_), ExtendedMode::Data(Mode::Alphanumeric)) => 1,
            (VersionKind::Micro(_), ExtendedMode::Data(Mode::Byte)) => 0b10,
            (VersionKind::Micro(_), ExtendedMode::Data(Mode::Kanji)) => 0b11,
            (VersionKind::Micro(_), _) => return Err(QrError::UnsupportedCharacterSet),
            (_, ExtendedMode::Data(Mode::Numeric)) => 0b0001,
            (_, ExtendedMode::Data(Mode::Alphanumeric)) => 0b0010,
            (_, ExtendedMode::Data(Mode::Byte)) => 0b0100,
            (_, ExtendedMode::Data(Mode::Kanji)) => 0b1000,
            (_, ExtendedMode::Eci) => 0b0111,
            (_, ExtendedMode::Fnc1First) => 0b0101,
            (_, ExtendedMode::Fnc1Second) => 0b1001,
            (_, ExtendedMode::StructuredAppend) => 0b0011,
        };
        let bits = self.version.mode_bits_count();
        self.push_number_checked(bits, number).or(Err(QrError::UnsupportedCharacterSet))
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ ECI

impl Bits {
    /// Push an ECI (Extended Channel Interpretation) designator to the bits.
    ///
    /// An ECI designator is a 6-digit number to specify the character set of
    /// the following binary data. After calling this method, one could call
    /// `.push_byte_data()` or similar methods to insert the actual data, e.g.
    ///
    /// ```
    /// #![allow(unused_must_use)]
    ///
    /// use qrcode::bits::Bits;
    /// use qrcode::types::Version;
    ///
    /// let mut bits = Bits::new(Version::normal(1).unwrap());
    /// bits.push_eci_designator(9); // 9 = ISO-8859-7 (Greek).
    /// bits.push_byte_data(b"\xa1\xa2\xa3\xa4\xa5"); // ΑΒΓΔΕ
    /// ```
    ///
    /// The full list of ECI designator values can be found from
    /// <http://strokescribe.com/en/ECI.html>. Some example values are:
    ///
    /// | ECI # | Character set                             |
    /// | ----- | ----------------------------------------- |
    /// | 3     | ISO-8859-1 (Western European)             |
    /// | 20    | Shift JIS (Japanese)                      |
    /// | 23    | Windows 1252 (Latin 1) (Western European) |
    /// | 25    | UTF-16 Big Endian                         |
    /// | 26    | UTF-8                                     |
    /// | 28    | Big 5 (Traditional Chinese)               |
    /// | 29    | GB-18030 (Simplified Chinese)             |
    /// | 30    | EUC-KR (Korean)                           |
    ///
    /// # Errors
    ///
    /// If the QR code version does not support ECI, this method will return
    /// `Err(QrError::UnsupportedCharacterSet)`.
    ///
    /// If the designator is outside of the expected range, this method will
    /// return `Err(QrError::InvalidECIDesignator)`.
    pub fn push_eci_designator(&mut self, eci_designator: u32) -> QrResult<()> {
        self.reserve(12); // assume the common case that eci_designator <= 127.
        self.push_mode_indicator(ExtendedMode::Eci)?;
        match eci_designator {
            0..=127 => {
                self.push_number(8, eci_designator.as_u16());
            }
            128..=16383 => {
                self.push_number(2, 0b10);
                self.push_number(14, eci_designator.as_u16());
            }
            16384..=999_999 => {
                self.push_number(3, 0b110);
                self.push_number(5, (eci_designator >> 16).as_u16());
                self.push_number(16, (eci_designator & 0xffff).as_u16());
            }
            _ => return Err(QrError::InvalidEciDesignator),
        }
        Ok(())
    }
}

#[cfg(test)]
mod eci_tests {
    use crate::bits::Bits;
    use crate::types::{QrError, Version};
    use alloc::vec;

    #[test]
    fn test_9() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_eci_designator(9), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b0111__0000, 0b1001__0000]);
    }

    #[test]
    fn test_899() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_eci_designator(899), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b0111__10_00, 0b00111000, 0b0011__0000]);
    }

    #[test]
    fn test_999999() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_eci_designator(999999), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b0111__110_0, 0b11110100, 0b00100011, 0b1111__0000]);
    }

    #[test]
    fn test_invalid_designator() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_eci_designator(1000000), Err(QrError::InvalidEciDesignator));
    }

    #[test]
    fn test_unsupported_character_set() {
        let mut bits = Bits::new(Version::micro(4).unwrap());
        assert_eq!(bits.push_eci_designator(9), Err(QrError::UnsupportedCharacterSet));
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Mode::Numeric mode

impl Bits {
    fn push_header(&mut self, mode: Mode, raw_data_len: usize) -> QrResult<()> {
        let length_bits = mode.length_bits_count(self.version);
        self.reserve(length_bits + 4 + mode.data_bits_count(raw_data_len));
        self.push_mode_indicator(ExtendedMode::Data(mode))?;
        self.push_number_checked(length_bits, raw_data_len)?;
        Ok(())
    }

    /// Encodes a numeric string to the bits.
    ///
    /// The data should only contain the characters 0 to 9.
    ///
    /// # Errors
    ///
    /// Returns `Err(QrError::DataTooLong)` on overflow.
    ///
    /// Returns `Err(QrError::InvalidCharacter)` if the data contains a byte
    /// which is not an ASCII digit.
    pub fn push_numeric_data(&mut self, data: &[u8]) -> QrResult<()> {
        self.push_header(Mode::Numeric, data.len())?;
        for chunk in data.chunks(3) {
            let mut number = 0_u16;
            for b in chunk {
                let digit = numeric_digit(*b).ok_or(QrError::InvalidCharacter)?;
                number = number * 10 + digit;
            }
            let length = chunk.len() * 3 + 1;
            self.push_number(length, number);
        }
        Ok(())
    }
}

/// In QR code `Mode::Numeric` mode, a triple of digits is encoded as a base-10
/// integer. `numeric_digit` converts a single character into its corresponding
/// base-10 digit, or `None` if it is not an ASCII digit.
///
/// The conversion is specified in ISO/IEC 18004:2006, §8.4.2.
#[inline]
const fn numeric_digit(character: u8) -> Option<u16> {
    match character {
        b'0'..=b'9' => Some((character - b'0') as u16),
        _ => None,
    }
}

#[cfg(test)]
mod numeric_tests {
    use crate::bits::Bits;
    use crate::types::{QrError, Version};
    use alloc::vec;

    #[test]
    fn test_iso_18004_2006_example_1() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"01234567"), Ok(()));
        assert_eq!(
            bits.into_bytes(),
            vec![0b0001_0000, 0b001000_00, 0b00001100, 0b01010110, 0b01_100001, 0b1__0000000]
        );
    }

    #[test]
    fn test_iso_18004_2000_example_2() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"0123456789012345"), Ok(()));
        assert_eq!(
            bits.into_bytes(),
            vec![
                0b0001_0000,
                0b010000_00,
                0b00001100,
                0b01010110,
                0b01_101010,
                0b0110_1110,
                0b000101_00,
                0b11101010,
                0b0101__0000,
            ]
        );
    }

    #[test]
    fn test_iso_18004_2006_example_2() {
        let mut bits = Bits::new(Version::micro(3).unwrap());
        assert_eq!(bits.push_numeric_data(b"0123456789012345"), Ok(()));
        assert_eq!(
            bits.into_bytes(),
            vec![
                0b00_10000_0,
                0b00000110,
                0b0_0101011,
                0b001_10101,
                0b00110_111,
                0b0000101_0,
                0b01110101,
                0b00101__000,
            ]
        );
    }

    #[test]
    fn test_data_too_long_error() {
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"12345678"), Err(QrError::DataTooLong));
    }

    #[test]
    fn test_invalid_character() {
        // These used to overflow `*b - b'0'` and abort the process instead.
        for data in [&b"12/45"[..], b"!!", b"\xff\xff", b" 1"] {
            let mut bits = Bits::new(Version::normal(1).unwrap());
            assert_eq!(bits.push_numeric_data(data), Err(QrError::InvalidCharacter), "{data:?}");
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Mode::Alphanumeric mode

/// In QR code `Mode::Alphanumeric` mode, a pair of alphanumeric characters will
/// be encoded as a base-45 integer. `alphanumeric_digit` converts each
/// character into its corresponding base-45 digit.
///
/// The conversion is specified in ISO/IEC 18004:2006, §8.4.3, Table 5.
///
/// Returns `None` if the character is outside of the alphanumeric character set.
#[inline]
const fn alphanumeric_digit(character: u8) -> Option<u16> {
    Some(match character {
        b'0'..=b'9' => (character - b'0') as u16,
        b'A'..=b'Z' => (character - b'A') as u16 + 10,
        b' ' => 36,
        b'$' => 37,
        b'%' => 38,
        b'*' => 39,
        b'+' => 40,
        b'-' => 41,
        b'.' => 42,
        b'/' => 43,
        b':' => 44,
        _ => return None,
    })
}

impl Bits {
    /// Encodes an alphanumeric string to the bits.
    ///
    /// The data should only contain the charaters A to Z (excluding lowercase),
    /// 0 to 9, space, `$`, `%`, `*`, `+`, `-`, `.`, `/` or `:`.
    ///
    /// # Errors
    ///
    /// Returns `Err(QrError::DataTooLong)` on overflow.
    ///
    /// Returns `Err(QrError::InvalidCharacter)` if the data contains a byte
    /// outside of the alphanumeric character set.
    pub fn push_alphanumeric_data(&mut self, data: &[u8]) -> QrResult<()> {
        self.push_header(Mode::Alphanumeric, data.len())?;
        for chunk in data.chunks(2) {
            let mut number = 0_u16;
            for b in chunk {
                let digit = alphanumeric_digit(*b).ok_or(QrError::InvalidCharacter)?;
                number = number * 45 + digit;
            }
            let length = chunk.len() * 5 + 1;
            self.push_number(length, number);
        }
        Ok(())
    }
}

#[cfg(test)]
mod alphanumeric_tests {
    use crate::bits::Bits;
    use crate::types::{QrError, Version};
    use alloc::vec;

    #[test]
    fn test_iso_18004_2006_example() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_alphanumeric_data(b"AC-42"), Ok(()));
        assert_eq!(
            bits.into_bytes(),
            vec![0b0010_0000, 0b00101_001, 0b11001110, 0b11100111, 0b001_00001, 0b0__0000000]
        );
    }

    #[test]
    fn test_micro_qr_unsupported() {
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_alphanumeric_data(b"A"), Err(QrError::UnsupportedCharacterSet));
    }

    #[test]
    fn test_data_too_long() {
        let mut bits = Bits::new(Version::micro(2).unwrap());
        assert_eq!(bits.push_alphanumeric_data(b"ABCDEFGH"), Err(QrError::DataTooLong));
    }

    #[test]
    fn test_invalid_character() {
        // Lowercase letters and other symbols used to be silently encoded as
        // the digit 0, corrupting the payload without any error.
        for data in [&b"abc"[..], b"A;B", b"\x00", b"A\x1dB"] {
            let mut bits = Bits::new(Version::normal(1).unwrap());
            assert_eq!(bits.push_alphanumeric_data(data), Err(QrError::InvalidCharacter), "{data:?}");
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Mode::Byte mode

impl Bits {
    /// Encodes 8-bit byte data to the bits.
    ///
    /// # Errors
    ///
    /// Returns `Err(QrError::DataTooLong)` on overflow.
    pub fn push_byte_data(&mut self, data: &[u8]) -> QrResult<()> {
        self.push_header(Mode::Byte, data.len())?;
        for b in data {
            self.push_number(8, u16::from(*b));
        }
        Ok(())
    }
}

#[cfg(test)]
mod byte_tests {
    use crate::bits::Bits;
    use crate::types::{QrError, Version};
    use alloc::vec;

    #[test]
    fn test() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_byte_data(b"\x12\x34\x56\x78\x9a\xbc\xde\xf0"), Ok(()));
        assert_eq!(
            bits.into_bytes(),
            vec![
                0b0100_0000,
                0b1000_0001,
                0b0010_0011,
                0b0100_0101,
                0b0110_0111,
                0b1000_1001,
                0b1010_1011,
                0b1100_1101,
                0b1110_1111,
                0b0000__0000,
            ]
        );
    }

    #[test]
    fn test_micro_qr_unsupported() {
        let mut bits = Bits::new(Version::micro(2).unwrap());
        assert_eq!(bits.push_byte_data(b"?"), Err(QrError::UnsupportedCharacterSet));
    }

    #[test]
    fn test_data_too_long() {
        let mut bits = Bits::new(Version::micro(3).unwrap());
        assert_eq!(bits.push_byte_data(b"0123456701234567"), Err(QrError::DataTooLong));
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Mode::Kanji mode

impl Bits {
    /// Encodes Shift JIS double-byte data to the bits.
    ///
    /// # Errors
    ///
    /// Returns `Err(QrError::DataTooLong)` on overflow.
    ///
    /// Returns `Err(QrError::InvalidCharacter)` if the data is not Shift JIS
    /// double-byte data (e.g. if the length of data is not an even number, or
    /// if a character is outside of the ranges encodable in Kanji mode).
    pub fn push_kanji_data(&mut self, data: &[u8]) -> QrResult<()> {
        self.push_header(Mode::Kanji, data.len() / 2)?;
        for kanji in data.chunks(2) {
            let &[hi, lo] = kanji else {
                return Err(QrError::InvalidCharacter);
            };
            let cp = u16::from(hi) * 256 + u16::from(lo);
            // ISO/IEC 18004:2006 §8.4.5 — only these two ranges are compactable
            // into the 13 bits of a Kanji mode character. Anything else (e.g.
            // ASCII, or a lead byte of 0xeb followed by a high trail byte) would
            // underflow the subtraction or overflow the 13-bit field.
            let bytes = match cp {
                0x8140..=0x9ffc => cp - 0x8140,
                0xe040..=0xebbf => cp - 0xc140,
                _ => return Err(QrError::InvalidCharacter),
            };
            let number = (bytes >> 8) * 0xc0 + (bytes & 0xff);
            debug_assert!(number < (1 << 13), "{cp:#x} is not encodable in Kanji mode");
            self.push_number(13, number);
        }
        Ok(())
    }
}

#[cfg(test)]
mod kanji_tests {
    use crate::bits::Bits;
    use crate::types::{QrError, Version};
    use alloc::vec;

    #[test]
    fn test_iso_18004_example() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_kanji_data(b"\x93\x5f\xe4\xaa"), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b1000_0000, 0b0010_0110, 0b11001111, 0b1_1101010, 0b101010__00]);
    }

    #[test]
    fn test_micro_qr_unsupported() {
        let mut bits = Bits::new(Version::micro(2).unwrap());
        assert_eq!(bits.push_kanji_data(b"?"), Err(QrError::UnsupportedCharacterSet));
    }

    #[test]
    fn test_data_too_long() {
        let mut bits = Bits::new(Version::micro(3).unwrap());
        assert_eq!(bits.push_kanji_data(b"\x93_\x93_\x93_\x93_\x93_\x93_\x93_\x93_"), Err(QrError::DataTooLong));
    }

    #[test]
    fn test_invalid_character() {
        // `b"AA"` used to overflow `cp - 0x8140`, and `b"\xeb\xc0"` used to
        // overflow the 13-bit field and stomp on the neighbouring bits.
        for data in [&b"AA"[..], b"\x00\x00", b"\xeb\xc0", b"\x9f\xfd", b"\xe0\x3f", b"\xff\xff"] {
            let mut bits = Bits::new(Version::normal(1).unwrap());
            assert_eq!(bits.push_kanji_data(data), Err(QrError::InvalidCharacter), "{data:?}");
        }
    }

    #[test]
    fn test_boundary_characters() {
        // The extremes of both ranges must still round-trip successfully.
        for data in [&b"\x81\x40"[..], b"\x9f\xfc", b"\xe0\x40", b"\xeb\xbf"] {
            let mut bits = Bits::new(Version::normal(1).unwrap());
            assert_eq!(bits.push_kanji_data(data), Ok(()), "{data:?}");
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ FNC1 mode

impl Bits {
    /// Encodes an indicator that the following data are formatted according to
    /// the UCC/EAN Application Identifiers standard.
    ///
    /// ```
    /// #![allow(unused_must_use)]
    ///
    /// use qrcode::bits::Bits;
    /// use qrcode::types::Version;
    ///
    /// let mut bits = Bits::new(Version::normal(1).unwrap());
    /// bits.push_fnc1_first_position();
    /// bits.push_numeric_data(b"01049123451234591597033130128");
    /// bits.push_alphanumeric_data(b"%10ABC123");
    /// ```
    ///
    /// In QR code, the character `%` is used as the data field separator (0x1D).
    ///
    /// # Errors
    ///
    /// If the mode is not supported in the provided version, this method
    /// returns `Err(QrError::UnsupportedCharacterSet)`.
    pub fn push_fnc1_first_position(&mut self) -> QrResult<()> {
        self.push_mode_indicator(ExtendedMode::Fnc1First)
    }

    /// Encodes an indicator that the following data are formatted in accordance
    /// with specific industry or application specifications previously agreed
    /// with AIM International.
    ///
    /// ```
    /// #![allow(unused_must_use)]
    ///
    /// use qrcode::bits::Bits;
    /// use qrcode::types::Version;
    ///
    /// let mut bits = Bits::new(Version::normal(1).unwrap());
    /// bits.push_fnc1_second_position(37);
    /// bits.push_alphanumeric_data(b"AA1234BBB112");
    /// bits.push_byte_data(b"text text text text\r");
    /// ```
    ///
    /// If the application indicator is a single Latin alphabet (a–z / A–Z),
    /// please pass in its ASCII value + 100:
    ///
    /// ```ignore
    /// bits.push_fnc1_second_position(b'A' + 100);
    /// ```
    ///
    /// # Errors
    ///
    /// If the mode is not supported in the provided version, this method
    /// returns `Err(QrError::UnsupportedCharacterSet)`.
    pub fn push_fnc1_second_position(&mut self, application_indicator: u8) -> QrResult<()> {
        self.push_mode_indicator(ExtendedMode::Fnc1Second)?;
        self.push_number(8, u16::from(application_indicator));
        Ok(())
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Finish

// This table is copied from ISO/IEC 18004:2006 §6.4.10, Table 7.
static DATA_LENGTHS: [[usize; 4]; 44] = [
    // Normal versions
    [152, 128, 104, 72],
    [272, 224, 176, 128],
    [440, 352, 272, 208],
    [640, 512, 384, 288],
    [864, 688, 496, 368],
    [1088, 864, 608, 480],
    [1248, 992, 704, 528],
    [1552, 1232, 880, 688],
    [1856, 1456, 1056, 800],
    [2192, 1728, 1232, 976],
    [2592, 2032, 1440, 1120],
    [2960, 2320, 1648, 1264],
    [3424, 2672, 1952, 1440],
    [3688, 2920, 2088, 1576],
    [4184, 3320, 2360, 1784],
    [4712, 3624, 2600, 2024],
    [5176, 4056, 2936, 2264],
    [5768, 4504, 3176, 2504],
    [6360, 5016, 3560, 2728],
    [6888, 5352, 3880, 3080],
    [7456, 5712, 4096, 3248],
    [8048, 6256, 4544, 3536],
    [8752, 6880, 4912, 3712],
    [9392, 7312, 5312, 4112],
    [10208, 8000, 5744, 4304],
    [10960, 8496, 6032, 4768],
    [11744, 9024, 6464, 5024],
    [12248, 9544, 6968, 5288],
    [13048, 10136, 7288, 5608],
    [13880, 10984, 7880, 5960],
    [14744, 11640, 8264, 6344],
    [15640, 12328, 8920, 6760],
    [16568, 13048, 9368, 7208],
    [17528, 13800, 9848, 7688],
    [18448, 14496, 10288, 7888],
    [19472, 15312, 10832, 8432],
    [20528, 15936, 11408, 8768],
    [21616, 16816, 12016, 9136],
    [22496, 17728, 12656, 9776],
    [23648, 18672, 13328, 10208],
    // Micro versions
    [20, 0, 0, 0],
    [40, 32, 0, 0],
    [84, 68, 0, 0],
    [128, 112, 80, 0],
];

impl Bits {
    /// Pushes the ending bits to indicate no more data.
    ///
    /// # Errors
    ///
    /// Returns `Err(QrError::DataTooLong)` on overflow.
    ///
    /// Returns `Err(QrError::InvalidVersion)` if it is not valid to use the
    /// `ec_level` for the given version (e.g. `Version::micro(1)` with
    /// `EcLevel::H`).
    pub fn push_terminator(&mut self, ec_level: EcLevel) -> QrResult<()> {
        let terminator_size = if let VersionKind::Micro(a) = self.version.kind() { a.as_usize() * 2 + 1 } else { 4 };

        let cur_length = self.len();
        let data_length = self.max_len(ec_level)?;
        if cur_length > data_length {
            return Err(QrError::DataTooLong);
        }

        let terminator_size = min(terminator_size, data_length - cur_length);
        if terminator_size > 0 {
            self.push_number(terminator_size, 0);
        }

        if self.len() < data_length {
            const PADDING_BYTES: &[u8] = &[0b1110_1100, 0b0001_0001];

            self.bit_offset = 0;
            let data_bytes_length = data_length / 8;
            // `data_length` is not a multiple of 8 for Micro QR versions M1 and
            // M3, whose final data codeword is only 4 bits wide. When the
            // terminator lands inside that half codeword, `self.data` already
            // holds `data_bytes_length + 1` bytes and no padding is due.
            let padding_bytes_count = data_bytes_length.saturating_sub(self.data.len());
            let padding = PADDING_BYTES.iter().copied().cycle().take(padding_bytes_count);
            self.data.extend(padding);
        }

        if self.len() < data_length {
            self.data.push(0);
        }

        Ok(())
    }
}

#[cfg(test)]
mod finish_tests {
    use crate::bits::{Bits, all_versions};
    use crate::types::{EcLevel, QrError, Version};
    use alloc::vec;

    #[test]
    fn test_hello_world() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_alphanumeric_data(b"HELLO WORLD"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::Q), Ok(()));
        assert_eq!(
            bits.into_bytes(),
            vec![
                0b00100000, 0b01011011, 0b00001011, 0b01111000, 0b11010001, 0b01110010, 0b11011100, 0b01001101,
                0b01000011, 0b01000000, 0b11101100, 0b00010001, 0b11101100,
            ]
        );
    }

    #[test]
    fn test_too_long() {
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"9999999"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Err(QrError::DataTooLong));
    }

    #[test]
    fn test_no_terminator() {
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"99999"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b101_11111, 0b00111_110, 0b0011__0000]);
    }

    #[test]
    fn test_no_padding() {
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"9999"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b100_11111, 0b00111_100, 0b1_000__0000]);
    }

    #[test]
    fn test_micro_version_1_half_byte_padding() {
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"999"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b011_11111, 0b00111_000, 0b0000__0000]);
    }

    #[test]
    fn test_micro_version_1_full_byte_padding() {
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_numeric_data(b""), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b000_000_00, 0b11101100, 0]);
    }

    /// M1 and M3 hold a non-multiple of 8 data bits, so the terminator may land
    /// inside the trailing half codeword. Computing the padding used to
    /// underflow there and abort the process.
    #[test]
    fn test_terminator_inside_trailing_half_codeword() {
        // M3-L holds 84 bits; 20 digits occupy 74 and the 7-bit terminator
        // brings that to 81, i.e. into the 11th (half) codeword.
        let mut bits = Bits::new(Version::micro(3).unwrap());
        assert_eq!(bits.push_numeric_data(b"01234567890123456789"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Ok(()));
        assert_eq!(bits.into_bytes().len(), 11);

        // M1 holds 20 bits; two 1-digit segments occupy 14 and the terminator
        // brings that to 17, i.e. into the 3rd (half) codeword.
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"1"), Ok(()));
        assert_eq!(bits.push_numeric_data(b"2"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Ok(()));
        assert_eq!(bits.into_bytes().len(), 3);
    }

    /// Every version and error correction level must terminate into exactly the
    /// number of codewords the error correction stage expects.
    #[test]
    fn test_terminator_fills_every_symbol() {
        use crate::ec::construct_codewords;

        for version in all_versions() {
            for ec_level in [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H] {
                let mut bits = Bits::new(version);
                let Ok(()) = bits.push_terminator(ec_level) else {
                    continue; // invalid version / ec level combination
                };
                let data = bits.into_bytes();
                assert!(
                    construct_codewords(&data, version, ec_level).is_ok(),
                    "{version:?} {ec_level:?} terminated into {} bytes",
                    data.len()
                );
            }
        }
    }
}

/// Every version defined by the standard, used by the exhaustive tests.
#[cfg(test)]
pub(crate) fn all_versions() -> impl Iterator<Item = Version> {
    let normal = (1..=40).filter_map(|n| Version::normal(n).ok());
    normal.chain((1..=4).filter_map(|n| Version::micro(n).ok()))
}

//}}}
//------------------------------------------------------------------------------
//{{{ Front end.

impl Bits {
    /// Push a segmented data to the bits, and then terminate it.
    ///
    /// # Errors
    ///
    /// Returns `Err(QrError::DataTooLong)` on overflow.
    ///
    /// Returns `Err(QrError::InvalidData)` if the segment refers to incorrectly
    /// encoded byte sequence.
    pub fn push_segments<I>(&mut self, data: &[u8], segments_iter: I) -> QrResult<()>
    where
        I: Iterator<Item = Segment>,
    {
        for segment in segments_iter {
            // The segments may come from the caller rather than from `Parser`,
            // so they are not guaranteed to index `data`.
            let slice = data.get(segment.begin..segment.end).ok_or(QrError::InvalidCharacter)?;
            match segment.mode {
                Mode::Numeric => self.push_numeric_data(slice),
                Mode::Alphanumeric => self.push_alphanumeric_data(slice),
                Mode::Byte => self.push_byte_data(slice),
                Mode::Kanji => self.push_kanji_data(slice),
            }?;
        }
        Ok(())
    }

    /// Pushes the data the bits, using the optimal encoding.
    ///
    /// # Errors
    ///
    /// Returns `Err(QrError::DataTooLong)` on overflow.
    pub fn push_optimal_data(&mut self, data: &[u8]) -> QrResult<()> {
        let segments = Parser::new(data).optimize(self.version);
        self.push_segments(data, segments)
    }
}

#[cfg(test)]
mod encode_tests {
    use crate::bits::Bits;
    use crate::types::{EcLevel, QrError, QrResult, Version};
    use alloc::vec;
    use alloc::vec::Vec;

    fn encode(data: &[u8], version: Version, ec_level: EcLevel) -> QrResult<Vec<u8>> {
        let mut bits = Bits::new(version);
        bits.push_optimal_data(data)?;
        bits.push_terminator(ec_level)?;
        Ok(bits.into_bytes())
    }

    #[test]
    fn test_alphanumeric() {
        let res = encode(b"HELLO WORLD", Version::normal(1).unwrap(), EcLevel::Q);
        assert_eq!(
            res,
            Ok(vec![
                0b00100000, 0b01011011, 0b00001011, 0b01111000, 0b11010001, 0b01110010, 0b11011100, 0b01001101,
                0b01000011, 0b01000000, 0b11101100, 0b00010001, 0b11101100,
            ])
        );
    }

    #[test]
    fn test_auto_mode_switch() {
        let res = encode(b"123A", Version::micro(2).unwrap(), EcLevel::L);
        assert_eq!(res, Ok(vec![0b0_0011_000, 0b1111011_1, 0b001_00101, 0b0_00000__00, 0b11101100]));
    }

    #[test]
    fn test_too_long() {
        let res = encode(b">>>>>>>>", Version::normal(1).unwrap(), EcLevel::H);
        assert_eq!(res, Err(QrError::DataTooLong));
    }

    /// The parser assigns a mode to every byte sequence, so the per-mode
    /// encoders must accept everything it hands them. Any `InvalidCharacter`
    /// here means the two disagree about which bytes a mode can represent.
    #[test]
    fn test_optimal_data_accepts_every_byte_sequence() {
        use crate::bits::Bits;

        for hi in 0..=u8::MAX {
            let mut bits = Bits::new(Version::normal(40).unwrap());
            assert_eq!(bits.push_optimal_data(&[hi]), Ok(()), "{hi:#04x}");

            for lo in 0..=u8::MAX {
                let mut bits = Bits::new(Version::normal(40).unwrap());
                assert_eq!(bits.push_optimal_data(&[hi, lo]), Ok(()), "{hi:#04x} {lo:#04x}");
            }
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Auto version minimization

/// Automatically determines the minimum version to store the data, and encode
/// the result.
///
/// This method will not consider any Micro QR code versions.
///
/// # Errors
///
/// Returns `Err(QrError::DataTooLong)` if the data is too long to fit even the
/// highest QR code version.
pub fn encode_auto(data: &[u8], ec_level: EcLevel) -> QrResult<Bits> {
    let segments = Parser::new(data).collect::<Vec<Segment>>();
    // The boundaries of the three character-count-bit groups of ISO/IEC
    // 18004:2006 Table 3, largest symbol of each group.
    for version in [Version::normal(9)?, Version::normal(26)?, Version::normal(40)?] {
        let opt_segments = Optimizer::new(segments.iter().copied(), version).collect::<Vec<_>>();
        let total_len = total_encoded_len(&opt_segments, version);
        let data_capacity = version.fetch(ec_level, &DATA_LENGTHS)?;
        if total_len <= data_capacity {
            let min_version = find_min_version(total_len, ec_level)?;
            let mut bits = Bits::new(min_version);
            bits.reserve(total_len);
            bits.push_segments(data, opt_segments.into_iter())?;
            bits.push_terminator(ec_level)?;
            return Ok(bits);
        }
    }
    Err(QrError::DataTooLong)
}

/// Finds the smallest version (QR code only) that can store N bits of data
/// in the given error correction level.
///
/// # Errors
///
/// Returns `Err(QrError::InvalidVersion)` if the search leaves the 1..=40
/// range, which the binary search below makes unreachable.
fn find_min_version(length: usize, ec_level: EcLevel) -> QrResult<Version> {
    /// Data capacity of the version at `index`, or 0 past the end of the table
    /// so the search walks left rather than aborting.
    fn capacity(index: usize, ec_level: EcLevel) -> usize {
        DATA_LENGTHS.get(index).and_then(|row| row.get(ec_level as usize)).copied().unwrap_or(0)
    }

    let mut base = 0_usize;
    let mut size = 39;
    while size > 1 {
        let half = size / 2;
        let mid = base + half;
        // mid is always in [0, size).
        // mid >= 0: by definition
        // mid < size: mid = size / 2 + size / 4 + size / 8 ...
        base = if capacity(mid, ec_level) > length { base } else { mid };
        size -= half;
    }
    // base is always in [0, mid) because base <= mid.
    base = if capacity(base, ec_level) >= length { base } else { base + 1 };
    // `base` is confined to 0..40 by the binary search above, so the version
    // number is always in 1..=40 and this constructor always succeeds.
    Version::normal((base + 1).as_i16())
}

#[cfg(test)]
mod encode_auto_tests {
    use crate::bits::{encode_auto, find_min_version};
    use crate::types::{EcLevel, Version};

    #[test]
    fn test_find_min_version() {
        assert_eq!(find_min_version(60, EcLevel::L), Version::normal(1));
        assert_eq!(find_min_version(200, EcLevel::L), Version::normal(2));
        assert_eq!(find_min_version(200, EcLevel::H), Version::normal(3));
        assert_eq!(find_min_version(20000, EcLevel::L), Version::normal(37));
        assert_eq!(find_min_version(640, EcLevel::L), Version::normal(4));
        assert_eq!(find_min_version(641, EcLevel::L), Version::normal(5));
        assert_eq!(find_min_version(999999, EcLevel::H), Version::normal(40));
    }

    #[test]
    fn test_alpha_q() {
        let bits = encode_auto(b"HELLO WORLD", EcLevel::Q).unwrap();
        assert_eq!(bits.version(), Version::normal(1).unwrap());
    }

    #[test]
    fn test_alpha_h() {
        let bits = encode_auto(b"HELLO WORLD", EcLevel::H).unwrap();
        assert_eq!(bits.version(), Version::normal(2).unwrap());
    }

    #[test]
    fn test_mixed() {
        let bits = encode_auto(b"This is a mixed data test. 1234567890", EcLevel::H).unwrap();
        assert_eq!(bits.version(), Version::normal(4).unwrap());
    }
}

#[cfg(feature = "bench")]
#[bench]
fn bench_find_min_version(bencher: &mut test::Bencher) {
    use test::black_box;

    bencher.iter(|| {
        black_box(find_min_version(60, EcLevel::L));
        black_box(find_min_version(200, EcLevel::L));
        black_box(find_min_version(200, EcLevel::H));
        black_box(find_min_version(20000, EcLevel::L));
        black_box(find_min_version(640, EcLevel::L));
        black_box(find_min_version(641, EcLevel::L));
        black_box(find_min_version(999999, EcLevel::H));
    })
}

//}}}
//------------------------------------------------------------------------------
