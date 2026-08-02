//! `bits` modülü ikili veriyi bir QR kodunda kullanılan ham bitlere kodlar.

// Bu modülün testlerindeki ikili literaller eşit uzunlukta dörtlükler yerine
// QR kodu bit alanı sınırlarına (mod göstergesi, karakter sayısı, veri) göre
// gruplanmıştır. Onları ISO/IEC 18004'e karşı gözle denetlenebilir kılan da
// budur; bu yüzden iki literal biçimlendirme linti burada bilinçli olarak kapalı.
#![allow(clippy::unusual_byte_groupings, clippy::unreadable_literal)]

use alloc::vec::Vec;
use core::cmp::min;

use crate::cast::{As, Truncate};
use crate::optimize::{Optimizer, Parser, Segment, total_encoded_len};
use crate::types::{EcLevel, Mode, QrError, QrResult, Version, VersionKind};

//------------------------------------------------------------------------------
//{{{ Bitler

/// `Bits` yapısı bir QR kodu için kodlanmış veriyi saklar.
#[derive(Clone)]
pub struct Bits {
    data: Vec<u8>,
    bit_offset: usize,
    version: Version,
    // ISO/IEC 18004:2024 §7.4.9'un sıra/teklik kurallarını uygulamak ve Ek F
    // sembol tanımlayıcısının `m` değerini türetmek için tutulan durum.
    eci_used: bool,
    fnc1_first_used: bool,
    fnc1_second_used: bool,
    data_started: bool,
}

impl Bits {
    /// Yeni, boş bir bit yapısı kurar.
    pub const fn new(version: Version) -> Self {
        Self {
            data: Vec::new(),
            bit_offset: 0,
            version,
            eci_used: false,
            fnc1_first_used: false,
            fnc1_second_used: false,
            data_started: false,
        }
    }

    /// Bitlerin sonuna N bitlik büyük-endian bir tam sayı ekler.
    ///
    /// Not: `number`'ın gerçekten yalnızca `n` bit boyutunda olmasını sağlamak
    /// geliştiricinin sorumluluğundadır. Aksi hâlde fazla bitler mevcut olanların
    /// üzerine basabilir.
    #[expect(
        clippy::expect_used,
        reason = "sıfırdan farklı bit_offset, özel alanlarla son baytın varlığını garanti eder"
    )]
    fn push_number(&mut self, n: usize, number: u16) {
        assert!(n == 16 || n < 16 && number < (1 << n), "Bits iç değişmezi: {number}, {n} bitlik alana sığmıyor");

        let b = self.bit_offset + n;
        match (self.bit_offset, b) {
            (0, 0..=8) => {
                self.data.push((number << (8 - b)).truncate_as_u8());
            }
            (0, _) => {
                self.data.push((number >> (b - 8)).truncate_as_u8());
                self.data.push((number << (16 - b)).truncate_as_u8());
            }
            // Sıfırdan farklı bir `bit_offset`, son baytın kısmen dolu olduğu anlamına
            // gelir; yani aşağıdaki kollarda `last_mut()` her zaman `Some`'dur.
            (_, 0..=8) => {
                let last = self.data.last_mut().expect("Bits iç değişmezi: kısmi bit ofsetinde son bayt bulunmalı");
                *last |= (number << (8 - b)).truncate_as_u8();
            }
            (_, 9..=16) => {
                let last = self.data.last_mut().expect("Bits iç değişmezi: kısmi bit ofsetinde son bayt bulunmalı");
                *last |= (number >> (b - 8)).truncate_as_u8();
                self.data.push((number << (16 - b)).truncate_as_u8());
            }
            _ => {
                let last = self.data.last_mut().expect("Bits iç değişmezi: kısmi bit ofsetinde son bayt bulunmalı");
                *last |= (number >> (b - 8)).truncate_as_u8();
                self.data.push((number >> (b - 16)).truncate_as_u8());
                self.data.push((number << (24 - b)).truncate_as_u8());
            }
        }
        self.bit_offset = b & 7;
    }

    /// Ekleme için `n` bitlik ek alan ayırır.
    fn reserve(&mut self, n: usize) {
        let extra_bytes = (n + (8 - self.bit_offset) % 8).div_ceil(8);
        self.data.reserve(extra_bytes);
    }

    /// Bitleri bir bayt vektörüne dönüştürür.
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    /// Şu ana dek eklenmiş toplam bit sayısı.
    pub fn len(&self) -> usize {
        if self.bit_offset == 0 { self.data.len() * 8 } else { (self.data.len() - 1) * 8 + self.bit_offset }
    }

    /// Hiç bit eklenip eklenmediği.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Verilen QR kodu sürümü ve hata düzeltme seviyesinin izin verdiği en fazla
    /// bit sayısı.
    ///
    /// # Errors
    ///
    /// Verilen sürüm için `ec_level` kullanmak geçerli değilse (örneğin
    /// `EcLevel::H` ile `Version::micro(1)`) `Err(QrError::InvalidVersion)`
    /// döndürür.
    pub fn max_len(&self, ec_level: EcLevel) -> QrResult<usize> {
        self.version.fetch(ec_level, &DATA_LENGTHS)
    }

    /// QR kodunun sürümü.
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

//}}}
//------------------------------------------------------------------------------
//{{{ Mod göstergesi

/// Veri taşıyanların ötesinde QR kodunun desteklediği tüm göstergeleri içeren
/// "genişletilmiş" mod göstergesi.
#[derive(Copy, Clone)]
pub enum ExtendedMode {
    /// Bir ECI tanımlayıcısı tanıtmak için ECI mod göstergesi.
    Eci,

    /// Veri tanıtmak için normal mod.
    Data(Mode),

    /// İlk konumda FNC-1 modu.
    Fnc1First,

    /// İkinci konumda FNC-1 modu.
    Fnc1Second,

    /// Yapılandırılmış ekleme.
    StructuredAppend,
}

impl Bits {
    /// Mod göstergesini, bitlere dokunmadan doğrular ve sayısal değerini verir.
    fn mode_indicator_number(&self, mode: ExtendedMode) -> QrResult<Option<u16>> {
        let number = match (self.version.kind(), mode) {
            (VersionKind::Micro(1), ExtendedMode::Data(Mode::Numeric)) => return Ok(None),
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
        if bits < 16 && usize::from(number) >= (1_usize << bits) {
            return Err(QrError::UnsupportedCharacterSet);
        }
        Ok(Some(number))
    }

    /// Bitlerin sonuna mod göstergesini ekler.
    ///
    /// # Errors
    ///
    /// Mod verilen sürümde desteklenmiyorsa bu metot
    /// `Err(QrError::UnsupportedCharacterSet)` döndürür.
    pub fn push_mode_indicator(&mut self, mode: ExtendedMode) -> QrResult<()> {
        self.validate_segment_order(mode)?;
        if let Some(number) = self.mode_indicator_number(mode)? {
            // Kapalı `ExtendedMode` enumunun yukarıdaki eşlemesi, sayının sürümün
            // mod alanına sığdığını garanti eder.
            self.push_number(self.version.mode_bits_count(), number);
        }
        match mode {
            ExtendedMode::Eci => self.eci_used = true,
            ExtendedMode::Data(_) => self.data_started = true,
            ExtendedMode::Fnc1First => self.fnc1_first_used = true,
            ExtendedMode::Fnc1Second => self.fnc1_second_used = true,
            ExtendedMode::StructuredAppend => {}
        }
        Ok(())
    }

    /// ISO/IEC 18004:2024 §7.4.9 ve §8'in gösterge sıralama kurallarını
    /// uygular: FNC1 göstergesi sembolde bir kez, ilk veri modundan önce ve
    /// varsa ECI/Structured Append başlığından sonra gelmelidir; Structured
    /// Append başlığı bit akışının en başında durmalıdır.
    fn validate_segment_order(&self, mode: ExtendedMode) -> QrResult<()> {
        let fnc1_used = self.fnc1_first_used || self.fnc1_second_used;
        let valid = match mode {
            ExtendedMode::StructuredAppend => self.is_empty(),
            ExtendedMode::Fnc1First | ExtendedMode::Fnc1Second => !fnc1_used && !self.data_started,
            // Bir FNC1 göstergesi eklendiyse ilk veri modu onu hemen izlemeli;
            // araya ECI giremez. Veri başladıktan sonra yeni ECI parçaları
            // serbesttir (§7.4.3.3, çoklu ECI).
            ExtendedMode::Eci => self.data_started || !fnc1_used,
            ExtendedMode::Data(_) => true,
        };
        if valid { Ok(()) } else { Err(QrError::InvalidSegment) }
    }

    /// Bitlere bir ECI tanımlayıcısı eklenip eklenmediği.
    pub const fn eci_used(&self) -> bool {
        self.eci_used
    }

    /// Birinci konumda FNC1 göstergesi eklenip eklenmediği.
    pub const fn fnc1_first_used(&self) -> bool {
        self.fnc1_first_used
    }

    /// İkinci konumda FNC1 göstergesi eklenip eklenmediği.
    pub const fn fnc1_second_used(&self) -> bool {
        self.fnc1_second_used
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ ECI

impl Bits {
    /// Bitlere bir ECI (Extended Channel Interpretation) tanımlayıcısı ekler.
    ///
    /// Bir ECI tanımlayıcısı, ardından gelen ikili verinin karakter kümesini
    /// belirten 6 haneli bir sayıdır. Bu metodu çağırdıktan sonra gerçek veriyi
    /// eklemek için `.push_byte_data()` ya da benzeri metotlar çağrılabilir, ör.
    ///
    /// ```
    /// use qrcode::bits::Bits;
    /// use qrcode::types::Version;
    ///
    /// let mut bits = Bits::new(Version::normal(1).unwrap());
    /// bits.push_eci_designator(9).unwrap(); // 9 = ISO-8859-7 (Yunanca).
    /// bits.push_byte_data(b"\xa1\xa2\xa3\xa4\xa5").unwrap(); // ΑΒΓΔΕ
    /// ```
    ///
    /// ECI tanımlayıcı değerlerinin tam listesi
    /// <http://strokescribe.com/en/ECI.html> adresinde bulunabilir. Bazı örnek
    /// değerler:
    ///
    /// | ECI # | Karakter kümesi                           |
    /// | ----- | ----------------------------------------- |
    /// | 3     | ISO-8859-1 (Batı Avrupa)                  |
    /// | 20    | Shift JIS (Japonca)                       |
    /// | 23    | Windows 1252 (Latin 1) (Batı Avrupa)      |
    /// | 25    | UTF-16 Big Endian                         |
    /// | 26    | UTF-8                                     |
    /// | 28    | Big 5 (Geleneksel Çince)                  |
    /// | 29    | GB-18030 (Basitleştirilmiş Çince)         |
    /// | 30    | EUC-KR (Korece)                           |
    ///
    /// # Errors
    ///
    /// QR kodu sürümü ECI'yi desteklemiyorsa bu metot
    /// `Err(QrError::UnsupportedCharacterSet)` döndürür.
    ///
    /// Tanımlayıcı beklenen aralığın dışındaysa bu metot
    /// `Err(QrError::InvalidEciDesignator)` döndürür.
    pub fn push_eci_designator(&mut self, eci_designator: u32) -> QrResult<()> {
        if eci_designator > 999_999 {
            return Err(QrError::InvalidEciDesignator);
        }
        self.mode_indicator_number(ExtendedMode::Eci)?;
        self.reserve(12); // eci_designator <= 127 olan yaygın durumu varsayıyoruz.
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
//{{{ Mode::Numeric modu

impl Bits {
    fn push_header(&mut self, mode: Mode, raw_data_len: usize) -> QrResult<()> {
        self.mode_indicator_number(ExtendedMode::Data(mode))?;
        let length_bits = mode.length_bits_count(self.version);
        let data_bits = mode.data_bits_count(raw_data_len)?;
        if length_bits > 16 || raw_data_len >= (1_usize << length_bits) {
            return Err(QrError::DataTooLong);
        }
        let reserve = self
            .version
            .mode_bits_count()
            .checked_add(length_bits)
            .and_then(|bits| bits.checked_add(data_bits))
            .ok_or(QrError::DataTooLong)?;
        self.reserve(reserve);
        self.push_mode_indicator(ExtendedMode::Data(mode))?;
        self.push_number(length_bits, raw_data_len.as_u16());
        Ok(())
    }

    /// Bitlere sayısal bir dizgi kodlar.
    ///
    /// Veri yalnızca 0-9 karakterlerini içermelidir.
    ///
    /// # Errors
    ///
    /// Taşma durumunda `Err(QrError::DataTooLong)` döndürür.
    ///
    /// Veri ASCII rakamı olmayan bir bayt içeriyorsa
    /// `Err(QrError::InvalidCharacter)` döndürür.
    pub fn push_numeric_data(&mut self, data: &[u8]) -> QrResult<()> {
        if !data.iter().all(|byte| numeric_digit(*byte).is_some()) {
            return Err(QrError::InvalidCharacter);
        }
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

/// QR kodu `Mode::Numeric` modunda üçlü rakam grupları 10 tabanlı bir tam sayı
/// olarak kodlanır. `numeric_digit` tek bir karakteri karşılık gelen 10 tabanlı
/// rakama, ASCII rakamı değilse `None`'a dönüştürür.
///
/// Dönüşüm ISO/IEC 18004:2024, §7.4.4'te (2015: §7.4.3) tanımlanmıştır.
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
    fn test_iso_18004_2024_numeric_example_1() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"01234567"), Ok(()));
        assert_eq!(
            bits.into_bytes(),
            vec![0b0001_0000, 0b001000_00, 0b00001100, 0b01010110, 0b01_100001, 0b1__0000000]
        );
    }

    /// ISO/IEC 18004:2024 Ek I.3.2: "01234567" verisinin M2-L bit akışı —
    /// 1 bitlik mod göstergesi, 4 bitlik karakter sayısı, 5 bitlik sonlandırıcı
    /// ve son kod kelimesini dolduran 3 bitlik 0 dolgusu dahil.
    #[test]
    fn test_iso_18004_annex_i_micro_bit_stream() {
        let mut bits = Bits::new(Version::micro(2).unwrap());
        assert_eq!(bits.push_numeric_data(b"01234567"), Ok(()));
        assert_eq!(bits.push_terminator(crate::types::EcLevel::L), Ok(()));
        assert_eq!(
            bits.into_bytes(),
            vec![0b0_1000_000, 0b00011_000, 0b10101100, 0b1_1000011, 0b00000_000]
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
    fn test_iso_18004_2024_numeric_example_2() {
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
        // Bunlar eskiden `*b - b'0'` işlemini taşırıp süreci sonlandırıyordu.
        for data in [&b"12/45"[..], b"!!", b"\xff\xff", b" 1"] {
            let mut bits = Bits::new(Version::normal(1).unwrap());
            assert_eq!(bits.push_numeric_data(data), Err(QrError::InvalidCharacter), "{data:?}");
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Mode::Alphanumeric modu

/// QR kodu `Mode::Alphanumeric` modunda alfanümerik karakter çiftleri 45
/// tabanlı bir tam sayı olarak kodlanır. `alphanumeric_digit` her karakteri
/// karşılık gelen 45 tabanlı rakama dönüştürür.
///
/// Dönüşüm ISO/IEC 18004:2024, §7.4.5, Tablo 5'te (2015: §7.4.4) tanımlanmıştır.
///
/// Karakter alfanümerik karakter kümesinin dışındaysa `None` döndürür.
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
    /// Bitlere alfanümerik bir dizgi kodlar.
    ///
    /// Veri yalnızca A-Z harflerini (küçük harfler hariç), 0-9 rakamlarını,
    /// boşluğu, `$`, `%`, `*`, `+`, `-`, `.`, `/` ya da `:` karakterlerini
    /// içermelidir.
    ///
    /// # Errors
    ///
    /// Taşma durumunda `Err(QrError::DataTooLong)` döndürür.
    ///
    /// Veri alfanümerik karakter kümesi dışında bir bayt içeriyorsa
    /// `Err(QrError::InvalidCharacter)` döndürür.
    pub fn push_alphanumeric_data(&mut self, data: &[u8]) -> QrResult<()> {
        if !data.iter().all(|byte| alphanumeric_digit(*byte).is_some()) {
            return Err(QrError::InvalidCharacter);
        }
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
    fn test_iso_18004_2024_alphanumeric_example() {
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
        // Küçük harfler ve diğer semboller eskiden sessizce 0 rakamı olarak
        // kodlanıyor, yükü hiçbir hata vermeden bozuyordu.
        for data in [&b"abc"[..], b"A;B", b"\x00", b"A\x1dB"] {
            let mut bits = Bits::new(Version::normal(1).unwrap());
            assert_eq!(bits.push_alphanumeric_data(data), Err(QrError::InvalidCharacter), "{data:?}");
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Mode::Byte modu

impl Bits {
    /// Bitlere 8 bitlik bayt verisi kodlar.
    ///
    /// # Errors
    ///
    /// Taşma durumunda `Err(QrError::DataTooLong)` döndürür.
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
//{{{ Mode::Kanji modu

impl Bits {
    /// Bitlere Shift JIS çift baytlı veri kodlar.
    ///
    /// # Errors
    ///
    /// Taşma durumunda `Err(QrError::DataTooLong)` döndürür.
    ///
    /// Veri Shift JIS çift baytlı veri değilse (örneğin verinin uzunluğu çift
    /// sayı değilse ya da bir karakter Kanji modunda kodlanabilir aralıkların
    /// dışındaysa) `Err(QrError::InvalidCharacter)` döndürür.
    ///
    /// # Panics
    ///
    /// Yalnızca ön doğrulama ile aynı saf Kanji dönüşümü daha sonraki kodlama
    /// adımında çelişirse, yani kütüphanenin iç değişmezi bozulursa panikler.
    #[expect(
        clippy::expect_used,
        reason = "tüm Kanji çiftleri bitlere dokunulmadan önce aynı saf dönüşümle doğrulanır"
    )]
    pub fn push_kanji_data(&mut self, data: &[u8]) -> QrResult<()> {
        self.mode_indicator_number(ExtendedMode::Data(Mode::Kanji))?;
        if data.chunks(2).any(|kanji| kanji_number(kanji).is_none()) {
            return Err(QrError::InvalidCharacter);
        }
        self.push_header(Mode::Kanji, data.len() / 2)?;
        for kanji in data.chunks(2) {
            let number = kanji_number(kanji).expect("Kanji iç değişmezi: önceden doğrulanan çift dönüştürülebilmeli");
            self.push_number(13, number);
        }
        Ok(())
    }
}

/// Bir Shift JIS bayt çiftini QR Kanji modunun 13 bitlik değerine dönüştürür.
fn kanji_number(kanji: &[u8]) -> Option<u16> {
    let &[hi, lo] = kanji else { return None };
    let cp = u16::from(hi) * 256 + u16::from(lo);
    // ISO/IEC 18004:2024 §7.4.7 (2015: §7.4.6) — yalnızca bu iki aralık bir Kanji modu
    // karakterinin 13 bitine sıkıştırılabilir.
    let bytes = match cp {
        0x8140..=0x9ffc => cp - 0x8140,
        0xe040..=0xebbf => cp - 0xc140,
        _ => return None,
    };
    let number = (bytes >> 8) * 0xc0 + (bytes & 0xff);
    if number < (1 << 13) { Some(number) } else { None }
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
        // `b"AA"` eskiden `cp - 0x8140` işlemini taşırıyordu ve `b"\xeb\xc0"` 13
        // bitlik alanı aşıp komşu bitlerin üzerine basıyordu.
        for data in [&b"AA"[..], b"\x00\x00", b"\xeb\xc0", b"\x9f\xfd", b"\xe0\x3f", b"\xff\xff"] {
            let mut bits = Bits::new(Version::normal(1).unwrap());
            assert_eq!(bits.push_kanji_data(data), Err(QrError::InvalidCharacter), "{data:?}");
        }
    }

    #[test]
    fn test_boundary_characters() {
        // Her iki aralığın uçları hâlâ başarıyla gidip gelebilmeli.
        for data in [&b"\x81\x40"[..], b"\x9f\xfc", b"\xe0\x40", b"\xeb\xbf"] {
            let mut bits = Bits::new(Version::normal(1).unwrap());
            assert_eq!(bits.push_kanji_data(data), Ok(()), "{data:?}");
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ FNC1 modu

impl Bits {
    /// Ardından gelen verinin UCC/EAN Application Identifiers standardına göre
    /// biçimlendirildiğini belirten bir gösterge kodlar.
    ///
    /// ```
    /// use qrcode::bits::Bits;
    /// use qrcode::types::Version;
    ///
    /// let mut bits = Bits::new(Version::normal(1).unwrap());
    /// bits.push_fnc1_first_position().unwrap();
    /// bits.push_numeric_data(b"01049123451234591597033130128").unwrap();
    /// bits.push_alphanumeric_data(b"%10ABC123").unwrap();
    /// ```
    ///
    /// QR kodunda `%` karakteri veri alanı ayırıcısı (0x1D) olarak kullanılır.
    ///
    /// # Errors
    ///
    /// Mod verilen sürümde desteklenmiyorsa bu metot
    /// `Err(QrError::UnsupportedCharacterSet)` döndürür.
    pub fn push_fnc1_first_position(&mut self) -> QrResult<()> {
        // Sıra kuralları (bir kez, ilk veri modundan önce, ECI'den sonra)
        // `push_mode_indicator` içinde doğrulanır.
        self.push_mode_indicator(ExtendedMode::Fnc1First)
    }

    /// Ardından gelen verinin, AIM International ile önceden mutabık kalınmış
    /// belirli bir sektör ya da uygulama şartnamesine göre biçimlendirildiğini
    /// belirten bir gösterge kodlar.
    ///
    /// ```
    /// use qrcode::bits::Bits;
    /// use qrcode::types::Version;
    ///
    /// let mut bits = Bits::new(Version::normal(1).unwrap());
    /// bits.push_fnc1_second_position(37).unwrap();
    /// bits.push_alphanumeric_data(b"AA1234BBB112").unwrap();
    /// bits.push_byte_data(b"text text text text\r").unwrap();
    /// ```
    ///
    /// Uygulama göstergesi tek bir Latin harfiyse (a–z / A–Z) lütfen ASCII
    /// değerine 100 ekleyerek geçin:
    ///
    /// ```ignore
    /// bits.push_fnc1_second_position(b'A' + 100).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Mod verilen sürümde desteklenmiyorsa bu metot
    /// `Err(QrError::UnsupportedCharacterSet)` döndürür.
    pub fn push_fnc1_second_position(&mut self, application_indicator: u8) -> QrResult<()> {
        self.push_mode_indicator(ExtendedMode::Fnc1Second)?;
        self.push_number(8, u16::from(application_indicator));
        Ok(())
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Structured Append

/// Bir Structured Append mesajının parite baytını hesaplar.
///
/// ISO/IEC 18004:2024 §8.3: parite, mesajın sembollere bölünmeden *önceki*
/// özgün giriş verisinin tamamının bayt bayt XOR'udur. Kanji karakterleri iki
/// baytlık Shift JIS değerleriyle temsil edilir; `data` zaten bayt dizisi
/// olduğundan bu doğal olarak sağlanır. Mod göstergeleri, karakter sayısı
/// göstergeleri, dolgu ve sonlandırıcı hesaba katılmaz.
///
/// ```
/// use qrcode::bits::structured_append_parity;
///
/// // Standardın örneği: "0123456789日本" (Kanji kısmı Shift JIS 93FA 967B).
/// let message = b"0123456789\x93\xfa\x96\x7b";
/// assert_eq!(structured_append_parity(message), 0x85);
/// ```
pub fn structured_append_parity(data: &[u8]) -> u8 {
    data.iter().fold(0, |parity, byte| parity ^ byte)
}

impl Bits {
    /// Bit akışının başına bir Structured Append başlığı ekler.
    ///
    /// ISO/IEC 18004:2024 §8'e göre bir mesaj en fazla 16 QR kodu sembolüne
    /// bölünebilir; her sembol, 0011 mod göstergesi + sembol sırası göstergesi
    /// (4 bit konum − 1, 4 bit toplam − 1) + parite baytından oluşan 20 bitlik
    /// bu başlıkla açılır. `position` 1 tabanlıdır. `parity` tüm sembollerde
    /// aynı olmalı ve [`structured_append_parity`] ile mesajın tamamından
    /// hesaplanmalıdır.
    ///
    /// ```
    /// use qrcode::bits::{Bits, structured_append_parity};
    /// use qrcode::types::{EcLevel, Version};
    ///
    /// let message = b"0123456789";
    /// let parity = structured_append_parity(message);
    /// let mut first = Bits::new(Version::normal(1).unwrap());
    /// first.push_structured_append_header(1, 2, parity).unwrap();
    /// first.push_numeric_data(&message[..5]).unwrap();
    /// first.push_terminator(EcLevel::L).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Başlık akışın en başında değilse ya da `position`/`total` aralık
    /// dışındaysa `Err(QrError::InvalidSegment)`; sürüm bir Micro QR koduysa
    /// (§8.1 Structured Append'i Micro'da yasaklar)
    /// `Err(QrError::UnsupportedCharacterSet)` döndürür.
    pub fn push_structured_append_header(&mut self, position: u8, total: u8, parity: u8) -> QrResult<()> {
        if !(2..=16).contains(&total) || !(1..=total).contains(&position) {
            return Err(QrError::InvalidSegment);
        }
        self.push_mode_indicator(ExtendedMode::StructuredAppend)?;
        self.push_number(4, u16::from(position - 1));
        self.push_number(4, u16::from(total - 1));
        self.push_number(8, u16::from(parity));
        Ok(())
    }
}

#[cfg(test)]
mod structured_append_tests {
    use crate::bits::{Bits, structured_append_parity};
    use crate::types::{QrError, Version};
    use alloc::vec;

    /// ISO/IEC 18004:2024 §8.3'ün örneği: "0123456789日本" verisinin paritesi.
    #[test]
    fn test_parity_example() {
        assert_eq!(structured_append_parity(b"0123456789\x93\xfa\x96\x7b"), 0x85);
        assert_eq!(structured_append_parity(b""), 0);
    }

    /// §8.2'nin örneği: yedi sembollük dizinin üçüncüsü 0010 0110 kodlanır.
    #[test]
    fn test_header_layout() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_structured_append_header(3, 7, 0x85), Ok(()));
        assert_eq!(bits.into_bytes(), vec![0b0011_0010, 0b0110_1000, 0b0101_0000]);
    }

    #[test]
    fn test_invalid_usage() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_structured_append_header(1, 1, 0), Err(QrError::InvalidSegment));
        assert_eq!(bits.push_structured_append_header(0, 2, 0), Err(QrError::InvalidSegment));
        assert_eq!(bits.push_structured_append_header(3, 2, 0), Err(QrError::InvalidSegment));
        assert_eq!(bits.push_structured_append_header(1, 17, 0), Err(QrError::InvalidSegment));

        // Başlık akışın en başında olmalıdır (§8.1).
        assert_eq!(bits.push_numeric_data(b"123"), Ok(()));
        assert_eq!(bits.push_structured_append_header(1, 2, 0), Err(QrError::InvalidSegment));

        // Micro QR sembollerinde Structured Append yoktur (§8.1).
        let mut micro = Bits::new(Version::micro(4).unwrap());
        assert_eq!(micro.push_structured_append_header(1, 2, 0), Err(QrError::UnsupportedCharacterSet));
    }

    /// §8.1: varsayılan olmayan ECI'ler ve §7.4.9 gereği FNC1, Structured
    /// Append başlığını izleyebilir.
    #[test]
    fn test_header_then_eci_and_fnc1() {
        let mut bits = Bits::new(Version::normal(2).unwrap());
        assert_eq!(bits.push_structured_append_header(1, 2, 0x42), Ok(()));
        assert_eq!(bits.push_eci_designator(9), Ok(()));
        assert_eq!(bits.push_byte_data(b"\xa1\xa2"), Ok(()));

        let mut bits = Bits::new(Version::normal(2).unwrap());
        assert_eq!(bits.push_structured_append_header(2, 2, 0x42), Ok(()));
        assert_eq!(bits.push_fnc1_first_position(), Ok(()));
        assert_eq!(bits.push_numeric_data(b"123"), Ok(()));
    }
}

#[cfg(test)]
mod segment_order_tests {
    use crate::bits::Bits;
    use crate::types::{QrError, Version};

    /// ISO/IEC 18004:2024 §7.4.9.2 Örnek 1'in sıralaması: FNC1 göstergesi,
    /// ardından sayısal ve alfasayısal veri modları.
    #[test]
    fn test_fnc1_before_data_is_valid() {
        let mut bits = Bits::new(Version::normal(3).unwrap());
        assert_eq!(bits.push_fnc1_first_position(), Ok(()));
        assert_eq!(bits.push_numeric_data(b"01049123451234591597033130128"), Ok(()));
        assert_eq!(bits.push_alphanumeric_data(b"%10ABC123"), Ok(()));
        assert!(bits.fnc1_first_used());
    }

    #[test]
    fn test_fnc1_after_data_is_rejected() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"123"), Ok(()));
        assert_eq!(bits.push_fnc1_first_position(), Err(QrError::InvalidSegment));
        assert_eq!(bits.push_fnc1_second_position(37), Err(QrError::InvalidSegment));
    }

    #[test]
    fn test_second_fnc1_is_rejected() {
        let mut bits = Bits::new(Version::normal(1).unwrap());
        assert_eq!(bits.push_fnc1_first_position(), Ok(()));
        assert_eq!(bits.push_fnc1_first_position(), Err(QrError::InvalidSegment));
        assert_eq!(bits.push_fnc1_second_position(37), Err(QrError::InvalidSegment));
    }

    /// FNC1 "varsa ECI'den sonra" gelmelidir; yani FNC1 ile ilk veri modunun
    /// arasına ECI giremez, ama hem ECI -> FNC1 sırası hem de veri başladıktan
    /// sonra yeni ECI parçaları (§7.4.3.3) serbesttir.
    #[test]
    fn test_eci_ordering_around_fnc1() {
        let mut bits = Bits::new(Version::normal(2).unwrap());
        assert_eq!(bits.push_eci_designator(9), Ok(()));
        assert_eq!(bits.push_fnc1_first_position(), Ok(()));
        assert_eq!(bits.push_eci_designator(9), Err(QrError::InvalidSegment));
        assert_eq!(bits.push_byte_data(b"data"), Ok(()));
        assert_eq!(bits.push_eci_designator(3), Ok(()));
        assert!(bits.eci_used());
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Sonlandırma

// Bu tablo ISO/IEC 18004:2024 §7.4.11, Tablo 7'den (2015: §7.4.10)
// kopyalanmıştır; iki baskının değerleri birebir aynıdır.
static DATA_LENGTHS: [[usize; 4]; 44] = [
    // Normal sürümler
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
    // Micro sürümler
    [20, 0, 0, 0],
    [40, 32, 0, 0],
    [84, 68, 0, 0],
    [128, 112, 80, 0],
];

impl Bits {
    /// Daha fazla veri olmadığını belirten bitiş bitlerini ekler.
    ///
    /// # Errors
    ///
    /// Taşma durumunda `Err(QrError::DataTooLong)` döndürür.
    ///
    /// Verilen sürüm için `ec_level` kullanmak geçerli değilse (örneğin
    /// `EcLevel::H` ile `Version::micro(1)`) `Err(QrError::InvalidVersion)`
    /// döndürür.
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
            // Son veri kod kelimesi yalnızca 4 bit genişliğinde olan Micro QR sürümleri
            // M1 ve M3 için `data_length` 8'in katı değildir. Sonlandırıcı o yarım kod
            // kelimesinin içine düştüğünde `self.data` zaten `data_bytes_length + 1`
            // bayt tutar ve dolgu gerekmez.
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

    /// M1 ve M3, 8'in katı olmayan sayıda veri biti tutar, yani sonlandırıcı
    /// sondaki yarım kod kelimesinin içine düşebilir. Dolgunun hesaplanması
    /// burada eskiden taşıyor ve süreci sonlandırıyordu.
    #[test]
    fn test_terminator_inside_trailing_half_codeword() {
        // M3-L 84 bit tutar; 20 rakam 74 bit kaplar ve 7 bitlik sonlandırıcı bunu
        // 81'e, yani 11. (yarım) kod kelimesine taşır.
        let mut bits = Bits::new(Version::micro(3).unwrap());
        assert_eq!(bits.push_numeric_data(b"01234567890123456789"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Ok(()));
        assert_eq!(bits.into_bytes().len(), 11);

        // M1 20 bit tutar; 1'er rakamlık iki parça 14 bit kaplar ve sonlandırıcı
        // bunu 17'ye, yani 3. (yarım) kod kelimesine taşır.
        let mut bits = Bits::new(Version::micro(1).unwrap());
        assert_eq!(bits.push_numeric_data(b"1"), Ok(()));
        assert_eq!(bits.push_numeric_data(b"2"), Ok(()));
        assert_eq!(bits.push_terminator(EcLevel::L), Ok(()));
        assert_eq!(bits.into_bytes().len(), 3);
    }

    /// Her sürüm ve hata düzeltme seviyesi, hata düzeltme aşamasının beklediği
    /// kod kelimesi sayısına tam olarak ulaşarak sonlanmalıdır.
    #[test]
    fn test_terminator_fills_every_symbol() {
        use crate::ec::construct_codewords;

        for version in all_versions() {
            for ec_level in [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H] {
                let mut bits = Bits::new(version);
                let Ok(()) = bits.push_terminator(ec_level) else {
                    continue; // geçersiz sürüm / ec seviyesi bileşimi
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

/// Kapsamlı testlerin kullandığı, standardın tanımladığı her sürüm.
#[cfg(test)]
pub(crate) fn all_versions() -> impl Iterator<Item = Version> {
    let normal = (1..=40).filter_map(|n| Version::normal(n).ok());
    normal.chain((1..=4).filter_map(|n| Version::micro(n).ok()))
}

//}}}
//------------------------------------------------------------------------------
//{{{ Ön yüz

impl Bits {
    /// Bitlere parçalanmış veri ekler.
    ///
    /// # Errors
    ///
    /// Taşma durumunda `Err(QrError::DataTooLong)` döndürür.
    ///
    /// Parça veri dizisinin dışına taşıyorsa `Err(QrError::InvalidSegment)`,
    /// parça seçilen moda uymayan bir bayt içeriyorsa
    /// `Err(QrError::InvalidCharacter)` döndürür. Herhangi bir parça geçersizse
    /// `Bits` değişmeden kalır.
    pub fn push_segments<I>(&mut self, data: &[u8], segments_iter: I) -> QrResult<()>
    where
        I: Iterator<Item = Segment>,
    {
        let mut staged = self.clone();
        for segment in segments_iter {
            // Parçalar `Parser` yerine çağırandan gelmiş olabilir, bu yüzden `data`'yı
            // indeksleyecekleri garanti değildir.
            let slice = data.get(segment.begin()..segment.end()).ok_or(QrError::InvalidSegment)?;
            match segment.mode() {
                Mode::Numeric => staged.push_numeric_data(slice),
                Mode::Alphanumeric => staged.push_alphanumeric_data(slice),
                Mode::Byte => staged.push_byte_data(slice),
                Mode::Kanji => staged.push_kanji_data(slice),
            }?;
        }
        *self = staged;
        Ok(())
    }

    /// Veriyi en uygun kodlamayı kullanarak bitlere ekler.
    ///
    /// # Errors
    ///
    /// Taşma durumunda `Err(QrError::DataTooLong)` döndürür.
    pub fn push_optimal_data(&mut self, data: &[u8]) -> QrResult<()> {
        let segments = Parser::new(data)?.optimize(self.version);
        self.push_segments(data, segments)
    }
}

#[cfg(test)]
mod capacity_boundary_tests {
    use crate::bits::{Bits, DATA_LENGTHS};
    use crate::types::{EcLevel, Mode, QrError, Version};
    use alloc::vec;
    use alloc::vec::Vec;

    /// Verilen sürüm/seviye/mod için standarda göre sığabilecek en fazla
    /// karakter sayısı: mod göstergesi + karakter sayısı göstergesi + veri
    /// bitleri toplamı kapasiteyi aşmayana dek sayar.
    fn max_chars(version: Version, ec_level: EcLevel, mode: Mode) -> usize {
        let capacity = DATA_LENGTHS[version.table_index()][ec_level as usize];
        let overhead = version.mode_bits_count() + mode.length_bits_count(version);
        let mut chars = 0;
        while overhead + mode.data_bits_count(chars + 1).unwrap() <= capacity {
            chars += 1;
        }
        chars
    }

    /// `chars` karakterlik tek modlu bir veri parçasını uçtan uca kodlamayı
    /// dener.
    fn try_encode(version: Version, ec_level: EcLevel, mode: Mode, chars: usize) -> Result<(), QrError> {
        let data: Vec<u8> = match mode {
            Mode::Numeric => vec![b'7'; chars],
            Mode::Alphanumeric => vec![b'A'; chars],
            Mode::Byte => vec![0xAB; chars],
            // 0x889F, Kanji modunun 0x8140-0x9FFC aralığında geçerli bir
            // Shift JIS karakteridir.
            Mode::Kanji => [0x88, 0x9F].iter().copied().cycle().take(chars * 2).collect(),
        };
        let mut bits = Bits::new(version);
        match mode {
            Mode::Numeric => bits.push_numeric_data(&data)?,
            Mode::Alphanumeric => bits.push_alphanumeric_data(&data)?,
            Mode::Byte => bits.push_byte_data(&data)?,
            Mode::Kanji => bits.push_kanji_data(&data)?,
        }
        bits.push_terminator(ec_level)
    }

    /// Sürümün desteklediği modlar: M1 yalnız sayısal, M2 sayısal +
    /// alfasayısal, M3 ve üzeri dört mod (ISO/IEC 18004:2024 Tablo 2).
    fn supported_modes(version: Version) -> &'static [Mode] {
        match (version.is_micro(), version.number()) {
            (true, 1) => &[Mode::Numeric],
            (true, 2) => &[Mode::Numeric, Mode::Alphanumeric],
            _ => &[Mode::Numeric, Mode::Alphanumeric, Mode::Byte, Mode::Kanji],
        }
    }

    /// Her sürüm/seviye/mod bileşiminde azami karakter sayısı sığmalı, bir
    /// fazlası `DataTooLong` vermelidir (ISO/IEC 18004:2024 Tablo 7 sınırları).
    #[test]
    fn test_all_capacity_boundaries() {
        for version in crate::bits::all_versions() {
            for ec_level in [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H] {
                if DATA_LENGTHS[version.table_index()][ec_level as usize] == 0 {
                    continue; // standardın tanımlamadığı bileşim
                }
                for &mode in supported_modes(version) {
                    let limit = max_chars(version, ec_level, mode);
                    assert_eq!(
                        try_encode(version, ec_level, mode, limit),
                        Ok(()),
                        "{version:?}-{ec_level:?} {mode:?}: {limit} karakter sığmalıydı"
                    );
                    assert_eq!(
                        try_encode(version, ec_level, mode, limit + 1),
                        Err(QrError::DataTooLong),
                        "{version:?}-{ec_level:?} {mode:?}: {} karakter sığmamalıydı",
                        limit + 1
                    );
                }
            }
        }
    }

    /// ISO/IEC 18004:2024 Tablo 7'nin karakter kapasitesi sütunlarından
    /// (sayfa görüntüsünden okunan) noktasal değerler: (sürüm, seviye,
    /// [sayısal, alfasayısal, bayt, Kanji]).
    #[test]
    fn test_table_7_spot_values() {
        let micro = |n| Version::micro(n).unwrap();
        let normal = |n| Version::normal(n).unwrap();
        #[rustfmt::skip]
        let expected: &[(Version, EcLevel, &[(Mode, usize)])] = &[
            (micro(1), EcLevel::L, &[(Mode::Numeric, 5)]),
            (micro(2), EcLevel::L, &[(Mode::Numeric, 10), (Mode::Alphanumeric, 6)]),
            (micro(2), EcLevel::M, &[(Mode::Numeric, 8), (Mode::Alphanumeric, 5)]),
            (micro(3), EcLevel::L, &[(Mode::Numeric, 23), (Mode::Alphanumeric, 14), (Mode::Byte, 9), (Mode::Kanji, 6)]),
            (micro(3), EcLevel::M, &[(Mode::Numeric, 18), (Mode::Alphanumeric, 11), (Mode::Byte, 7), (Mode::Kanji, 4)]),
            (micro(4), EcLevel::L, &[(Mode::Numeric, 35), (Mode::Alphanumeric, 21), (Mode::Byte, 15), (Mode::Kanji, 9)]),
            (micro(4), EcLevel::M, &[(Mode::Numeric, 30), (Mode::Alphanumeric, 18), (Mode::Byte, 13), (Mode::Kanji, 8)]),
            (micro(4), EcLevel::Q, &[(Mode::Numeric, 21), (Mode::Alphanumeric, 13), (Mode::Byte, 9), (Mode::Kanji, 5)]),
            (normal(1), EcLevel::L, &[(Mode::Numeric, 41), (Mode::Alphanumeric, 25), (Mode::Byte, 17), (Mode::Kanji, 10)]),
            (normal(1), EcLevel::M, &[(Mode::Numeric, 34), (Mode::Alphanumeric, 20), (Mode::Byte, 14), (Mode::Kanji, 8)]),
            (normal(1), EcLevel::Q, &[(Mode::Numeric, 27), (Mode::Alphanumeric, 16), (Mode::Byte, 11), (Mode::Kanji, 7)]),
            (normal(1), EcLevel::H, &[(Mode::Numeric, 17), (Mode::Alphanumeric, 10), (Mode::Byte, 7), (Mode::Kanji, 4)]),
            (normal(2), EcLevel::L, &[(Mode::Numeric, 77), (Mode::Alphanumeric, 47), (Mode::Byte, 32), (Mode::Kanji, 20)]),
            (normal(40), EcLevel::L, &[(Mode::Numeric, 7089)]),
        ];
        for &(version, ec_level, entries) in expected {
            for &(mode, chars) in entries {
                assert_eq!(
                    max_chars(version, ec_level, mode),
                    chars,
                    "{version:?}-{ec_level:?} {mode:?} kapasitesi Tablo 7 ile uyuşmuyor"
                );
            }
        }
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

    /// Ayrıştırıcı her bayt dizisine bir mod atar, dolayısıyla mod başına
    /// kodlayıcılar kendilerine verilen her şeyi kabul etmelidir. Buradaki bir
    /// `InvalidCharacter`, ikisinin bir modun hangi baytları temsil edebileceği
    /// konusunda anlaşamadığı anlamına gelir.
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
//{{{ Otomatik sürüm küçültme

/// Veriyi saklamak için gereken en küçük sürümü otomatik olarak belirler ve
/// sonucu kodlar.
///
/// Bu metot hiçbir Micro QR kodu sürümünü dikkate almaz.
///
/// # Errors
///
/// Veri en yüksek QR kodu sürümüne bile sığmayacak kadar uzunsa
/// `Err(QrError::DataTooLong)` döndürür.
pub fn encode_auto(data: &[u8], ec_level: EcLevel) -> QrResult<Bits> {
    let segments = Parser::new(data)?.collect::<Vec<Segment>>();
    // ISO/IEC 18004:2024 Tablo 3'teki üç karakter sayısı biti grubunun sınırları;
    // her grubun en büyük sembolü.
    for version in [Version::normal(9)?, Version::normal(26)?, Version::normal(40)?] {
        let opt_segments = Optimizer::new(segments.iter().copied(), version).collect::<Vec<_>>();
        let total_len = total_encoded_len(&opt_segments, version)?;
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

/// Verilen hata düzeltme seviyesinde N bitlik veriyi saklayabilen en küçük
/// sürümü (yalnızca QR kodu) bulur.
///
/// # Errors
///
/// Arama 1..=40 aralığından çıkarsa `Err(QrError::InvalidVersion)` döndürür;
/// aşağıdaki ikili arama bunu erişilemez kılar.
fn find_min_version(length: usize, ec_level: EcLevel) -> QrResult<Version> {
    #[expect(
        clippy::expect_used,
        reason = "ikili arama indeksleri 40 sürümlük ve dört seviyeli sabit tabloların içinde kalır"
    )]
    fn capacity(index: usize, ec_level: EcLevel) -> usize {
        *DATA_LENGTHS
            .get(index)
            .and_then(|row| row.get(ec_level as usize))
            .expect("sürüm kapasitesi iç değişmezi: indeks tabloda bulunmalı")
    }

    if length > capacity(39, ec_level) {
        return Err(QrError::DataTooLong);
    }

    let mut base = 0_usize;
    let mut size = 39;
    while size > 1 {
        let half = size / 2;
        let mid = base + half;
        // mid her zaman [0, size) aralığındadır.
        // mid >= 0: tanım gereği
        // mid < size: mid = size / 2 + size / 4 + size / 8 ...
        base = if capacity(mid, ec_level) > length { base } else { mid };
        size -= half;
    }
    // base <= mid olduğundan base her zaman [0, mid) aralığındadır.
    base = if capacity(base, ec_level) >= length { base } else { base + 1 };
    // `base`, yukarıdaki ikili arama tarafından 0..40 aralığına hapsedilir; yani
    // sürüm numarası her zaman 1..=40'tadır ve bu kurucu her zaman başarılı olur.
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
        assert_eq!(find_min_version(999999, EcLevel::H), Err(crate::types::QrError::DataTooLong));
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

//}}}
//------------------------------------------------------------------------------
