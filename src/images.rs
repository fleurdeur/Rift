use crate::model::{accent_for, accent_rgb, hash_id, Track};
use crate::paths;
use image::{DynamicImage, GenericImageView};
use std::collections::HashMap;
use std::io::Cursor;
use std::cell::RefCell;
use std::sync::OnceLock;

thread_local! {
    static IMAGE_CACHE: RefCell<HashMap<String, slint::Image>> = RefCell::new(HashMap::new());
}

pub fn http_client() -> reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .user_agent("Mozilla/5.0 (Linux; Android 13) Rift/1.0")
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(12))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new())
        })
        .clone()
}

pub fn download_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = http_client()
        .get(url)
        .header("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
        .send()?
        .error_for_status()?;
    let bytes = resp.bytes()?.to_vec();
    Ok(bytes)
}

pub fn ensure_artwork(track: &mut Track) {
    if track.artwork_path.is_some() {
        return;
    }
    let Some(url) = track.artwork.as_ref() else {
        track.accent = accent_for(&track.id);
        return;
    };
    if !url.starts_with("http") {
        return;
    }
    let path = paths::paths().art.join(format!("{}.jpg", hash_id(&track.id)));
    if path.exists() {
        track.artwork_path = Some(path.to_string_lossy().to_string());
        if let Some(color) = image_accent(&path) {
            track.accent = color;
        }
        return;
    }
    match download_bytes(url) {
        Ok(bytes) => match save_resized(&bytes, &path) {
            Ok(()) => {
                track.artwork_path = Some(path.to_string_lossy().to_string());
                if let Some(color) = image_accent(&path) {
                    track.accent = color;
                }
            }
            Err(_) => {
                track.accent = accent_for(&track.id);
            }
        },
        Err(_) => {
            track.accent = accent_for(&track.id);
        }
    }
}

pub fn save_embedded(bytes: &[u8], id: &str) -> Option<String> {
    let path = paths::paths().art.join(format!("{}.jpg", hash_id(id)));
    if path.exists() {
        return Some(path.to_string_lossy().to_string());
    }
    save_resized(bytes, &path).ok()?;
    Some(path.to_string_lossy().to_string())
}

fn save_resized(bytes: &[u8], path: &std::path::Path) -> anyhow::Result<()> {
    let img = image::load_from_memory(bytes)?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        anyhow::bail!("empty image");
    }
    let thumb = img.thumbnail(640, 640);
    let rgb = thumb.to_rgb8();
    rgb.save(path)?;
    Ok(())
}

pub fn image_accent(path: &std::path::Path) -> Option<String> {
    let img = image::open(path).ok()?;
    let img = img.thumbnail(64, 64);
    let mut r = 0f64;
    let mut g = 0f64;
    let mut b = 0f64;
    let mut weight = 0f64;
    for (_, _, px) in img.pixels() {
        let [pr, pg, pb, pa] = px.0;
        if pa < 32 {
            continue;
        }
        let max = pr.max(pg).max(pb) as f64;
        let min = pr.min(pg).min(pb) as f64;
        let sat = (max - min) / 255.0;
        let lum = (max + min) / 510.0;
        let w = 0.25 + sat * 1.5 + (lum - 0.45).abs() * -0.2;
        r += pr as f64 * w;
        g += pg as f64 * w;
        b += pb as f64 * w;
        weight += w;
    }
    if weight <= 0.0 {
        return None;
    }
    let mut r = (r / weight).clamp(40.0, 230.0) as u8;
    let mut g = (g / weight).clamp(40.0, 230.0) as u8;
    let mut b = (b / weight).clamp(40.0, 230.0) as u8;
    // Lift dark colors so text stays readable on top.
    let mx = r.max(g).max(b).max(1);
    if mx < 120 {
        let factor = 130.0 / mx as f64;
        r = (r as f64 * factor).min(220.0) as u8;
        g = (g as f64 * factor).min(220.0) as u8;
        b = (b as f64 * factor).min(220.0) as u8;
    }
    Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
}

pub fn image_for_track(track: &Track) -> slint::Image {
    let key = track
        .artwork_path
        .clone()
        .unwrap_or_else(|| format!("accent:{}", track.accent));
    IMAGE_CACHE.with(|cache| {
        if let Some(img) = cache.borrow().get(&key).cloned() {
            return img;
        }
        let img = track
            .artwork_path
            .as_ref()
            .and_then(|p| load_image(std::path::Path::new(p)))
            .unwrap_or_default();
        let mut c = cache.borrow_mut();
        if c.len() > 220 {
            c.clear();
        }
        c.insert(key, img.clone());
        img
    })
}

pub fn load_image(path: &std::path::Path) -> Option<slint::Image> {
    let data = std::fs::read(path).ok()?;
    image_from_bytes(&data)
}

pub fn image_from_bytes(bytes: &[u8]) -> Option<slint::Image> {
    let img = image::load_from_memory(bytes).ok()?;
    Some(to_slint_image(img))
}

pub fn to_slint_image(img: DynamicImage) -> slint::Image {
    let thumb = img.thumbnail(640, 640).to_rgba8();
    let (w, h) = thumb.dimensions();
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
    for (i, px) in thumb.pixels().enumerate() {
        buffer.make_mut_slice()[i] = slint::Rgba8Pixel {
            r: px[0],
            g: px[1],
            b: px[2],
            a: px[3],
        };
    }
    slint::Image::from_rgba8(buffer)
}

pub fn cached_image_for_url(url: &str) -> slint::Image {
    let path = paths::paths().art.join(format!("{}.jpg", hash_id(url)));
    if path.exists() {
        if let Some(img) = load_image(&path) {
            return img;
        }
    }
    slint::Image::default()
}

pub fn download_artwork_url(url: &str) -> Option<std::path::PathBuf> {
    if !url.starts_with("http") {
        return None;
    }
    let path = paths::paths().art.join(format!("{}.jpg", hash_id(url)));
    if path.exists() {
        return Some(path);
    }
    let bytes = download_bytes(url).ok()?;
    save_resized(&bytes, &path).ok()?;
    Some(path)
}

pub fn color_from_hex(hex: &str) -> slint::Color {
    let (r, g, b) = accent_rgb(hex);
    slint::Color::from_rgb_u8(r, g, b)
}

pub fn decode_data_uri(uri: &str) -> Option<slint::Image> {
    let data = uri.split(',').nth(1)?;
    use base64_decode::decode;
    let bytes = decode(data).ok()?;
    image_from_bytes(&bytes)
}

mod base64_decode {
    pub fn decode(input: &str) -> Result<Vec<u8>, ()> {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::with_capacity(input.len() * 3 / 4 + 3);
        let mut buf = 0u32;
        let mut bits = 0u32;
        for b in input.bytes() {
            if b == b'=' || b == b'\n' || b == b'\r' {
                continue;
            }
            let Some(v) = TABLE.iter().position(|&c| c == b) else {
                continue;
            };
            buf = (buf << 6) | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
            }
        }
        Ok(out)
    }
}

pub fn placeholder_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([20, 20, 26, 255]));
    let mut cursor = Cursor::new(Vec::new());
    img.write_to(&mut cursor, image::ImageFormat::Png).ok();
    cursor.into_inner()
}
