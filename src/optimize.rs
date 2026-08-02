//! Bir veri parçasını kodlamak için en uygun veri modu dizisini bulur.
use crate::types::{Mode, QrError, QrResult, Version};
use alloc::vec::Vec;
use core::slice::Iter;

/// Standart bir QR kodunun alabileceği en uzun ham girdi.
///
/// Bu üst sınır, sürüm 40-L sembolünde sayısal modda kodlanabilen 7089
/// rakamdan gelir. Başka hiçbir mod daha uzun bir ham bayt dizisi taşıyamaz.
pub const MAX_INPUT_BYTES: usize = 7089;

//------------------------------------------------------------------------------
//{{{ Veri parçası

/// Bir kodlama moduna bağlanmış veri parçası.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub struct Segment {
    /// Veri parçasının kodlama modu.
    pub(crate) mode: Mode,

    /// Parçanın başlangıç indeksi.
    pub(crate) begin: usize,

    /// Parçanın bitiş indeksi (hariç).
    pub(crate) end: usize,
}

impl Segment {
    /// Doğrulanmış bir veri parçası kurar.
    ///
    /// # Errors
    ///
    /// `begin`, `end` değerinden büyükse veya `end` QR standardının izin
    /// verdiği en uzun girdiyi aşıyorsa `Err(QrError::InvalidSegment)` döndürür.
    pub const fn new(mode: Mode, begin: usize, end: usize) -> QrResult<Self> {
        if begin > end || end > MAX_INPUT_BYTES { Err(QrError::InvalidSegment) } else { Ok(Self { mode, begin, end }) }
    }

    /// Veri parçasının kodlama modu.
    pub const fn mode(self) -> Mode {
        self.mode
    }

    /// Parçanın başlangıç indeksi.
    pub const fn begin(self) -> usize {
        self.begin
    }

    /// Parçanın bitiş indeksi (hariç).
    pub const fn end(self) -> usize {
        self.end
    }

    /// Bu parça kodlandığında oluşacak bit sayısını (mod göstergesi ve uzunluk
    /// bitlerinin boyutu dahil) hesaplar.
    ///
    /// # Panics
    ///
    /// Yalnızca özel alanları kuran [`Segment::new`] ile bit uzunluğu
    /// hesaplamasının sınırları birbiriyle çelişirse panikler.
    #[expect(clippy::expect_used, reason = "Segment kurucusu uzunluğu QR standardının küçük üst sınırına hapseder")]
    pub fn encoded_len(&self, version: Version) -> usize {
        let byte_size = self.end - self.begin;
        let chars_count = if self.mode == Mode::Kanji { byte_size / 2 } else { byte_size };

        let mode_bits_count = version.mode_bits_count();
        let length_bits_count = self.mode.length_bits_count(version);
        let data_bits_count = self
            .mode
            .data_bits_count(chars_count)
            .expect("Segment iç değişmezi: doğrulanmış uzunluğun bit sayısı taşmamalı");

        mode_bits_count + length_bits_count + data_bits_count
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Ayrıştırıcı

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
    /// let parse_res = Parser::new(b"ABC123abcd").unwrap().collect::<Vec<Segment>>();
    /// assert_eq!(
    ///     parse_res,
    ///     &[
    ///         Segment::new(Alphanumeric, 0, 3).unwrap(),
    ///         Segment::new(Numeric, 3, 6).unwrap(),
    ///         Segment::new(Byte, 6, 10).unwrap(),
    ///     ]
    /// );
    /// ```
    ///
    /// # Errors
    ///
    /// Veri herhangi bir standart QR koduna sığamayacak kadar uzunsa
    /// `Err(QrError::DataTooLong)` döndürür.
    pub fn new(data: &[u8]) -> QrResult<Parser<'_>> {
        if data.len() > MAX_INPUT_BYTES {
            return Err(QrError::DataTooLong);
        }
        Ok(Parser {
            ecs_iter: EcsIter { base: data.iter(), index: 0, ended: false },
            state: State::Init,
            begin: 0,
            pending_single_byte: false,
        })
    }
}

impl Iterator for Parser<'_> {
    type Item = Segment;

    #[expect(
        clippy::expect_used,
        reason = "kapalı State ve ExclCharSet enumları toplam 70 girdilik tabloyu tam indeksler"
    )]
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
            let (next_state, action) = *STATE_TRANSITION
                .get(self.state as usize + ecs as usize)
                .expect("ayrıştırıcı iç değişmezi: durum geçişi tabloda bulunmalı");
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
        Parser::new(data).unwrap().collect()
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
        // Kodlama bozulması mı?
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
//{{{ İyileştirici

/// QR kodu veri optimize edici.
///
/// Ayrıştırıcının ürettiği komşu parçaları, toplam kodlanmış bit uzunluğunu
/// en aza indiren bölümlemeye dinamik programlamayla yeniden ayırır. Parça
/// sınırları çalışma (aynı karakter sınıfından ardışık dizi) sınırları
/// üzerinde seçilir; ISO/IEC 18004:2024 Ek J'nin eşik kuralları bu optimumun
/// yaklaşık bir sezgiselidir, burada optimum doğrudan hesaplanır. Komşu
/// olmayan parça zincirleri birbirinden bağımsız optimize edilir.
pub struct Optimizer {
    segments: alloc::vec::IntoIter<Segment>,
}

impl Optimizer {
    /// Verilen parça dizisini, toplam bit uzunluğu en düşük olacak biçimde
    /// yeniden bölümler.
    pub fn new<I: Iterator<Item = Segment>>(segments: I, version: Version) -> Self {
        let mut result = Vec::new();
        let mut chain: Vec<Segment> = Vec::new();
        for segment in segments {
            // Kamu kurucusu tek tek parçaları doğrular; farklı veri
            // aralıklarına ait, komşu olmayan parçaları birleştirmek ise
            // anlamlı değildir. Zinciri burada keseriz.
            if chain.last().is_some_and(|last| last.end != segment.begin) {
                optimize_chain(&chain, version, &mut result);
                chain.clear();
            }
            chain.push(segment);
        }
        optimize_chain(&chain, version, &mut result);
        Self { segments: result.into_iter() }
    }
}

/// Komşu `runs` parçalarını en düşük toplam bit uzunluğunu veren bölümlemeyle
/// `out` içine yazar.
///
/// `best[i]`, ilk `i` çalışmanın en düşük toplam uzunluğu; `cut[i]` ise bu
/// çözümde son parçanın başladığı çalışma indeksidir. Son parçanın adayları
/// `j..i` aralıkları olduğundan karmaşıklık O(n²)'dir; `j` küçüldükçe aday
/// parçanın uzunluğu tekdüze arttığından, tek başına `best[i]`'yi aşan aday
/// bulunduğunda arama erkenden kesilir.
fn optimize_chain(runs: &[Segment], version: Version, out: &mut Vec<Segment>) {
    if runs.len() <= 1 {
        out.extend_from_slice(runs);
        return;
    }

    // `best` ve `cut` yalnızca bu döngüde, çalışma başına birer giriş itilerek
    // büyür; `i` girişleri ilk `i` çalışmayı kapsar.
    let mut best: Vec<usize> = Vec::with_capacity(runs.len() + 1);
    let mut cut: Vec<usize> = Vec::with_capacity(runs.len() + 1);
    best.push(0);
    cut.push(0);
    for last_run in runs {
        let mut mode = last_run.mode;
        let mut best_total = usize::MAX;
        let mut best_begin = 0;
        // Aday son parça, `begin` çalışmasından `last_run`'a kadar uzanır;
        // `zip` eşlemesi `begin`'i o âna dek hesaplanmış `best` girişleriyle
        // sınırlar ve `begin` sondan başa yürür. Aday uzunluğu `begin`
        // küçüldükçe tekdüze arttığından, tek başına eldeki en iyi toplamı
        // aşan aday görüldüğünde arama kesilir.
        for (begin, (run, &prefix_total)) in runs.iter().zip(&best).enumerate().rev() {
            mode = mode.max(run.mode);
            let candidate = Segment { mode, begin: run.begin, end: last_run.end };
            let candidate_len = candidate.encoded_len(version);
            if best_total <= candidate_len {
                break;
            }
            if let Some(total) = prefix_total.checked_add(candidate_len)
                && total < best_total
            {
                best_total = total;
                best_begin = begin;
            }
        }
        best.push(best_total);
        cut.push(best_begin);
    }

    // Bölüm sınırlarını geriden öne çıkar.
    let mut bounds = Vec::new();
    let mut index = runs.len();
    while index > 0 {
        bounds.push(index);
        #[expect(
            clippy::expect_used,
            reason = "cut, çalışma başına bir giriş itilerek kuruldu; index her zaman tablo içinde kalır"
        )]
        {
            index = *cut.get(index).expect("Optimizer iç değişmezi: kesim indeksi tablonun içinde kalmalı");
        }
    }

    // Çalışmaları soldan sağa biriktirip her bölüm sınırında tek parça yaz.
    let mut ends = bounds.iter().rev().copied().peekable();
    let mut accumulated: Option<Segment> = None;
    for (index, run) in runs.iter().enumerate() {
        accumulated = Some(match accumulated.take() {
            None => *run,
            Some(seg) => Segment { mode: seg.mode.max(run.mode), begin: seg.begin, end: run.end },
        });
        if ends.peek() == Some(&(index + 1))
            && let Some(seg) = accumulated.take()
        {
            out.push(seg);
            ends.next();
        }
    }
}

impl Parser<'_> {
    /// Bu ayrıştırıcıya dayalı yeni bir `Optimizer` oluşturur.
    pub fn optimize(self, version: Version) -> Optimizer {
        Optimizer::new(self, version)
    }
}

impl Iterator for Optimizer {
    type Item = Segment;

    fn next(&mut self) -> Option<Segment> {
        self.segments.next()
    }
}

/// Tüm parçaların toplam kodlanmış uzunluğunu hesaplar.
///
/// # Errors
///
/// Toplam bir `usize` içine sığmıyorsa `Err(QrError::DataTooLong)` döndürür.
pub fn total_encoded_len(segments: &[Segment], version: Version) -> QrResult<usize> {
    segments
        .iter()
        .try_fold(0_usize, |total, seg| total.checked_add(seg.encoded_len(version)).ok_or(QrError::DataTooLong))
}

#[cfg(test)]
mod optimize_tests {
    use crate::optimize::{Optimizer, Segment, total_encoded_len};
    use crate::types::{Mode, Version};
    use alloc::vec::Vec;

    fn test_optimization_result(given: &[Segment], expected: &[Segment], version: Version) {
        let prev_len = total_encoded_len(given, version).unwrap();
        let opt_segs = Optimizer::new(given.iter().copied(), version).collect::<Vec<_>>();
        let new_len = total_encoded_len(&opt_segs, version).unwrap();
        if given != opt_segs {
            assert!(prev_len > new_len, "{prev_len} > {new_len}");
        }
        assert!(
            opt_segs == expected,
            "optimizasyon beklenenden daha iyi sonuç verdi: {new_len} < {} ({opt_segs:?})",
            total_encoded_len(expected, version).unwrap()
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

    /// ISO/IEC 18004:2024 Ek J.2 c)3'ün eşiği: alfasayısal verinin ortasındaki
    /// bir sayısal dizi, 1-9 sürüm grubunda ancak 13 karakterden itibaren ayrı
    /// bir parçaya değer. 12 rakamda tek parça (123 bit), 13 rakamda üçlü
    /// bölümleme (128 bit) en iyisidir. Eski ikili-açgözlü birleştirici 12
    /// rakamlık durumda üçlü birleşmeyi göremeyip 124 bitte kalıyordu.
    #[test]
    fn test_annex_j_numeric_run_threshold() {
        let version = Version::normal(1).unwrap();
        test_optimization_result(
            &[
                Segment { mode: Mode::Alphanumeric, begin: 0, end: 4 },
                Segment { mode: Mode::Numeric, begin: 4, end: 16 },
                Segment { mode: Mode::Alphanumeric, begin: 16, end: 20 },
            ],
            &[Segment { mode: Mode::Alphanumeric, begin: 0, end: 20 }],
            version,
        );
        test_optimization_result(
            &[
                Segment { mode: Mode::Alphanumeric, begin: 0, end: 4 },
                Segment { mode: Mode::Numeric, begin: 4, end: 17 },
                Segment { mode: Mode::Alphanumeric, begin: 17, end: 21 },
            ],
            &[
                Segment { mode: Mode::Alphanumeric, begin: 0, end: 4 },
                Segment { mode: Mode::Numeric, begin: 4, end: 17 },
                Segment { mode: Mode::Alphanumeric, begin: 17, end: 21 },
            ],
            version,
        );
    }

    /// ISO/IEC 18004:2024 Ek J.3.2'nin işlenmiş örneği: "123456ABCDEFGH"
    /// verisi (6 sayısal + 8 alfasayısal), mod ve karakter sayısı göstergeleri
    /// dahil M3'te toplam 77, M4'te 81 bit tutar.
    #[test]
    fn test_annex_j_micro_capacity_example() {
        use crate::optimize::Parser;
        for (version, expected_bits) in [(Version::micro(3).unwrap(), 77), (Version::micro(4).unwrap(), 81)] {
            let segments = Parser::new(b"123456ABCDEFGH").unwrap().optimize(version).collect::<Vec<_>>();
            assert_eq!(total_encoded_len(&segments, version), Ok(expected_bits));
        }
    }

    /// Optimizer'ın dinamik programlaması, çalışma sınırları üzerindeki tüm
    /// 2^(n-1) bölümlemeyi tarayan kaba kuvvetin bulduğu optimumla her durumda
    /// aynı toplam uzunluğu vermelidir.
    #[test]
    fn test_dp_matches_brute_force() {
        let modes = [Mode::Numeric, Mode::Alphanumeric, Mode::Byte, Mode::Kanji];
        let versions = [
            Version::normal(1).unwrap(),
            Version::normal(10).unwrap(),
            Version::normal(40).unwrap(),
            Version::micro(3).unwrap(),
            Version::micro(4).unwrap(),
        ];
        // Deterministik LCG; testin tekrarlanabilir olması için sabit tohum.
        let mut seed = 0x2024_u32;
        let mut next = move |bound: usize| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            usize::try_from(seed >> 16).unwrap() % bound
        };

        for case in 0..300 {
            let run_count = next(6) + 1;
            let mut runs = Vec::new();
            let mut begin = 0_usize;
            for _ in 0..run_count {
                let mode = modes[next(modes.len())];
                let mut len = next(13) + 1;
                if mode == Mode::Kanji {
                    len = len.div_ceil(2) * 2; // Kanji çalışmaları çift bayt olmalı
                }
                runs.push(Segment { mode, begin, end: begin + len });
                begin += len;
            }
            let version = versions[next(versions.len())];

            let mut brute_force_best = usize::MAX;
            for cut_mask in 0_u32..(1 << (run_count - 1)) {
                let mut total = 0_usize;
                let mut start = 0_usize;
                for i in 0..run_count {
                    if i + 1 == run_count || cut_mask & (1 << i) != 0 {
                        let mode = runs[start..=i].iter().fold(runs[start].mode, |m, r| m.max(r.mode));
                        let seg = Segment { mode, begin: runs[start].begin, end: runs[i].end };
                        total += seg.encoded_len(version);
                        start = i + 1;
                    }
                }
                brute_force_best = brute_force_best.min(total);
            }

            let opt_segs = Optimizer::new(runs.iter().copied(), version).collect::<Vec<_>>();
            let dp_total = total_encoded_len(&opt_segs, version).unwrap();
            assert_eq!(dp_total, brute_force_best, "durum {case}: {runs:?} @ {version:?}");
        }
    }
}

//}}}
//------------------------------------------------------------------------------
//{{{ Ayrıştırma için iç tipler ve veriler

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
    // STATE_TRANSITION[geçerli_durum + sonraki_karakter] == (sonraki_durum, yapılacak_iş)

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
