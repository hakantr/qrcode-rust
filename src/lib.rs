//! QR kodu kodlayıcı
//!
//! Bu crate ikili veriler için bir QR kodu ve Micro QR kodu kodlayıcı sağlar.
//!
//! ```
//! # #[cfg(feature = "image")]
//! # {
//! use image::Luma;
//! use qrcode::QrCode;
//!
//! // Bir miktar veriyi bitlere kodla.
//! let code = QrCode::new(b"01234567").unwrap();
//!
//! // Bitleri bir görüntüye çiz.
//! let image = code.render::<Luma<u8>>().build();
//!
//! // Görüntüyü kaydet.
//! # if cfg!(unix) {
//! image.save("/tmp/qrcode.png").unwrap();
//! # }
//!
//! // Onu bir metne de çizebilirsiniz.
//! let string = code.render().light_color(' ').dark_color('#').build();
//! println!("{string}");
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(feature = "bench", feature(test))] // Kararsız kütüphaneler
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![warn(missing_docs)]
// Testlerin sert olmasına izin var: bir testteki `unwrap` çağırana sızan bir
// panik değil, başarısız bir doğrulamadır. Cargo.toml'daki lint kapısı,
// panik sözleşmesinin konusu olan kütüphane kodunu kapsar.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unreachable,
        reason = "test kodu panikleyerek doğrulama yapar"
    )
)]
#![cfg_attr(feature = "bench", doc = include_str!("../README.md"))]
// ^ README.md'mizi test edebildiğimizden emin oluyoruz.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Index;

pub mod bits;
pub mod canvas;
mod cast;
pub mod ec;
pub mod optimize;
pub mod render;
pub mod types;

pub use crate::types::{Color, EcLevel, QrResult, Version};

use crate::cast::As;
use crate::render::{Pixel, Renderer};

/// Kodlanmış QR kodu sembolü.
#[derive(Clone)]
pub struct QrCode {
    content: Vec<Color>,
    version: Version,
    ec_level: EcLevel,
    width: usize,
}

impl QrCode {
    /// Verilen veriyi otomatik olarak kodlayan yeni bir QR kodu kurar.
    ///
    /// Bu metot "orta" hata düzeltme seviyesini kullanır ve en küçük QR kodunu
    /// otomatik olarak seçer.
    ///
    /// ```
    /// use qrcode::QrCode;
    ///
    /// let code = QrCode::new(b"Some data").unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// QR kodu kurulamıyorsa, örneğin veri fazla uzunsa, hata döndürür.
    pub fn new<D: AsRef<[u8]>>(data: D) -> QrResult<Self> {
        Self::with_error_correction_level(data, EcLevel::M)
    }

    /// Verilen veriyi belirli bir hata düzeltme seviyesinde otomatik olarak
    /// kodlayan yeni bir QR kodu kurar.
    ///
    /// Bu metot en küçük QR kodunu otomatik olarak seçer.
    ///
    /// ```
    /// use qrcode::{EcLevel, QrCode};
    ///
    /// let code = QrCode::with_error_correction_level(b"Some data", EcLevel::H).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// QR kodu kurulamıyorsa, örneğin veri fazla uzunsa, hata döndürür.
    pub fn with_error_correction_level<D: AsRef<[u8]>>(data: D, ec_level: EcLevel) -> QrResult<Self> {
        let bits = bits::encode_auto(data.as_ref(), ec_level)?;
        Self::with_bits(bits, ec_level)
    }

    /// Verilen sürüm ve hata düzeltme seviyesi için yeni bir QR kodu kurar.
    ///
    /// ```
    /// use qrcode::{EcLevel, QrCode, Version};
    ///
    /// let code = QrCode::with_version(b"Some data", Version::normal(5).unwrap(), EcLevel::M).unwrap();
    /// ```
    ///
    /// Bu metot Micro QR kodu üretmek için de kullanılabilir.
    ///
    /// ```
    /// use qrcode::{EcLevel, QrCode, Version};
    ///
    /// let micro_code = QrCode::with_version(b"123", Version::micro(1).unwrap(), EcLevel::L).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// QR kodu kurulamıyorsa, örneğin veri fazla uzunsa ya da sürüm ile hata
    /// düzeltme seviyesi uyumsuzsa, hata döndürür.
    pub fn with_version<D: AsRef<[u8]>>(data: D, version: Version, ec_level: EcLevel) -> QrResult<Self> {
        let mut bits = bits::Bits::new(version);
        bits.push_optimal_data(data.as_ref())?;
        bits.push_terminator(ec_level)?;
        Self::with_bits(bits, ec_level)
    }

    /// Kodlanmış bitlerle yeni bir QR kodu kurar.
    ///
    /// Bu metodu yalnızca kodlamadan önce ham bitleri değiştirmek için çok özel
    /// bir ihtiyaç varsa kullanın. Bazı örnekler:
    ///
    /// * ECI ile belirli bir karakter kümesi kullanarak veri kodlamak
    /// * FNC1 modlarını kullanmak
    /// * En uygun bölümleme algoritmasından kaçınmak
    ///
    /// Ayrıntı için `Bits` yapısına bakın.
    ///
    /// ```
    /// #![allow(unused_must_use)]
    ///
    /// use qrcode::bits::Bits;
    /// use qrcode::{EcLevel, QrCode, Version};
    ///
    /// let mut bits = Bits::new(Version::normal(1).unwrap());
    /// bits.push_eci_designator(9);
    /// bits.push_byte_data(b"\xca\xfe\xe4\xe9\xea\xe1\xf2 QR");
    /// bits.push_terminator(EcLevel::L);
    /// let qrcode = QrCode::with_bits(bits, EcLevel::L);
    /// ```
    ///
    /// # Errors
    ///
    /// QR kodu kurulamıyorsa, örneğin bitler fazla uzunsa ya da sürüm ile hata
    /// düzeltme seviyesi uyumsuzsa, hata döndürür.
    pub fn with_bits(bits: bits::Bits, ec_level: EcLevel) -> QrResult<Self> {
        let version = bits.version();
        let data = bits.into_bytes();
        let (encoded_data, ec_data) = ec::construct_codewords(&data, version, ec_level)?;
        let mut canvas = canvas::Canvas::new(version, ec_level);
        canvas.draw_all_functional_patterns();
        canvas.draw_data(&encoded_data, &ec_data);
        let canvas = canvas.apply_best_mask()?;
        Ok(Self { content: canvas.into_colors(), version, ec_level, width: version.width().as_usize() })
    }

    /// Bu QR kodunun sürümünü verir.
    pub const fn version(&self) -> Version {
        self.version
    }

    /// Bu QR kodunun hata düzeltme seviyesini verir.
    pub const fn error_correction_level(&self) -> EcLevel {
        self.ec_level
    }

    /// Kenar başına modül sayısını, yani bu QR kodunun genişliğini verir.
    ///
    /// Buradaki genişlik sessiz bölge dolgularını içermez.
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Veri bozulmadan önce eklenebilecek en fazla hatalı modül sayısını verir.
    /// Hataların işlevsel modüllere eklenmemesi gerektiğine dikkat edin.
    ///
    /// Bir `QrCode` yalnızca standardın tanımladığı bir sürüm ve hata düzeltme
    /// seviyesi için var olabilir, dolayısıyla bu arama başarısız olamaz.
    pub fn max_allowed_errors(&self) -> usize {
        ec::max_allowed_errors(self.version, self.ec_level).unwrap_or(0)
    }

    /// (x, y) koordinatındaki modülün işlevsel bir modül olup olmadığını, ya da
    /// koordinat QR kodunun dışındaysa `None` döndürür.
    pub fn get_functional(&self, x: usize, y: usize) -> Option<bool> {
        if x >= self.width || y >= self.width {
            return None;
        }
        Some(canvas::is_functional(self.version, self.version.width(), x.as_i16(), y.as_i16()))
    }

    /// (x, y) koordinatındaki modülün işlevsel bir modül olup olmadığını kontrol
    /// eder.
    ///
    /// # Panics
    ///
    /// `x` ya da `y` QR kodunun boyutunu aşarsa panikler. Kontrollü biçim için
    /// [`QrCode::get_functional`] kullanın.
    pub fn is_functional(&self, x: usize, y: usize) -> bool {
        #[expect(clippy::expect_used, reason = "belgelenmiş panik; kontrollü biçim get_functional")]
        self.get_functional(x, y).expect("koordinat QR kodu için fazla büyük")
    }

    /// (x, y) koordinatındaki modülün rengini, ya da koordinat QR kodunun
    /// dışındaysa `None` döndürür.
    ///
    /// Bu, `code[(x, y)]` ile indekslemenin paniklemeyen karşılığıdır.
    pub fn get(&self, x: usize, y: usize) -> Option<Color> {
        // Her iki sınır da baştan kontrol edilmeli: büyük bir `y` için
        // `y * self.width`, dilim aramasının başarısız olma şansı doğmadan taşar.
        if x >= self.width || y >= self.width {
            return None;
        }
        self.content.get(y * self.width + x).copied()
    }

    /// QR kodunu insan tarafından okunabilir bir metne dönüştürür. Bu esas olarak
    /// yalnızca hata ayıklama içindir.
    pub fn to_debug_str(&self, on_char: char, off_char: char) -> String {
        self.render().quiet_zone(false).dark_color(on_char).light_color(off_char).build()
    }

    /// QR kodunu bir bool vektörüne dönüştürür. Her girdi modülün rengini temsil
    /// eder; "true" koyu, "false" açık demektir.
    #[deprecated(since = "0.4.0", note = "use `to_colors()` instead")]
    pub fn to_vec(&self) -> Vec<bool> {
        self.content.iter().map(|c| *c != Color::Light).collect()
    }

    /// QR kodunu bir bool vektörüne dönüştürür. Her girdi modülün rengini temsil
    /// eder; "true" koyu, "false" açık demektir.
    #[deprecated(since = "0.4.0", note = "use `into_colors()` instead")]
    pub fn into_vec(self) -> Vec<bool> {
        self.content.into_iter().map(|c| c != Color::Light).collect()
    }

    /// QR kodunu bir renk vektörüne dönüştürür.
    pub fn to_colors(&self) -> Vec<Color> {
        self.content.clone()
    }

    /// QR kodunu bir renk vektörüne dönüştürür.
    pub fn into_colors(self) -> Vec<Color> {
        self.content
    }

    /// QR kodunu bir görüntüye çizer. Sonuç bir görüntü kurucusudur; somut bir
    /// görüntüye kopyalamadan önce ek yapılandırma yapabilirsiniz.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "image")]
    /// # {
    /// # use qrcode::QrCode;
    /// # use image::Rgb;
    ///
    /// let image = QrCode::new(b"hello")
    ///     .unwrap()
    ///     .render()
    ///     .dark_color(Rgb([0, 0, 128]))
    ///     .light_color(Rgb([224, 224, 224])) // renkleri ayarla
    ///     .quiet_zone(false) // sessiz bölgeyi kapat (beyaz kenarlık)
    ///     .min_dimensions(300, 300) // en küçük görüntü boyutunu ayarlar
    ///     .build();
    /// # }
    /// ```
    ///
    /// Not: `image` crate'i görüntüyü döndürmek ya da QR kodunun üzerine logo
    /// bindirmek için de metotlar sağlar.
    pub fn render<P: Pixel>(&self) -> Renderer<'_, P> {
        let quiet_zone = if self.version.is_micro() { 2 } else { 4 };
        Renderer::new(&self.content, self.width, quiet_zone)
    }
}

impl Index<(usize, usize)> for QrCode {
    type Output = Color;

    /// (x, y) koordinatındaki modülün rengini verir.
    ///
    /// # Panics
    ///
    /// `x` ya da `y` QR kodunun boyutunu aşarsa panikler. `x` üzerindeki kontrol
    /// olmasa, aralık dışı bir sütun sessizce komşu bir satıra takma ad olurdu.
    fn index(&self, (x, y): (usize, usize)) -> &Color {
        assert!(x < self.width && y < self.width, "koordinat QR kodu için fazla büyük");
        #[expect(clippy::indexing_slicing, reason = "sınırlar bir üstteki satırda kontrol edildi")]
        &self.content[y * self.width + x]
    }
}

#[cfg(test)]
mod tests {
    use crate::{EcLevel, QrCode, Version};

    #[test]
    fn test_annex_i_qr() {
        // Bu, test vektörü olarak ISO Ek I'i kullanır.
        let code = QrCode::with_version(b"01234567", Version::normal(1).unwrap(), EcLevel::M).unwrap();
        assert_eq!(
            &*code.to_debug_str('#', '.'),
            "\
             #######..#.##.#######\n\
             #.....#..####.#.....#\n\
             #.###.#.#.....#.###.#\n\
             #.###.#.##....#.###.#\n\
             #.###.#.#.###.#.###.#\n\
             #.....#.#...#.#.....#\n\
             #######.#.#.#.#######\n\
             ........#..##........\n\
             #.#####..#..#.#####..\n\
             ...#.#.##.#.#..#.##..\n\
             ..#...##.#.#.#..#####\n\
             ....#....#.....####..\n\
             ...######..#.#..#....\n\
             ........#.#####..##..\n\
             #######..##.#.##.....\n\
             #.....#.#.#####...#.#\n\
             #.###.#.#...#..#.##..\n\
             #.###.#.##..#..#.....\n\
             #.###.#.#.##.#..#.#..\n\
             #.....#........##.##.\n\
             #######.####.#..#.#.."
        );
    }

    #[test]
    fn test_annex_i_micro_qr() {
        let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
        assert_eq!(
            &*code.to_debug_str('#', '.'),
            "\
             #######.#.#.#\n\
             #.....#.###.#\n\
             #.###.#..##.#\n\
             #.###.#..####\n\
             #.###.#.###..\n\
             #.....#.#...#\n\
             #######..####\n\
             .........##..\n\
             ##.#....#...#\n\
             .##.#.#.#.#.#\n\
             ###..#######.\n\
             ...#.#....##.\n\
             ###.#..##.###"
        );
    }
}

#[cfg(test)]
mod smoke_tests {
    use crate::bits::all_versions;
    use crate::cast::As;
    use crate::{EcLevel, QrCode, Version};
    use alloc::vec::Vec;

    /// Kodlama, standardın tanımladığı her sürüm ve hata düzeltme seviyesi için
    /// başarılı olmalı ve süreci abort etmemeli. Micro sürüm M1 ve M3, sondaki
    /// yarım kod kelimesini doldururken taşıyor ve `QrCode::with_version`'ı da
    /// kendileriyle birlikte çökertiyordu.
    #[test]
    fn test_every_version_and_ec_level() {
        for version in all_versions() {
            // Yalnızca Micro sürümler 8'in katı olmayan sayıda veri biti tutar, bu yüzden
            // sonlandırıcı ofsetinin bir yarım kod kelimesinin içine düşebileceği yerler
            // onlardır. Asıl önemli olan orada her uzunluğu taramak; Normal sürümler hızlı
            // kalmak için her biri tek bir kodlamayla yetinir.
            let lengths = if version.is_micro() { 0..24 } else { 0..1 };

            for ec_level in [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H] {
                for length in lengths.clone() {
                    let digits = b"0123456789".iter().copied().cycle().take(length).collect::<Vec<u8>>();
                    let Ok(code) = QrCode::with_version(&digits, version, ec_level) else {
                        continue; // geçersiz bileşim ya da veri sığmıyor
                    };
                    assert_eq!(code.width(), version.width().as_usize(), "{version:?} {ec_level:?} {length}");
                    assert_eq!(code.to_colors().len(), code.width() * code.width());
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "koordinat QR kodu için fazla büyük")]
    fn test_index_out_of_range_column() {
        // 25, 21x21 bir kodun arka deposunda geçerli bir *indekstir* ama geçerli bir
        // sütun değildir: eskiden sessizce sonraki satıra takma ad oluyordu.
        let code = QrCode::with_version(b"01234567", Version::normal(1).unwrap(), EcLevel::M).unwrap();
        let _ = code[(25, 0)];
    }

    #[test]
    #[should_panic(expected = "koordinat QR kodu için fazla büyük")]
    fn test_is_functional_out_of_range() {
        let code = QrCode::with_version(b"01234567", Version::normal(1).unwrap(), EcLevel::M).unwrap();
        let _ = code.is_functional(21, 0);
    }

    #[test]
    fn test_index_matches_to_colors() {
        let code = QrCode::new(b"hello world").unwrap();
        let colors = code.to_colors();
        for y in 0..code.width() {
            for x in 0..code.width() {
                assert_eq!(code[(x, y)], colors[y * code.width() + x]);
            }
        }
    }
}

#[cfg(all(test, feature = "image"))]
mod image_tests {
    use crate::{EcLevel, QrCode, Version};
    use image::{Luma, Rgb, load_from_memory};

    #[test]
    fn test_annex_i_qr_as_image() {
        let code = QrCode::new(b"01234567").unwrap();
        let image = code.render::<Luma<u8>>().build();
        let expected = load_from_memory(include_bytes!("test_annex_i_qr_as_image.png")).unwrap().into_luma8();
        assert_eq!(image.dimensions(), expected.dimensions());
        assert_eq!(image.into_raw(), expected.into_raw());
    }

    #[test]
    fn test_annex_i_micro_qr_as_image() {
        let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
        let image = code
            .render()
            .min_dimensions(200, 200)
            .dark_color(Rgb([128, 0, 0]))
            .light_color(Rgb([255, 255, 128]))
            .build();
        let expected = load_from_memory(include_bytes!("test_annex_i_micro_qr_as_image.png")).unwrap().into_rgb8();
        assert_eq!(image.dimensions(), expected.dimensions());
        assert_eq!(image.into_raw(), expected.into_raw());
    }
}

#[cfg(all(test, feature = "svg"))]
mod svg_tests {
    use crate::render::svg::Color as SvgColor;
    use crate::{EcLevel, QrCode, Version};

    #[test]
    fn test_annex_i_qr_as_svg() {
        let code = QrCode::new(b"01234567").unwrap();
        let image = code.render::<SvgColor>().build();
        let expected = include_str!("test_annex_i_qr_as_svg.svg");
        assert_eq!(&image, expected);
    }

    #[test]
    fn test_annex_i_micro_qr_as_svg() {
        let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
        let image = code
            .render()
            .min_dimensions(200, 200)
            .dark_color(SvgColor("#800000"))
            .light_color(SvgColor("#ffff80"))
            .build();
        let expected = include_str!("test_annex_i_micro_qr_as_svg.svg");
        assert_eq!(&image, expected);
    }
}

#[cfg(all(test, feature = "pic"))]
mod pic_tests {
    use crate::render::pic::Color as PicColor;
    use crate::{EcLevel, QrCode, Version};

    #[test]
    fn test_annex_i_qr_as_pic() {
        let code = QrCode::new(b"01234567").unwrap();
        let image = code.render::<PicColor>().build();
        let expected = include_str!("test_annex_i_qr_as_pic.pic");
        assert_eq!(&image, expected);
    }

    #[test]
    fn test_annex_i_micro_qr_as_pic() {
        let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
        let image = code.render::<PicColor>().min_dimensions(1, 1).build();
        let expected = include_str!("test_annex_i_micro_qr_as_pic.pic");
        assert_eq!(&image, expected);
    }
}

#[cfg(all(test, feature = "eps"))]
mod eps_tests {
    use crate::render::eps::Color as EpsColor;
    use crate::{EcLevel, QrCode, Version};

    #[test]
    fn test_annex_i_qr_as_eps() {
        let code = QrCode::new(b"01234567").unwrap();
        let image = code.render::<EpsColor>().build();
        let expected = include_str!("test_annex_i_qr_as_eps.eps");
        assert_eq!(&image, expected);
    }

    #[test]
    fn test_annex_i_micro_qr_as_eps() {
        let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
        let image = code
            .render()
            .min_dimensions(200, 200)
            .dark_color(EpsColor([0.5, 0.0, 0.0]))
            .light_color(EpsColor([1.0, 1.0, 0.5]))
            .build();
        let expected = include_str!("test_annex_i_micro_qr_as_eps.eps");
        assert_eq!(&image, expected);
    }
}
