//! Numeric conversions whose range the caller has already established.
//!
//! Every conversion in this module is reached only with a value that is
//! provably in range: coordinates are normalised against a validated
//! [`Version`](crate::Version) before use, table indices are derived from that
//! same version, and bit counts are bounded by the field widths of ISO/IEC
//! 18004. A failure here is a bug in this crate, not something a caller can
//! provoke, which is why these return the value directly instead of a `Result`.
//!
//! What matters is that they check in **release** builds too. The previous
//! implementation used `#[cfg(debug_assertions)]` to pick between a checked
//! conversion and a bare `as` cast, so a release build silently wrapped and
//! went on to emit a corrupt QR code where a debug build aborted. Trading a
//! silently wrong answer for a loud one is the whole point of this module.

/// Signals a conversion that the type system cannot rule out but the crate's
/// invariants do. Kept in one place so there is a single thing to audit.
#[cold]
#[track_caller]
#[expect(clippy::panic, reason = "unreachable by construction; see the module docs")]
fn out_of_range() -> ! {
    panic!("qrcode internal error: numeric conversion out of range")
}

pub trait Truncate {
    fn truncate_as_u8(self) -> u8;
}

impl Truncate for u16 {
    fn truncate_as_u8(self) -> u8 {
        // Masking first makes the conversion exact rather than truncating.
        (self & 0xff) as u8
    }
}

#[expect(clippy::wrong_self_convention, reason = "mirrors the `as` operator these methods replace")]
pub trait As {
    fn as_u16(self) -> u16;
    fn as_i16(self) -> i16;
    fn as_usize(self) -> usize;
    fn as_isize(self) -> isize;
}

macro_rules! impl_as {
    ($ty:ty) => {
        impl As for $ty {
            fn as_u16(self) -> u16 {
                match u16::try_from(self) {
                    Ok(value) => value,
                    Err(_) => out_of_range(),
                }
            }

            fn as_i16(self) -> i16 {
                match i16::try_from(self) {
                    Ok(value) => value,
                    Err(_) => out_of_range(),
                }
            }

            fn as_usize(self) -> usize {
                match usize::try_from(self) {
                    Ok(value) => value,
                    Err(_) => out_of_range(),
                }
            }

            fn as_isize(self) -> isize {
                match isize::try_from(self) {
                    Ok(value) => value,
                    Err(_) => out_of_range(),
                }
            }
        }
    };
}

impl_as!(i16);
impl_as!(u32);
impl_as!(usize);
impl_as!(isize);

#[cfg(test)]
mod tests {
    use super::As;

    /// Debug and release must agree. Before this module was collapsed into a
    /// single implementation, the release build wrapped instead of failing.
    #[test]
    fn test_in_range_conversions_are_exact() {
        assert_eq!(177_i16.as_usize(), 177);
        assert_eq!(65535_usize.as_u16(), 65535);
        assert_eq!(40_usize.as_i16(), 40);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn test_negative_to_usize_is_caught_in_every_profile() {
        let _ = (-1_i16).as_usize();
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn test_too_large_to_u16_is_caught_in_every_profile() {
        let _ = 65536_usize.as_u16();
    }
}
