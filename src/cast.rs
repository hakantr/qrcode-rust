//! Aralığı çağıran tarafından zaten belirlenmiş sayısal dönüşümler.
//!
//! Bu modüldeki her dönüşüme yalnızca aralıkta olduğu kanıtlanabilir bir
//! değerle ulaşılır: koordinatlar kullanılmadan önce doğrulanmış bir
//! [`Version`](crate::Version) üzerinden normalleştirilir, tablo indeksleri
//! yine aynı sürümden türetilir ve bit sayıları ISO/IEC 18004'ün alan
//! genişlikleriyle sınırlıdır. Buradaki bir hata çağıranın tetikleyebileceği
//! bir şey değil, bu crate'te bir bugtır; bu yüzden `Result` yerine doğrudan
//! değeri döndürürler.
//!
//! Asıl önemli olan, **release** derlemelerinde de kontrol yapmalarıdır.
//! Önceki implementasyon kontrollü dönüşümle çıplak `as` arasında seçim
//! yapmak için `#[cfg(debug_assertions)]` kullanıyordu; yani release derlemesi
//! sessizce sarmalanıp bozuk bir QR kodu üretirken debug derlemesi abort
//! ediyordu. Sessizce yanlış cevabı gürültülü olanla takas etmek bu modülün
//! varlık sebebidir.

/// Tip sisteminin eleyemediği ama crate'in değişmezlerinin elediği bir
/// dönüşümü bildirir. Denetlenecek tek bir yer kalsın diye burada toplanmıştır.
#[cold]
#[track_caller]
#[expect(clippy::panic, reason = "yapı gereği erişilemez; modül belgelerine bakın")]
fn out_of_range() -> ! {
    panic!("qrcode iç hatası: sayısal dönüşüm aralık dışı")
}

pub trait Truncate {
    fn truncate_as_u8(self) -> u8;
}

impl Truncate for u16 {
    fn truncate_as_u8(self) -> u8 {
        // Önce maskelemek dönüşümü kırpma değil birebir hale getirir.
        (self & 0xff) as u8
    }
}

#[expect(clippy::wrong_self_convention, reason = "bu metotların yerini aldığı `as` operatörünü yansıtır")]
pub trait As {
    fn as_u16(self) -> u16;
    fn as_i16(self) -> i16;
    fn as_usize(self) -> usize;
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

    /// Debug ve release aynı davranmalı. Bu modül tek implementasyona
    /// indirilmeden önce release derlemesi hata vermek yerine sarmalıyordu.
    #[test]
    fn test_in_range_conversions_are_exact() {
        assert_eq!(177_i16.as_usize(), 177);
        assert_eq!(65535_usize.as_u16(), 65535);
        assert_eq!(40_usize.as_i16(), 40);
    }

    #[test]
    #[should_panic(expected = "aralık dışı")]
    fn test_negative_to_usize_is_caught_in_every_profile() {
        let _ = (-1_i16).as_usize();
    }

    #[test]
    #[should_panic(expected = "aralık dışı")]
    fn test_too_large_to_u16_is_caught_in_every_profile() {
        let _ = 65536_usize.as_u16();
    }
}
