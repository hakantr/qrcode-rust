//! Bir veri parçasını kodlamak için en uygun veri modu dizisini bulur.
use crate::types::{Mode, Version};
use core::slice::Iter;

#[cfg(feature = "bench")]
extern crate test;

//------------------------------------------------------------------------------
//{{{ Segment

/// Bir kodlama moduna bağlanmış veri parçası.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub struct Segment {
    /// Veri parçasının kodlama modu.
    pub mode: Mode,

    /// Parçanın başlangıç indeksi.
    pub begin: usize,

    /// Parçanın bitiş indeksi (hariç).
    pub end: usize,
}

impl Segment {
    /// Bu parça kodlandığında oluşacak bit sayısını (mod göstergesi ve uzunluk
    /// bitlerinin boyutu dahil) hesaplar.
    pub fn encoded_len(&self, version: Version) -> usize {
        let byte_size = self.end - self.begin;
        let chars_count = if self.mode == Mode::Kanji { byte_size / 2 } else { byte_size };

        let mode_bits_count = version.mode_bits_count();
        let length_bits_count = self.mode.length_bits_count(version);
        let data_bits_count = self.mode.data_bits_count(chars_count);

        mode_bits_count + length_bits_count + data_bits_count
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Parser

/// Bu yineleyici temelde şuna eşdeğerdir:
///
/// ```ignore
/// data.map(|c| ExclCharSet::from_u8(*c))
///     .chain(Some(ExclCharSet::End).move_iter())
///     .enumerate()
/// ```
///
/// Ama tipi yazmak fazla zor, dolayısıyla yeni tip.
struct EcsIter<I> {
    base: I,
    index: usize,
    ended: bool,
}

impl<'a, I: Iterator<Item = &'a u8>> Iterator for EcsIter<I> {
    type Item = (usize, ExclCharSet);

    fn next(&mut self) -> Option<(usize, ExclCharSet)> {
        if self.ended {
            return None;
        }

        match self.base.next() {
            None => {
                self.ended = true;
                Some((self.index, ExclCharSet::End))
            }
            Some(c) => {
                let old_index = self.index;
                self.index += 1;
                Some((old_index, ExclCharSet::from_u8(*c)))
            }
        }
    }
}

/// Girdiyi ayrı parçalara sınıflandıran QR kodu veri ayrıştırıcısı.
pub struct Parser<'a> {
    ecs_iter: EcsIter<Iter<'a, u8>>,
    state: State,
    begin: usize,
    pending_single_byte: bool,
}

impl Parser<'_> {
    /// Veriyi yalnızca kendi ayrık alt kümelerini içeren parçalara ayrıştıran yeni
    /// bir yineleyici oluşturur. Bu aşamada hiçbir optimizasyon yapılmaz.
    ///
    /// ```
    /// use qrcode::optimize::{Parser, Segment};
    /// use qrcode::types::Mode::{Alphanumeric, Byte, Numeric};
    ///
    /// let parse_res = Parser::new(b"ABC123abcd").collect::<Vec<Segment>>();
    /// assert_eq!(
    ///     parse_res,
    ///     &[
    ///         Segment { mode: Alphanumeric, begin: 0, end: 3 },
    ///         Segment { mode: Numeric, begin: 3, end: 6 },
    ///         Segment { mode: Byte, begin: 6, end: 10 }
    ///     ]
    /// );
    /// ```
    pub fn new(data: &[u8]) -> Parser<'_> {
        Parser {
            ecs_iter: EcsIter { base: data.iter(), index: 0, ended: false },
            state: State::Init,
            begin: 0,
            pending_single_byte: false,
        }
    }
}

impl Iterator for Parser<'_> {
    type Item = Segment;

    fn next(&mut self) -> Option<Segment> {
        if self.pending_single_byte {
            self.pending_single_byte = false;
            self.begin += 1;
            return Some(Segment { mode: Mode::Byte, begin: self.begin - 1, end: self.begin });
        }

        loop {
            let (i, ecs) = self.ecs_iter.next()?;
            // `State` 60'a kadar 10'un katıdır ve `ExclCharSet` 0..=9 aralığındadır,
            // yani toplam 70 girdilik tabloyu tam olarak indeksler.
            let (next_state, action) = *STATE_TRANSITION.get(self.state as usize + ecs as usize)?;
            self.state = next_state;

            let old_begin = self.begin;
            let push_mode = match action {
                Action::Idle => continue,
                Action::Numeric => Mode::Numeric,
                Action::Alpha => Mode::Alphanumeric,
                Action::Byte => Mode::Byte,
                Action::Kanji => Mode::Kanji,
                Action::KanjiAndSingleByte => {
                    let next_begin = i - 1;
                    if self.begin == next_begin {
                        Mode::Byte
                    } else {
                        self.pending_single_byte = true;
                        self.begin = next_begin;
                        return Some(Segment { mode: Mode::Kanji, begin: old_begin, end: next_begin });
                    }
                }
            };

            self.begin = i;
            return Some(Segment { mode: push_mode, begin: old_begin, end: i });
        }
    }
}

#[cfg(test)]
mod parse_tests {
    use crate::optimize::{Parser, Segment};
    use crate::types::Mode;
    use alloc::vec::Vec;

    fn parse(data: &[u8]) -> Vec<Segment> {
        Parser::new(data).collect()
    }

    #[test]
    fn test_parse_1() {
        let segs = parse(b"01049123451234591597033130128%10ABC123");
        assert_eq!(
            segs,
            &[
                Segment { mode: Mode::Numeric, begin: 0, end: 29 },
                Segment { mode: Mode::Alphanumeric, begin: 29, end: 30 },
                Segment { mode: Mode::Numeric, begin: 30, end: 32 },
                Segment { mode: Mode::Alphanumeric, begin: 32, end: 35 },
                Segment { mode: Mode::Numeric, begin: 35, end: 38 },
            ]
        );
    }

    #[test]
    fn test_parse_shift_jis_example_1() {
        let segs = parse(b"\x82\xa0\x81\x41\x41\xb1\x81\xf0"); // "あ、AｱÅ"
        assert_eq!(
            segs,
            &[
                Segment { mode: Mode::Kanji, begin: 0, end: 4 },
                Segment { mode: Mode::Alphanumeric, begin: 4, end: 5 },
                Segment { mode: Mode::Byte, begin: 5, end: 6 },
                Segment { mode: Mode::Kanji, begin: 6, end: 8 },
            ]
        );
    }

    #[test]
    fn test_parse_utf_8() {
        // Mojibake?
        let segs = parse(b"\xe3\x81\x82\xe3\x80\x81A\xef\xbd\xb1\xe2\x84\xab");
        assert_eq!(
            segs,
            &[
                Segment { mode: Mode::Kanji, begin: 0, end: 4 },
                Segment { mode: Mode::Byte, begin: 4, end: 5 },
                Segment { mode: Mode::Kanji, begin: 5, end: 7 },
                Segment { mode: Mode::Byte, begin: 7, end: 10 },
                Segment { mode: Mode::Kanji, begin: 10, end: 12 },
                Segment { mode: Mode::Byte, begin: 12, end: 13 },
            ]
        );
    }

    #[test]
    fn test_not_kanji_1() {
        let segs = parse(b"\x81\x30");
        assert_eq!(
            segs,
            &[Segment { mode: Mode::Byte, begin: 0, end: 1 }, Segment { mode: Mode::Numeric, begin: 1, end: 2 },]
        );
    }

    #[test]
    fn test_not_kanji_2() {
        // Bayt dizisinin ikiye bölünmesinin bir implementasyon ayrıntısı olduğuna
        // dikkat edin. Belki test bunu kontrol edecek şekilde ayarlanmalı.
        let segs = parse(b"\xeb\xc0");
        assert_eq!(
            segs,
            &[Segment { mode: Mode::Byte, begin: 0, end: 1 }, Segment { mode: Mode::Byte, begin: 1, end: 2 },]
        );
    }

    #[test]
    fn test_not_kanji_3() {
        let segs = parse(b"\x81\x7f");
        assert_eq!(
            segs,
            &[Segment { mode: Mode::Byte, begin: 0, end: 1 }, Segment { mode: Mode::Byte, begin: 1, end: 2 },]
        );
    }

    #[test]
    fn test_not_kanji_4() {
        let segs = parse(b"\x81\x40\x81");
        assert_eq!(
            segs,
            &[Segment { mode: Mode::Kanji, begin: 0, end: 2 }, Segment { mode: Mode::Byte, begin: 2, end: 3 },]
        );
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Optimizer

/// QR kodu veri optimize edici.
pub struct Optimizer<I> {
    parser: I,
    last_segment: Segment,
    last_segment_size: usize,
    version: Version,
    ended: bool,
}

impl<I: Iterator<Item = Segment>> Optimizer<I> {
    /// Mümkün olduğunda komşu parçaları birleştirerek parçaları optimize eder.
    ///
    /// Şu anda bu metot açgözlü bir algoritma kullanır: yeni parça öncekinden
    /// uzun olana dek parçaları soldan sağa birleştirir. Bu metot ISO
    /// standardındaki Ek J'yi kullan*maz*.
    pub fn new(mut segments: I, version: Version) -> Self {
        match segments.next() {
            None => Self {
                parser: segments,
                last_segment: Segment { mode: Mode::Numeric, begin: 0, end: 0 },
                last_segment_size: 0,
                version,
                ended: true,
            },
            Some(segment) => Self {
                parser: segments,
                last_segment: segment,
                last_segment_size: segment.encoded_len(version),
                version,
                ended: false,
            },
        }
    }
}

impl Parser<'_> {
    /// Bu ayrıştırıcıya dayalı yeni bir `Optimizer` oluşturur.
    pub fn optimize(self, version: Version) -> Optimizer<Self> {
        Optimizer::new(self, version)
    }
}

impl<I: Iterator<Item = Segment>> Iterator for Optimizer<I> {
    type Item = Segment;

    fn next(&mut self) -> Option<Segment> {
        if self.ended {
            return None;
        }

        loop {
            match self.parser.next() {
                None => {
                    self.ended = true;
                    return Some(self.last_segment);
                }
                Some(segment) => {
                    let seg_size = segment.encoded_len(self.version);

                    let new_segment = Segment {
                        mode: self.last_segment.mode.max(segment.mode),
                        begin: self.last_segment.begin,
                        end: segment.end,
                    };
                    let new_size = new_segment.encoded_len(self.version);

                    if self.last_segment_size + seg_size >= new_size {
                        self.last_segment = new_segment;
                        self.last_segment_size = new_size;
                    } else {
                        let old_segment = self.last_segment;
                        self.last_segment = segment;
                        self.last_segment_size = seg_size;
                        return Some(old_segment);
                    }
                }
            }
        }
    }
}

/// Tüm parçaların toplam kodlanmış uzunluğunu hesaplar.
pub fn total_encoded_len(segments: &[Segment], version: Version) -> usize {
    segments.iter().map(|seg| seg.encoded_len(version)).sum()
}

#[cfg(test)]
mod optimize_tests {
    use crate::optimize::{Optimizer, Segment, total_encoded_len};
    use crate::types::{Mode, Version};
    use alloc::vec::Vec;

    fn test_optimization_result(given: &[Segment], expected: &[Segment], version: Version) {
        let prev_len = total_encoded_len(given, version);
        let opt_segs = Optimizer::new(given.iter().copied(), version).collect::<Vec<_>>();
        let new_len = total_encoded_len(&opt_segs, version);
        if given != opt_segs {
            assert!(prev_len > new_len, "{prev_len} > {new_len}");
        }
        assert!(
            opt_segs == expected,
            "Optimization gave something better: {new_len} < {} ({opt_segs:?})",
            total_encoded_len(expected, version)
        );
    }

    #[test]
    fn test_example_1() {
        test_optimization_result(
            &[
                Segment { mode: Mode::Alphanumeric, begin: 0, end: 3 },
                Segment { mode: Mode::Numeric, begin: 3, end: 6 },
                Segment { mode: Mode::Byte, begin: 6, end: 10 },
            ],
            &[Segment { mode: Mode::Alphanumeric, begin: 0, end: 6 }, Segment { mode: Mode::Byte, begin: 6, end: 10 }],
            Version::normal(1).unwrap(),
        );
    }

    #[test]
    fn test_example_2() {
        test_optimization_result(
            &[
                Segment { mode: Mode::Numeric, begin: 0, end: 29 },
                Segment { mode: Mode::Alphanumeric, begin: 29, end: 30 },
                Segment { mode: Mode::Numeric, begin: 30, end: 32 },
                Segment { mode: Mode::Alphanumeric, begin: 32, end: 35 },
                Segment { mode: Mode::Numeric, begin: 35, end: 38 },
            ],
            &[
                Segment { mode: Mode::Numeric, begin: 0, end: 29 },
                Segment { mode: Mode::Alphanumeric, begin: 29, end: 38 },
            ],
            Version::normal(9).unwrap(),
        );
    }

    #[test]
    fn test_example_3() {
        test_optimization_result(
            &[
                Segment { mode: Mode::Kanji, begin: 0, end: 4 },
                Segment { mode: Mode::Alphanumeric, begin: 4, end: 5 },
                Segment { mode: Mode::Byte, begin: 5, end: 6 },
                Segment { mode: Mode::Kanji, begin: 6, end: 8 },
            ],
            &[Segment { mode: Mode::Byte, begin: 0, end: 8 }],
            Version::normal(1).unwrap(),
        );
    }

    #[test]
    fn test_example_4() {
        test_optimization_result(
            &[Segment { mode: Mode::Kanji, begin: 0, end: 10 }, Segment { mode: Mode::Byte, begin: 10, end: 11 }],
            &[Segment { mode: Mode::Kanji, begin: 0, end: 10 }, Segment { mode: Mode::Byte, begin: 10, end: 11 }],
            Version::normal(1).unwrap(),
        );
    }

    #[test]
    fn test_annex_j_guideline_1a() {
        test_optimization_result(
            &[
                Segment { mode: Mode::Numeric, begin: 0, end: 3 },
                Segment { mode: Mode::Alphanumeric, begin: 3, end: 4 },
            ],
            &[
                Segment { mode: Mode::Numeric, begin: 0, end: 3 },
                Segment { mode: Mode::Alphanumeric, begin: 3, end: 4 },
            ],
            Version::micro(2).unwrap(),
        );
    }

    #[test]
    fn test_annex_j_guideline_1b() {
        test_optimization_result(
            &[
                Segment { mode: Mode::Numeric, begin: 0, end: 2 },
                Segment { mode: Mode::Alphanumeric, begin: 2, end: 4 },
            ],
            &[Segment { mode: Mode::Alphanumeric, begin: 0, end: 4 }],
            Version::micro(2).unwrap(),
        );
    }

    #[test]
    fn test_annex_j_guideline_1c() {
        test_optimization_result(
            &[
                Segment { mode: Mode::Numeric, begin: 0, end: 3 },
                Segment { mode: Mode::Alphanumeric, begin: 3, end: 4 },
            ],
            &[Segment { mode: Mode::Alphanumeric, begin: 0, end: 4 }],
            Version::micro(3).unwrap(),
        );
    }
}

#[cfg(feature = "bench")]
#[bench]
fn bench_optimize(bencher: &mut test::Bencher) {
    use crate::types::Version;

    let data = b"QR\x83R\x81[\x83h\x81i\x83L\x83\x85\x81[\x83A\x81[\x83\x8b\x83R\x81[\x83h\x81j\
                 \x82\xc6\x82\xcd\x81A1994\x94N\x82\xc9\x83f\x83\x93\x83\\\x81[\x82\xcc\x8aJ\
                 \x94\xad\x95\x94\x96\xe5\x81i\x8c\xbb\x8d\xdd\x82\xcd\x95\xaa\x97\xa3\x82\xb5\x83f\
                 \x83\x93\x83\\\x81[\x83E\x83F\x81[\x83u\x81j\x82\xaa\x8aJ\x94\xad\x82\xb5\x82\xbd\
                 \x83}\x83g\x83\x8a\x83b\x83N\x83X\x8c^\x93\xf1\x8e\x9f\x8c\xb3\x83R\x81[\x83h\
                 \x82\xc5\x82\xa0\x82\xe9\x81B\x82\xc8\x82\xa8\x81AQR\x83R\x81[\x83h\x82\xc6\
                 \x82\xa2\x82\xa4\x96\xbc\x8f\xcc\x81i\x82\xa8\x82\xe6\x82\xd1\x92P\x8c\xea\x81j\
                 \x82\xcd\x83f\x83\x93\x83\\\x81[\x83E\x83F\x81[\x83u\x82\xcc\x93o\x98^\x8f\xa4\
                 \x95W\x81i\x91\xe64075066\x8d\x86\x81j\x82\xc5\x82\xa0\x82\xe9\x81BQR\x82\xcd\
                 Quick Response\x82\xc9\x97R\x97\x88\x82\xb5\x81A\x8d\x82\x91\xac\x93\xc7\x82\xdd\
                 \x8e\xe6\x82\xe8\x82\xaa\x82\xc5\x82\xab\x82\xe9\x82\xe6\x82\xa4\x82\xc9\x8aJ\
                 \x94\xad\x82\xb3\x82\xea\x82\xbd\x81B\x93\x96\x8f\x89\x82\xcd\x8e\xa9\x93\xae\
                 \x8e\xd4\x95\x94\x95i\x8dH\x8f\xea\x82\xe2\x94z\x91\x97\x83Z\x83\x93\x83^\x81[\
                 \x82\xc8\x82\xc7\x82\xc5\x82\xcc\x8eg\x97p\x82\xf0\x94O\x93\xaa\x82\xc9\x8aJ\
                 \x94\xad\x82\xb3\x82\xea\x82\xbd\x82\xaa\x81A\x8c\xbb\x8d\xdd\x82\xc5\x82\xcd\x83X\
                 \x83}\x81[\x83g\x83t\x83H\x83\x93\x82\xcc\x95\x81\x8by\x82\xc8\x82\xc7\x82\xc9\
                 \x82\xe6\x82\xe8\x93\xfa\x96{\x82\xc9\x8c\xc0\x82\xe7\x82\xb8\x90\xa2\x8aE\x93I\
                 \x82\xc9\x95\x81\x8by\x82\xb5\x82\xc4\x82\xa2\x82\xe9\x81B";
    bencher.iter(|| Parser::new(data).optimize(Version::normal(15).unwrap()));
}

//}}}
//------------------------------------------------------------------------------
//{{{ Internal types and data for parsing

/// Hangi kodlamanın kullanılacağı belirlenirken tüm `u8` değerleri 9 farklı
/// karakter kümesine ayrılabilir. Bu enum, ayrıştırma amacıyla bu grupları
/// temsil eder.
#[derive(Copy, Clone)]
enum ExclCharSet {
    /// Dizginin sonu.
    End = 0,

    /// Alphanumeric kodlamasının desteklediği tüm semboller; yani boşluk, `$`,
    /// `%`, `*`, `+`, `-`, `.`, `/` ve `:`.
    Symbol = 1,

    /// Tüm rakamlar (0–9).
    Numeric = 2,

    /// Tüm büyük harfler (A–Z). Bu karakterler bir Shift JIS 2 baytlık
    /// kodlamasının ikinci baytında da görünebilir.
    Alpha = 3,

    /// Bir Shift JIS 2 baytlık kodlamasının 0x81–0x9f aralığındaki ilk baytı.
    KanjiHi1 = 4,

    /// Bir Shift JIS 2 baytlık kodlamasının 0xe0–0xea aralığındaki ilk baytı.
    KanjiHi2 = 5,

    /// Bir Shift JIS 2 baytlık kodlamasının 0xeb değerindeki ilk baytı. Bu,
    /// ikinci baytın daha dar bir aralığa sahip olması bakımından diğer iki
    /// aralıktan farklıdır.
    KanjiHi3 = 6,

    /// Bir Shift JIS 2 baytlık kodlamasının 0x40–0xbf aralığındaki ikinci baytı;
    /// harfler (`Alpha` kapsıyor), 0x81–0x9f (`KanjiHi1` kapsıyor) ve geçersiz
    /// bayt 0x7f hariç.
    KanjiLo1 = 7,

    /// Bir Shift JIS 2 baytlık kodlamasının 0xc0–0xfc aralığındaki ikinci baytı;
    /// 0xe0–0xeb aralığı (`KanjiHi2` ve `KanjiHi3` kapsıyor) hariç. Bayt
    /// çiftinin bu yarısı `KanjiHi3` tarafından öncelenen ikinci bayt olarak
    /// görünemez.
    KanjiLo2 = 8,

    /// Yukarıdaki karakter kümelerinin kapsamadığı diğer tüm değerler.
    Byte = 9,
}

impl ExclCharSet {
    /// Bir baytın hangi karakter kümesinde olduğunu belirler.
    const fn from_u8(c: u8) -> Self {
        match c {
            0x20 | 0x24 | 0x25 | 0x2a | 0x2b | 0x2d..=0x2f | 0x3a => Self::Symbol,
            0x30..=0x39 => Self::Numeric,
            0x41..=0x5a => Self::Alpha,
            0x81..=0x9f => Self::KanjiHi1,
            0xe0..=0xea => Self::KanjiHi2,
            0xeb => Self::KanjiHi3,
            0x40 | 0x5b..=0x7e | 0x80 | 0xa0..=0xbf => Self::KanjiLo1,
            0xc0..=0xdf | 0xec..=0xfc => Self::KanjiLo2,
            _ => Self::Byte,
        }
    }
}

/// Geçerli ayrıştırma durumu.
#[derive(Copy, Clone)]
enum State {
    /// Yeni başlatıldı.
    Init = 0,

    /// Yalnızca Numeric olarak kodlanabilecek bir dizginin içinde.
    Numeric = 10,

    /// Yalnızca Alphanumeric olarak kodlanabilecek bir dizginin içinde.
    Alpha = 20,

    /// Yalnızca 8-Bit Byte olarak kodlanabilecek bir dizginin içinde.
    Byte = 30,

    /// `KanjiHi1` ya da `KanjiHi2` kümesinden bir Shift JIS 2 baytlık dizisinin
    /// ilk baytıyla yeni karşılaşıldı.
    KanjiHi12 = 40,

    /// `KanjiHi3` kümesinden bir Shift JIS 2 baytlık dizisinin ilk baytıyla yeni
    /// karşılaşıldı.
    KanjiHi3 = 50,

    /// Yalnızca Kanji olarak kodlanabilecek bir dizginin içinde.
    Kanji = 60,
}

/// Bir durum geçişinden sonra ayrıştırıcının ne yapması gerektiği.
#[derive(Copy, Clone)]
enum Action {
    /// Ayrıştırıcı hiçbir şey yapmamalı.
    Idle,

    /// Geçerli parçayı Numeric dizgi olarak ekle ve işaretleri sıfırla.
    Numeric,

    /// Geçerli parçayı Alphanumeric dizgi olarak ekle ve işaretleri sıfırla.
    Alpha,

    /// Geçerli parçayı 8-Bit Byte dizgi olarak ekle ve işaretleri sıfırla.
    Byte,

    /// Geçerli parçayı Kanji dizgi olarak ekle ve işaretleri sıfırla.
    Kanji,

    /// Son bayt hariç geçerli parçayı Kanji dizgi olarak ekle, ardından kalan tek
    /// baytı Byte dizgi olarak ekle ve işaretleri sıfırla.
    KanjiAndSingleByte,
}

static STATE_TRANSITION: [(State, Action); 70] = [
    // STATE_TRANSITION[current_state + next_character] == (next_state, what_to_do)

    // Init durumu:
    (State::Init, Action::Idle),      // End
    (State::Alpha, Action::Idle),     // Symbol
    (State::Numeric, Action::Idle),   // Numeric
    (State::Alpha, Action::Idle),     // Alpha
    (State::KanjiHi12, Action::Idle), // KanjiHi1
    (State::KanjiHi12, Action::Idle), // KanjiHi2
    (State::KanjiHi3, Action::Idle),  // KanjiHi3
    (State::Byte, Action::Idle),      // KanjiLo1
    (State::Byte, Action::Idle),      // KanjiLo2
    (State::Byte, Action::Idle),      // Byte
    // Numeric durumu:
    (State::Init, Action::Numeric),      // End
    (State::Alpha, Action::Numeric),     // Symbol
    (State::Numeric, Action::Idle),      // Numeric
    (State::Alpha, Action::Numeric),     // Alpha
    (State::KanjiHi12, Action::Numeric), // KanjiHi1
    (State::KanjiHi12, Action::Numeric), // KanjiHi2
    (State::KanjiHi3, Action::Numeric),  // KanjiHi3
    (State::Byte, Action::Numeric),      // KanjiLo1
    (State::Byte, Action::Numeric),      // KanjiLo2
    (State::Byte, Action::Numeric),      // Byte
    // Alpha durumu:
    (State::Init, Action::Alpha),      // End
    (State::Alpha, Action::Idle),      // Symbol
    (State::Numeric, Action::Alpha),   // Numeric
    (State::Alpha, Action::Idle),      // Alpha
    (State::KanjiHi12, Action::Alpha), // KanjiHi1
    (State::KanjiHi12, Action::Alpha), // KanjiHi2
    (State::KanjiHi3, Action::Alpha),  // KanjiHi3
    (State::Byte, Action::Alpha),      // KanjiLo1
    (State::Byte, Action::Alpha),      // KanjiLo2
    (State::Byte, Action::Alpha),      // Byte
    // Byte durumu:
    (State::Init, Action::Byte),      // End
    (State::Alpha, Action::Byte),     // Symbol
    (State::Numeric, Action::Byte),   // Numeric
    (State::Alpha, Action::Byte),     // Alpha
    (State::KanjiHi12, Action::Byte), // KanjiHi1
    (State::KanjiHi12, Action::Byte), // KanjiHi2
    (State::KanjiHi3, Action::Byte),  // KanjiHi3
    (State::Byte, Action::Idle),      // KanjiLo1
    (State::Byte, Action::Idle),      // KanjiLo2
    (State::Byte, Action::Idle),      // Byte
    // KanjiHi12 durumu:
    (State::Init, Action::KanjiAndSingleByte),    // End
    (State::Alpha, Action::KanjiAndSingleByte),   // Symbol
    (State::Numeric, Action::KanjiAndSingleByte), // Numeric
    (State::Kanji, Action::Idle),                 // Alpha
    (State::Kanji, Action::Idle),                 // KanjiHi1
    (State::Kanji, Action::Idle),                 // KanjiHi2
    (State::Kanji, Action::Idle),                 // KanjiHi3
    (State::Kanji, Action::Idle),                 // KanjiLo1
    (State::Kanji, Action::Idle),                 // KanjiLo2
    (State::Byte, Action::KanjiAndSingleByte),    // Byte
    // KanjiHi3 durumu:
    (State::Init, Action::KanjiAndSingleByte),      // End
    (State::Alpha, Action::KanjiAndSingleByte),     // Symbol
    (State::Numeric, Action::KanjiAndSingleByte),   // Numeric
    (State::Kanji, Action::Idle),                   // Alpha
    (State::Kanji, Action::Idle),                   // KanjiHi1
    (State::KanjiHi12, Action::KanjiAndSingleByte), // KanjiHi2
    (State::KanjiHi3, Action::KanjiAndSingleByte),  // KanjiHi3
    (State::Kanji, Action::Idle),                   // KanjiLo1
    (State::Byte, Action::KanjiAndSingleByte),      // KanjiLo2
    (State::Byte, Action::KanjiAndSingleByte),      // Byte
    // Kanji durumu:
    (State::Init, Action::Kanji),     // End
    (State::Alpha, Action::Kanji),    // Symbol
    (State::Numeric, Action::Kanji),  // Numeric
    (State::Alpha, Action::Kanji),    // Alpha
    (State::KanjiHi12, Action::Idle), // KanjiHi1
    (State::KanjiHi12, Action::Idle), // KanjiHi2
    (State::KanjiHi3, Action::Idle),  // KanjiHi3
    (State::Byte, Action::Kanji),     // KanjiLo1
    (State::Byte, Action::Kanji),     // KanjiLo2
    (State::Byte, Action::Kanji),     // Byte
];

//}}}
