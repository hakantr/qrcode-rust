//! Bir QR kodunu görüntüye çizer.

use crate::types::{Color, QrError, QrResult};
use core::cmp::max;

pub mod eps;
pub mod image;
pub mod pic;
pub mod string;
pub mod svg;
pub mod unicode;

/// [`Renderer::try_build`]'in ayırmayı deneyeceği en büyük görüntü, piksel
/// cinsinden. Bunun üzerindeki her şey ayırıcıya verilmek yerine
/// `QrError::ImageTooLarge` olarak bildirilir.
///
/// Tavan 2<sup>32</sup>−1 pikseldir: sessiz bölgesiyle birlikte bir sürüm 40
/// sembolü 185 modül genişliğindedir, yani bu hâlâ modül başına 300 pikselin
/// çok üzerine — pratikte hiçbir çizimin gerektirmeyeceği kadarına — izin
/// verirken düşmanca bir `module_dimensions` isteğinin süreci sonlandırmasını
/// engeller.
pub const MAX_IMAGE_PIXELS: u64 = 4_294_967_295;

//------------------------------------------------------------------------------
//{{{ Piksel trait'i

/// Bir görüntü pikselinin soyutlaması.
pub trait Pixel: Copy + Sized {
    /// Tamamlanmış görüntünün tipi.
    type Image: Sized + 'static;

    /// Somut bir görüntüye dönüştürülmeden önce ara tamponu saklayan tip.
    type Canvas: Canvas<Pixel = Self, Image = Self::Image>;

    /// Varsayılan modül boyutunu verir. Sonuç en az 1×1 olmalıdır.
    fn default_unit_size() -> (u32, u32) {
        (8, 8)
    }

    /// Modül koyu ya da açık olduğunda varsayılan piksel rengini verir.
    fn default_color(color: Color) -> Self;
}

/// Bir QR kodu görüntüsünün çizim tuvali.
pub trait Canvas: Sized {
    /// Bir görüntü pikselinin tipi.
    type Pixel: Sized;

    /// Tamamlanmış görüntünün tipi.
    type Image: Sized;

    /// Verilen boyutlarda yeni bir tuval kurar.
    ///
    /// Bu düşük seviyeli metot normalde [`Renderer::try_build`] tarafından
    /// çağrılır. Doğrudan çağıranlar `width * height <= MAX_IMAGE_PIXELS`
    /// sözleşmesine uymalıdır.
    ///
    /// # Panics
    ///
    /// Boyutlar bu sözleşmeyi ihlal ederse veya bellek ayrılamazsa arka uç
    /// panikleyebilir.
    fn new(width: u32, height: u32, dark_pixel: Self::Pixel, light_pixel: Self::Pixel) -> Self;

    /// (x, y) koordinatına tek bir koyu piksel çizer.
    ///
    /// # Panics
    ///
    /// Koordinat [`Canvas::new`] ile kurulan tuvalin dışındaysa arka uç
    /// panikleyebilir. `Renderer` bu sözleşmeyi her zaman korur.
    fn draw_dark_pixel(&mut self, x: u32, y: u32);

    /// (`left`, `top`) koordinatına `width`×`height` boyutlarında koyu bir
    /// dikdörtgen çizer.
    ///
    /// # Panics
    ///
    /// Dikdörtgen tuvalin dışına taşıyorsa arka uç panikleyebilir. `Renderer`
    /// bu sözleşmeyi her zaman korur.
    fn draw_dark_rect(&mut self, left: u32, top: u32, width: u32, height: u32) {
        for y in top..(top + height) {
            for x in left..(left + width) {
                self.draw_dark_pixel(x, y);
            }
        }
    }

    /// Tuvali gerçek bir görüntüye dönüştürür.
    fn into_image(self) -> Self::Image;
}

//}}}
//------------------------------------------------------------------------------
//{{{ Çizici

/// Bir QR kodu çizici. Bir bool vektörünü görüntüye dönüştüren bir kurucu
/// tipidir.
pub struct Renderer<'a, P: Pixel> {
    content: &'a [Color],
    modules_count: u32, // <- `width` ile karışmasın diye burada `modules_count` diyoruz.
    quiet_zone: u32,
    module_size: (u32, u32),

    dark_color: P,
    light_color: P,
    has_quiet_zone: bool,
}

impl<'a, P: Pixel> Renderer<'a, P> {
    /// Yeni bir çizici oluşturur.
    ///
    /// # Panics
    ///
    /// `content`'in uzunluğu tam olarak `modules_count * modules_count` değilse
    /// ya da `modules_count` bir `u32`'ye sığmıyorsa panikler. Kontrollü biçim
    /// için [`Renderer::try_new`] kullanın.
    pub fn new(content: &'a [Color], modules_count: usize, quiet_zone: u32) -> Self {
        #[expect(clippy::expect_used, reason = "belgelenmiş panik; kontrollü biçim try_new")]
        Self::try_new(content, modules_count, quiet_zone).expect("içerik uzunluğu modül sayısıyla uyuşmuyor")
    }

    /// Yeni bir çizici oluşturur; uyuşmayan `content`'i paniklemek yerine bildirir.
    ///
    /// # Errors
    ///
    /// `content`'in uzunluğu tam olarak `modules_count * modules_count` değilse
    /// `Err(QrError::InvalidDataLength)`, `modules_count` bir `u32`'ye sığmıyorsa
    /// `Err(QrError::ImageTooLarge)` döndürür.
    ///
    /// Çarpma kontrollüdür; yani devasa bir `modules_count`, tesadüfen uyan bir
    /// uzunluğa sarmalanmak yerine hata bildirir.
    pub fn try_new(content: &'a [Color], modules_count: usize, quiet_zone: u32) -> QrResult<Self> {
        let area = modules_count.checked_mul(modules_count).ok_or(QrError::ImageTooLarge)?;
        if area != content.len() {
            return Err(QrError::InvalidDataLength);
        }
        let modules_count = u32::try_from(modules_count).or(Err(QrError::ImageTooLarge))?;
        Ok(Renderer {
            content,
            modules_count,
            quiet_zone,
            module_size: P::default_unit_size(),
            dark_color: P::default_color(Color::Dark),
            light_color: P::default_color(Color::Light),
            has_quiet_zone: true,
        })
    }

    /// Koyu bir modülün rengini ayarlar. Varsayılan opak siyahtır.
    pub fn dark_color(&mut self, color: P) -> &mut Self {
        self.dark_color = color;
        self
    }

    /// Açık bir modülün rengini ayarlar. Varsayılan opak beyazdır.
    pub fn light_color(&mut self, color: P) -> &mut Self {
        self.light_color = color;
        self
    }

    /// Üretilen görüntüye sessiz bölgenin dahil edilip edilmeyeceği.
    pub fn quiet_zone(&mut self, has_quiet_zone: bool) -> &mut Self {
        self.has_quiet_zone = has_quiet_zone;
        self
    }

    /// Her modülün piksel cinsinden boyutunu ayarlar. Varsayılan 8px.
    #[deprecated(since = "0.4.0", note = "bunun yerine `.module_dimensions(width, width)` kullanın")]
    pub fn module_size(&mut self, width: u32) -> &mut Self {
        self.module_dimensions(width, width)
    }

    /// Her modülün piksel cinsinden boyutunu ayarlar. Varsayılan 8×8.
    pub fn module_dimensions(&mut self, width: u32, height: u32) -> &mut Self {
        self.module_size = (max(width, 1), max(height, 1));
        self
    }

    /// Geçerliyse sessiz bölge dahil, piksel cinsinden en küçük toplam görüntü
    /// genişliğini (ve dolayısıyla yüksekliğini) ayarlar. Çizici, QR kodundaki
    /// her modül tek biçim boyutta olacak (bozulma olmayacak) şekilde boyutu
    /// olabildiğince küçük tutmaya çalışır.
    ///
    /// Örneğin bir sürüm 1 QR kodu sessiz bölgesiyle 19 modül genişliğindedir.
    /// ≥200px genişlikte bir görüntü istersek her modülün boyutu 11px olur,
    /// yani gerçek görüntü boyutu 209px olur.
    #[deprecated(since = "0.4.0", note = "bunun yerine `.min_dimensions(width, width)` kullanın")]
    pub fn min_width(&mut self, width: u32) -> &mut Self {
        self.min_dimensions(width, width)
    }

    /// Geçerliyse sessiz bölge dahil, piksel cinsinden en küçük toplam görüntü
    /// boyutunu ayarlar. Çizici, QR kodundaki her modül tek biçim boyutta olacak
    /// (bozulma olmayacak) şekilde boyutu olabildiğince küçük tutmaya çalışır.
    ///
    /// Örneğin bir sürüm 1 QR kodu sessiz bölgesiyle 19 modül genişliğindedir.
    /// ≥200×200 boyutunda bir görüntü istersek her modülün boyutu 11×11 olur,
    /// yani gerçek görüntü boyutu 209×209 olur.
    pub fn min_dimensions(&mut self, width: u32, height: u32) -> &mut Self {
        let width_in_modules = self.width_in_modules();
        // İstenen boyut `u32::MAX`'a yakınken taşan `(a + b - 1) / b` yerine
        // `div_ceil`.
        let unit_width = width.div_ceil(width_in_modules);
        let unit_height = height.div_ceil(width_in_modules);
        self.module_dimensions(unit_width, unit_height)
    }

    /// Sessiz bölge dahil, tamamlanmış görüntüdeki modül sayısı.
    ///
    /// En az 1'e kırpılır: boş bir QR kodunun modülü yoktur ve istenen görüntü
    /// boyutunu sıfıra bölmek aksi hâlde sürecin sonlanmasına yol açardı.
    fn width_in_modules(&self) -> u32 {
        let quiet_zone = if self.has_quiet_zone { 2 } else { 0 } * self.quiet_zone;
        self.modules_count.saturating_add(quiet_zone).max(1)
    }

    /// Geçerliyse sessiz bölge dahil, piksel cinsinden en büyük toplam görüntü
    /// boyutunu ayarlar. Çizici, QR kodundaki her modül tek biçim boyutta olacak
    /// (bozulma olmayacak) şekilde boyutu olabildiğince büyük tutmaya çalışır.
    ///
    /// Örneğin bir sürüm 1 QR kodu sessiz bölgesiyle 19 modül genişliğindedir.
    /// ≤200×200 boyutunda bir görüntü istersek her modülün boyutu 10×10 olur,
    /// yani gerçek görüntü boyutu 190×190 olur.
    ///
    /// Modül boyutu en az 1×1'dir; yani kısıt fazla küçükse nihai görüntü
    /// girdiden *büyük* olabilir.
    pub fn max_dimensions(&mut self, width: u32, height: u32) -> &mut Self {
        let width_in_modules = self.width_in_modules();
        let unit_width = width / width_in_modules;
        let unit_height = height / width_in_modules;
        self.module_dimensions(unit_width, unit_height)
    }

    /// QR kodunu bir görüntüye çizer.
    #[deprecated(since = "0.4.0", note = "görüntü bağını geri plana almak için adı `.build()` oldu")]
    pub fn to_image(&self) -> P::Image {
        self.build()
    }

    /// QR kodunu bir görüntüye çizer.
    ///
    /// # Panics
    ///
    /// Ortaya çıkan görüntü boyutları bir `u32`'yi taşırırsa panikler.
    /// Kontrollü biçim için [`Renderer::try_build`] kullanın.
    pub fn build(&self) -> P::Image {
        #[expect(clippy::expect_used, reason = "belgelenmiş panik; kontrollü biçim try_build")]
        self.try_build().expect("görüntü boyutları taşıyor")
    }

    /// QR kodunu bir görüntüye çizer; aşırı büyük çıktıyı paniklemek yerine
    /// bildirir.
    ///
    /// # Errors
    ///
    /// Modül sayısı, sessiz bölge ve modül boyutu çarpıldığında `u32`'yi aşan
    /// boyutlar veriyorsa `Err(QrError::ImageTooLarge)` döndürür. Bu kontrol
    /// olmadan release derlemelerinde sarmalanır ve sessizce yanlış boyutta bir
    /// görüntü üretir.
    ///
    /// # Panics
    ///
    /// Yalnızca özel `Renderer` alanlarının, [`Renderer::try_new`] tarafından
    /// kurulan içerik uzunluğu değişmezini ihlal etmesi durumunda panikler. Bir
    /// arka uç uygulaması veya bellek ayırıcı ayrıca kendi belgelenmiş
    /// koşullarında panikleyebilir.
    #[expect(
        clippy::expect_used,
        reason = "Renderer::try_new içerik uzunluğunu modül alanına eşitler ve alanlar daha sonra özel kalır"
    )]
    pub fn try_build(&self) -> QrResult<P::Image> {
        let w = self.modules_count;
        let qz = if self.has_quiet_zone { self.quiet_zone } else { 0 };
        let width = qz.checked_mul(2).and_then(|qz| w.checked_add(qz)).ok_or(QrError::ImageTooLarge)?;

        let (mw, mh) = self.module_size;
        let real_width = width.checked_mul(mw).ok_or(QrError::ImageTooLarge)?;
        let real_height = width.checked_mul(mh).ok_or(QrError::ImageTooLarge)?;

        // Arka uçlar `real_width * real_height` öğeyi baştan ayırır. Bu kontrol
        // olmadan yeterince büyük bir istek `Vec`'e doymuş bir uzunlukla ulaşır ve
        // çağıranın yakalayamayacağı bir kapasite taşmasıyla süreci sonlandırır.
        // `MAX_IMAGE_PIXELS`, "ayrılamayacak kadar büyük"ü sıradan bir hataya
        // çeviren tek yerdir.
        let area = u64::from(real_width) * u64::from(real_height);
        if area > MAX_IMAGE_PIXELS {
            return Err(QrError::ImageTooLarge);
        }

        let mut canvas = P::Canvas::new(real_width, real_height, self.dark_color, self.light_color);
        let mut i = 0;
        for y in 0..width {
            for x in 0..width {
                if qz <= x && x < w + qz && qz <= y && y < w + qz {
                    let color = self
                        .content
                        .get(i)
                        .copied()
                        .expect("Renderer iç değişmezi: içerik alanı modül sayısıyla eşleşmeli");
                    if color != Color::Light {
                        canvas.draw_dark_rect(x * mw, y * mh, mw, mh);
                    }
                    i += 1;
                }
            }
        }

        Ok(canvas.into_image())
    }
}

//}}}
