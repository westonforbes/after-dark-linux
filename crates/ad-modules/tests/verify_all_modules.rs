//! Comprehensive automated + visual contact-sheet verification for ALL 37 built-in After Dark 1.0a modules.
//!
//! Verifies:
//! 1. Full 37-module catalog registration, unique IDs, and `RANDOM.ADS` / `MESSAGES.ADS` / `GRAFSTAT.TXT` round-trips
//! 2. Headless software-GDI rendering of EVERY single one of the 37 modules (with a realistic synthetic
//!    Windows 3.1 desktop pre-painted onto the surface so screen-effect modules like Fade Away, Down the Drain,
//!    Puzzle, Punch Out, Spotlight, Mondrian, and Can of Worms visibly manipulate the desktop)
//! 3. Non-trivial output assertions (all 36 visual modules alter pixels cleanly; Blanker blanks cleanly to black)
//! 4. Exports 4 labeled PNG contact sheets (`target/verify_snapshots/sheet_1.png`..`sheet_4.png`)
//!    for 100% visual inspection of every single screensaver!

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use ad_modules::grafstat::GraphStatModule;
use ad_modules::message::{
    MESSAGE_RECORD_SIZE, MESSAGE_SLOT_COUNT, load_messages_ads, parse_messages_ads,
    serialize_messages_ads,
};
use ad_modules::{build_catalog, create_module_by_id};
use ad_sdk::canvas::{AdSurface, ROP_SRCCOPY};
use ad_sdk::palette::{AdPalette, rgb};
use ad_sdk::prefs::load_random_ads;
use ad_sdk::sound::AdSound;
use ad_sdk::{AdContext, AdModuleParams, AdResult, AdSystem, MsvcRand, SaverMode};
use ad_sdk::canvas::RECT;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Paint a crisp synthetic Windows 3.1 desktop onto `surface` before `do_initialize` + `do_blank`
/// so desktop-manipulating modules (Spotlight, Puzzle, Punch Out, Fade Away, Down the Drain, etc.)
/// operate on rich, unmistakable visual content.
fn paint_synthetic_win31_desktop(surface: &mut AdSurface) {
    let w = surface.width;
    let h = surface.height;
    // Classic Windows 3.1 teal wallpaper
    surface.fill_rect_rgb(0, 0, w, h, rgb(0, 128, 128));

    // Subtle checkered pixel dither pattern on desktop
    for y in (0..h).step_by(8) {
        for x in (0..w).step_by(8) {
            surface.set_pixel(x, y, rgb(0, 148, 148));
        }
    }

    // Program Manager window frame
    let wl = 28;
    let wt = 22;
    let wr = w - 28;
    let wb = h - 34;
    surface.rectangle(wl, wt, wr, wb, Some(rgb(0, 0, 0)), Some(rgb(192, 192, 192)));
    surface.rectangle(
        wl + 3,
        wt + 3,
        wr - 3,
        wb - 3,
        Some(rgb(0, 0, 0)),
        Some(rgb(255, 255, 255)),
    );
    // Navy blue Win3.1 title bar
    surface.fill_rect_rgb(wl + 4, wt + 4, wr - 4, wt + 22, rgb(0, 0, 128));
    // Menu bar
    surface.fill_rect_rgb(wl + 4, wt + 22, wr - 4, wt + 36, rgb(224, 224, 224));
    surface.line(wl + 4, wt + 36, wr - 4, wt + 36, 1, rgb(0, 0, 0));

    // Colorful Win3.1 application icons inside Program Manager
    let icon_colors = [
        rgb(255, 0, 0),
        rgb(0, 192, 0),
        rgb(0, 0, 255),
        rgb(255, 200, 0),
        rgb(192, 0, 192),
        rgb(0, 192, 220),
    ];
    for i in 0..6 {
        let col = i % 3;
        let row = i / 3;
        let ix = wl + 26 + col * 82;
        let iy = wt + 52 + row * 64;
        surface.rectangle(ix, iy, ix + 36, iy + 32, Some(rgb(0, 0, 0)), Some(icon_colors[i as usize]));
        surface.ellipse(
            ix + 6,
            iy + 5,
            ix + 30,
            iy + 27,
            Some(rgb(255, 255, 255)),
            Some(rgb(255, 255, 180)),
        );
        // Icon caption bar
        surface.fill_rect_rgb(ix - 8, iy + 36, ix + 44, iy + 44, rgb(0, 0, 128));
    }

    // Bottom minimized icons strip
    for i in 0..4 {
        let bx = 24 + i * 72;
        let by = h - 26;
        surface.rectangle(
            bx,
            by,
            bx + 54,
            by + 20,
            Some(rgb(0, 0, 0)),
            Some(rgb(220, 220, 220)),
        );
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn push_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(payload);
    let c = crc32(&out[start..]);
    out.extend_from_slice(&c.to_be_bytes());
}

/// Zero-dependency pure-Rust PNG encoder (RGB24 + stored DEFLATE blocks) for crisp contact sheets.
fn write_surface_png(surface: &AdSurface, path: &Path) {
    let w = surface.width as usize;
    let h = surface.height as usize;
    let pixels = surface.pixels();

    let mut raw = Vec::with_capacity(h * (1 + w * 3));
    for y in 0..h {
        raw.push(0); // filter type 0 (None)
        let row = &pixels[y * w..(y + 1) * w];
        for &p in row {
            raw.push(((p >> 16) & 0xFF) as u8); // R
            raw.push(((p >> 8) & 0xFF) as u8);  // G
            raw.push((p & 0xFF) as u8);         // B
        }
    }

    // Wrap raw scanlines in valid zlib stream using uncompressed DEFLATE stored blocks (max 65535 bytes each)
    let mut zlib = Vec::with_capacity(raw.len() + (raw.len() / 65535 + 1) * 5 + 6);
    zlib.push(0x78); // CMF: deflate, 32K window
    zlib.push(0x01); // FLG: FCHECK
    let mut pos = 0;
    while pos < raw.len() {
        let len = (raw.len() - pos).min(65535);
        let is_last = pos + len == raw.len();
        zlib.push(if is_last { 0x01 } else { 0x00 });
        zlib.extend_from_slice(&(len as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(len as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw[pos..pos + len]);
        pos += len;
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type: truecolor RGB
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    push_png_chunk(&mut png, b"IHDR", &ihdr);
    push_png_chunk(&mut png, b"IDAT", &zlib);
    push_png_chunk(&mut png, b"IEND", &[]);

    fs::write(path, png).expect("write png");
}

fn draw_centered_label(surface: &mut AdSurface, x: i32, y: i32, w: i32, h: i32, text: &str) {
    // DT_CENTER | DT_VCENTER | DT_SINGLELINE
    let (_, th) = surface.measure_text_font("Segoe UI", -15, true, false, text);
    let top = y + (h - th) / 2;
    let rc = RECT {
        left: x,
        top,
        right: x + w,
        bottom: top + th,
    };
    surface.draw_text_font("Segoe UI", -15, true, false, false, rgb(240, 244, 255), rc, true, false, text);
}

#[test]
fn verify_all_65_modules_and_render_contact_sheets() {
    let root = workspace_root();
    let assets_dir = root.join("assets");
    let snap_dir = root.join("target/verify_snapshots");
    fs::create_dir_all(&snap_dir).unwrap();

    // 1. Verify catalog completeness (all 77 built-in modules: 37 classic 1991 + 15 Simpsons + 13 Star Trek TNG + 12 Looney Tunes!)
    let catalog = build_catalog(None);
    assert_eq!(
        catalog.len(),
        77,
        "Expected all 77 built-in modules (37 After Dark 1.0a + 15 Simpsons + 13 Star Trek TNG + 12 Looney Tunes), got {}",
        catalog.len()
    );
    let mut seen_ids = BTreeSet::new();
    for entry in &catalog {
        assert!(
            seen_ids.insert(entry.meta.id.clone()),
            "Duplicate module id: {}",
            entry.meta.id
        );
    }

    // 2. Verify MESSAGES.ADS 1968-byte binary round-trip & GRAFSTAT.TXT 4-section parsing
    let msgs = load_messages_ads(assets_dir.to_str().unwrap());
    assert_eq!(msgs.len(), MESSAGE_SLOT_COUNT);
    let serialized = serialize_messages_ads(&msgs);
    assert_eq!(serialized.len(), MESSAGE_SLOT_COUNT * MESSAGE_RECORD_SIZE);
    let reparsed = parse_messages_ads(&serialized);
    assert_eq!(reparsed.len(), MESSAGE_SLOT_COUNT);
    assert_eq!(reparsed[0].text, msgs[0].text);

    let graf_txt = GraphStatModule::load_grafstat_text(assets_dir.to_str().unwrap());
    assert!(graf_txt.len() > 200);

    // 3. Verify every entry in authentic 1991 RANDOM.ADS resolves cleanly in our 37-module catalog
    let rads = load_random_ads(&assets_dir.join("RANDOM.ADS"));
    assert!(!rads.is_empty());
    for re in &rads {
        let found = catalog.iter().any(|c| {
            c.meta.id != "randomizer"
                && (c.meta.name.eq_ignore_ascii_case(&re.module_name)
                    || (!re.filename.eq_ignore_ascii_case("AD.EXE")
                        && c.meta.filename.eq_ignore_ascii_case(&re.filename)))
        });
        assert!(
            found,
            "RANDOM.ADS entry '{}' ({}) not found in catalog",
            re.module_name, re.filename
        );
    }

    // 4. Render EVERY SINGLE ONE of the 37 modules on a 320x240 surface pre-painted with Win3.1 desktop
    let tile_w = 320i32;
    let tile_h = 240i32;
    let mut desktop_ref = AdSurface::new(tile_w, tile_h).unwrap();
    paint_synthetic_win31_desktop(&mut desktop_ref);
    let desktop_pixels: Vec<u32> = desktop_ref.pixels().to_vec();

    let sound = AdSound::new();
    let rand_pool = vec!["toasters".to_string(), "starry".to_string(), "satori".to_string()];
    let empty_ctrls = BTreeMap::new();

    let mut rendered: Vec<(String, String, AdSurface)> = Vec::with_capacity(catalog.len());

    for (idx, entry) in catalog.iter().enumerate() {
        let mut module = create_module_by_id(&entry.meta.id, &rand_pool, &empty_ctrls)
            .unwrap_or_else(|| panic!("Failed to instantiate module '{}'", entry.meta.id));
        let meta = module.metadata();

        let mut surface = AdSurface::new(tile_w, tile_h).unwrap();
        paint_synthetic_win31_desktop(&mut surface);

        let mut system = AdSystem::new(tile_w, tile_h, SaverMode::Saver);
        system.sound_enabled = false;
        system.data_dir = assets_dir.to_string_lossy().to_string();
        system.slide_show_dir = assets_dir.join("bitmaps").to_string_lossy().to_string();

        let mut params = AdModuleParams::new(tile_w, tile_h, &meta.controls);
        let mut rng = MsvcRand::new(1991 + (idx as u32) * 37);

        let step_ms = meta.target_frame_ms.max(16) as u64;
        let frames: usize = match meta.id.as_str() {
            "satori" => 360,     // Let spiral refine cleanly across the screen
            "stained" => 220,    // Complete 1-2 stained-glass tiles + 3x3 tiling
            "mountain" => 100,   // Full water + land mountain rendering
            "grafstat" => 60,    // Draw several graphs
            "vertigo" => 90,
            "sgyra" | "rose" | "string" | "magic" => 110,
            "draino" | "puzzle" | "fade" | "worms" | "punch" => 90,
            "aqua" | "toasters" | "warp" | "laser" | "3dbounce" | "grav" => 80,
            "nocturne" => 120,   // Enough time for eyes to open & blink
            "rainstor" | "zot" => 90,
            _ => 70,
        };

        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: 0,
            };
            let _ = module.do_pre_initialize(&mut ctx);
            if let AdResult::PaletteRequest(kind) = module.do_initialize(&mut ctx) {
                ctx.module.palette = Some(AdPalette::build(kind));
            }
            if let AdResult::PaletteRequest(kind) = module.do_blank(&mut ctx) {
                ctx.module.palette = Some(AdPalette::build(kind));
            }
        }

        let mut best_snapshot: Option<AdSurface> = None;
        let mut best_lit = 0usize;

        for f in 1..=frames {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (f as u64) * step_ms,
            };
            match module.do_draw_frame(&mut ctx) {
                AdResult::RestartMe => {
                    let _ = module.do_close(&mut ctx);
                    let _ = module.do_initialize(&mut ctx);
                    let _ = module.do_blank(&mut ctx);
                }
                AdResult::PaletteRequest(kind) => {
                    ctx.module.palette = Some(AdPalette::build(kind));
                }
                _ => {}
            }
            if meta.id == "zot" {
                let lit = surface
                    .pixels()
                    .iter()
                    .filter(|&&p| (p & 0x00FF_FFFF) != 0)
                    .count();
                if lit > best_lit {
                    best_lit = lit;
                    best_snapshot = surface.clone_surface();
                }
            }
        }

        // Measure live rendered pixels & clone snapshot BEFORE `do_close` runs on exit!
        let snapshot = best_snapshot
            .unwrap_or_else(|| surface.clone_surface().expect("clone snapshot surface"));
        let lit_pixels = snapshot
            .pixels()
            .iter()
            .filter(|&&p| (p & 0x00FF_FFFF) != 0)
            .count();
        let diff_from_desktop = snapshot
            .pixels()
            .iter()
            .zip(desktop_pixels.iter())
            .filter(|&(&a, &b)| (a & 0x00FF_FFFF) != (b & 0x00FF_FFFF))
            .count();

        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (frames as u64) * step_ms,
            };
            let _ = module.do_close(&mut ctx);
        }

        if meta.id == "blanker" {
            assert_eq!(lit_pixels, 0, "Blanker should blank screen to pure black");
        } else {
            assert!(
                lit_pixels > 40,
                "Module '{}' ({}) rendered too few lit pixels ({})",
                meta.name,
                meta.id,
                lit_pixels
            );
            assert!(
                diff_from_desktop > 40,
                "Module '{}' ({}) barely modified desktop ({} px)",
                meta.name,
                meta.id,
                diff_from_desktop
            );
        }

        rendered.push((meta.name, meta.id, snapshot));
    }

    // 5. Pack all 37 modules into 4 crisp PNG contact sheets (10 tiles per sheet -> 2x5 or 3x4;
    //    let's use 4 sheets of 3 cols x 4 rows = up to 12 tiles each; 10, 10, 10, 7 or 10/9/9/9!)
    let cols = 3i32;
    let pad = 8i32;
    let hdr_h = 24i32;
    let cell_w = tile_w + pad;
    let cell_h = tile_h + hdr_h + pad;

    let chunks: Vec<&[(String, String, AdSurface)]> = rendered.chunks(10).collect();
    assert_eq!(chunks.len(), 8);

    for (sheet_idx, chunk) in chunks.iter().enumerate() {
        let rows = ((chunk.len() as i32) + cols - 1) / cols;
        let sheet_w = cols * cell_w + pad;
        let sheet_h = rows * cell_h + pad;
        let mut sheet = AdSurface::new(sheet_w, sheet_h).unwrap();
        sheet.fill_rect_rgb(0, 0, sheet_w, sheet_h, rgb(24, 26, 34));

        for (i, (name, id, tile)) in chunk.iter().enumerate() {
            let c = (i as i32) % cols;
            let r = (i as i32) / cols;
            let x = pad + c * cell_w;
            let y = pad + r * cell_h;

            // Header bar
            sheet.fill_rect_rgb(x, y, x + tile_w, y + hdr_h, rgb(36, 48, 86));
            let label = format!("{} ({})", name, id);
            draw_centered_label(&mut sheet, x, y, tile_w, hdr_h, &label);

            // Tile border + image
            sheet.bit_blt_surface(x, y + hdr_h, tile_w, tile_h, &tile, 0, 0, ROP_SRCCOPY);
            sheet.rectangle(
                x - 1,
                y - 1,
                x + tile_w + 1,
                y + hdr_h + tile_h + 1,
                Some(rgb(90, 105, 140)),
                None,
            );
        }

        let out_path = snap_dir.join(format!("sheet_{}.png", sheet_idx + 1));
        write_surface_png(&sheet, &out_path);
    }

    // 6. Render dedicated high-res 16:9 widescreen (960x540) contact sheets of ALL 15 Simpsons modules!
    let sim_w = 960i32;
    let sim_h = 540i32;
    let mut sim_rendered: Vec<(String, String, AdSurface)> = Vec::new();
    let simpsons_entries: Vec<_> = catalog
        .iter()
        .filter(|e| e.meta.id.starts_with("simpsons_"))
        .collect();
    assert_eq!(simpsons_entries.len(), 15);

    for (idx, entry) in simpsons_entries.iter().enumerate() {
        let mut module = create_module_by_id(&entry.meta.id, &rand_pool, &empty_ctrls).unwrap();
        let meta = module.metadata();
        let mut surface = AdSurface::new(sim_w, sim_h).unwrap();
        paint_synthetic_win31_desktop(&mut surface);

        let mut system = AdSystem::new(sim_w, sim_h, SaverMode::Saver);
        system.sound_enabled = false;
        system.data_dir = assets_dir.to_string_lossy().to_string();
        let mut params = AdModuleParams::new(sim_w, sim_h, &meta.controls);
        let mut rng = MsvcRand::new(1994 + (idx as u32) * 53);

        let step_ms = meta.target_frame_ms.max(16) as u64;
        let frames: usize = match meta.id.as_str() {
            "simpsons_chalkboard" => 110,
            "simpsons_how_to_draw" => 95,
            "simpsons_homer_eats" => 85,
            "simpsons_objets_bart" => 95,
            "simpsons_trivia" => 40,
            _ => 65,
        };
        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: 0,
            };
            let _ = module.do_initialize(&mut ctx);
            let _ = module.do_blank(&mut ctx);
        }
        for f in 1..=frames {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (f as u64) * step_ms,
            };
            let _ = module.do_draw_frame(&mut ctx);
        }
        let snap = surface.clone_surface().unwrap();
        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (frames as u64) * step_ms,
            };
            let _ = module.do_close(&mut ctx);
        }
        sim_rendered.push((meta.name, meta.id, snap));
    }

    let s_cols = 2i32;
    let s_cell_w = sim_w + pad;
    let s_cell_h = sim_h + hdr_h + pad;
    for (si, chunk) in sim_rendered.chunks(8).enumerate() {
        let rows = ((chunk.len() as i32) + s_cols - 1) / s_cols;
        let sw = s_cols * s_cell_w + pad;
        let sh = rows * s_cell_h + pad;
        let mut sheet = AdSurface::new(sw, sh).unwrap();
        sheet.fill_rect_rgb(0, 0, sw, sh, rgb(20, 24, 34));
        for (i, (name, id, tile)) in chunk.iter().enumerate() {
            let c = (i as i32) % s_cols;
            let r = (i as i32) / s_cols;
            let x = pad + c * s_cell_w;
            let y = pad + r * s_cell_h;
            sheet.fill_rect_rgb(x, y, x + sim_w, y + hdr_h, rgb(180, 130, 12));
            draw_centered_label(
                &mut sheet,
                x,
                y,
                sim_w,
                hdr_h,
                &format!("{} ({}) — 16:9 HD", name, id),
            );
            sheet.bit_blt_surface(x, y + hdr_h, sim_w, sim_h, &tile, 0, 0, ROP_SRCCOPY);
        }
        write_surface_png(&sheet, &snap_dir.join(format!("simpsons_sheet_{}.png", si + 1)));
    }

    // 7. Render dedicated high-res 16:9 widescreen (960x540) contact sheets of ALL 13 Star Trek: TNG modules!
    let tng_w = 960;
    let tng_h = 540;
    let mut tng_rendered: Vec<(String, String, AdSurface)> = Vec::new();
    let tng_entries: Vec<_> = catalog
        .iter()
        .filter(|e| e.meta.id.starts_with("tng_"))
        .collect();
    assert_eq!(tng_entries.len(), 13);

    for (idx, entry) in tng_entries.iter().enumerate() {
        let mut module = create_module_by_id(&entry.meta.id, &rand_pool, &empty_ctrls).unwrap();
        let meta = module.metadata();
        let mut surface = AdSurface::new(tng_w, tng_h).unwrap();
        paint_synthetic_win31_desktop(&mut surface);

        let mut system = AdSystem::new(tng_w, tng_h, SaverMode::Saver);
        system.sound_enabled = false;
        system.data_dir = assets_dir.to_string_lossy().to_string();
        let mut params = AdModuleParams::new(tng_w, tng_h, &meta.controls);
        let mut rng = MsvcRand::new(1701 + (idx as u32) * 47);

        let step_ms = meta.target_frame_ms.max(16) as u64;
        let frames: usize = match meta.id.as_str() {
            "tng_borg" => 95,
            "tng_nanites" => 95,
            "tng_warp_effect" => 75,
            "tng_tachyon" => 75,
            _ => 55,
        };
        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: 0,
            };
            let _ = module.do_initialize(&mut ctx);
            let _ = module.do_blank(&mut ctx);
        }
        for f in 1..=frames {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (f as u64) * step_ms,
            };
            let _ = module.do_draw_frame(&mut ctx);
        }
        let snap = surface.clone_surface().unwrap();
        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (frames as u64) * step_ms,
            };
            let _ = module.do_close(&mut ctx);
        }
        tng_rendered.push((meta.name, meta.id, snap));
    }

    let t_cols = 2;
    let t_cell_w = tng_w + pad;
    let t_cell_h = tng_h + hdr_h + pad;
    for (si, chunk) in tng_rendered.chunks(8).enumerate() {
        let rows = ((chunk.len() as i32) + t_cols - 1) / t_cols;
        let sw = t_cols * t_cell_w + pad;
        let sh = rows * t_cell_h + pad;
        let mut sheet = AdSurface::new(sw, sh).unwrap();
        sheet.fill_rect_rgb(0, 0, sw, sh, rgb(12, 16, 28));
        for (i, (name, id, tile)) in chunk.iter().enumerate() {
            let c = (i as i32) % t_cols;
            let r = (i as i32) / t_cols;
            let x = pad + c * t_cell_w;
            let y = pad + r * t_cell_h;
            sheet.fill_rect_rgb(x, y, x + tng_w, y + hdr_h, rgb(155, 85, 195));
            draw_centered_label(&mut sheet, x, y, tng_w, hdr_h, &format!("{} ({}) — 16:9 HD", name, id));
            sheet.bit_blt_surface(x, y + hdr_h, tng_w, tng_h, &tile, 0, 0, ROP_SRCCOPY);
        }
        write_surface_png(&sheet, &snap_dir.join(format!("tng_sheet_{}.png", si + 1)));
    }

    // 8. Render dedicated high-res 16:9 widescreen (960x540) contact sheets of ALL 12 Looney Tunes modules!
    let mut looney_rendered: Vec<(String, String, AdSurface)> = Vec::new();
    let looney_entries: Vec<_> = catalog
        .iter()
        .filter(|e| e.meta.id.starts_with("looney_"))
        .collect();
    assert_eq!(looney_entries.len(), 12);

    for (idx, entry) in looney_entries.iter().enumerate() {
        let mut module = create_module_by_id(&entry.meta.id, &rand_pool, &empty_ctrls).unwrap();
        let meta = module.metadata();
        let mut surface = AdSurface::new(sim_w, sim_h).unwrap();
        paint_synthetic_win31_desktop(&mut surface);

        let mut system = AdSystem::new(sim_w, sim_h, SaverMode::Saver);
        system.sound_enabled = false;
        system.data_dir = assets_dir.to_string_lossy().to_string();
        let mut params = AdModuleParams::new(sim_w, sim_h, &meta.controls);
        let mut rng = MsvcRand::new(1995 + (idx as u32) * 61);

        let step_ms = meta.target_frame_ms.max(16) as u64;
        let frames: usize = match meta.id.as_str() {
            "looney_acme_shopping" => 65,
            "looney_wockets" => 40,
            _ => 55,
        };
        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: 0,
            };
            let _ = module.do_initialize(&mut ctx);
            let _ = module.do_blank(&mut ctx);
        }
        for f in 1..=frames {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (f as u64) * step_ms,
            };
            let _ = module.do_draw_frame(&mut ctx);
        }
        let snap = surface.clone_surface().unwrap();
        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (frames as u64) * step_ms,
            };
            let _ = module.do_close(&mut ctx);
        }
        looney_rendered.push((meta.name, meta.id, snap));
    }

    for (si, chunk) in looney_rendered.chunks(6).enumerate() {
        let rows = ((chunk.len() as i32) + s_cols - 1) / s_cols;
        let sw = s_cols * s_cell_w + pad;
        let sh = rows * s_cell_h + pad;
        let mut sheet = AdSurface::new(sw, sh).unwrap();
        sheet.fill_rect_rgb(0, 0, sw, sh, rgb(18, 14, 24));
        for (i, (name, id, tile)) in chunk.iter().enumerate() {
            let c = (i as i32) % s_cols;
            let r = (i as i32) / s_cols;
            let x = pad + c * s_cell_w;
            let y = pad + r * s_cell_h;
            sheet.fill_rect_rgb(x, y, x + sim_w, y + hdr_h, rgb(195, 52, 28));
            draw_centered_label(
                &mut sheet,
                x,
                y,
                sim_w,
                hdr_h,
                &format!("{} ({}) — 16:9 HD", name, id),
            );
            sheet.bit_blt_surface(x, y + hdr_h, sim_w, sim_h, &tile, 0, 0, ROP_SRCCOPY);
        }
        write_surface_png(&sheet, &snap_dir.join(format!("looney_sheet_{}.png", si + 1)));
    }

    // 9. Render dedicated 16:9 HD (1280x720) showcase sheet of the upgraded Classic 1991 modules!
    let cls_ids = [
        "toasters", "clocks", "aqua", "nocturne", "globe", "logo", "slidesho", "starry",
        "grafstat", "message", "spotlite", "worms",
    ];
    let cls_w = 960i32;
    let cls_h = 540i32;
    let mut cls_rendered: Vec<(String, String, AdSurface)> = Vec::new();
    for (idx, cid) in cls_ids.iter().enumerate() {
        let mut module = create_module_by_id(cid, &rand_pool, &empty_ctrls).unwrap();
        let meta = module.metadata();
        let mut surface = AdSurface::new(cls_w, cls_h).unwrap();
        paint_synthetic_win31_desktop(&mut surface);
        let mut system = AdSystem::new(cls_w, cls_h, SaverMode::Saver);
        system.sound_enabled = false;
        system.data_dir = assets_dir.to_string_lossy().to_string();
        system.slide_show_dir = assets_dir.join("bitmaps").to_string_lossy().to_string();
        let mut params = AdModuleParams::new(cls_w, cls_h, &meta.controls);
        let mut rng = MsvcRand::new(1991 + (idx as u32) * 73);
        let step_ms = meta.target_frame_ms.max(16) as u64;
        let frames = if *cid == "nocturne" { 160 } else { 90 };
        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: 0,
            };
            let _ = module.do_initialize(&mut ctx);
            let _ = module.do_blank(&mut ctx);
        }
        for f in 1..=frames {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (f as u64) * step_ms,
            };
            let _ = module.do_draw_frame(&mut ctx);
        }
        let snap = surface.clone_surface().unwrap();
        {
            let mut ctx = AdContext {
                system: &mut system,
                module: &mut params,
                surface: &mut surface,
                sound: &sound,
                rng: &mut rng,
                elapsed_ms: (frames as u64) * step_ms,
            };
            let _ = module.do_close(&mut ctx);
        }
        cls_rendered.push((meta.name, meta.id, snap));
    }

    for (si, chunk) in cls_rendered.chunks(6).enumerate() {
        let rows = ((chunk.len() as i32) + s_cols - 1) / s_cols;
        let sw = s_cols * s_cell_w + pad;
        let sh = rows * s_cell_h + pad;
        let mut sheet = AdSurface::new(sw, sh).unwrap();
        sheet.fill_rect_rgb(0, 0, sw, sh, rgb(16, 20, 30));
        for (i, (name, id, tile)) in chunk.iter().enumerate() {
            let c = (i as i32) % s_cols;
            let r = (i as i32) / s_cols;
            let x = pad + c * s_cell_w;
            let y = pad + r * s_cell_h;
            sheet.fill_rect_rgb(x, y, x + cls_w, y + hdr_h, rgb(42, 76, 138));
            draw_centered_label(
                &mut sheet,
                x,
                y,
                cls_w,
                hdr_h,
                &format!("{} ({}) — 16:9 HD", name, id),
            );
            sheet.bit_blt_surface(x, y + hdr_h, cls_w, cls_h, &tile, 0, 0, ROP_SRCCOPY);
        }
        write_surface_png(&sheet, &snap_dir.join(format!("classic_hd_sheet_{}.png", si + 1)));
    }
}
