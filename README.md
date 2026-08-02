qrcode-rust
===========

[![Derleme durumu](https://github.com/kennytm/qrcode-rust/workflows/Rust/badge.svg)](https://github.com/kennytm/qrcode-rust/actions?query=workflow%3ARust)
[![crates.io](https://img.shields.io/crates/v/qrcode.svg)](https://crates.io/crates/qrcode)
[![MIT OR Apache 2.0](https://img.shields.io/badge/license-MIT%20%2f%20Apache%202.0-blue.svg)](./LICENSE-APACHE.txt)

Rust ile QR kodu ve Micro QR kodu kodlayıcı. [Belgeler](https://docs.rs/qrcode).

Geçersiz dış girdi, olağan çalışma hatası veya desteklenmeyen seçenek süreci
kasıtlı olarak panikletmez; kodlayıcının reddettiği durumlar hata ayıklama ve yayın
derlemelerinde aynı biçimde
[`QrError`](https://docs.rs/qrcode/latest/qrcode/types/enum.QrError.html) olarak
döner. Ayrıntı için [Panikler ve hatalar](#panikler-ve-hatalar) bölümüne bakın.

Cargo.toml
----------

```toml
[dependencies]
qrcode = "0.15.0"
```

Varsayılan özellikler `image` crate'ine bağlıdır. Görüntü üretmeye ihtiyacınız
yoksa `default-features` seçeneğini kapatın:

```toml
[dependencies]
qrcode = { version = "0.15.0", default-features = false, features = ["std"] }
```

Örnekler
--------

## Görüntü üretme

```rust
use qrcode::QrCode;
use image::Luma;

fn main() {
    // Bir miktar veriyi bitlere kodla.
    let code = QrCode::new(b"01234567").unwrap();

    // Bitleri bir görüntüye çiz.
    let image = code.render::<Luma<u8>>().build();

    // Görüntüyü kaydet.
    image.save("/tmp/qrcode.png").unwrap();
}
```

Şu görüntüyü üretir:

![Çıktı](src/test_annex_i_qr_as_image.png)

## Karakter dizisi üretme

```rust
use qrcode::QrCode;

fn main() {
    let code = QrCode::new(b"Hello").unwrap();
    let string = code.render::<char>()
        .dark_color('#')
        .quiet_zone(false)
        .module_dimensions(2, 1)
        .build();
    println!("{string}");
}
```

Şu çıktıyı üretir:

```none
##############    ########  ##############
##          ##          ##  ##          ##
##  ######  ##  ##  ##  ##  ##  ######  ##
##  ######  ##  ##  ##      ##  ######  ##
##  ######  ##  ####    ##  ##  ######  ##
##          ##  ####  ##    ##          ##
##############  ##  ##  ##  ##############
                ##  ##
##  ##########    ##  ##    ##########
      ##        ##    ########    ####  ##
    ##########    ####  ##  ####  ######
    ##    ##  ####  ##########    ####
  ######    ##########  ##    ##        ##
                ##      ##    ##  ##
##############    ##  ##  ##    ##  ####
##          ##  ##  ##        ##########
##  ######  ##  ##    ##  ##    ##    ##
##  ######  ##  ####  ##########  ##
##  ######  ##  ####    ##  ####    ##
##          ##    ##  ########  ######
##############  ####    ##      ##    ##
```

## SVG üretme

```rust
use qrcode::{QrCode, Version, EcLevel};
use qrcode::render::svg;

fn main() {
    let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
    let image = code.render()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#800000"))
        .light_color(svg::Color("#ffff80"))
        .build();
    println!("{image}");
}
```

Şu SVG'yi üretir:

[![Çıktı](src/test_annex_i_micro_qr_as_svg.svg)](src/test_annex_i_micro_qr_as_svg.svg)

## Unicode karakter dizisi üretme

```rust
use qrcode::QrCode;
use qrcode::render::unicode;

fn main() {
    let code = QrCode::new("mow mow").unwrap();
    let image = code.render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    println!("{image}");
}
```

Şu çıktıyı üretir:

```text
█████████████████████████████
█████████████████████████████
████ ▄▄▄▄▄ █ ▀▀▀▄█ ▄▄▄▄▄ ████
████ █   █ █▀ ▀ ▀█ █   █ ████
████ █▄▄▄█ ██▄  ▀█ █▄▄▄█ ████
████▄▄▄▄▄▄▄█ ▀▄▀ █▄▄▄▄▄▄▄████
████▄▀ ▄▀ ▄ █▄█  ▀ ▀█ █▄ ████
████▄██▄▄▀▄▄▀█▄ ██▀▀█▀▄▄▄████
█████▄▄▄█▄▄█  ▀▀▄█▀▀▀▄█▄▄████
████ ▄▄▄▄▄ █   ▄▄██▄ ▄ ▀▀████
████ █   █ █▀▄▄▀▄▄ ▄▄▄▄ ▄████
████ █▄▄▄█ █▄  █▄▀▄▀██▄█▀████
████▄▄▄▄▄▄▄█▄████▄█▄██▄██████
█████████████████████████████
▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
```

## PIC üretme

```rust
use qrcode::render::pic;
use qrcode::QrCode;

fn main() {
    let code = QrCode::new(b"01234567").unwrap();
    let image = code
        .render::<pic::Color>()
        .min_dimensions(1, 1)
        .build();
    println!("{image}");
}
```

Aşağıdaki gibi çizilen bir
[PIC](https://en.wikipedia.org/wiki/PIC_(markup_language)) çıktısı üretir:

```pic
maxpswid=29;maxpsht=29;movewid=0;moveht=1;boxwid=1;boxht=1
define p { box wid $3 ht $4 fill 1 thickness 0.1 with .nw at $1,-$2 }
box wid maxpswid ht maxpsht with .nw at 0,0
p(4,4,1,1)
p(5,4,1,1)
p(6,4,1,1)
p(7,4,1,1)
p(8,4,1,1)
p(9,4,1,1)
…
```
Tam örnek için
[`test_annex_i_micro_qr_as_pic.pic`](src/test_annex_i_micro_qr_as_pic.pic)
dosyasına bakın.

## EPS üretme

```rust
use qrcode::render::eps;
use qrcode::{EcLevel, QrCode, Version};

fn main() {
    let code = QrCode::with_version(b"01234567", Version::micro(2).unwrap(), EcLevel::L).unwrap();
    let image = code
        .render()
        .min_dimensions(200, 200)
        .dark_color(eps::Color([0.5, 0.0, 0.0]))
        .light_color(eps::Color([1.0, 1.0, 0.5]))
        .build();
    println!("{image}");
}
```

Aşağıdaki gibi çizilen bir
[EPS](https://en.wikipedia.org/wiki/Encapsulated_PostScript) çıktısı üretir:

```postscript
%!PS-Adobe-3.0 EPSF-3.0
%%BoundingBox: 0 0 204 204
%%Pages: 1
%%EndComments
gsave
1 1 0.5 setrgbcolor
0 0 204 204 rectfill
grestore
0.5 0 0 setrgbcolor
24 180 12 12 rectfill
36 180 12 12 rectfill
48 180 12 12 rectfill
60 180 12 12 rectfill
72 180 12 12 rectfill
84 180 12 12 rectfill
…
```
Tam örnek için
[`test_annex_i_micro_qr_as_eps.eps`](src/test_annex_i_micro_qr_as_eps.eps)
dosyasına bakın.

Panikler ve hatalar
-------------------

Kamu API'sinin hata sözleşmesi şöyledir:

> Geçersiz dış girdi, olağan çalışma hatası veya desteklenmeyen seçenek
> kütüphaneyi kasıtlı olarak panikletmez; yapılandırılmış bir `QrError` döndürür.
> Panik yalnızca belgelenmiş bir programlama sözleşmesi ihlalinde veya
> kütüphanenin iç değişmezinin bozulduğunu gösteren bir programlama hatasında
> kullanılabilir.

Başka bir deyişle girdi sınırda doğrulanır, özel alanlı geçerli bir tipe
dönüştürülür ve çekirdek işlemler mümkün olduğunca doğrudan değer döndürür.
Örneğin `Segment::new` hatalı aralığı reddeder; geçerli bir `Segment`in
`encoded_len` metodu bundan sonra gereksiz bir `Result` üretmez. Hata ayıklama ve
yayın profilleri aynı denetimleri yapar; taşma hiçbir profilde bozuk bir QR koduna
dönüşmez.

Bu sözleşmeyi üç katman korur:

* `Cargo.toml`, kütüphane için `clippy::{indexing_slicing, unwrap_used,
  expect_used, panic, unreachable}` lintlerini reddeder. Yeni bir panik ihtimali
  ancak neden dış girdiden erişilemediğini açıklayan
  `#[expect(..., reason = "...")]` ile bilinçli biçimde eklenebilir.
* `tests/no_panic.rs`; her sürümü, hata düzeltme seviyesini, maske desenini,
  bayt çiftlerini, sınır koordinatlarını, hatalı çağrı sıralarını ve
  deterministik rastgele girdileri tarar.
* `fuzz/`, kodlama ve çizim yolları için `overflow-checks = true` ile derlenen
  `cargo fuzz` hedeflerini içerir:

  ```sh
  cargo install cargo-fuzz
  cargo +nightly fuzz run encode
  cargo +nightly fuzz run render
  ```

Paniklemeye devam edebilen metotlar yalnızca açık bir çağıran sözleşmesi
ihlalinde bunu yapar. Her birinin `# Panics` bölümü ve kontrollü karşılığı vardır:

| Panikleyen | Kontrollü |
| --- | --- |
| `code[(x, y)]` | `QrCode::get(x, y) -> Option<Color>` |
| `QrCode::is_functional` | `QrCode::get_functional -> Option<bool>` |
| `Canvas::get` / `get_mut` | `Canvas::get_module` / `get_module_mut` |
| `Canvas::put` | `Canvas::try_put` |
| `Canvas::apply_mask` | `Canvas::try_apply_mask` |
| `Renderer::new` | `Renderer::try_new` |
| `Renderer::build` | `Renderer::try_build` |

Bellek tükenmesi, çağrı yığını taşması ve bağımlılık panikleri bu garantinin dışındadır.
`Renderer::try_build`, doymuş bir uzunluğu ayırıcıya göndermek yerine
`render::MAX_IMAGE_PIXELS` üzerindeki istekleri reddeder. Bu sınır yalnızca
*piksel sayısını* kapsar; arka ucun öğe boyutuyla çarpıldığında izin verilen bir
istek yine birkaç gigabayt tutabilir. Uygun görüntü bütçesini belirlemek
çağıranın sorumluluğudur.

0.14 sürümünden yükseltme
-------------------------

`Version` artık kurulurken doğrulanır; böylece sürüm tablolarındaki iç aramalar
hatasız olur. Eski enum varyantları kaldırılmıştır:

```rust
// 0.14
let version = Version::Normal(5);
let micro = Version::Micro(2);

// 0.15
let version = Version::normal(5)?;
let micro = Version::micro(2)?;
```

`Version::normal` ve `Version::micro`, sırasıyla 1..=40 ve 1..=4 dışındaki
değerler için `Err(QrError::InvalidVersion)` döndürür. Sayı
`Version::number()` ile geri okunabilir. Artık bir `Version` üzerinde doğrudan
varyant eşleştirmesi yapılamaz; `is_micro()`, `number()` ve `width()` kullanın.

Diğer kırıcı değişiklikler:

* `Version::fetch`, dilim yerine `&[[T; 4]; VERSION_COUNT]` alır. Kısa bir tablo
  artık indeks paniği değil derleme hatasıdır. Varsayılan değerli normal sürüm
  girdileri de Micro sürümlerde olduğu gibi reddedilir.
* `Segment` alanları özeldir. `Segment::new(mode, begin, end)` aralığı
  doğrular; `mode()`, `begin()` ve `end()` erişim için kullanılabilir.
* `Parser::new` aşırı uzun girdiyi daha toplamadan reddettiği için
  `QrResult<Parser>` döndürür. Mutlak ham girdi sınırı `MAX_INPUT_BYTES` ile
  yayımlanır.
* `Mode::data_bits_count` ve `total_encoded_len` aritmetik taşmayı
  `QrError::DataTooLong` olarak bildirmek için `QrResult` döndürür.
* `ec::create_error_correction_code`, `QrResult<Vec<u8>>` döndürür;
  `ec::MAX_EC_CODE_SIZE` üzerindeki boyutlar indeks paniği yerine hatadır.
* `ec::construct_codewords`, `rawbits` uzunluğunu yalnızca hata ayıklama derlemesinde
  assert etmek yerine her profilde doğrular.
* `Canvas::draw_data`, kod kelimesi uzunluklarını ve çağrı sırasını doğrular;
  `QrResult<()>` döndürür. `Canvas::apply_best_mask` da
  `QrResult<Canvas>` döndürür.
* `canvas::is_functional`, genişliği `Version`dan türetir; fazladan `width`
  parametresi kaldırılmıştır.
* `QrError`; `InvalidSegment`, `InvalidDataLength`, `InvalidCanvasState`,
  `InvalidMaskPattern`, `CoordinateOutOfRange` ve `ImageTooLarge` varyantlarını
  kazanmıştır. `no_std` derlemelerinde de `core::error::Error` uygular.
* `push_numeric_data`, `push_alphanumeric_data` ve `push_kanji_data`, kendi
  karakter kümesinin dışındaki baytlar için `Err(QrError::InvalidCharacter)`
  döndürür. Başarısız bir yazım mevcut `Bits` değerini değiştirmez.
* SVG renkleri XML özniteliklerine eklenmeden önce kaçışlanır.
* `ec::max_allowed_errors` içindeki yanlış-kod-çözme koruma kod kelimesi (`p`)
  değerleri ISO/IEC 18004:2024 §7.5.1 ve Tablo 9 ile birebir doğrulandı:
  `p` yalnızca 1-L/M2-L (3), 1-M/2-L/M1/M2-M/M3-L/M4-L (2) ve 1-Q/1-H/3-L (1)
  sembollerinde düşülür. M3-M, M4-M ve M4-Q'da `p = 0` olduğundan kapasiteleri
  sırasıyla 4, 5 ve 7 hatadır.
* Kararsız `bench` özelliği kaldırılmıştır; tüm özellikler kararlı Rust ile
  birlikte sınanabilir.
