//! Hiçbir genel giriş noktasının süreci abort etmediğini doğrulayan kapsamlı
//! tarama.
//!
//! Crate'in sözleşmesi şudur: *geçerli* bir girdi asla paniklemez, geçersiz
//! olan ise [`QrError`] olarak geri döner. Bu sessizce bozulması kolay bir
//! şeydir; bu yüzden bu test tüm bileşim uzayını -- her sürüm, her hata
//! düzeltme seviyesi, her maske deseni, her kenarın üzerindeki ve ötesindeki
//! koordinatlar -- dolaşır ve yanlış bir cevapta değil, ilk panikte başarısız
//! olur.
//!
//! Bir entegrasyon testi olarak çalışır, yani yalnızca genel API'yi görür;
//! sözleşmenin kapsadığı yüzey de tam olarak budur. Belgelenmiş kontrollü bir
//! karşılığı olan panikleyen metotlar (`Canvas::get`, `Renderer::new`,
//! `QrCode::is_functional`, indeksleme) burada bilinçli olarak kontrollü
//! biçimleri üzerinden kullanılır.

// Bu test panikleyerek doğrulama yapar; crate'in kendi panik kapısının geri
// çekilmek zorunda olduğu tek yer burasıdır.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::match_wild_err_arm,
    clippy::indexing_slicing,
    reason = "test hataları panikleyerek bildirir"
)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use qrcode::canvas::{Canvas, MaskPattern};
use qrcode::render::Renderer;
use qrcode::types::Color;
use qrcode::{EcLevel, QrCode, Version};

const EC_LEVELS: [EcLevel; 4] = [EcLevel::L, EcLevel::M, EcLevel::Q, EcLevel::H];

const MASK_PATTERNS: [MaskPattern; 8] = [
    MaskPattern::Checkerboard,
    MaskPattern::HorizontalLines,
    MaskPattern::VerticalLines,
    MaskPattern::DiagonalLines,
    MaskPattern::LargeCheckerboard,
    MaskPattern::Fields,
    MaskPattern::Diamonds,
    MaskPattern::Meadow,
];

/// Standardın tanımladığı her sürüm; ayrıca kurucuların da denenmesi için her
/// ailenin aralığının hemen dışındaki sayılar.
fn all_versions() -> Vec<Version> {
    let normal = (1..=40).filter_map(|n| Version::normal(n).ok());
    normal.chain((1..=4).filter_map(|n| Version::micro(n).ok())).collect()
}

/// Rastgele tarama için tur sayıları. Bir debug derlemesi kodlayıcıyı yaklaşık
/// yirmi kat yavaş çalıştırır, bu yüzden daha hafif bir geçiş yapar; CI bu
/// dosyayı `--release` ile çalıştırır ki panik güvencesinin konusu olan profil
/// de odur.
const RANDOM_ROUNDS: usize = if cfg!(debug_assertions) { 300 } else { 4_000 };
const SIZING_ROUNDS: usize = if cfg!(debug_assertions) { 200 } else { 1_000 };

#[track_caller]
fn no_panic<T>(what: &str, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => panic!("panikledi: {what}"),
    }
}

#[test]
fn version_constructors_reject_out_of_range_numbers() {
    for number in [i16::MIN, -1, 0, 41, 100, i16::MAX] {
        no_panic(&format!("Version::normal({number})"), || {
            assert!(Version::normal(number).is_err(), "Version::normal({number}) should be rejected");
        });
    }
    for number in [i16::MIN, -1, 0, 5, 100, i16::MAX] {
        no_panic(&format!("Version::micro({number})"), || {
            assert!(Version::micro(number).is_err(), "Version::micro({number}) should be rejected");
        });
    }
}

#[test]
fn encoding_never_panics_for_any_version_and_ec_level() {
    for version in all_versions() {
        // Yalnızca Micro sürümler 8'in katı olmayan sayıda veri biti tutar, yani
        // sonlandırıcı ofsetinin önem taşıdığı yerler onlardır. Orada her uzunluğu
        // taramak ve Normal olanları örnekleme yapmak bu testi her commit'te
        // çalıştırılabilecek kadar hızlı tutar.
        // Bir Normal sürüm her zaman tam sayıda kod kelimesi tutar, yani orada bir
        // kısa ve bir uzun yük yeterlidir.
        let lengths: &[usize] = if version.is_micro() {
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 16, 20, 21, 22, 23, 24, 32]
        } else {
            &[0, 7]
        };

        for ec_level in EC_LEVELS {
            for &length in lengths {
                let data: Vec<u8> = b"0123456789".iter().copied().cycle().take(length).collect();
                let what = format!("with_version({version:?}, {ec_level:?}, len={length})");
                no_panic(&what, || {
                    if let Ok(code) = QrCode::with_version(&data, version, ec_level) {
                        let _ = code.max_allowed_errors();
                        let _ = code.to_colors();
                    }
                });
            }
        }
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    use qrcode::bits::Bits;

    // Ayrıştırıcı her bayta bir mod atar ve mod başına kodlayıcılar onun
    // ürettiği her şeyi kabul etmelidir. Her sıralı çift, kodlanabilir aralığın
    // hemen dışındakiler dahil, her Shift JIS öncü/izleyen bileşimini kapsar.
    //
    // Bu doğrudan bölümleme ve bit paketleme aşamalarını sürer; maske aramasını
    // 65536 kez çalıştırmak, aşağıdaki taramanın kapsamadığı hiçbir şeyi
    // kapsamadan çalışma süresine hâkim olurdu.
    let version = Version::normal(40).unwrap();
    for hi in 0..=u8::MAX {
        for lo in 0..=u8::MAX {
            no_panic(&format!("push_optimal_data([{hi:#04x}, {lo:#04x}])"), || {
                let mut bits = Bits::new(version);
                assert_eq!(bits.push_optimal_data(&[hi, lo]), Ok(()), "{hi:#04x} {lo:#04x}");
            });
        }
    }

    // Ve daha küçük ama yine de zorlayıcı bir örneklem üzerinde tüm boru hattı.
    for hi in (0..=u8::MAX).step_by(7) {
        no_panic(&format!("QrCode::new([{hi:#04x}])"), || {
            let _ = QrCode::new([hi]);
        });
        for lo in [0x00, 0x3f, 0x40, 0x7e, 0x7f, 0x80, 0xbf, 0xc0, 0xeb, 0xfc, 0xfd, 0xff] {
            no_panic(&format!("QrCode::new([{hi:#04x}, {lo:#04x}])"), || {
                let _ = QrCode::new([hi, lo]);
            });
        }
    }
}

#[test]
fn every_mask_pattern_on_every_symbol_is_accepted_or_rejected_cleanly() {
    for version in all_versions() {
        for ec_level in EC_LEVELS {
            for pattern in MASK_PATTERNS {
                let what = format!("try_apply_mask({version:?}, {ec_level:?}, {pattern:?})");
                no_panic(&what, || {
                    let mut canvas = Canvas::new(version, ec_level);
                    canvas.draw_all_functional_patterns();
                    // Micro semboller 8 desenden yalnızca 4'ünü destekler ve bazı
                    // sürüm/ec_level çiftleri hiç var olmaz. Her ikisi de abort değil, hata
                    // olarak geri dönmelidir.
                    let _ = canvas.try_apply_mask(pattern);
                });
            }

            no_panic(&format!("apply_best_mask({version:?}, {ec_level:?})"), || {
                let mut canvas = Canvas::new(version, ec_level);
                canvas.draw_all_functional_patterns();
                let _ = canvas.apply_best_mask();
            });
        }
    }
}

#[test]
fn coordinates_outside_the_symbol_are_reported_not_fatal() {
    for version in all_versions() {
        let width = version.width();
        let canvas = Canvas::new(version, EcLevel::L);

        for coord in [i16::MIN, -width - 1, -width, -1, 0, width - 1, width, width + 1, i16::MAX] {
            no_panic(&format!("get_module({coord}, 0) on {version:?}"), || {
                let inside = (-width..width).contains(&coord);
                assert_eq!(canvas.get_module(coord, 0).is_some(), inside, "{version:?} x={coord}");
                assert_eq!(canvas.get_module(0, coord).is_some(), inside, "{version:?} y={coord}");
            });
        }
    }

    let code = QrCode::new(b"01234567").unwrap();
    let width = code.width();
    for coord in [0, width - 1, width, width + 1, usize::MAX] {
        no_panic(&format!("QrCode::get({coord}, 0)"), || {
            let inside = coord < width;
            assert_eq!(code.get(coord, 0).is_some(), inside, "x={coord}");
            assert_eq!(code.get(0, coord).is_some(), inside, "y={coord}");
            assert_eq!(code.get_functional(coord, 0).is_some(), inside, "functional x={coord}");
        });
    }
}

#[test]
fn renderer_reports_bad_input_instead_of_aborting() {
    let content = [Color::Dark; 9];

    // Uyuşmayan uzunluk.
    for modules_count in [0, 1, 2, 4, 100] {
        no_panic(&format!("Renderer::try_new(len=9, {modules_count})"), || {
            assert!(Renderer::<char>::try_new(&content, modules_count, 4).is_err());
        });
    }
    no_panic("Renderer::try_new(len=9, 3)", || {
        assert!(Renderer::<char>::try_new(&content, 3, 4).is_ok());
    });

    // Karesi `usize`'ı taşıran bir `modules_count` eskiden tesadüfen uyabilecek
    // bir uzunluğa sarmalanıyordu.
    no_panic("Renderer::try_new(overflowing modules_count)", || {
        assert!(Renderer::<char>::try_new(&content, usize::MAX, 4).is_err());
    });

    // Boş bir QR kodunun modülü yoktur; boyutlandırma eskiden sıfıra bölüyordu.
    no_panic("Renderer with zero modules", || {
        let mut renderer = Renderer::<char>::try_new(&[], 0, 0).unwrap();
        let _ = renderer.min_dimensions(200, 200).try_build();
        let _ = renderer.max_dimensions(200, 200).try_build();
    });

    // `u32`'yi taşıran boyutlar sarmalanmalı değil, bildirilmelidir.
    no_panic("Renderer with overflowing module size", || {
        let code = QrCode::new(b"hi").unwrap();
        assert!(code.render::<char>().module_dimensions(u32::MAX, u32::MAX).try_build().is_err());
    });
}

#[test]
fn error_correction_rejects_oversized_blocks() {
    use qrcode::ec::{MAX_EC_CODE_SIZE, create_error_correction_code};

    for size in [0, 1, MAX_EC_CODE_SIZE, MAX_EC_CODE_SIZE + 1, 1000, usize::from(u8::MAX)] {
        no_panic(&format!("create_error_correction_code(_, {size})"), || {
            let result = create_error_correction_code(b"data", size);
            assert_eq!(result.is_ok(), size <= MAX_EC_CODE_SIZE, "size={size}");
        });
    }
}

#[test]
fn renderers_never_panic_for_any_symbol() {
    for version in all_versions() {
        for ec_level in EC_LEVELS {
            let Ok(code) = QrCode::with_version(b"1", version, ec_level) else { continue };
            let what = format!("render {version:?} {ec_level:?}");
            no_panic(&what, || {
                let _ = code.to_debug_str('#', '.');
                #[cfg(feature = "svg")]
                let _ = code.render::<qrcode::render::svg::Color>().try_build();
                #[cfg(feature = "eps")]
                let _ = code.render::<qrcode::render::eps::Color>().try_build();
                #[cfg(feature = "pic")]
                let _ = code.render::<qrcode::render::pic::Color>().try_build();
                let _ = code.render::<qrcode::render::unicode::Dense1x2>().try_build();
            });
        }
    }
}

/// `fuzz/` hedeflerinin kapsadığı aynı yüzey üzerinde bağımlılıksız bir
/// rastgele tur; böylece güvence yalnızca biri `cargo fuzz` çalıştırdığında
/// değil, her `cargo test`te kontrol edilir. Deterministiktir: tohum sabit
/// olduğundan bir başarısızlık yeniden üretilebilir.
#[test]
fn randomised_inputs_never_panic() {
    // xorshift64*; yapılandırılmış taramanın kaçırdığı biçimleri silkelemeye yeter.
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };

    let versions = all_versions();
    for round in 0..RANDOM_ROUNDS {
        let length = usize::try_from(next() % 40).unwrap();
        let data: Vec<u8> = (0..length).map(|_| u8::try_from(next() % 256).unwrap()).collect();

        // Kısıtsız sürüm numaraları: kurucular aralık dışı olanları bir tablo
        // aramasına ulaşmalarına izin vermek yerine reddetmelidir.
        let number = i16::try_from(next() % 64).unwrap() - 8;
        let version = if next() % 2 == 0 { Version::normal(number) } else { Version::micro(number) };
        let ec_level = EC_LEVELS[usize::try_from(next() % 4).unwrap()];

        let what = format!("round {round}: version_number={number} ec={ec_level:?} data={data:02x?}");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = QrCode::with_error_correction_level(&data, ec_level);
            if let Ok(version) = version
                && let Ok(code) = QrCode::with_version(&data, version, ec_level)
            {
                let _ = code.max_allowed_errors();
                let _ = code.to_debug_str('#', '.');
                let _ = code.render::<qrcode::render::unicode::Dense1x2>().try_build();
            }
        }));
        assert!(result.is_ok(), "panicked: {what}");
    }

    // Boyutlandırma istekleri diğer yarısıdır: aritmetik eskiden sarmalanıyordu.
    let code = QrCode::new(b"fuzz").unwrap();
    for round in 0..SIZING_ROUNDS {
        let mw = u32::try_from(next() % 4).unwrap().saturating_mul(u32::MAX / 3);
        let mh = u32::try_from(next() % 4).unwrap().saturating_mul(u32::MAX / 3);
        let what = format!("sizing round {round}: module_dimensions({mw}, {mh})");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = code.render::<char>().module_dimensions(mw, mh).try_build();
            let _ = code.render::<char>().min_dimensions(mw, mh).try_build();
            let _ = code.render::<char>().max_dimensions(mw, mh).try_build();
        }));
        assert!(result.is_ok(), "panicked: {what}");
    }

    let _ = versions;
}
