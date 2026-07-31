//! `canvas` modülü ham bitleri QR kodu tuvaline yerleştirir.
//!
//! ```
//! use qrcode::canvas::{Canvas, MaskPattern};
//! use qrcode::types::{EcLevel, Version};
//!
//! let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);
//! c.draw_all_functional_patterns();
//! c.draw_data(&[0; 19], &[0; 7]).unwrap();
//! c.try_apply_mask(MaskPattern::Checkerboard).unwrap();
//! let colors = c.into_colors();
//! ```

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::{cmp::max, iter};

use crate::cast::As;
use crate::types::{Color, EcLevel, QrError, QrResult, Version, VersionKind};

//------------------------------------------------------------------------------
//{{{ Modüller

/// QR kodundaki bir modülün (pikselin) rengi.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Module {
    /// Modül boş.
    Empty,

    /// Modül, maskelenemeyen işlevsel desenlere ait ya da maskelenmiş piksellere
    /// aittir.
    Masked(Color),

    /// Modül, maskelemeden önceki veri ve hata düzeltme bitlerine aittir.
    Unmasked(Color),
}

impl From<Module> for Color {
    fn from(module: Module) -> Self {
        match module {
            Module::Empty => Self::Light,
            Module::Masked(c) | Module::Unmasked(c) => c,
        }
    }
}

impl Module {
    /// Bir modülün koyu olup olmadığını kontrol eder.
    pub fn is_dark(self) -> bool {
        Color::from(self) == Color::Dark
    }

    /// Maskelenmemiş modüllere bir maske uygular.
    ///
    /// ```
    /// use qrcode::canvas::Module;
    /// use qrcode::types::Color;
    ///
    /// assert_eq!(Module::Unmasked(Color::Light).mask(true), Module::Masked(Color::Dark));
    /// assert_eq!(Module::Unmasked(Color::Dark).mask(true), Module::Masked(Color::Light));
    /// assert_eq!(Module::Unmasked(Color::Light).mask(false), Module::Masked(Color::Light));
    /// assert_eq!(Module::Masked(Color::Dark).mask(true), Module::Masked(Color::Dark));
    /// assert_eq!(Module::Masked(Color::Dark).mask(false), Module::Masked(Color::Dark));
    /// ```
    #[must_use]
    pub fn mask(self, should_invert: bool) -> Self {
        match (self, should_invert) {
            (Self::Empty, true) => Self::Masked(Color::Dark),
            (Self::Empty, false) => Self::Masked(Color::Light),
            (Self::Unmasked(c), true) => Self::Masked(!c),
            (Self::Unmasked(c), false) | (Self::Masked(c), _) => Self::Masked(c),
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Tuval

/// `Canvas`, hata düzeltmesi yapılmış veriyi bir QR koduna çizmek için
/// kullanılan ara yardımcı yapıdır.
#[derive(Clone)]
pub struct Canvas {
    /// Tuvalin genişliği ve yüksekliği (sık gerektiği için önbelleklenmiştir).
    width: i16,

    /// QR kodunun sürümü.
    version: Version,

    /// QR kodunun hata düzeltme seviyesi.
    ec_level: EcLevel,

    /// QR kodunun modülleri. Modüller soldan sağa, sonra yukarıdan aşağıya
    /// sıralanmıştır.
    modules: Vec<Module>,
}

impl Canvas {
    /// İşlevsel desenleri, veri bitlerini ve artakalan modülleri maskeleme
    /// öncesindeki beklenen yapıya karşı doğrular.
    fn validate_ready_for_mask(&self) -> QrResult<()> {
        for y in 0..self.width {
            for x in 0..self.width {
                if is_functional(self.version, x, y) && !matches!(self.get(x, y), Module::Masked(_)) {
                    return Err(QrError::InvalidCanvasState);
                }
            }
        }

        let (data_words, ec_words) = crate::ec::codeword_lengths(self.version, self.ec_level)?;
        let half_codeword = usize::from(matches!(self.version.kind(), VersionKind::Micro(1 | 3)));
        let expected_bits = (data_words + ec_words) * 8 - half_codeword * 4;
        let mut visited_bits = 0_usize;

        for (x, y) in DataModuleIter::new(self.version).filter(|&(x, y)| !is_functional(self.version, x, y)) {
            let expected = if visited_bits < expected_bits {
                matches!(self.get(x, y), Module::Unmasked(_))
            } else {
                self.get(x, y) == Module::Empty
            };
            if !expected {
                return Err(QrError::InvalidCanvasState);
            }
            visited_bits += 1;
        }

        if visited_bits < expected_bits {
            return Err(QrError::InvalidCanvasState);
        }
        Ok(())
    }

    /// Verilen sürümdeki bir QR kodu için yeterince büyük yeni bir tuval kurar.
    pub fn new(version: Version, ec_level: EcLevel) -> Self {
        let width = version.width();
        Self { width, version, ec_level, modules: vec![Module::Empty; (width * width).as_usize()] }
    }

    /// Tuvali insan tarafından okunabilir bir metne dönüştürür.
    #[cfg(test)]
    fn to_debug_str(&self) -> alloc::string::String {
        let width = self.width;
        let mut res = alloc::string::String::with_capacity((width * (width + 1)).as_usize());
        for y in 0..width {
            res.push('\n');
            for x in 0..width {
                res.push(match self.get(x, y) {
                    Module::Empty => '?',
                    Module::Masked(Color::Light) => '.',
                    Module::Masked(Color::Dark) => '#',
                    Module::Unmasked(Color::Light) => '-',
                    Module::Unmasked(Color::Dark) => '*',
                });
            }
        }
        res
    }

    /// Bir koordinat çiftini `self.modules` içindeki bir indekse, sembolün
    /// dışına düşüyorsa `None`'a eşler. Negatif koordinatlar başa sarar, yani
    /// kabul edilen aralık her iki eksende `-width..width`'tir.
    fn coords_to_index(&self, x: i16, y: i16) -> Option<usize> {
        let normalise = |v: i16| match v {
            _ if v >= self.width => None,
            _ if v >= 0 => Some(v.as_usize()),
            _ if v >= -self.width => Some((v + self.width).as_usize()),
            _ => None,
        };
        let x = normalise(x)?;
        let y = normalise(y)?;
        // width <= 177, yani bu en fazla 31328'dir ve taşamaz.
        Some(y * self.width.as_usize() + x)
    }

    /// Verilen koordinattaki modülü, koordinatlar sembolün dışına düşüyorsa
    /// `None` döndürür. Kolaylık olsun diye negatif koordinatlar başa sarar.
    pub fn get_module(&self, x: i16, y: i16) -> Option<Module> {
        self.modules.get(self.coords_to_index(x, y)?).copied()
    }

    /// Verilen koordinattaki değiştirilebilir modülü, koordinatlar sembolün
    /// dışına düşüyorsa `None` döndürür. Kolaylık olsun diye negatif koordinatlar
    /// başa sarar.
    pub fn get_module_mut(&mut self, x: i16, y: i16) -> Option<&mut Module> {
        let index = self.coords_to_index(x, y)?;
        self.modules.get_mut(index)
    }

    /// Verilen koordinattaki modülü verir. Kolaylık olsun diye negatif
    /// koordinatlar başa sarar.
    ///
    /// # Panics
    ///
    /// Koordinatlar sembolün dışına düşerse panikler. Kontrollü biçim için
    /// [`Canvas::get_module`] kullanın.
    pub fn get(&self, x: i16, y: i16) -> Module {
        #[expect(clippy::expect_used, reason = "belgelenmiş panik; kontrollü biçim get_module")]
        self.get_module(x, y).expect("koordinat aralık dışı")
    }

    /// Verilen koordinattaki değiştirilebilir modülü verir. Kolaylık olsun diye
    /// negatif koordinatlar başa sarar.
    ///
    /// # Panics
    ///
    /// Koordinatlar sembolün dışına düşerse panikler. Kontrollü biçim için
    /// [`Canvas::get_module_mut`] kullanın.
    pub fn get_mut(&mut self, x: i16, y: i16) -> &mut Module {
        #[expect(clippy::expect_used, reason = "belgelenmiş panik; kontrollü biçim get_module_mut")]
        self.get_module_mut(x, y).expect("koordinat aralık dışı")
    }

    /// Verilen koordinattaki işlevsel modülün rengini ayarlar. Kolaylık olsun
    /// diye negatif koordinatlar başa sarar.
    ///
    /// # Errors
    ///
    /// Koordinatlar sembolün dışına düşerse tuvale dokunmadan
    /// `Err(QrError::CoordinateOutOfRange)` döndürür.
    pub fn try_put(&mut self, x: i16, y: i16, color: Color) -> QrResult<()> {
        match self.get_module_mut(x, y) {
            Some(module) => {
                *module = Module::Masked(color);
                Ok(())
            }
            None => Err(QrError::CoordinateOutOfRange),
        }
    }

    /// Verilen koordinattaki işlevsel modülün rengini ayarlar. Kolaylık olsun
    /// diye negatif koordinatlar başa sarar.
    ///
    /// # Panics
    ///
    /// Koordinatlar sembolün dışına düşerse panikler. Kontrollü biçim için
    /// [`Canvas::try_put`] kullanın.
    pub fn put(&mut self, x: i16, y: i16, color: Color) {
        *self.get_mut(x, y) = Module::Masked(color);
    }
}

#[cfg(test)]
mod basic_canvas_tests {
    use crate::canvas::{Canvas, Module};
    use crate::types::{Color, EcLevel, Version};

    #[test]
    fn test_index() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);

        assert_eq!(c.get(0, 4), Module::Empty);
        assert_eq!(c.get(-1, -7), Module::Empty);
        assert_eq!(c.get(21 - 1, 21 - 7), Module::Empty);

        c.put(0, 0, Color::Dark);
        c.put(-1, -7, Color::Light);
        assert_eq!(c.get(0, 0), Module::Masked(Color::Dark));
        assert_eq!(c.get(21 - 1, -7), Module::Masked(Color::Light));
        assert_eq!(c.get(-1, 21 - 7), Module::Masked(Color::Light));
    }

    #[test]
    fn test_debug_str() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);

        for i in 3_i16..20 {
            for j in 3_i16..20 {
                *c.get_mut(i, j) = match ((i * 3) ^ j) % 5 {
                    0 => Module::Empty,
                    1 => Module::Masked(Color::Light),
                    2 => Module::Masked(Color::Dark),
                    3 => Module::Unmasked(Color::Light),
                    4 => Module::Unmasked(Color::Dark),
                    _ => unreachable!(),
                };
            }
        }

        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????####****....---?\n\
             ???--.##-..##?..#??.?\n\
             ???#*?-.*?#.-*#?-*.??\n\
             ?????*?*?****-*-*---?\n\
             ???*.-.-.-?-?#?#?#*#?\n\
             ???.*#.*.*#.*#*#.*#*?\n\
             ?????.#-#--??.?.#---?\n\
             ???-.?*.-#?-.?#*-#?.?\n\
             ???##*??*..##*--*..??\n\
             ?????-???--??---?---?\n\
             ???*.#.*.#**.#*#.#*#?\n\
             ???##.-##..##..?#..??\n\
             ???.-?*.-?#.-?#*-?#*?\n\
             ????-.#?-.**#?-.#?-.?\n\
             ???**?-**??--**?-**??\n\
             ???#?*?#?*#.*-.-*-.-?\n\
             ???..-...--??###?###?\n\
             ?????????????????????"
        );
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Bulucu desenleri

impl Canvas {
    /// Merkezi (x, y) olan tek bir bulucu deseni çizer.
    fn draw_finder_pattern_at(&mut self, x: i16, y: i16) {
        let (dx_left, dx_right) = if x >= 0 { (-3, 4) } else { (-4, 3) };
        let (dy_top, dy_bottom) = if y >= 0 { (-3, 4) } else { (-4, 3) };
        for j in dy_top..=dy_bottom {
            for i in dx_left..=dx_right {
                self.put(
                    x + i,
                    y + j,
                    match (i, j) {
                        (4 | -4, _) | (_, 4 | -4) => Color::Light,
                        (3 | -3, _) | (_, 3 | -3) => Color::Dark,
                        (2 | -2, _) | (_, 2 | -2) => Color::Light,
                        _ => Color::Dark,
                    },
                );
            }
        }
    }

    /// Bulucu desenlerini çizer.
    ///
    /// Bulucu desenleri bir QR kodunun üç köşesinde görünen 7×7 kare desenlerdir.
    /// Tarayıcının QR kodunu bulmasını ve yönelimi belirlemesini sağlarlar.
    fn draw_finder_patterns(&mut self) {
        self.draw_finder_pattern_at(3, 3);

        match self.version.kind() {
            VersionKind::Micro(_) => {}
            VersionKind::Normal(_) => {
                self.draw_finder_pattern_at(-4, 3);
                self.draw_finder_pattern_at(3, -4);
            }
        }
    }
}

#[cfg(test)]
mod finder_pattern_tests {
    use crate::canvas::Canvas;
    use crate::types::{EcLevel, Version};

    #[test]
    fn test_qr() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);
        c.draw_finder_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######.?????.#######\n\
             #.....#.?????.#.....#\n\
             #.###.#.?????.#.###.#\n\
             #.###.#.?????.#.###.#\n\
             #.###.#.?????.#.###.#\n\
             #.....#.?????.#.....#\n\
             #######.?????.#######\n\
             ........?????........\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ........?????????????\n\
             #######.?????????????\n\
             #.....#.?????????????\n\
             #.###.#.?????????????\n\
             #.###.#.?????????????\n\
             #.###.#.?????????????\n\
             #.....#.?????????????\n\
             #######.?????????????"
        );
    }

    #[test]
    fn test_micro_qr() {
        let mut c = Canvas::new(Version::micro(1).unwrap(), EcLevel::L);
        c.draw_finder_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######.???\n\
             #.....#.???\n\
             #.###.#.???\n\
             #.###.#.???\n\
             #.###.#.???\n\
             #.....#.???\n\
             #######.???\n\
             ........???\n\
             ???????????\n\
             ???????????\n\
             ???????????"
        );
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Hizalama desenleri

impl Canvas {
    /// Merkezi (x, y) olan bir hizalama deseni çizer.
    fn draw_alignment_pattern_at(&mut self, x: i16, y: i16) {
        if self.get(x, y) != Module::Empty {
            return;
        }
        for j in -2..=2 {
            for i in -2..=2 {
                self.put(
                    x + i,
                    y + j,
                    match (i, j) {
                        (2 | -2, _) | (_, 2 | -2) | (0, 0) => Color::Dark,
                        _ => Color::Light,
                    },
                );
            }
        }
    }

    /// Hizalama desenlerini çizer.
    ///
    /// Hizalama desenleri, tarayıcının kare ızgarayı oluşturmasına yardımcı olmak
    /// için QR kodu sembolünün içindeki 5×5 kare desenlerdir.
    #[expect(
        clippy::expect_used,
        reason = "doğrulanmış normal sürüm 7..=40, 34 satırlık hizalama tablosunu tam indeksler"
    )]
    fn draw_alignment_patterns(&mut self) {
        match self.version.kind() {
            VersionKind::Micro(_) | VersionKind::Normal(1) => {}
            VersionKind::Normal(2..=6) => self.draw_alignment_pattern_at(-7, -7),
            VersionKind::Normal(a) => {
                // `a` burada 7..=40 aralığındadır, yani indeks 0..=33 ve
                // tablonun 34 satırı vardır.
                let positions = ALIGNMENT_PATTERN_POSITIONS
                    .get((a - 7).as_usize())
                    .copied()
                    .expect("sürüm iç değişmezi: hizalama satırı bulunmalı");
                for x in positions {
                    for y in positions {
                        self.draw_alignment_pattern_at(*x, *y);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod alignment_pattern_tests {
    use crate::canvas::Canvas;
    use crate::types::{EcLevel, Version};

    #[test]
    fn test_draw_alignment_patterns_1() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);
        c.draw_finder_patterns();
        c.draw_alignment_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######.?????.#######\n\
             #.....#.?????.#.....#\n\
             #.###.#.?????.#.###.#\n\
             #.###.#.?????.#.###.#\n\
             #.###.#.?????.#.###.#\n\
             #.....#.?????.#.....#\n\
             #######.?????.#######\n\
             ........?????........\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ........?????????????\n\
             #######.?????????????\n\
             #.....#.?????????????\n\
             #.###.#.?????????????\n\
             #.###.#.?????????????\n\
             #.###.#.?????????????\n\
             #.....#.?????????????\n\
             #######.?????????????"
        );
    }

    #[test]
    fn test_draw_alignment_patterns_3() {
        let mut c = Canvas::new(Version::normal(3).unwrap(), EcLevel::L);
        c.draw_finder_patterns();
        c.draw_alignment_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######.?????????????.#######\n\
             #.....#.?????????????.#.....#\n\
             #.###.#.?????????????.#.###.#\n\
             #.###.#.?????????????.#.###.#\n\
             #.###.#.?????????????.#.###.#\n\
             #.....#.?????????????.#.....#\n\
             #######.?????????????.#######\n\
             ........?????????????........\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ?????????????????????????????\n\
             ????????????????????#####????\n\
             ........????????????#...#????\n\
             #######.????????????#.#.#????\n\
             #.....#.????????????#...#????\n\
             #.###.#.????????????#####????\n\
             #.###.#.?????????????????????\n\
             #.###.#.?????????????????????\n\
             #.....#.?????????????????????\n\
             #######.?????????????????????"
        );
    }

    #[test]
    fn test_draw_alignment_patterns_7() {
        let mut c = Canvas::new(Version::normal(7).unwrap(), EcLevel::L);
        c.draw_finder_patterns();
        c.draw_alignment_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######.?????????????????????????????.#######\n\
             #.....#.?????????????????????????????.#.....#\n\
             #.###.#.?????????????????????????????.#.###.#\n\
             #.###.#.?????????????????????????????.#.###.#\n\
             #.###.#.????????????#####????????????.#.###.#\n\
             #.....#.????????????#...#????????????.#.....#\n\
             #######.????????????#.#.#????????????.#######\n\
             ........????????????#...#????????????........\n\
             ????????????????????#####????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ????#####???????????#####???????????#####????\n\
             ????#...#???????????#...#???????????#...#????\n\
             ????#.#.#???????????#.#.#???????????#.#.#????\n\
             ????#...#???????????#...#???????????#...#????\n\
             ????#####???????????#####???????????#####????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ????????????????????#####???????????#####????\n\
             ........????????????#...#???????????#...#????\n\
             #######.????????????#.#.#???????????#.#.#????\n\
             #.....#.????????????#...#???????????#...#????\n\
             #.###.#.????????????#####???????????#####????\n\
             #.###.#.?????????????????????????????????????\n\
             #.###.#.?????????????????????????????????????\n\
             #.....#.?????????????????????????????????????\n\
             #######.?????????????????????????????????????"
        );
    }
}

/// `ALIGNMENT_PATTERN_POSITIONS`, hizalama desenlerinin merkezinin x ve y
/// koordinatlarını tanımlar. QR kodu simetrik olduğundan yalnızca tek bir
/// koordinat yeterlidir.
static ALIGNMENT_PATTERN_POSITIONS: [&[i16]; 34] = [
    &[6, 22, 38],
    &[6, 24, 42],
    &[6, 26, 46],
    &[6, 28, 50],
    &[6, 30, 54],
    &[6, 32, 58],
    &[6, 34, 62],
    &[6, 26, 46, 66],
    &[6, 26, 48, 70],
    &[6, 26, 50, 74],
    &[6, 30, 54, 78],
    &[6, 30, 56, 82],
    &[6, 30, 58, 86],
    &[6, 34, 62, 90],
    &[6, 28, 50, 72, 94],
    &[6, 26, 50, 74, 98],
    &[6, 30, 54, 78, 102],
    &[6, 28, 54, 80, 106],
    &[6, 32, 58, 84, 110],
    &[6, 30, 58, 86, 114],
    &[6, 34, 62, 90, 118],
    &[6, 26, 50, 74, 98, 122],
    &[6, 30, 54, 78, 102, 126],
    &[6, 26, 52, 78, 104, 130],
    &[6, 30, 56, 82, 108, 134],
    &[6, 34, 60, 86, 112, 138],
    &[6, 30, 58, 86, 114, 142],
    &[6, 34, 62, 90, 118, 146],
    &[6, 30, 54, 78, 102, 126, 150],
    &[6, 24, 50, 76, 102, 128, 154],
    &[6, 28, 54, 80, 106, 132, 158],
    &[6, 32, 58, 84, 110, 136, 162],
    &[6, 26, 54, 82, 110, 138, 166],
    &[6, 30, 58, 86, 114, 142, 170],
];

//}}}
//------------------------------------------------------------------------------
//{{{ Zamanlama desenleri

impl Canvas {
    /// (x1, y1)'den (x2, y2)'ye, uçlar dahil bir çizgi çizer.
    ///
    /// Çizgi ya yatay ya da dikey olmalıdır; yani `x1 == x2 || y1 == y2`.
    /// Ayrıca ilk koordinatlar ikincilerden küçük olmalıdır.
    ///
    /// Çift koordinatlarda `color_even`, tek koordinatlarda ise `color_odd`
    /// çizilir. Zamanlama deseni böylece bu metotla çizilebilir.
    fn draw_line(&mut self, x1: i16, y1: i16, x2: i16, y2: i16, color_even: Color, color_odd: Color) {
        assert!(x1 == x2 || y1 == y2, "tuval iç değişmezi: çizgi yatay ya da dikey olmalı");

        if y1 == y2 {
            // Yatay çizgi.
            for x in x1..=x2 {
                self.put(x, y1, if x % 2 == 0 { color_even } else { color_odd });
            }
        } else {
            // Dikey çizgi.
            for y in y1..=y2 {
                self.put(x1, y, if y % 2 == 0 { color_even } else { color_odd });
            }
        }
    }

    /// Zamanlama desenlerini çizer.
    ///
    /// Zamanlama desenleri, tarama sırasında ince taneli modül koordinatlarını
    /// belirlemek için QR kodu sembolünün kenarına yakın, dama tahtası renkli
    /// çizgilerdir.
    fn draw_timing_patterns(&mut self) {
        let width = self.width;
        let (y, x1, x2) = match self.version.kind() {
            VersionKind::Micro(_) => (0, 8, width - 1),
            VersionKind::Normal(_) => (6, 8, width - 9),
        };
        self.draw_line(x1, y, x2, y, Color::Dark, Color::Light);
        self.draw_line(y, x1, y, x2, Color::Dark, Color::Light);
    }
}

#[cfg(test)]
mod timing_pattern_tests {
    use crate::canvas::Canvas;
    use crate::types::{EcLevel, Version};

    #[test]
    fn test_draw_timing_patterns_qr() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);
        c.draw_timing_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ????????#.#.#????????\n\
             ?????????????????????\n\
             ??????#??????????????\n\
             ??????.??????????????\n\
             ??????#??????????????\n\
             ??????.??????????????\n\
             ??????#??????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????"
        );
    }

    #[test]
    fn test_draw_timing_patterns_micro_qr() {
        let mut c = Canvas::new(Version::micro(1).unwrap(), EcLevel::L);
        c.draw_timing_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ????????#.#\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             #??????????\n\
             .??????????\n\
             #??????????"
        );
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Biçim bilgisi ve sürüm bilgisi

impl Canvas {
    /// Verilen koordinatlarla tuvale büyük-endian bir tam sayı çizer.
    ///
    /// 1 bitleri `on_color`, 0 bitleri `off_color` ile çizilir. Koordinatlar
    /// `coords` yineleyicisinden alınır. En anlamlı bitten başlar, dolayısıyla
    /// *sondaki* sıfırlar yok sayılır.
    fn draw_number(&mut self, number: u32, bits: u32, on_color: Color, off_color: Color, coords: &[(i16, i16)]) {
        let mut mask = 1 << (bits - 1);
        for &(x, y) in coords {
            let color = if (mask & number) == 0 { off_color } else { on_color };
            self.put(x, y, color);
            mask >>= 1;
        }
    }

    /// Kodlanmış bir sayı için biçim bilgisi desenlerini çizer.
    fn draw_format_info_patterns_with_number(&mut self, format_info: u16) {
        let format_info = u32::from(format_info);
        match self.version.kind() {
            VersionKind::Micro(_) => {
                self.draw_number(format_info, 15, Color::Dark, Color::Light, &FORMAT_INFO_COORDS_MICRO_QR);
            }
            VersionKind::Normal(_) => {
                self.draw_number(format_info, 15, Color::Dark, Color::Light, &FORMAT_INFO_COORDS_QR_MAIN);
                self.draw_number(format_info, 15, Color::Dark, Color::Light, &FORMAT_INFO_COORDS_QR_SIDE);
                self.put(8, -8, Color::Dark); // Koyu modül.
            }
        }
    }

    /// Biçim bilgisinin yerleştirileceği alanı ayırır.
    fn draw_reserved_format_info_patterns(&mut self) {
        self.draw_format_info_patterns_with_number(0);
    }

    /// Sürüm bilgisi desenlerini çizer.
    #[expect(
        clippy::expect_used,
        reason = "doğrulanmış normal sürüm 7..=40, 34 girdilik sürüm tablosunu tam indeksler"
    )]
    fn draw_version_info_patterns(&mut self) {
        match self.version.kind() {
            VersionKind::Micro(_) | VersionKind::Normal(1..=6) => {}
            VersionKind::Normal(a) => {
                // `a` burada 7..=40 aralığındadır, yani 34 girdilik tabloya indeks 0..=33'tür.
                let version_info = *VERSION_INFOS
                    .get((a - 7).as_usize())
                    .expect("sürüm iç değişmezi: sürüm bilgisi tabloda bulunmalı");
                self.draw_number(version_info, 18, Color::Dark, Color::Light, &VERSION_INFO_COORDS_BL);
                self.draw_number(version_info, 18, Color::Dark, Color::Light, &VERSION_INFO_COORDS_TR);
            }
        }
    }
}

#[cfg(test)]
mod draw_version_info_tests {
    use crate::canvas::Canvas;
    use crate::types::{Color, EcLevel, Version};

    #[test]
    fn test_draw_number() {
        let mut c = Canvas::new(Version::micro(1).unwrap(), EcLevel::L);
        c.draw_number(0b1010_1101, 8, Color::Dark, Color::Light, &[(0, 0), (0, -1), (-2, -2), (-2, 0)]);
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #????????.?\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ???????????\n\
             ?????????#?\n\
             .??????????"
        );
    }

    #[test]
    fn test_draw_version_info_1() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);
        c.draw_version_info_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????"
        );
    }

    #[test]
    fn test_draw_version_info_7() {
        let mut c = Canvas::new(Version::normal(7).unwrap(), EcLevel::L);
        c.draw_version_info_patterns();

        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ??????????????????????????????????..#????????\n\
             ??????????????????????????????????.#.????????\n\
             ??????????????????????????????????.#.????????\n\
             ??????????????????????????????????.##????????\n\
             ??????????????????????????????????###????????\n\
             ??????????????????????????????????...????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ....#.???????????????????????????????????????\n\
             .####.???????????????????????????????????????\n\
             #..##.???????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????\n\
             ?????????????????????????????????????????????"
        );
    }

    #[test]
    fn test_draw_reserved_format_info_patterns_qr() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);
        c.draw_reserved_format_info_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ?????????????????????\n\
             ????????.????????????\n\
             ......?..????........\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ????????#????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????.????????????"
        );
    }

    #[test]
    fn test_draw_reserved_format_info_patterns_micro_qr() {
        let mut c = Canvas::new(Version::micro(1).unwrap(), EcLevel::L);
        c.draw_reserved_format_info_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ???????????\n\
             ????????.??\n\
             ????????.??\n\
             ????????.??\n\
             ????????.??\n\
             ????????.??\n\
             ????????.??\n\
             ????????.??\n\
             ?........??\n\
             ???????????\n\
             ???????????"
        );
    }
}

static VERSION_INFO_COORDS_BL: [(i16, i16); 18] = [
    (5, -9),
    (5, -10),
    (5, -11),
    (4, -9),
    (4, -10),
    (4, -11),
    (3, -9),
    (3, -10),
    (3, -11),
    (2, -9),
    (2, -10),
    (2, -11),
    (1, -9),
    (1, -10),
    (1, -11),
    (0, -9),
    (0, -10),
    (0, -11),
];

static VERSION_INFO_COORDS_TR: [(i16, i16); 18] = [
    (-9, 5),
    (-10, 5),
    (-11, 5),
    (-9, 4),
    (-10, 4),
    (-11, 4),
    (-9, 3),
    (-10, 3),
    (-11, 3),
    (-9, 2),
    (-10, 2),
    (-11, 2),
    (-9, 1),
    (-10, 1),
    (-11, 1),
    (-9, 0),
    (-10, 0),
    (-11, 0),
];

static FORMAT_INFO_COORDS_QR_MAIN: [(i16, i16); 15] = [
    (0, 8),
    (1, 8),
    (2, 8),
    (3, 8),
    (4, 8),
    (5, 8),
    (7, 8),
    (8, 8),
    (8, 7),
    (8, 5),
    (8, 4),
    (8, 3),
    (8, 2),
    (8, 1),
    (8, 0),
];

static FORMAT_INFO_COORDS_QR_SIDE: [(i16, i16); 15] = [
    (8, -1),
    (8, -2),
    (8, -3),
    (8, -4),
    (8, -5),
    (8, -6),
    (8, -7),
    (-8, 8),
    (-7, 8),
    (-6, 8),
    (-5, 8),
    (-4, 8),
    (-3, 8),
    (-2, 8),
    (-1, 8),
];

static FORMAT_INFO_COORDS_MICRO_QR: [(i16, i16); 15] = [
    (1, 8),
    (2, 8),
    (3, 8),
    (4, 8),
    (5, 8),
    (6, 8),
    (7, 8),
    (8, 8),
    (8, 7),
    (8, 6),
    (8, 5),
    (8, 4),
    (8, 3),
    (8, 2),
    (8, 1),
];

static VERSION_INFOS: [u32; 34] = [
    0x07c94, 0x085bc, 0x09a99, 0x0a4d3, 0x0bbf6, 0x0c762, 0x0d847, 0x0e60d, 0x0f928, 0x10b78, 0x1145d, 0x12a17,
    0x13532, 0x149a6, 0x15683, 0x168c9, 0x177ec, 0x18ec4, 0x191e1, 0x1afab, 0x1b08e, 0x1cc1a, 0x1d33f, 0x1ed75,
    0x1f250, 0x209d5, 0x216f0, 0x228ba, 0x2379f, 0x24b0b, 0x2542e, 0x26a64, 0x27541, 0x28c69,
];

//}}}
//------------------------------------------------------------------------------
//{{{ Veri yerleşiminden önce tüm işlevsel desenler

impl Canvas {
    /// Veri yerleşiminden önce tüm işlevsel desenleri çizer.
    ///
    /// Biçim bilgisi deseni *hariç* tüm işlevsel desenler (ör. bulucu deseni)
    /// doldurulur. Biçim bilgisi deseni bunun yerine açık modüllerle doldurulur.
    /// Veri bitleri daha sonra `.draw_data()` ile boş modüllere yerleştirilebilir.
    pub fn draw_all_functional_patterns(&mut self) {
        self.draw_finder_patterns();
        self.draw_alignment_patterns();
        self.draw_reserved_format_info_patterns();
        self.draw_timing_patterns();
        self.draw_version_info_patterns();
    }
}

/// Verilen koordinattaki modülün işlevsel bir modülü temsil edip etmediğini
/// verir. Koordinat sembolün dışındaysa `false` döndürür; `Canvas` metotlarında
/// olduğu gibi `-width..0` aralığındaki negatif koordinatlar karşı kenardan
/// başlayarak normalleştirilir.
///
/// # Panics
///
/// Yalnızca doğrulanmış bir `Version` ile crate içindeki sabit hizalama tablosu
/// birbiriyle çelişirse panikler.
#[expect(clippy::expect_used, reason = "doğrulanmış normal sürüm 7..=40, 34 satırlık hizalama tablosunu tam indeksler")]
pub fn is_functional(version: Version, x: i16, y: i16) -> bool {
    let width = version.width();
    if x < -width || x >= width || y < -width || y >= width {
        return false;
    }

    let x = if x < 0 { x + width } else { x };
    let y = if y < 0 { y + width } else { y };

    match version.kind() {
        VersionKind::Micro(_) => x == 0 || y == 0 || (x < 9 && y < 9),
        VersionKind::Normal(a) => {
            // Sürüm bilgisi blokları, sol alt ve sağ üst bulucu desenlerinin yanındaki
            // 6×3 alanlardır ve sürüm 7'den itibaren bulunurlar. Bkz.
            // `VERSION_INFO_COORDS_BL` / `VERSION_INFO_COORDS_TR`.
            let version_info_test = a >= 7
                && ((x < 6 && (width - 11..=width - 9).contains(&y))
                    || (y < 6 && (width - 11..=width - 9).contains(&x)));
            let non_alignment_test = x == 6 || y == 6 || // Zamanlama desenleri
                    (x < 9 && y < 9) ||                  // Sol üst bulucu deseni
                    (x < 9 && y >= width-8) ||           // Sol alt bulucu deseni
                    (x >= width-8 && y < 9) ||           // Sağ üst bulucu deseni
                    version_info_test;
            match a {
                _ if non_alignment_test => true,
                1 => false,
                2..=6 => (width - 7 - x).abs() <= 2 && (width - 7 - y).abs() <= 2,
                _ => {
                    let positions = ALIGNMENT_PATTERN_POSITIONS
                        .get((a - 7).as_usize())
                        .copied()
                        .expect("sürüm iç değişmezi: hizalama satırı bulunmalı");
                    let last =
                        positions.len().checked_sub(1).expect("sürüm iç değişmezi: hizalama satırı boş olmamalı");
                    for (i, align_x) in positions.iter().enumerate() {
                        for (j, align_y) in positions.iter().enumerate() {
                            if i == 0 && (j == 0 || j == last) || (i == last && j == 0) {
                                continue;
                            }
                            if (*align_x - x).abs() <= 2 && (*align_y - y).abs() <= 2 {
                                return true;
                            }
                        }
                    }
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod all_functional_patterns_tests {
    use crate::canvas::{Canvas, is_functional};
    use crate::types::{EcLevel, Version};

    #[test]
    fn test_all_functional_patterns_qr() {
        let mut c = Canvas::new(Version::normal(2).unwrap(), EcLevel::L);
        c.draw_all_functional_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######..????????.#######\n\
             #.....#..????????.#.....#\n\
             #.###.#..????????.#.###.#\n\
             #.###.#..????????.#.###.#\n\
             #.###.#..????????.#.###.#\n\
             #.....#..????????.#.....#\n\
             #######.#.#.#.#.#.#######\n\
             .........????????........\n\
             ......#..????????........\n\
             ??????.??????????????????\n\
             ??????#??????????????????\n\
             ??????.??????????????????\n\
             ??????#??????????????????\n\
             ??????.??????????????????\n\
             ??????#??????????????????\n\
             ??????.??????????????????\n\
             ??????#?????????#####????\n\
             ........#???????#...#????\n\
             #######..???????#.#.#????\n\
             #.....#..???????#...#????\n\
             #.###.#..???????#####????\n\
             #.###.#..????????????????\n\
             #.###.#..????????????????\n\
             #.....#..????????????????\n\
             #######..????????????????"
        );
    }

    #[test]
    fn test_all_functional_patterns_micro_qr() {
        let mut c = Canvas::new(Version::micro(1).unwrap(), EcLevel::L);
        c.draw_all_functional_patterns();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######.#.#\n\
             #.....#..??\n\
             #.###.#..??\n\
             #.###.#..??\n\
             #.###.#..??\n\
             #.....#..??\n\
             #######..??\n\
             .........??\n\
             #........??\n\
             .??????????\n\
             #??????????"
        );
    }

    #[test]
    fn test_is_functional_qr_1() {
        let version = Version::normal(1).unwrap();
        assert!(is_functional(version, 0, 0));
        assert!(is_functional(version, 10, 6));
        assert!(!is_functional(version, 10, 5));
        assert!(!is_functional(version, 14, 14));
        assert!(is_functional(version, 6, 11));
        assert!(!is_functional(version, 4, 11));
        assert!(is_functional(version, 4, 13));
        assert!(is_functional(version, 17, 7));
        assert!(!is_functional(version, 17, 17));
    }

    #[test]
    fn test_is_functional_qr_3() {
        let version = Version::normal(3).unwrap();
        assert!(is_functional(version, 0, 0));
        assert!(!is_functional(version, 25, 24));
        assert!(is_functional(version, 24, 24));
        assert!(!is_functional(version, 9, 25));
        assert!(!is_functional(version, 20, 0));
        assert!(is_functional(version, 21, 0));
    }

    #[test]
    fn test_is_functional_qr_7() {
        let version = Version::normal(7).unwrap();
        assert!(is_functional(version, 21, 4));
        assert!(is_functional(version, 7, 21));
        assert!(is_functional(version, 22, 22));
        assert!(is_functional(version, 8, 8));
        assert!(!is_functional(version, 19, 5));
        assert!(is_functional(version, 38, 38));

        // Sürüm 7'den itibaren çizilen 6×3 sürüm bilgisi blokları. Bunlar eskiden
        // sıradan veri modülü olarak yanlış bildiriliyordu.
        assert!(is_functional(version, 36, 3));
        assert!(is_functional(version, 4, 36));
        assert!(is_functional(version, 34, 0));
        assert!(is_functional(version, 0, 34));
        assert!(!is_functional(version, 33, 3)); // bloğun hemen solu
        assert!(!is_functional(version, 3, 33)); // bloğun hemen üstü
    }

    #[test]
    fn test_is_functional_micro() {
        let version = Version::micro(1).unwrap();
        assert!(is_functional(version, 8, 0));
        assert!(is_functional(version, 10, 0));
        assert!(!is_functional(version, 10, 1));
        assert!(is_functional(version, 8, 8));
        assert!(is_functional(version, 0, 9));
        assert!(!is_functional(version, 1, 9));
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Veri yerleşimi yineleyicisi

struct DataModuleIter {
    x: i16,
    y: i16,
    width: i16,
    timing_pattern_column: i16,
}

impl DataModuleIter {
    const fn new(version: Version) -> Self {
        let width = version.width();
        Self {
            x: width - 1,
            y: width - 1,
            width,
            timing_pattern_column: match version.kind() {
                VersionKind::Micro(_) => 0,
                VersionKind::Normal(_) => 6,
            },
        }
    }
}

impl Iterator for DataModuleIter {
    type Item = (i16, i16);

    fn next(&mut self) -> Option<(i16, i16)> {
        let adjusted_ref_col = if self.x <= self.timing_pattern_column { self.x + 1 } else { self.x };
        if adjusted_ref_col <= 0 {
            return None;
        }

        let res = (self.x, self.y);
        let column_type = (self.width - adjusted_ref_col) % 4;

        match column_type {
            2 if self.y > 0 => {
                self.y -= 1;
                self.x += 1;
            }
            0 if self.y < self.width - 1 => {
                self.y += 1;
                self.x += 1;
            }
            0 | 2 if self.x == self.timing_pattern_column + 1 => {
                self.x -= 2;
            }
            _ => {
                self.x -= 1;
            }
        }

        Some(res)
    }
}

#[cfg(test)]
#[rustfmt::skip] // dosya fazla uzamasın diye atlanıyor.
mod data_iter_tests {
    use crate::canvas::DataModuleIter;
    use crate::types::Version;
    use alloc::vec::Vec;
    use alloc::vec;

    #[test]
    fn test_qr() {
        let res = DataModuleIter::new(Version::normal(1).unwrap()).collect::<Vec<(i16, i16)>>();
        assert_eq!(res, vec![
            (20, 20), (19, 20), (20, 19), (19, 19), (20, 18), (19, 18),
            (20, 17), (19, 17), (20, 16), (19, 16), (20, 15), (19, 15),
            (20, 14), (19, 14), (20, 13), (19, 13), (20, 12), (19, 12),
            (20, 11), (19, 11), (20, 10), (19, 10), (20, 9), (19, 9),
            (20, 8), (19, 8), (20, 7), (19, 7), (20, 6), (19, 6),
            (20, 5), (19, 5), (20, 4), (19, 4), (20, 3), (19, 3),
            (20, 2), (19, 2), (20, 1), (19, 1), (20, 0), (19, 0),

            (18, 0), (17, 0), (18, 1), (17, 1), (18, 2), (17, 2),
            (18, 3), (17, 3), (18, 4), (17, 4), (18, 5), (17, 5),
            (18, 6), (17, 6), (18, 7), (17, 7), (18, 8), (17, 8),
            (18, 9), (17, 9), (18, 10), (17, 10), (18, 11), (17, 11),
            (18, 12), (17, 12), (18, 13), (17, 13), (18, 14), (17, 14),
            (18, 15), (17, 15), (18, 16), (17, 16), (18, 17), (17, 17),
            (18, 18), (17, 18), (18, 19), (17, 19), (18, 20), (17, 20),

            (16, 20), (15, 20), (16, 19), (15, 19), (16, 18), (15, 18),
            (16, 17), (15, 17), (16, 16), (15, 16), (16, 15), (15, 15),
            (16, 14), (15, 14), (16, 13), (15, 13), (16, 12), (15, 12),
            (16, 11), (15, 11), (16, 10), (15, 10), (16, 9), (15, 9),
            (16, 8), (15, 8), (16, 7), (15, 7), (16, 6), (15, 6),
            (16, 5), (15, 5), (16, 4), (15, 4), (16, 3), (15, 3),
            (16, 2), (15, 2), (16, 1), (15, 1), (16, 0), (15, 0),

            (14, 0), (13, 0), (14, 1), (13, 1), (14, 2), (13, 2),
            (14, 3), (13, 3), (14, 4), (13, 4), (14, 5), (13, 5),
            (14, 6), (13, 6), (14, 7), (13, 7), (14, 8), (13, 8),
            (14, 9), (13, 9), (14, 10), (13, 10), (14, 11), (13, 11),
            (14, 12), (13, 12), (14, 13), (13, 13), (14, 14), (13, 14),
            (14, 15), (13, 15), (14, 16), (13, 16), (14, 17), (13, 17),
            (14, 18), (13, 18), (14, 19), (13, 19), (14, 20), (13, 20),

            (12, 20), (11, 20), (12, 19), (11, 19), (12, 18), (11, 18),
            (12, 17), (11, 17), (12, 16), (11, 16), (12, 15), (11, 15),
            (12, 14), (11, 14), (12, 13), (11, 13), (12, 12), (11, 12),
            (12, 11), (11, 11), (12, 10), (11, 10), (12, 9), (11, 9),
            (12, 8), (11, 8), (12, 7), (11, 7), (12, 6), (11, 6),
            (12, 5), (11, 5), (12, 4), (11, 4), (12, 3), (11, 3),
            (12, 2), (11, 2), (12, 1), (11, 1), (12, 0), (11, 0),

            (10, 0), (9, 0), (10, 1), (9, 1), (10, 2), (9, 2),
            (10, 3), (9, 3), (10, 4), (9, 4), (10, 5), (9, 5),
            (10, 6), (9, 6), (10, 7), (9, 7), (10, 8), (9, 8),
            (10, 9), (9, 9), (10, 10), (9, 10), (10, 11), (9, 11),
            (10, 12), (9, 12), (10, 13), (9, 13), (10, 14), (9, 14),
            (10, 15), (9, 15), (10, 16), (9, 16), (10, 17), (9, 17),
            (10, 18), (9, 18), (10, 19), (9, 19), (10, 20), (9, 20),

            (8, 20), (7, 20), (8, 19), (7, 19), (8, 18), (7, 18),
            (8, 17), (7, 17), (8, 16), (7, 16), (8, 15), (7, 15),
            (8, 14), (7, 14), (8, 13), (7, 13), (8, 12), (7, 12),
            (8, 11), (7, 11), (8, 10), (7, 10), (8, 9), (7, 9),
            (8, 8), (7, 8), (8, 7), (7, 7), (8, 6), (7, 6),
            (8, 5), (7, 5), (8, 4), (7, 4), (8, 3), (7, 3),
            (8, 2), (7, 2), (8, 1), (7, 1), (8, 0), (7, 0),

            (5, 0), (4, 0), (5, 1), (4, 1), (5, 2), (4, 2),
            (5, 3), (4, 3), (5, 4), (4, 4), (5, 5), (4, 5),
            (5, 6), (4, 6), (5, 7), (4, 7), (5, 8), (4, 8),
            (5, 9), (4, 9), (5, 10), (4, 10), (5, 11), (4, 11),
            (5, 12), (4, 12), (5, 13), (4, 13), (5, 14), (4, 14),
            (5, 15), (4, 15), (5, 16), (4, 16), (5, 17), (4, 17),
            (5, 18), (4, 18), (5, 19), (4, 19), (5, 20), (4, 20),

            (3, 20), (2, 20), (3, 19), (2, 19), (3, 18), (2, 18),
            (3, 17), (2, 17), (3, 16), (2, 16), (3, 15), (2, 15),
            (3, 14), (2, 14), (3, 13), (2, 13), (3, 12), (2, 12),
            (3, 11), (2, 11), (3, 10), (2, 10), (3, 9), (2, 9),
            (3, 8), (2, 8), (3, 7), (2, 7), (3, 6), (2, 6),
            (3, 5), (2, 5), (3, 4), (2, 4), (3, 3), (2, 3),
            (3, 2), (2, 2), (3, 1), (2, 1), (3, 0), (2, 0),

            (1, 0), (0, 0), (1, 1), (0, 1), (1, 2), (0, 2),
            (1, 3), (0, 3), (1, 4), (0, 4), (1, 5), (0, 5),
            (1, 6), (0, 6), (1, 7), (0, 7), (1, 8), (0, 8),
            (1, 9), (0, 9), (1, 10), (0, 10), (1, 11), (0, 11),
            (1, 12), (0, 12), (1, 13), (0, 13), (1, 14), (0, 14),
            (1, 15), (0, 15), (1, 16), (0, 16), (1, 17), (0, 17),
            (1, 18), (0, 18), (1, 19), (0, 19), (1, 20), (0, 20),
        ]);
    }

    #[test]
    fn test_micro_qr() {
        let res = DataModuleIter::new(Version::micro(1).unwrap()).collect::<Vec<(i16, i16)>>();
        assert_eq!(res, vec![
            (10, 10), (9, 10), (10, 9), (9, 9), (10, 8), (9, 8),
            (10, 7), (9, 7), (10, 6), (9, 6), (10, 5), (9, 5),
            (10, 4), (9, 4), (10, 3), (9, 3), (10, 2), (9, 2),
            (10, 1), (9, 1), (10, 0), (9, 0),

            (8, 0), (7, 0), (8, 1), (7, 1), (8, 2), (7, 2),
            (8, 3), (7, 3), (8, 4), (7, 4), (8, 5), (7, 5),
            (8, 6), (7, 6), (8, 7), (7, 7), (8, 8), (7, 8),
            (8, 9), (7, 9), (8, 10), (7, 10),

            (6, 10), (5, 10), (6, 9), (5, 9), (6, 8), (5, 8),
            (6, 7), (5, 7), (6, 6), (5, 6), (6, 5), (5, 5),
            (6, 4), (5, 4), (6, 3), (5, 3), (6, 2), (5, 2),
            (6, 1), (5, 1), (6, 0), (5, 0),

            (4, 0), (3, 0), (4, 1), (3, 1), (4, 2), (3, 2),
            (4, 3), (3, 3), (4, 4), (3, 4), (4, 5), (3, 5),
            (4, 6), (3, 6), (4, 7), (3, 7), (4, 8), (3, 8),
            (4, 9), (3, 9), (4, 10), (3, 10),

            (2, 10), (1, 10), (2, 9), (1, 9), (2, 8), (1, 8),
            (2, 7), (1, 7), (2, 6), (1, 6), (2, 5), (1, 5),
            (2, 4), (1, 4), (2, 3), (1, 3), (2, 2), (1, 2),
            (2, 1), (1, 1), (2, 0), (1, 0),
        ]);
    }

    #[test]
    fn test_micro_qr_2() {
        let res = DataModuleIter::new(Version::micro(2).unwrap()).collect::<Vec<(i16, i16)>>();
        assert_eq!(res, vec![
            (12, 12), (11, 12), (12, 11), (11, 11), (12, 10), (11, 10),
            (12, 9), (11, 9), (12, 8), (11, 8), (12, 7), (11, 7),
            (12, 6), (11, 6), (12, 5), (11, 5), (12, 4), (11, 4),
            (12, 3), (11, 3), (12, 2), (11, 2), (12, 1), (11, 1),
            (12, 0), (11, 0),

            (10, 0), (9, 0), (10, 1), (9, 1), (10, 2), (9, 2),
            (10, 3), (9, 3), (10, 4), (9, 4), (10, 5), (9, 5),
            (10, 6), (9, 6), (10, 7), (9, 7), (10, 8), (9, 8),
            (10, 9), (9, 9), (10, 10), (9, 10), (10, 11), (9, 11),
            (10, 12), (9, 12),

            (8, 12), (7, 12), (8, 11), (7, 11), (8, 10), (7, 10),
            (8, 9), (7, 9), (8, 8), (7, 8), (8, 7), (7, 7),
            (8, 6), (7, 6), (8, 5), (7, 5), (8, 4), (7, 4),
            (8, 3), (7, 3), (8, 2), (7, 2), (8, 1), (7, 1),
            (8, 0), (7, 0),

            (6, 0), (5, 0), (6, 1), (5, 1), (6, 2), (5, 2),
            (6, 3), (5, 3), (6, 4), (5, 4), (6, 5), (5, 5),
            (6, 6), (5, 6), (6, 7), (5, 7), (6, 8), (5, 8),
            (6, 9), (5, 9), (6, 10), (5, 10), (6, 11), (5, 11),
            (6, 12), (5, 12),

            (4, 12), (3, 12), (4, 11), (3, 11), (4, 10), (3, 10),
            (4, 9), (3, 9), (4, 8), (3, 8), (4, 7), (3, 7),
            (4, 6), (3, 6), (4, 5), (3, 5), (4, 4), (3, 4),
            (4, 3), (3, 3), (4, 2), (3, 2), (4, 1), (3, 1),
            (4, 0), (3, 0),

            (2, 0), (1, 0), (2, 1), (1, 1), (2, 2), (1, 2),
            (2, 3), (1, 3), (2, 4), (1, 4), (2, 5), (1, 5),
            (2, 6), (1, 6), (2, 7), (1, 7), (2, 8), (1, 8),
            (2, 9), (1, 9), (2, 10), (1, 10), (2, 11), (1, 11),
            (2, 12), (1, 12),
        ]);
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Veri yerleşimi

impl Canvas {
    /// İşlevsel desenlerin çizilmiş, veri alanının ise henüz boş olduğunu
    /// doğrular.
    fn validate_ready_for_data(&self) -> QrResult<()> {
        for y in 0..self.width {
            for x in 0..self.width {
                let module = self.get(x, y);
                let valid = if is_functional(self.version, x, y) {
                    matches!(module, Module::Masked(_))
                } else {
                    module == Module::Empty
                };
                if !valid {
                    return Err(QrError::InvalidCanvasState);
                }
            }
        }
        Ok(())
    }

    #[expect(
        clippy::expect_used,
        reason = "uzunluk ve tuval durumu çizimden önce doğrulanır; koordinat tükenmesi iç değişmez ihlalidir"
    )]
    fn draw_codewords<I>(&mut self, codewords: &[u8], is_half_codeword_at_end: bool, coords: &mut I)
    where
        I: Iterator<Item = (i16, i16)>,
    {
        let length = codewords.len();
        let last_word = if is_half_codeword_at_end {
            length.checked_sub(1).expect("tuval iç değişmezi: yarım kod kelimeli sembolün veri bölümü boş olmamalı")
        } else {
            length
        };
        for (i, b) in codewords.iter().enumerate() {
            let bits_end = if i == last_word { 4 } else { 0 };
            for j in (bits_end..=7).rev() {
                let color = if (*b & (1 << j)) == 0 { Color::Light } else { Color::Dark };
                let (x, y) = coords
                    .by_ref()
                    .find(|&(x, y)| self.get(x, y) == Module::Empty)
                    .expect("tuval iç değişmezi: doğrulanmış kod kelimeleri veri alanına sığmalı");
                *self.get_mut(x, y) = Module::Unmasked(color);
            }
        }
    }

    /// Kodlanmış veriyi ve hata düzeltme kodlarını boş modüllere çizer.
    ///
    /// # Errors
    ///
    /// Dilim uzunlukları sürüm ve hata düzeltme seviyesiyle eşleşmiyorsa
    /// `Err(QrError::InvalidDataLength)`, işlevsel desenler henüz çizilmemişse
    /// veya tuvale daha önce veri yerleştirilmişse
    /// `Err(QrError::InvalidCanvasState)` döndürür. Her iki durumda da tuval
    /// değişmeden kalır.
    pub fn draw_data(&mut self, data: &[u8], ec: &[u8]) -> QrResult<()> {
        let (expected_data, expected_ec) = crate::ec::codeword_lengths(self.version, self.ec_level)?;
        if data.len() != expected_data || ec.len() != expected_ec {
            return Err(QrError::InvalidDataLength);
        }
        self.validate_ready_for_data()?;

        // ISO/IEC 18004:2006 Tablo 7 §6.4.10: M1 ve M3 sembolleri 8'in katı olmayan
        // sayıda veri biti (20 ve 84/68) tutar, yani son veri kod kelimeleri yalnızca
        // 4 bit genişliğindedir. Bu, yalnızca `M` için değil, M3'ün *her iki* hata
        // düzeltme seviyesi için geçerlidir.
        let is_half_codeword_at_end = matches!(self.version.kind(), VersionKind::Micro(1 | 3));
        let mut coords = DataModuleIter::new(self.version);
        self.draw_codewords(data, is_half_codeword_at_end, &mut coords);
        self.draw_codewords(ec, false, &mut coords);
        Ok(())
    }
}

#[cfg(test)]
mod draw_codewords_test {
    use crate::canvas::Canvas;
    use crate::types::{EcLevel, Version};

    #[test]
    fn test_micro_qr_1() {
        let mut c = Canvas::new(Version::micro(1).unwrap(), EcLevel::L);
        c.draw_all_functional_patterns();
        c.draw_data(b"\x6e\x5d\xe2", b"\x2b\x63").unwrap();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######.#.#\n\
             #.....#..-*\n\
             #.###.#..**\n\
             #.###.#..*-\n\
             #.###.#..**\n\
             #.....#..*-\n\
             #######..*-\n\
             .........-*\n\
             #........**\n\
             .***-**---*\n\
             #---*-*-**-"
        );
    }

    /// Bir sembol için üretilen kod kelimeleri, yerleştirme yineleyicisinin
    /// ziyaret ettiği modülleri tam olarak doldurmalıdır: bir bit fazlası hata
    /// düzeltme verisinin kuyruğunu sessizce düşürür, bir bit eksiği modülleri
    /// boş bırakır.
    ///
    /// M3-L'nin, ISO/IEC 18004:2006 §6.4.10'un gerektirdiği 4 bitlik kod kelimesi
    /// yerine tam bir sondaki veri kod kelimesiyle çizildiğini yakalayan da budur.
    #[test]
    fn test_codeword_bits_match_module_count() {
        use crate::bits::{Bits, all_versions};
        use crate::canvas::{DataModuleIter, is_functional};
        use crate::ec::construct_codewords;
        use crate::types::VersionKind;

        for version in all_versions() {
            for ec_level in [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H] {
                let mut bits = Bits::new(version);
                let Ok(()) = bits.push_terminator(ec_level) else {
                    continue; // geçersiz sürüm / ec seviyesi bileşimi
                };
                let Ok((data, ec)) = construct_codewords(&bits.into_bytes(), version, ec_level) else {
                    continue;
                };

                let half_codeword = matches!(version.kind(), VersionKind::Micro(1 | 3));
                let bits_count = (data.len() + ec.len()) * 8 - usize::from(half_codeword) * 4;
                let modules_count =
                    DataModuleIter::new(version).filter(|&(x, y)| !is_functional(version, x, y)).count();

                // Artakalanlar, ISO/IEC 18004:2006 §6.7.2, Tablo 1'deki, tasarım gereği
                // kullanılmadan bırakılan "kalan bitler"dir.
                let remainder_bits = match version.kind() {
                    VersionKind::Normal(2..=6) => 7,
                    VersionKind::Normal(14..=20 | 28..=34) => 3,
                    VersionKind::Normal(21..=27) => 4,
                    _ => 0,
                };
                assert_eq!(bits_count + remainder_bits, modules_count, "{version:?} {ec_level:?}");
            }
        }
    }

    #[test]
    fn test_qr_2() {
        let mut c = Canvas::new(Version::normal(2).unwrap(), EcLevel::L);
        c.draw_all_functional_patterns();
        let codewords = &b"\x92I$\x92I$\x92I$\x92I$\x92I$\x92I$\x92I$\x92I$\
                              \x92I$\x92I$\x92I$\x92I$\x92I$\x92I$\x92I$"[..44];
        let (data, ec) = codewords.split_at(34);
        c.draw_data(data, ec).unwrap();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######..--*---*-.#######\n\
             #.....#..-*-*-*-*.#.....#\n\
             #.###.#..*---*---.#.###.#\n\
             #.###.#..--*---*-.#.###.#\n\
             #.###.#..-*-*-*-*.#.###.#\n\
             #.....#..*---*---.#.....#\n\
             #######.#.#.#.#.#.#######\n\
             .........--*---*-........\n\
             ......#..-*-*-*-*........\n\
             --*-*-.-**---*---*--**--*\n\
             -*-*--#----*---*---------\n\
             *----*.*--*-*-*-*-**--**-\n\
             --*-*-#-**---*---*--**--*\n\
             ?*-*--.----*---*---------\n\
             ??---*#*--*-*-*-*-**--**-\n\
             ??*-*-.-**---*---*--**--*\n\
             ??-*--#----*---*#####----\n\
             ........#-*-*-*-#...#-**-\n\
             #######..*---*--#.#.#*--*\n\
             #.....#..--*---*#...#----\n\
             #.###.#..-*-*-*-#####-**-\n\
             #.###.#..*---*--*----*--*\n\
             #.###.#..--*------**-----\n\
             #.....#..-*-*-**-*--*-**-\n\
             #######..*---*--*----*--*"
        );
    }
}
//}}}
//------------------------------------------------------------------------------
//{{{ Maskeleme

/// Maske desenleri. QR kodu ve Micro QR kodu aynı desen numarasını
/// kullanmadığından, onları numaraya göre değil şekle göre adlandırıyoruz.
#[derive(Debug, Copy, Clone)]
pub enum MaskPattern {
    /// QR kodu deseni 000: `(x + y) % 2 == 0`.
    Checkerboard = 0b000,

    /// QR kodu deseni 001: `y % 2 == 0`.
    HorizontalLines = 0b001,

    /// QR kodu deseni 010: `x % 3 == 0`.
    VerticalLines = 0b010,

    /// QR kodu deseni 011: `(x + y) % 3 == 0`.
    DiagonalLines = 0b011,

    /// QR kodu deseni 100: `((x/3) + (y/2)) % 2 == 0`.
    LargeCheckerboard = 0b100,

    /// QR kodu deseni 101: `(x*y)%2 + (x*y)%3 == 0`.
    Fields = 0b101,

    /// QR kodu deseni 110: `((x*y)%2 + (x*y)%3) % 2 == 0`.
    Diamonds = 0b110,

    /// QR kodu deseni 111: `((x+y)%2 + (x*y)%3) % 2 == 0`.
    Meadow = 0b111,
}

mod mask_functions {
    pub const fn checkerboard(x: i16, y: i16) -> bool {
        (x + y) % 2 == 0
    }
    pub const fn horizontal_lines(_: i16, y: i16) -> bool {
        y % 2 == 0
    }
    pub const fn vertical_lines(x: i16, _: i16) -> bool {
        x % 3 == 0
    }
    pub const fn diagonal_lines(x: i16, y: i16) -> bool {
        (x + y) % 3 == 0
    }
    pub const fn large_checkerboard(x: i16, y: i16) -> bool {
        ((y / 2) + (x / 3)) % 2 == 0
    }
    pub const fn fields(x: i16, y: i16) -> bool {
        (x * y) % 2 + (x * y) % 3 == 0
    }
    pub const fn diamonds(x: i16, y: i16) -> bool {
        ((x * y) % 2 + (x * y) % 3) % 2 == 0
    }
    pub const fn meadow(x: i16, y: i16) -> bool {
        ((x + y) % 2 + (x * y) % 3) % 2 == 0
    }
}

fn get_mask_function(pattern: MaskPattern) -> fn(i16, i16) -> bool {
    match pattern {
        MaskPattern::Checkerboard => mask_functions::checkerboard,
        MaskPattern::HorizontalLines => mask_functions::horizontal_lines,
        MaskPattern::VerticalLines => mask_functions::vertical_lines,
        MaskPattern::DiagonalLines => mask_functions::diagonal_lines,
        MaskPattern::LargeCheckerboard => mask_functions::large_checkerboard,
        MaskPattern::Fields => mask_functions::fields,
        MaskPattern::Diamonds => mask_functions::diamonds,
        MaskPattern::Meadow => mask_functions::meadow,
    }
}

impl Canvas {
    /// Tuvale bir maske uygular. Bu metot biçim bilgisi desenlerini de çizer.
    ///
    /// # Panics
    ///
    /// `pattern` bu tuvalin sürümü ve hata düzeltme seviyesi tarafından
    /// desteklenmiyorsa panikler. Bunun yerine hata almak için
    /// [`Canvas::try_apply_mask`] kullanın; ikisi bunun dışında aynıdır.
    pub fn apply_mask(&mut self, pattern: MaskPattern) {
        #[expect(clippy::expect_used, reason = "belgelenmiş panik; kontrollü biçim try_apply_mask")]
        self.try_apply_mask(pattern).expect("bu sürüm ve ec seviyesi için desteklenmeyen maske deseni");
    }

    /// Tuvale bir maske uygular. Bu metot biçim bilgisi desenlerini de çizer.
    ///
    /// # Errors
    ///
    /// `pattern` bir Micro QR kodunun desteklediği dört desenden biri değilse
    /// `Err(QrError::InvalidMaskPattern)`, sürüm ve hata düzeltme seviyesi
    /// standardın tanımladığı bir sembolü adlandırmıyorsa
    /// `Err(QrError::InvalidVersion)` döndürür.
    ///
    /// Her iki kontrol de başarısız olduğunda tuvale dokunulmaz.
    pub fn try_apply_mask(&mut self, pattern: MaskPattern) -> QrResult<()> {
        // Önce biçim bilgisini çöz: reddedilen bir desen tuvali yarı maskelenmiş
        // bırakmamalı.
        let format_number = self.format_info(pattern)?;

        let mask_fn = get_mask_function(pattern);
        for x in 0..self.width {
            for y in 0..self.width {
                let module = self.get_mut(x, y);
                *module = module.mask(mask_fn(x, y));
            }
        }

        self.draw_format_info_patterns_with_number(format_number);
        Ok(())
    }

    /// Hata düzeltme seviyesini ve maske desenini kodlayan biçim bilgisini
    /// hesaplar.
    #[expect(
        clippy::expect_used,
        reason = "kapalı EcLevel/MaskPattern değerleri doğrulandıktan sonra 32 girdilik tabloları tam indeksler"
    )]
    fn format_info(&self, pattern: MaskPattern) -> QrResult<u16> {
        match self.version.kind() {
            VersionKind::Normal(_) => {
                let index = ((self.ec_level as usize) ^ 1) << 3 | (pattern as usize);
                // `ec_level` 0..4 ve `pattern` 0..8, yani `index` 0..32 aralığındadır.
                Ok(*FORMAT_INFOS_QR
                    .get(index)
                    .expect("tuval iç değişmezi: normal biçim bilgisi indeksi tabloda bulunmalı"))
            }
            VersionKind::Micro(a) => {
                let micro_pattern_number = match pattern {
                    MaskPattern::HorizontalLines => 0b00,
                    MaskPattern::LargeCheckerboard => 0b01,
                    MaskPattern::Diamonds => 0b10,
                    MaskPattern::Meadow => 0b11,
                    _ => return Err(QrError::InvalidMaskPattern),
                };
                let symbol_number = match (a, self.ec_level) {
                    (1, EcLevel::L) => 0b000,
                    (2, EcLevel::L) => 0b001,
                    (2, EcLevel::M) => 0b010,
                    (3, EcLevel::L) => 0b011,
                    (3, EcLevel::M) => 0b100,
                    (4, EcLevel::L) => 0b101,
                    (4, EcLevel::M) => 0b110,
                    (4, EcLevel::Q) => 0b111,
                    _ => return Err(QrError::InvalidVersion),
                };
                let index: usize = symbol_number << 2 | micro_pattern_number;
                Ok(*FORMAT_INFOS_MICRO_QR
                    .get(index)
                    .expect("tuval iç değişmezi: Micro biçim bilgisi indeksi tabloda bulunmalı"))
            }
        }
    }
}

#[cfg(test)]
mod mask_tests {
    use crate::canvas::{Canvas, MaskPattern};
    use crate::types::{EcLevel, Version};

    #[test]
    fn test_apply_mask_qr() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);
        c.draw_all_functional_patterns();
        c.apply_mask(MaskPattern::Checkerboard);

        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######...#.#.#######\n\
             #.....#..#.#..#.....#\n\
             #.###.#.#.#.#.#.###.#\n\
             #.###.#..#.#..#.###.#\n\
             #.###.#...#.#.#.###.#\n\
             #.....#..#.#..#.....#\n\
             #######.#.#.#.#######\n\
             ........##.#.........\n\
             ###.#####.#.###...#..\n\
             .#.#.#.#.#.#.#.#.#.#.\n\
             #.#.#.#.#.#.#.#.#.#.#\n\
             .#.#.#.#.#.#.#.#.#.#.\n\
             #.#.#.#.#.#.#.#.#.#.#\n\
             ........##.#.#.#.#.#.\n\
             #######.#.#.#.#.#.#.#\n\
             #.....#.##.#.#.#.#.#.\n\
             #.###.#.#.#.#.#.#.#.#\n\
             #.###.#..#.#.#.#.#.#.\n\
             #.###.#.#.#.#.#.#.#.#\n\
             #.....#.##.#.#.#.#.#.\n\
             #######.#.#.#.#.#.#.#"
        );
    }

    #[test]
    fn test_draw_format_info_patterns_qr() {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::L);
        let n = c.format_info(MaskPattern::LargeCheckerboard).unwrap();
        c.draw_format_info_patterns_with_number(n);
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ????????#????????????\n\
             ????????#????????????\n\
             ????????#????????????\n\
             ????????#????????????\n\
             ????????.????????????\n\
             ????????#????????????\n\
             ?????????????????????\n\
             ????????.????????????\n\
             ##..##?..????..#.####\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ?????????????????????\n\
             ????????#????????????\n\
             ????????.????????????\n\
             ????????#????????????\n\
             ????????#????????????\n\
             ????????.????????????\n\
             ????????.????????????\n\
             ????????#????????????\n\
             ????????#????????????"
        );
    }

    #[test]
    fn test_draw_format_info_patterns_micro_qr() {
        let mut c = Canvas::new(Version::micro(2).unwrap(), EcLevel::L);
        let n = c.format_info(MaskPattern::LargeCheckerboard).unwrap();
        c.draw_format_info_patterns_with_number(n);
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             ?????????????\n\
             ????????#????\n\
             ????????.????\n\
             ????????.????\n\
             ????????#????\n\
             ????????#????\n\
             ????????.????\n\
             ????????.????\n\
             ?#.#....#????\n\
             ?????????????\n\
             ?????????????\n\
             ?????????????\n\
             ?????????????"
        );
    }
}

static FORMAT_INFOS_QR: [u16; 32] = [
    0x5412, 0x5125, 0x5e7c, 0x5b4b, 0x45f9, 0x40ce, 0x4f97, 0x4aa0, 0x77c4, 0x72f3, 0x7daa, 0x789d, 0x662f, 0x6318,
    0x6c41, 0x6976, 0x1689, 0x13be, 0x1ce7, 0x19d0, 0x0762, 0x0255, 0x0d0c, 0x083b, 0x355f, 0x3068, 0x3f31, 0x3a06,
    0x24b4, 0x2183, 0x2eda, 0x2bed,
];

static FORMAT_INFOS_MICRO_QR: [u16; 32] = [
    0x4445, 0x4172, 0x4e2b, 0x4b1c, 0x55ae, 0x5099, 0x5fc0, 0x5af7, 0x6793, 0x62a4, 0x6dfd, 0x68ca, 0x7678, 0x734f,
    0x7c16, 0x7921, 0x06de, 0x03e9, 0x0cb0, 0x0987, 0x1735, 0x1202, 0x1d5b, 0x186c, 0x2508, 0x203f, 0x2f66, 0x2a51,
    0x34e3, 0x31d4, 0x3e8d, 0x3bba,
];

//}}}
//------------------------------------------------------------------------------
//{{{ Ceza puanı

impl Canvas {
    /// Aynı renkte fazla sayıda komşu modül bulunmasının ceza puanını hesaplar.
    ///
    /// Aynı sütun/satırda aynı renge sahip her 5+N komşu modül 3+N puan katar.
    fn compute_adjacent_penalty_score(&self, is_horizontal: bool) -> u32 {
        let mut total_score = 0_u32;

        for i in 0..self.width {
            let map_fn = |j| if is_horizontal { self.get(j, i) } else { self.get(i, j) };

            let colors = (0..self.width).map(map_fn).chain(iter::once(Module::Empty));
            let mut last_color = Module::Empty;
            let mut consecutive_len = 1_u32;

            for color in colors {
                if color == last_color {
                    consecutive_len += 1;
                } else {
                    last_color = color;
                    if consecutive_len >= 5 {
                        total_score += consecutive_len - 2;
                    }
                    consecutive_len = 1;
                }
            }
        }

        total_score
    }

    /// Aynı renkte fazla sayıda dikdörtgen bulunmasının ceza puanını hesaplar.
    ///
    /// Aynı renge sahip her 2×2 blok (üst üste binenler sayılarak) 3 puan katar.
    fn compute_block_penalty_score(&self) -> u32 {
        let mut total_score = 0_u32;

        for i in 0..self.width - 1 {
            for j in 0..self.width - 1 {
                let this = self.get(i, j);
                let right = self.get(i + 1, j);
                let bottom = self.get(i, j + 1);
                let bottom_right = self.get(i + 1, j + 1);
                if this == right && right == bottom && bottom == bottom_right {
                    total_score += 3;
                }
            }
        }

        total_score
    }

    /// Yanlış yerde bulucu desenine benzer bir desen bulunmasının ceza puanını
    /// hesaplar.
    ///
    /// Herhangi bir yönelimde `#.###.#....` gibi görünen her desen 40 puan ekler.
    fn compute_finder_penalty_score(&self, is_horizontal: bool) -> QrResult<u32> {
        static PATTERN: [Color; 7] =
            [Color::Dark, Color::Light, Color::Dark, Color::Dark, Color::Dark, Color::Light, Color::Dark];

        let mut total_score = 0_u32;

        for i in 0..self.width {
            for j in 0..self.width - 6 {
                // TODO: Bir kapanışa referans yeterli olmalı mı?
                let get: Box<dyn Fn(i16) -> Color> = if is_horizontal {
                    Box::new(|k| self.get(k, i).into())
                } else {
                    Box::new(|k| self.get(i, k).into())
                };

                if (j..(j + 7)).map(&*get).ne(PATTERN.iter().copied()) {
                    continue;
                }

                let check = |k| 0 <= k && k < self.width && get(k) != Color::Light;
                if !((j - 4)..j).any(&check) || !((j + 7)..(j + 11)).any(&check) {
                    total_score += 40;
                }
            }
        }

        // Üç gerçek bulucu deseni iki yönde de bu taramaya girer ve toplam 360
        // puan üretir. Daha düşük bir değer, veri yerleşim sırasının atlandığını
        // gösterir; dışarıdan kurulabilen `Canvas` için bu olağan bir API hatasıdır.
        total_score.checked_sub(360).ok_or(QrError::InvalidCanvasState)
    }

    /// Dengesiz koyu/açık oranının ceza puanını hesaplar.
    ///
    /// Puan, koyu modüllerin %50 oranından sapmasıyla doğrusal olarak verilir.
    /// Mümkün olan en yüksek puan 100'dür.
    ///
    /// Bu algoritmanın standarttan biraz farklı olduğuna dikkat edin: sonucu her
    /// %5'te bir yuvarlamıyoruz, ama fark ihmal edilebilir olmalı ve hangi
    /// maskenin seçileceğini etkilememeli.
    fn compute_balance_penalty_score(&self) -> u32 {
        let dark_modules = self.modules.iter().filter(|m| m.is_dark()).count();
        let total_modules = self.modules.len();
        let ratio = dark_modules * 200 / total_modules;
        u32::from(ratio.abs_diff(100).as_u16())
    }

    /// Kenarlarda fazla sayıda açık modül bulunmasının ceza puanını hesaplar.
    ///
    /// Bu ceza puanı Micro QR koduna özgüdür.
    ///
    /// Standardın bu metodun tersi anlama gelen *verimlilik* puanı formülünü
    /// verdiğine dikkat edin, ancak ikisi arasında dönüşüm çok kolaydır (bu puan
    /// (16×width − standart-puan) değeridir).
    fn compute_light_side_penalty_score(&self) -> u32 {
        let h = (1..self.width).filter(|j| !self.get(*j, -1).is_dark()).count();
        let v = (1..self.width).filter(|j| !self.get(-1, *j).is_dark()).count();

        u32::from((h + v + 15 * max(h, v)).as_u16())
    }

    /// Toplam ceza puanlarını hesaplar. Daha yüksek puana sahip bir QR kodu daha
    /// az tercih edilir.
    fn compute_total_penalty_scores(&self) -> QrResult<u32> {
        match self.version.kind() {
            VersionKind::Normal(_) => {
                let s1_a = self.compute_adjacent_penalty_score(true);
                let s1_b = self.compute_adjacent_penalty_score(false);
                let s2 = self.compute_block_penalty_score();
                let s3_a = self.compute_finder_penalty_score(true)?;
                let s3_b = self.compute_finder_penalty_score(false)?;
                let s4 = self.compute_balance_penalty_score();
                Ok(s1_a + s1_b + s2 + s3_a + s3_b + s4)
            }
            VersionKind::Micro(_) => Ok(self.compute_light_side_penalty_score()),
        }
    }
}

#[cfg(test)]
mod penalty_tests {
    use crate::canvas::{Canvas, MaskPattern};
    use crate::cast::As;
    use crate::types::{Color, EcLevel, Version};

    fn create_test_canvas() -> Canvas {
        let mut c = Canvas::new(Version::normal(1).unwrap(), EcLevel::Q);
        c.draw_all_functional_patterns();
        c.draw_data(
            b"\x20\x5b\x0b\x78\xd1\x72\xdc\x4d\x43\x40\xec\x11\x00",
            b"\xa8\x48\x16\x52\xd9\x36\x9c\x00\x2e\x0f\xb4\x7a\x10",
        )
        .unwrap();
        c.apply_mask(MaskPattern::Checkerboard);
        c
    }

    #[test]
    fn check_penalty_canvas() {
        let c = create_test_canvas();
        assert_eq!(
            &*c.to_debug_str(),
            "\n\
             #######.##....#######\n\
             #.....#.#..#..#.....#\n\
             #.###.#.#..##.#.###.#\n\
             #.###.#.#.....#.###.#\n\
             #.###.#.#.#...#.###.#\n\
             #.....#...#...#.....#\n\
             #######.#.#.#.#######\n\
             ........#............\n\
             .##.#.##....#.#.#####\n\
             .#......####....#...#\n\
             ..##.###.##...#.##...\n\
             .##.##.#..##.#.#.###.\n\
             #...#.#.#.###.###.#.#\n\
             ........##.#..#...#.#\n\
             #######.#.#....#.##..\n\
             #.....#..#.##.##.#...\n\
             #.###.#.#.#...#######\n\
             #.###.#..#.#.#.#...#.\n\
             #.###.#.#...####.#..#\n\
             #.....#.#.##.#...#.##\n\
             #######.....####....#"
        );
    }

    #[test]
    fn test_penalty_score_adjacent() {
        let c = create_test_canvas();
        assert_eq!(c.compute_adjacent_penalty_score(true), 88);
        assert_eq!(c.compute_adjacent_penalty_score(false), 92);
    }

    #[test]
    fn test_penalty_score_block() {
        let c = create_test_canvas();
        assert_eq!(c.compute_block_penalty_score(), 90);
    }

    #[test]
    fn test_penalty_score_finder() {
        let c = create_test_canvas();
        assert_eq!(c.compute_finder_penalty_score(true), Ok(0));
        assert_eq!(c.compute_finder_penalty_score(false), Ok(40));
    }

    #[test]
    fn test_penalty_score_balance() {
        let c = create_test_canvas();
        assert_eq!(c.compute_balance_penalty_score(), 2);
    }

    #[test]
    fn test_penalty_score_light_sides() {
        static HORIZONTAL_SIDE: [Color; 17] = [
            Color::Dark,
            Color::Light,
            Color::Light,
            Color::Dark,
            Color::Dark,
            Color::Dark,
            Color::Light,
            Color::Light,
            Color::Dark,
            Color::Light,
            Color::Dark,
            Color::Light,
            Color::Light,
            Color::Dark,
            Color::Light,
            Color::Light,
            Color::Light,
        ];
        static VERTICAL_SIDE: [Color; 17] = [
            Color::Dark,
            Color::Dark,
            Color::Dark,
            Color::Light,
            Color::Light,
            Color::Dark,
            Color::Dark,
            Color::Light,
            Color::Dark,
            Color::Light,
            Color::Dark,
            Color::Light,
            Color::Dark,
            Color::Light,
            Color::Light,
            Color::Dark,
            Color::Light,
        ];

        let mut c = Canvas::new(Version::micro(4).unwrap(), EcLevel::Q);
        for i in 0_i16..17 {
            c.put(i, -1, HORIZONTAL_SIDE[i.as_usize()]);
            c.put(-1, i, VERTICAL_SIDE[i.as_usize()]);
        }

        assert_eq!(c.compute_light_side_penalty_score(), 168);
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ En düşük ceza puanlı maskeyi seçme

static ALL_PATTERNS_QR: [MaskPattern; 8] = [
    MaskPattern::Checkerboard,
    MaskPattern::HorizontalLines,
    MaskPattern::VerticalLines,
    MaskPattern::DiagonalLines,
    MaskPattern::LargeCheckerboard,
    MaskPattern::Fields,
    MaskPattern::Diamonds,
    MaskPattern::Meadow,
];

static ALL_PATTERNS_MICRO_QR: [MaskPattern; 4] =
    [MaskPattern::HorizontalLines, MaskPattern::LargeCheckerboard, MaskPattern::Diamonds, MaskPattern::Meadow];

impl Canvas {
    /// Yeni bir tuval kurar ve en düşük ceza puanını veren en iyi maskelemeyi
    /// uygular.
    ///
    /// # Errors
    ///
    /// Veri eksiksiz çizilmemişse ya da tuvalin yapısı bozulmuşsa
    /// `Err(QrError::InvalidCanvasState)`, sürüm ve hata düzeltme seviyesi
    /// standardın tanımladığı bir sembolü adlandırmıyorsa
    /// `Err(QrError::InvalidVersion)` döndürür.
    pub fn apply_best_mask(&self) -> QrResult<Self> {
        self.validate_ready_for_mask()?;

        let patterns: &[MaskPattern] = match self.version.kind() {
            VersionKind::Normal(_) => &ALL_PATTERNS_QR,
            VersionKind::Micro(_) => &ALL_PATTERNS_MICRO_QR,
        };

        let mut best: Option<(u32, Self)> = None;
        for pattern in patterns {
            let mut candidate = self.clone();
            candidate.try_apply_mask(*pattern)?;
            let score = candidate.compute_total_penalty_scores()?;
            if best.as_ref().is_none_or(|(best_score, _)| score < *best_score) {
                best = Some((score, candidate));
            }
        }
        best.map(|(_, canvas)| canvas).ok_or(QrError::InvalidVersion)
    }

    /// Modülleri bir bool vektörüne dönüştürür.
    #[deprecated(since = "0.4.0", note = "bunun yerine `into_colors()` kullanın")]
    pub fn to_bools(&self) -> Vec<bool> {
        self.modules.iter().map(|m| m.is_dark()).collect()
    }

    /// Modülleri bir renk vektörüne dönüştürür.
    pub fn into_colors(self) -> Vec<Color> {
        self.modules.into_iter().map(Color::from).collect()
    }
}

//}}}
//------------------------------------------------------------------------------
