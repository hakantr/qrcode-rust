//! Çizicileri rastgele boyutlandırma istekleri üzerinde sürer.
//!
//! Boyutlandırma aritmetiği, bir `u32` taşmasının eskiden sessizce sarmalanıp
//! yanlış boyutta bir görüntü ürettiği yerdir. Buradaki her şey, süreci sonlandırmak
//! yerine bildirmesi gereken `try_` biçimlerinden geçer.
//!
//! Boyutlar tüm `u32` aralığı yerine iki banttan çekilir. İlginç durumlar
//! gerçekten ayırma yapıp çizen küçük olanlar ile herhangi bir şey ayrılmadan
//! önce reddedilmesi gereken uç olanlardır. Aradaki bant ikisi de değildir:
//! 526575x1785 gibi bir istek yasaldır, `MAX_IMAGE_PIXELS`'in altındadır ve
//! yalnızca birkaç gigabayt gerektirir -- orada ayırıcının tükenmesi bir defekt
//! değil çağıranın meselesidir ve bunu kovalamak diğer tüm bulguları bastırırdı.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qrcode::QrCode;
use qrcode::render::Renderer;
use qrcode::types::Color;

/// Çizilen bir görüntüyü birkaç megabaytın altında tutar.
const MAX_RENDERED_SIDE: u32 = 2048;

#[derive(arbitrary::Arbitrary, Debug)]
struct Dimension {
    /// Bandı seçer: uçlar reddedilmeli, küçük değerler çizilmelidir.
    extreme: bool,
    value: u32,
}

impl Dimension {
    fn get(&self) -> u32 {
        if self.extreme {
            // Taşması ya da MAX_IMAGE_PIXELS'i aşması gereken değerler. Reddetme
            // herhangi bir ayırmadan önce olur, bu yüzden bunlar ucuz kalır.
            u32::MAX - (self.value % 4)
        } else {
            self.value % MAX_RENDERED_SIDE
        }
    }
}

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
    data: Vec<u8>,
    module_width: Dimension,
    module_height: Dimension,
    min_width: Dimension,
    min_height: Dimension,
    max_width: Dimension,
    max_height: Dimension,
    quiet_zone: bool,
    /// Uzunluk kontrolü ve taşma koruması birlikte denensin diye uyuşmayan bir
    /// tamponla birlikte doğrudan `Renderer::try_new`'a verilir.
    raw_modules_count: usize,
}

fuzz_target!(|input: Input| {
    // Modül sayısı sıfır olan yoğun Unicode arka ucu eskiden satır genişliği
    // sıfırken `chunks_exact(0)` çağırıyordu.
    let _ = Renderer::<qrcode::render::unicode::Dense1x2>::try_new(&[], 0, 0)
        .expect("boş çizici geçerli olmalı")
        .try_build();

    // Elle kurulmuş bir çizici; `modules_count` muhtemelen içerik uzunluğuyla
    // uyuşmuyor.
    let content = [Color::Dark, Color::Light, Color::Light, Color::Dark];
    match Renderer::<char>::try_new(&content, input.raw_modules_count, 4) {
        Ok(mut renderer) => {
            // Burada try_new yalnızca modules_count == 2 için başarılı olur.
            assert_eq!(input.raw_modules_count, 2);
            let _ = renderer.module_dimensions(input.module_width.get(), input.module_height.get()).try_build();
        }
        Err(_) => assert_ne!(input.raw_modules_count, 2),
    }

    let Ok(code) = QrCode::new(&input.data) else { return };

    let mut renderer = code.render::<char>();
    let renderer = renderer
        .quiet_zone(input.quiet_zone)
        .module_dimensions(input.module_width.get(), input.module_height.get())
        .min_dimensions(input.min_width.get(), input.min_height.get())
        .max_dimensions(input.max_width.get(), input.max_height.get());

    if let Ok(image) = renderer.try_build() {
        // Çizilmiş bir görüntü her zaman en azından QR kodunun kendi modüllerini içerir.
        assert!(image.lines().count() > 0);
    }

    // Vektör arka uçları piksel başına ayırma yapmaz, bu yüzden boyutlandırma
    // aritmetiğinin kabul ettiği her boyutta sürülmeleri güvenlidir.
    let _ = code.render::<qrcode::render::unicode::Dense1x2>().try_build();
    let _ = code.render::<qrcode::render::svg::Color>().try_build();
    let _ = code.render::<qrcode::render::eps::Color>().try_build();
    let _ = code.render::<qrcode::render::pic::Color>().try_build();
});
