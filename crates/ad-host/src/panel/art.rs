//! Original 1991 `AD.EXE` artwork (logo, meteor, power switch, icon) decoded into egui textures.

use eframe::egui::{self, ColorImage, IconData, TextureHandle, TextureOptions};

const BMP_ON_SW: &[u8] = include_bytes!("../../../../assets/ad_host/RT_BITMAP_#1.bmp");
const BMP_OFF_SW: &[u8] = include_bytes!("../../../../assets/ad_host/RT_BITMAP_#2.bmp");
const BMP_LOGO: &[u8] = include_bytes!("../../../../assets/ad_host/RT_BITMAP_#5.bmp");
const BMP_LMETEOR: &[u8] = include_bytes!("../../../../assets/ad_host/RT_BITMAP_#12.bmp");
const ICO_AD: &[u8] = include_bytes!("../../../../assets/ad_host/icon_1.ico");

/// A decoded bitmap: straight (unpremultiplied) RGBA, top row first.
struct Rgba {
    width: usize,
    height: usize,
    px: Vec<[u8; 4]>,
}

fn rd_u16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn rd_u32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// Decode a `BITMAPINFOHEADER` DIB (1/4/8/24/32 bpp, uncompressed) starting at `dib`, with pixel
/// bits at `bits_off` (both offsets into `b`). `height` overrides the header height (icons store
/// XOR + AND masks, i.e. twice the real height).
fn decode_dib(b: &[u8], dib: usize, bits_off: Option<usize>, height: Option<usize>) -> Option<Rgba> {
    let hdr = rd_u32(b, dib)? as usize;
    let width = rd_u32(b, dib + 4)? as i32;
    let raw_h = rd_u32(b, dib + 8)? as i32;
    let bpp = rd_u16(b, dib + 14)? as usize;
    let clr_used = rd_u32(b, dib + 32)? as usize;
    if hdr < 40 || width <= 0 || raw_h == 0 || !matches!(bpp, 1 | 4 | 8 | 24 | 32) {
        return None;
    }
    let top_down = raw_h < 0;
    let (w, h) = (width as usize, height.unwrap_or(raw_h.unsigned_abs() as usize));
    let ncolors = if bpp <= 8 { if clr_used != 0 { clr_used } else { 1 << bpp } } else { 0 };
    let pal_off = dib + hdr;
    let palette: Vec<[u8; 4]> = (0..ncolors)
        .map(|i| {
            let p = pal_off + i * 4;
            let c = |o: usize| b.get(p + o).copied().unwrap_or(0);
            [c(2), c(1), c(0), 255]
        })
        .collect();
    let bits = bits_off.unwrap_or(pal_off + ncolors * 4);
    let stride = (w * bpp).div_ceil(32) * 4;
    let byte = |o: usize| b.get(o).copied().unwrap_or(0);
    let pal = |i: usize| palette.get(i).copied().unwrap_or([0, 0, 0, 255]);
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = bits + if top_down { y } else { h - 1 - y } * stride;
        for x in 0..w {
            px.push(match bpp {
                1 => pal(usize::from(byte(row + x / 8) >> (7 - x % 8) & 1)),
                4 => {
                    let v = byte(row + x / 2);
                    pal(usize::from(if x % 2 == 0 { v >> 4 } else { v & 0x0F }))
                }
                8 => pal(usize::from(byte(row + x))),
                _ => {
                    let p = row + x * bpp / 8;
                    [byte(p + 2), byte(p + 1), byte(p), 255]
                }
            });
        }
    }
    Some(Rgba { width: w, height: h, px })
}

fn decode_bmp(b: &[u8]) -> Option<Rgba> {
    if b.get(0..2)? != b"BM" {
        return None;
    }
    decode_dib(b, 14, Some(rd_u32(b, 10)? as usize), None)
}

/// First image of a `.ico` file, with its AND mask applied as alpha.
fn decode_ico(b: &[u8]) -> Option<Rgba> {
    if rd_u16(b, 4)? == 0 {
        return None;
    }
    let off = rd_u32(b, 18)? as usize;
    let w = rd_u32(b, off + 4)? as usize;
    let h = rd_u32(b, off + 8)? as usize / 2;
    let mut img = decode_dib(b, off, None, Some(h))?;
    let bpp = rd_u16(b, off + 14)? as usize;
    let clr_used = rd_u32(b, off + 32)? as usize;
    let ncolors = if bpp <= 8 { if clr_used != 0 { clr_used } else { 1 << bpp } } else { 0 };
    let xor_len = (w * bpp).div_ceil(32) * 4 * h;
    let and_off = off + rd_u32(b, off)? as usize + ncolors * 4 + xor_len;
    let and_stride = w.div_ceil(32) * 4;
    for y in 0..h {
        let row = and_off + (h - 1 - y) * and_stride;
        for x in 0..w {
            if b.get(row + x / 8).is_some_and(|v| v >> (7 - x % 8) & 1 == 1) {
                img.px[y * w + x][3] = 0;
            }
        }
    }
    Some(img)
}

fn to_color_image(img: &Rgba) -> ColorImage {
    let flat: Vec<u8> = img.px.iter().flatten().copied().collect();
    ColorImage::from_rgba_unmultiplied([img.width, img.height], &flat)
}

/// Window icon (`AD` icon from `AD.EXE`).
pub fn app_icon() -> Option<IconData> {
    let img = decode_ico(ICO_AD)?;
    Some(IconData {
        rgba: img.px.iter().flatten().copied().collect(),
        width: img.width as u32,
        height: img.height as u32,
    })
}

/// Textures for the Control Panel chrome.
pub struct Art {
    pub logo: TextureHandle,
    /// The red `LMETEOR` streak composited over the logo's top-left corner (white bits -> red).
    pub meteor: TextureHandle,
    pub on_switch: TextureHandle,
    pub off_switch: TextureHandle,
}

impl Art {
    pub fn load(ctx: &egui::Context) -> Option<Self> {
        let tex = |name: &str, img: &Rgba| ctx.load_texture(name, to_color_image(img), TextureOptions::NEAREST);
        let logo = decode_bmp(BMP_LOGO)?;
        let mut meteor = decode_bmp(BMP_LMETEOR)?;
        // DSPDxax with a red brush: source white -> brush (red), source black -> destination.
        for p in &mut meteor.px {
            *p = if p[0] as u32 + p[1] as u32 + p[2] as u32 > 384 { [255, 0, 0, 255] } else { [0, 0, 0, 0] };
        }
        Some(Self {
            logo: tex("ad-logo", &logo),
            meteor: tex("ad-meteor", &meteor),
            on_switch: tex("ad-on-sw", &decode_bmp(BMP_ON_SW)?),
            off_switch: tex("ad-off-sw", &decode_bmp(BMP_OFF_SW)?),
        })
    }

    /// Native pixel size of a texture.
    pub fn size(tex: &TextureHandle) -> egui::Vec2 {
        let [w, h] = tex.size();
        egui::vec2(w as f32, h as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_host_art() {
        let logo = decode_bmp(BMP_LOGO).unwrap();
        assert_eq!((logo.width, logo.height), (170, 63));
        let sw = decode_bmp(BMP_ON_SW).unwrap();
        assert_eq!((sw.width, sw.height), (18, 30));
        let m = decode_bmp(BMP_LMETEOR).unwrap();
        assert_eq!((m.width, m.height), (30, 22));
        let icon = decode_ico(ICO_AD).unwrap();
        assert_eq!((icon.width, icon.height), (32, 32));
        assert!(icon.px.iter().any(|p| p[3] == 0) && icon.px.iter().any(|p| p[3] == 255));
    }
}
