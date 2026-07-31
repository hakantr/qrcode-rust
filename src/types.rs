//! `types` modülü bir QR kodunun işlevsel öğeleriyle ilişkili tipleri içerir.

use crate::cast::As;
use core::cmp::{Ordering, PartialOrd};
use core::default::Default;
use core::fmt::{Display, Error, Formatter};
use core::ops::Not;

//------------------------------------------------------------------------------
//{{{ QrResult

/// `QrError` bir QR kodu üretilirken karşılaşılan hatayı kodlar.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum QrError {
    /// Veri, verilen sürüm için bir QR koduna sığmayacak kadar uzun.
    DataTooLong,

    /// Verilen sürüm / hata düzeltme seviyesi bileşimi geçersiz.
    InvalidVersion,

    /// Verideki bazı karakterler verilen QR kodu sürümü tarafından
    /// desteklenmiyor.
    UnsupportedCharacterSet,

    /// Verilen ECI tanımlayıcısı geçersiz. Geçerli bir tanımlayıcı 0 ile 999999
    /// arasında olmalıdır.
    InvalidEciDesignator,

    /// Karakter kümesine ait olmayan bir karakter bulundu.
    InvalidCharacter,

    /// Bir veri parçasının sınırları geçersiz.
    InvalidSegment,

    /// Verilen veri uzunluğu API'nin beklediği uzunlukla eşleşmiyor.
    InvalidDataLength,

    /// Tuval işlemleri gereken sırada uygulanmadığı için tuval geçersiz durumda.
    InvalidCanvasState,

    /// Maske deseni bu QR kodu sürümü tarafından desteklenmiyor. Micro QR
    /// kodları 8 desenden yalnızca 4'üne izin verir.
    InvalidMaskPattern,

    /// Bir koordinat QR kodu sembolünün dışında kalıyor.
    CoordinateOutOfRange,

    /// İstenen görüntü temsil edilemeyecek kadar büyük.
    ImageTooLarge,
}

impl Display for QrError {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        let msg = match *self {
            Self::DataTooLong => "veri çok uzun",
            Self::InvalidVersion => "geçersiz sürüm",
            Self::UnsupportedCharacterSet => "desteklenmeyen karakter kümesi",
            Self::InvalidEciDesignator => "geçersiz ECI tanımlayıcısı",
            Self::InvalidCharacter => "geçersiz karakter",
            Self::InvalidSegment => "geçersiz veri parçası",
            Self::InvalidDataLength => "geçersiz veri uzunluğu",
            Self::InvalidCanvasState => "geçersiz tuval durumu",
            Self::InvalidMaskPattern => "bu sürüm için geçersiz maske deseni",
            Self::CoordinateOutOfRange => "koordinat aralık dışı",
            Self::ImageTooLarge => "görüntü çok büyük",
        };
        fmt.write_str(msg)
    }
}

// `core::error::Error` Rust 1.81'de kararlı hale geldi ve `std::error::Error`
// onun yeniden dışa aktarımı; bu tek impl hem `std` hem `no_std` derlemelerini kapsar.
impl core::error::Error for QrError {}

/// `QrResult`, bir QR kodu üretim sonucu için kullanışlı bir takma addır.
pub type QrResult<T> = Result<T, QrError>;

//}}}
//------------------------------------------------------------------------------
//{{{ Renk

/// Bir modülün rengi.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Color {
    /// Modül açık renkli.
    Light,
    /// Modül koyu renkli.
    Dark,
}

impl Color {
    /// Modülün rengine göre bir değer seçer.
    /// `if self != Color::Light { dark } else { light }` ile eşdeğerdir.
    ///
    /// # Örnekler
    ///
    /// ```
    /// # use qrcode::types::Color;
    /// assert_eq!(Color::Light.select(1, 0), 0);
    /// assert_eq!(Color::Dark.select("siyah", "beyaz"), "siyah");
    /// ```
    pub fn select<T>(self, dark: T, light: T) -> T {
        match self {
            Self::Light => light,
            Self::Dark => dark,
        }
    }
}

impl Not for Color {
    type Output = Self;
    fn not(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Hata düzeltme seviyesi

/// Hata düzeltme seviyesi. Kodun bir kısmı hasar görse bile özgün bilginin
/// geri kazanılmasını sağlar.
#[derive(Debug, PartialEq, Eq, Copy, Clone, PartialOrd, Ord)]
pub enum EcLevel {
    /// Düşük hata düzeltme. Blokların en fazla %7'sinin hatalı olmasına izin verir.
    L = 0,

    /// Orta hata düzeltme (varsayılan). Blokların en fazla %15'ine izin verir.
    M = 1,

    /// "Quartile" hata düzeltme. Blokların en fazla %25'ine izin verir.
    Q = 2,

    /// Yüksek hata düzeltme. Blokların en fazla %30'una izin verir.
    H = 3,
}

//}}}
//------------------------------------------------------------------------------
//{{{ Sürüm

/// Sürüm başına sabit kodlanmış tablolardaki girdi sayısı: 40 QR kodu sürümü
/// ve ardından 4 Micro QR kodu sürümü.
pub const VERSION_COUNT: usize = 44;

/// Bir [`Version`]'ın hangi aileye ait olduğu ve o aile içindeki numarası.
///
/// Bir `Version` dahilde bu biçim üzerinden eşleştirilir. Bilinçli olarak
/// genel API'nin parçası değildir: bir `Version` yalnızca [`Version::normal`]
/// veya [`Version::micro`] ile kurulabilir; bu crate'teki her sürüm tablosu
/// aramasını hatasız yapan da budur.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub(crate) enum VersionKind {
    /// 1 ile 40 arasında normal bir QR kodu sürümü.
    Normal(i16),

    /// 1 ile 4 arasında bir Micro QR kodu sürümü.
    Micro(i16),
}

/// QR kodu terminolojisinde `Version`, üretilen görüntünün boyutu anlamına
/// gelir. Daha büyük sürüm, kodun daha büyük olması ve dolayısıyla daha çok
/// bilgi taşıyabilmesi demektir.
///
/// En küçük sürüm 21×21 boyutundaki `Version::normal(1)`, en büyüğü ise
/// 177×177 boyutundaki `Version::normal(40)`'tır.
///
/// Bir `Version` kurulurken doğrulanır, bu yüzden standardın tanımlamadığı
/// bir sembolü asla adlandıramaz. Dolayısıyla `Version` alan her işlem
/// totaldir: hiçbiri sürüm yüzünden panikleyemez.
///
/// ```
/// use qrcode::Version;
///
/// assert_eq!(Version::normal(1).unwrap().width(), 21);
/// assert_eq!(Version::normal(40).unwrap().width(), 177);
/// assert_eq!(Version::micro(4).unwrap().width(), 17);
///
/// assert!(Version::normal(41).is_err());
/// assert!(Version::micro(0).is_err());
/// ```
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct Version {
    kind: VersionKind,
}

impl Version {
    /// Normal bir QR kodu sürümü kurar.
    ///
    /// # Errors
    ///
    /// `number` 1 ile 40 arasında (dahil) değilse
    /// `Err(QrError::InvalidVersion)` döndürür.
    pub const fn normal(number: i16) -> QrResult<Self> {
        match number {
            1..=40 => Ok(Self { kind: VersionKind::Normal(number) }),
            _ => Err(QrError::InvalidVersion),
        }
    }

    /// Bir Micro QR kodu sürümü kurar.
    ///
    /// # Errors
    ///
    /// `number` 1 ile 4 arasında (dahil) değilse
    /// `Err(QrError::InvalidVersion)` döndürür.
    pub const fn micro(number: i16) -> QrResult<Self> {
        match number {
            1..=4 => Ok(Self { kind: VersionKind::Micro(number) }),
            _ => Err(QrError::InvalidVersion),
        }
    }

    /// Kendi ailesi içindeki sürüm numarası; yani normal bir QR kodu için 1-40,
    /// bir Micro QR kodu için 1-4.
    pub const fn number(self) -> i16 {
        match self.kind {
            VersionKind::Normal(v) | VersionKind::Micro(v) => v,
        }
    }

    pub(crate) const fn kind(self) -> VersionKind {
        self.kind
    }

    /// QR kodunun her kenarındaki "modül" sayısını, yani kodun genişlik ve
    /// yüksekliğini verir.
    ///
    /// Sonuç 11 ile 177 arasındadır, bu yüzden `width * width` her zaman bir
    /// `i16`'ya sığar.
    pub const fn width(self) -> i16 {
        match self.kind {
            VersionKind::Normal(v) => v * 4 + 17,
            VersionKind::Micro(v) => v * 2 + 9,
        }
    }

    /// Bu sürümün sürüm tablosundaki indeksi; yani normal bir QR kodu için 0-39,
    /// bir Micro QR kodu için 40-43.
    #[expect(clippy::cast_sign_loss, reason = "kurucular her iki sayıyı da 1..=40 aralığına hapseder")]
    pub(crate) const fn table_index(self) -> usize {
        // Yapı gereği her iki kol da 0..VERSION_COUNT aralığındadır.
        match self.kind {
            VersionKind::Normal(v) => (v as usize) - 1,
            VersionKind::Micro(v) => (v as usize) + 39,
        }
    }

    /// Sabit kodlanmış bir tablodan bir nesne alır.
    ///
    /// İlk 40 girdi 1'den 40'a QR kodu sürümlerine, son 4 girdi 1'den 4'e Micro
    /// QR kodu sürümlerine karşılık gelir. İç dizi her hata düzeltme seviyesindeki
    /// içeriği [L, M, Q, H] sırasıyla temsil eder.
    ///
    /// Tablo uzunluğu imza ile sabitlenmiştir ve bir `Version` her zaman aralık
    /// içindedir, dolayısıyla aramanın kendisi başarısız olamaz.
    ///
    /// # Errors
    ///
    /// Girdi `T`'nin varsayılan değerine eşitse bu metot
    /// `Err(QrError::InvalidVersion)` döndürür. Micro QR kodu sürümleri
    /// desteklemedikleri hata düzeltme seviyelerini böyle reddeder.
    ///
    /// # Panics
    ///
    /// Yalnızca doğrulanmış `Version`/`EcLevel` değerleri imzada uzunluğu
    /// sabitlenmiş tabloyu indeksleyemezse, yani bir crate iç değişmezi
    /// bozulursa panikler.
    #[expect(
        clippy::expect_used,
        reason = "Version kurucuları ve kapalı EcLevel enumu sabit tablo boyutlarını tam indeksler"
    )]
    pub fn fetch<T>(self, ec_level: EcLevel, table: &[[T; 4]; VERSION_COUNT]) -> QrResult<T>
    where
        T: PartialEq + Default + Copy,
    {
        // Yapı gereği `table_index()` 0..VERSION_COUNT ve `ec_level` 0..4
        // aralığındadır, bu yüzden her iki arama da her zaman isabet eder.
        let obj = table
            .get(self.table_index())
            .and_then(|row| row.get(ec_level as usize))
            .copied()
            .expect("Version iç değişmezi: sürüm ve hata düzeltme seviyesi tablo içinde kalmalı");
        if obj == T::default() { Err(QrError::InvalidVersion) } else { Ok(obj) }
    }

    /// Mod göstergesini kodlamak için gereken bit sayısı.
    #[expect(clippy::cast_sign_loss, reason = "Version::micro sayıyı 1..=4 aralığına hapseder")]
    pub const fn mode_bits_count(self) -> usize {
        match self.kind {
            // Micro sürümler 1..=4 aralığındadır, bu yüzden burada asla taşma olmaz.
            VersionKind::Micro(a) => (a as usize) - 1,
            VersionKind::Normal(_) => 4,
        }
    }

    /// Bu sürümün bir Micro QR koduna işaret edip etmediğini kontrol eder.
    pub const fn is_micro(self) -> bool {
        matches!(self.kind, VersionKind::Micro(_))
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Mod göstergesi

/// Kodlanan verinin karakter kümesini belirten mod göstergesi.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Mode {
    /// Veri yalnızca 0-9 karakterlerini içerir.
    Numeric,

    /// Veri yalnızca büyük harfleri (A–Z), rakamları (0–9) ve birkaç noktalama
    /// işaretini (boşluk, `$`, `%`, `*`, `+`, `-`, `.`, `/`, `:`) içerir.
    Alphanumeric,

    /// Veri rastgele ikili veri içerir.
    Byte,

    /// Veri Shift-JIS kodlanmış çift baytlı metin içerir.
    Kanji,
}

impl Mode {
    /// Veri uzunluğunu kodlamak için gereken bit sayısını hesaplar.
    ///
    /// ```
    /// use qrcode::types::{Mode, Version};
    ///
    /// assert_eq!(Mode::Numeric.length_bits_count(Version::normal(1).unwrap()), 10);
    /// ```
    ///
    pub fn length_bits_count(self, version: Version) -> usize {
        match version.kind() {
            VersionKind::Micro(a) => {
                let a = a.as_usize();
                match self {
                    Self::Numeric => 2 + a,
                    Self::Alphanumeric | Self::Byte => 1 + a,
                    Self::Kanji => a,
                }
            }
            VersionKind::Normal(1..=9) => match self {
                Self::Numeric => 10,
                Self::Alphanumeric => 9,
                Self::Byte | Self::Kanji => 8,
            },
            VersionKind::Normal(10..=26) => match self {
                Self::Numeric => 12,
                Self::Alphanumeric => 11,
                Self::Byte => 16,
                Self::Kanji => 10,
            },
            VersionKind::Normal(_) => match self {
                Self::Numeric => 14,
                Self::Alphanumeric => 13,
                Self::Byte => 16,
                Self::Kanji => 12,
            },
        }
    }

    /// Verilen ham uzunluktaki bir veriyi kodlamak için gereken bit sayısını hesaplar.
    ///
    /// ```
    /// use qrcode::types::Mode;
    ///
    /// assert_eq!(Mode::Numeric.data_bits_count(7), Ok(24));
    /// ```
    ///
    /// Kanji modunda `raw_data_len`'in Kanji sayısı olduğuna, yani toplam bayt
    /// boyutunun yarısı olduğuna dikkat edin.
    ///
    /// # Errors
    ///
    /// Sonuç bir `usize` içine sığmıyorsa `Err(QrError::DataTooLong)` döndürür.
    pub const fn data_bits_count(self, raw_data_len: usize) -> QrResult<usize> {
        let (multiplier, divisor) = match self {
            Self::Numeric => (10, 3),
            Self::Alphanumeric => (11, 2),
            Self::Byte => (8, 1),
            Self::Kanji => (13, 1),
        };
        match raw_data_len.checked_mul(multiplier) {
            Some(bits) => Ok(bits.div_ceil(divisor)),
            None => Err(QrError::DataTooLong),
        }
    }

    /// Her iki modun da uyumlu olduğu en düşük ortak modu bulur.
    ///
    /// ```
    /// use qrcode::types::Mode;
    ///
    /// let a = Mode::Numeric;
    /// let b = Mode::Kanji;
    /// let c = a.max(b);
    /// assert!(a <= c);
    /// assert!(b <= c);
    /// ```
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        match self.partial_cmp(&other) {
            Some(Ordering::Greater) => self,
            Some(_) => other,
            None => Self::Byte,
        }
    }
}

impl PartialOrd for Mode {
    /// Modlar arasında kısmi bir sıralama tanımlar. `a <= b` ise `b`, `a`'nın
    /// desteklediği tüm karakterlerin bir üst kümesini içerir.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (*self, *other) {
            (a, b) if a == b => Some(Ordering::Equal),
            (Self::Numeric, Self::Alphanumeric) | (_, Self::Byte) => Some(Ordering::Less),
            (Self::Alphanumeric, Self::Numeric) | (Self::Byte, _) => Some(Ordering::Greater),
            _ => None,
        }
    }
}

#[cfg(test)]
mod mode_tests {
    use crate::types::Mode::{Alphanumeric, Byte, Kanji, Numeric};

    #[test]
    #[expect(clippy::neg_cmp_op_on_partial_ord, reason = "iki modun kıyaslanamaz olduğunu doğruluyor")]
    fn test_mode_order() {
        assert!(Numeric < Alphanumeric);
        assert!(Byte > Kanji);
        assert!(!(Numeric < Kanji));
        assert!(!(Numeric >= Kanji));
    }

    #[test]
    fn test_max() {
        assert_eq!(Byte.max(Kanji), Byte);
        assert_eq!(Numeric.max(Alphanumeric), Alphanumeric);
        assert_eq!(Alphanumeric.max(Alphanumeric), Alphanumeric);
        assert_eq!(Numeric.max(Kanji), Byte);
        assert_eq!(Kanji.max(Numeric), Byte);
        assert_eq!(Alphanumeric.max(Numeric), Alphanumeric);
        assert_eq!(Kanji.max(Kanji), Kanji);
    }
}

//}}}
