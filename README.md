# After Dark v2.5 for Linux — Native Rust Port (77 Built-in Screensavers)

A complete **disassembly- & resource-backed native Rust port** of **Berkeley Systems' After Dark** for **Linux** (X11 and Wayland), uniting **all 77 classic screensaver modules** across four legendary collections into a single executable (`after-dark`) with full **16:9 HD / 4K widescreen scaling** and **pixel-verified 1991–1995 animations**. It is a fork of the Windows 11 port: the decompiled modules are unchanged, running on a pure-Rust re-implementation of the Win16/Win32 GDI drawing model.

- **Classic After Dark 1.0a (1991)** — **37 modules** (all 34 `.AD` modules, both `AD.EXE` built-in savers, and the `ADMODULE.SDK` Blanker sample)
- **The Simpsons After Dark (1994)** — **15 modules** (2,647 `RLEP` frames, 12,175 `OFst`/`OFtb` compound poses, 87 digitized cast voice clips, 9 polyphonic MIDI songs)
- **Star Trek: The Next Generation After Dark (1994)** — **13 modules** (1,518 `RLEP` frames, 3,240 `OFst`/`OFtb` compound poses, 56 bridge/cast audio clips, 8 MIDI scores)
- **Looney Tunes After Dark (1995)** — **12 modules** (2,050 `RLEP` frames, 8,534 `OFst`/`OFtb` compound poses, 103 cast voice/SFX clips including MACE 3:1 ADPCM audio, 13 MIDI cartoon themes)

---

## Visual Galleries (16:9 HD Contact Sheets)

### The Simpsons Collection (1994 — All 15 Modules, 16:9 HD)

| The Simpsons (1–8) | The Simpsons (9–15) |
| :---: | :---: |
| ![The Simpsons 1-8](docs/screenshots/simpsons_sheet_1.png) | ![The Simpsons 9-15](docs/screenshots/simpsons_sheet_2.png) |

### Star Trek: The Next Generation Collection (1994 — All 13 Modules, 16:9 HD)

| Star Trek: TNG (1–8) | Star Trek: TNG (9–13) |
| :---: | :---: |
| ![Star Trek TNG 1-8](docs/screenshots/tng_sheet_1.png) | ![Star Trek TNG 9-13](docs/screenshots/tng_sheet_2.png) |

### Looney Tunes Collection (1995 — All 12 Modules, 16:9 HD)

| Looney Tunes (1–6) | Looney Tunes (7–12) |
| :---: | :---: |
| ![Looney Tunes 1-6](docs/screenshots/looney_sheet_1.png) | ![Looney Tunes 7-12](docs/screenshots/looney_sheet_2.png) |

### Classic After Dark 1.0a (1991 — 16:9 HD Widescreen Showcase)

| Classic HD Showcase (1–6) | Classic HD Showcase (7–12) |
| :---: | :---: |
| ![Classic HD 1-6](docs/screenshots/classic_hd_sheet_1.png) | ![Classic HD 7-12](docs/screenshots/classic_hd_sheet_2.png) |

### Full 77-Module Catalog Contact Sheets (Alphabetical)

| Modules 1–10 | Modules 11–20 |
| :---: | :---: |
| ![Modules 1-10](docs/screenshots/sheet_1.png) | ![Modules 11-20](docs/screenshots/sheet_2.png) |

| Modules 21–30 | Modules 31–40 |
| :---: | :---: |
| ![Modules 21-30](docs/screenshots/sheet_3.png) | ![Modules 31-40](docs/screenshots/sheet_4.png) |

| Modules 41–50 | Modules 51–60 |
| :---: | :---: |
| ![Modules 41-50](docs/screenshots/sheet_5.png) | ![Modules 51-60](docs/screenshots/sheet_6.png) |

| Modules 61–70 | Modules 71–77 |
| :---: | :---: |
| ![Modules 61-70](docs/screenshots/sheet_7.png) | ![Modules 71-77](docs/screenshots/sheet_8.png) |

---

## Linux Port

### Install

Requirements: a Rust toolchain (`cargo`, edition 2024), and at runtime an X11 or Wayland desktop. No `-dev` packages are needed to build: X11 is spoken natively (`x11rb`), and audio libraries (PulseAudio/PipeWire, or ALSA) are loaded at runtime.

```sh
./install.sh            # builds a release binary and installs it into ~/.local, enables the idle daemon at login
./install.sh --uninstall
```

Then open **After Dark** from your applications menu (or run `after-dark`).

Optional but recommended: a General MIDI SoundFont for the Simpsons / Star Trek / Looney Tunes music (Debian/Ubuntu: `timgm6mb-soundfont` or `fluid-soundfont-gm`; set `AD_SOUNDFONT=/path/to/file.sf2` to pick one). Without one, a small built-in synthesizer plays the songs.

### How it fits together

| Command | Role |
|---|---|
| `after-dark` | **Control Panel** — choose a module, tweak its controls with a live **Demo**, Setup (timeout, hot corners, hotkey, scaling, multi-monitor, frame rate, audio), Randomizer, Messages / GraphStat editors. |
| `after-dark daemon` | **Idle monitor** (the Windows app's resident role): starts the saver after the timeout, on the *Sleep Now* hot corner, or on the Sleep Now hotkey (default **Ctrl+Shift+S**). Started at login via `~/.config/autostart/after-dark-daemon.desktop`. Re-reads settings whenever the Control Panel saves them, and respects session idle inhibitors (video playback, presentations). |
| `after-dark run [--module ID]` | Run the saver now. Any key, click, or mouse move beyond 12 px wakes it; hold **Shift** while it starts to just blank the screen. |
| `after-dark sleep-now` | Same as `run` with the selected module — bind it to a custom keyboard shortcut on Wayland. |
| `after-dark list` | List module ids. |

Settings live in `~/.config/after-dark/` (`AD_PREFS.INI`, `RANDOM.ADS`, `MESSAGES.ADS`, `GRAFSTAT.TXT`, in the original 1991 formats). External plugin modules go in `~/.local/share/after-dark/modules/*.so`.

**GNOME / KDE screen blanking**: the desktop's own blank-screen timer still applies. Set it (GNOME: *Settings → Power → Screen Blank*) longer than After Dark's timeout, or to *Never*, so you get to see the toasters.

**xscreensaver**: every module can also run as an xscreensaver hack. Add lines like these to the `programs:` list in `~/.xscreensaver`:

```
"Flying Toasters"   after-dark run --module toasters --root \n\
"Starry Night"      after-dark run --module starry --root   \n\
```

(`--window-id XID` / `$XSCREENSAVER_WINDOW` are honoured; sound is muted in this mode.)

### X11 vs. Wayland

| Feature | X11 | Wayland |
|---|---|---|
| Fullscreen saver, all modules, multi-monitor | ✅ | ✅ |
| Idle timeout | ✅ (MIT-SCREEN-SAVER) | ✅ GNOME / KDE (D-Bus) |
| Desktop-effect modules on the live desktop (Taz, Puzzle, Fade Away, Borg, …) | ✅ real desktop snapshot | Classic Windows 3.x teal desktop instead (Wayland has no unprompted screen capture) |
| Hot corners, Sleep Now hotkey, Shift-to-blank | ✅ | — (use `after-dark sleep-now` on a shortcut) |

### What changed from the Windows port

- **`ad-sdk` canvas**: Win32 GDI is replaced by a software GDI with the same API and semantics — all 256 ternary ROPs, the 16 ROP2 pen modes, monochrome-bitmap colorization via text/background colors, regions (union/intersect/xor/diff, clipping), GDI shape conventions (excluded right/bottom edges, `LineTo` end-pixel exclusion, NULL-pen quirks, counter-clockwise arcs/pies/chords, round-capped wide pens), `COLORONCOLOR` stretching, hatch & pattern brushes, and fontconfig-resolved text (`Arial` → Liberation Sans). Verified against the Windows-rendered contact sheets in `docs/screenshots/`.
- **Audio**: `waveOut` / `midiOut` → a mixer thread on PulseAudio (or ALSA), with SoundFont MIDI (`rustysynth`).
- **Plugins**: `.dll` → `.so`; the C ABI is now `ad_module_entry_v2(msg, AdCSurface*, state)` — the draw target is a pixel buffer instead of an `HDC` (see `crates/ad-blanker-sdk-example`).
- **Host**: the Win32 Control Panel / tray app is split into the eframe Control Panel, the idle daemon, and the winit/softbuffer saver runtime. The system-tray icon was dropped (GNOME has no tray); the daemon plus autostart replaces its role.

---

## What's New in Version 2.5

1. **Full 16:9 HD / 4K Widescreen Scaling & HiDPI Rendering Across All 4 Collections**:
   - Added continuous fractional-scale (`f32`) sprite, bitmap-font, stage-anchored cycle, and transparent mask blitting primitives (`stretch_blt_bitmap_with_brush`, `stretch_blt_surface_with_brush`, `blit_transformed_f`, `draw_stage_frame_f_ex`, `draw_cycle_frame_f`, `draw_bitmap_text_f`, `draw_looney_text_f`, `stage_fit`) so retro 640×480 sprites, fonts, and stages fill modern 1080p, 1440p, and 4K displays crisply without tiny postage-stamp sprites or blocky integer jumps.
2. **Forensic Animation & Sprite Overhaul Across All 15 *The Simpsons* Modules**:
   - **Simpsons Clocks**: Rebuilt all 3 animated wall clocks from exact pixel-measured geometry — **Duff Brewery Cuckoo Clock** (centered dial hands, recessed dark brick archways with Barney & Lenny behind hinged 3-stage double doors, Homer reclining-chair slide-out on alternate chimes, flickering purple `"Duff"` neon + sparks, forward-facing 7-frame scurrying rat), **Krusty Pop-Eye Clock** (forehead watch dial hands, roving eyeballs & pupils synced to the swinging pendulum pink tongue, chime-only starburst explosion + hair sparks + spring-popping eyeballs seated inside the eye sockets), and **Maggie Clock** (template-matched blinking eyes & pacifier + toddling/tripping Maggie underneath).
   - **Krusty**: Replaced sequential raw-frame dumping with 12 curated multi-layer theater routines compositing floor/wall spotlights, synchronized legs/unicycle frames, juggling, shadow puppets, pie-in-face/bandaged stars, and tomato heckling weighted by the `Success:` slider, centered cleanly on the spotlight while filtering Director offstage solid-black matte masks.
   - **Grass Skirts**: Stage-anchored Homer & Ned lawn-mowing walk cycles (zero foot slipping), clean wooden picket fence border against sky, mowed grass stripes aligned directly beneath the mower decks, jam-causing yard obstacles (Krusty doll, plutonium rod), authentic stuck/wave reactions, and toddling Maggie cameo.
   - **Homer Eats & Objets B'art**: Corrected Homer's chomp/walk/goodie/choke ranges (with full **Marge** chocolate-syrup/sugar/knife cameo) and Bart/Skinner native left-facing sprite flipping & spray/scold ranges (with **Milhouse** and **Nelson "HA-HA!"** cameos).
   - **Lisa's Mood Swings, Simpsons Trivia, Grampa's Wisdom, Simpsons Physics, Itchy & Scratchy, Mr. Burns & Snowball I**: Fixed Lisa's sax cycle to composite full-body poses over her floor spotlight; restricted *Trivia*'s shattering Krusty gauge, sparking switches, strobe, and CRT zig-zag interference strictly to `Meltdown` while adding widescreen Sector 7-G flanking instrumentation panels and removing an overlapping duplicate wall poster; mapped Grampa's exact talk/stand/recline/dream-cloud runs; replaced *Physics* character splitting with an exact 19-character alphabetical frame-slice table; stabilized skit cameras in *Itchy & Scratchy* and *Mr. Burns*; and smoothed *Snowball I*'s angel wing animation to ping-pong.
3. **Star Trek: TNG Widescreen LCARS UI Toolkit & Holodeck Upgrade**:
   - Built a dedicated widescreen **LCARS** layout engine (`crates/ad-modules/src/tng/mod.rs`) with authentic rounded elbows, telemetry panels, and high-DPI typography across *Counselor Troi*, *Officer's Review*, *Personnel Files*, *Science Stations*, *Encounters*, and *Starfleet Messages*, plus full-frame 16:9 HD scaling for *Data Dances*, *Starbase*, *The Borg*, *Nanites*, *Tachyon*, *Warp Effect*, and *Worf's Weapons*.
4. **Looney Tunes 16:9 HD Widescreen Stages & Fractional Typography**:
   - Upgraded all 12 Looney Tunes modules with widescreen theater stages, curtains, tiled ACME backdrops, and fractional-scale Looney bitmap text (*ACME Home Shopping*, *Cartoons 101*, *Conducktor*, *Desquetoppe D'Amour*, *Looney Messages*, *Marvin's Invasion*, *Michigan J. Frog*, *Putty Tat Splat!*, *Rabbitron*, *Taz Desktop*, *Wockets' Wed Gware*, *Yosemite Sam*).
5. **Classic After Dark 1.0a (1991) HiDPI & 16:9 HD Scaling**:
   - Upgraded classic bitmap and vector modules (*Flying Toasters*, *Aquatic Realm*, *Clocks*, *Nocturnes*, *Globe*, *Logo*, *Slide Show*, *Starry Night*, *GraphStat*, *Messages*, *Spotlight*, *Can of Worms*, *Punch Out*, *Rain Storm*, *Warp!*) to scale sprites, radii, stroke widths, and typography cleanly to native HD/4K resolutions, validated by two new 16:9 HD contact sheets (`classic_hd_sheet_1.png` & `classic_hd_sheet_2.png`).

---

## What's New in Version 2.0

1. **77 Built-in Screensavers Across 4 Collections**:
   - Complete integration of **Classic 1991 (`37`)**, **The Simpsons (`15`)**, **Star Trek: The Next Generation (`13`)**, and **Looney Tunes (`12`)** with all original `RLEP`/`OFst`/`OFtb` compound animations, custom bitmap fonts, trivia databases, digitized cast voices (`WinMM waveOut`), and polyphonic Standard MIDI (`WinMM midiOutShortMsg`) music.
2. **Real Windows 11 Desktop Overlay for All Desktop-Interacting Modules**:
   - Desktop modules (*Homer Eats*, *Grass Skirts*, *Itchy & Scratchy*, *Objets d'Bart*, *Snowball II*, *Bad Dog!*, *Borg*, *Crystalline Entity*, *Parallels*, *Desquetoppe D'Amour*, *Putty Tat Splat!*, *Taz Desktop*, *Yosemite Sam*, *Marvin's Invasion*, *Can of Worms*, *Down the Drain*, *Hard Rain*, *Punch Out*, *Puzzle*, *Spotlight*) capture and animate directly over your live Windows 11 desktop.
3. **Frame Rate & Smoothness Modes (`When / Setup...`)**:
   - **Authentic Retro (15–30 FPS)** — Original 1991–1995 module frame pacing (`target_frame_ms`).
   - **Smooth 60 FPS** — 16 ms frame pacing for silky movement on modern displays.
   - **High Refresh 120+ FPS** — 8 ms frame pacing for high-refresh monitors.
4. **6 Display Scale & Filter Modes (`When / Setup...`)**:
   - **Auto (Smart Retro 2x / 3x)** — Automatically chooses integer pixel scaling for 1080p/1440p/4K displays.
   - **1x (Native Monitor Pixels)** — Full monitor resolution.
   - **2x (Crisp Integer Pixel Art)** — Nearest-neighbor 2× pixel art scaling.
   - **3x (Crisp 4K Integer)** — Nearest-neighbor 3× pixel art scaling.
   - **640×480 Classic 4:3 Stage** — Authentic 640×480 4:3 canvas centered with aspect-correct pillarboxing on widescreen monitors.
   - **2x Retro CRT Scanlines** — 2× integer scaling with a classic CRT scanline overlay.
5. **Collection Filter, Live Search Box & Favorites (`★ Fav`)**:
   - Filter the Control Panel module list by collection (**All Collections (77)**, **Classic 1991 (37)**, **The Simpsons (15)**, **Star Trek: TNG (13)**, **Looney Tunes (12)**, **★ Favorites**), search live across all 77 modules, and star your favorites with one click.
6. **Audio Mixer & Test Audio Button (`When / Setup...`)**:
   - Independent **Voice / SFX Volume (0–100%)** and **MIDI Music Volume (0–100%)** controls with real-time PCM/tone amplitude scaling and MIDI velocity/CC#7 scaling, plus a **Test Audio** button.
7. **Pack-Aware Randomizer & One-Click Playlists (`Randomizer` → `Choose...`)**:
   - Every module in the Randomizer selector shows its `[Classic]`, `[Simpsons]`, `[Star Trek]`, or `[Looney Tunes]` collection badge, with 8 one-click preset buttons: **All (76)**, **Classic 1991**, **Simpsons**, **Star Trek**, **Looney Tunes**, **All Cartoons**, **★ Favorites**, and **Clear All**.
8. **4 Multi-Monitor Modes (`When / Setup...`)**:
   - **Independent Per Monitor (Same Module)** — Each display runs its own independent instance of the active module.
   - **Different Module Per Monitor (Random Pool)** — Each connected monitor runs a different screensaver chosen from your Randomizer pool simultaneously.
   - **Spanned Across All Monitors** — A single unified virtual-desktop canvas across all displays.
   - **Primary Monitor Only** — Runs on the primary display only.

---

## Complete Catalog of All 77 Built-in Screensavers

### Collection 1: Classic After Dark 1.0a (1991 — 37 Modules)

| Module | Original Binary | Highlights |
|---|---|---|
| **Aquatic Realm** | `AQUA.AD` | All 76 original sprites (angelfish, seahorses, butterflyfish, crabs, jellyfish, kelp, gravel), 40-fish school, bubbles & water-column depth |
| **Blanker** | `ADMODULE.SDK` | Solid-black blanker + loadable external `.so` plugin SDK sample |
| **Can of Worms** | `WORMS.AD` | Rainbow segmented worms burrowing across the captured live desktop |
| **Clocks** | `CLOCKS.AD` | All 14 original bitmaps (`Modern`, `Antique`, `Digital` 0–9), exact trig tables, PC-speaker tick & hourly chime |
| **Down the Drain** | `DRAINO.AD` | Rotating spiral vortex draining the captured desktop into black |
| **Fade Away** | `FADE.AD` | 1:1 1991 wipe/spiral pixel-fade styles erasing the captured desktop |
| **Flying Toasters** | `TOASTERS.AD` | All 12 original 64×64 sprites, exact wing cycle, collision avoidance, toast darkness |
| **GeoBounce** | `3DBOUNCE.AD` | 5 shaded 3D Platonic solids (Tetrahedron..Icosahedron) bouncing & spinning |
| **Globe** | `GLOBE.AD` | 3D orthographic spinning Earth with `EARTH.BMP` + custom `.BMP` map picker (`Map...`) |
| **GraphStat** | `GRAFSTAT.AD` | Meaningless radar/bar/line charts with authentic `GRAFSTAT.TXT` & native editor (`Edit..`) |
| **Gravity** | `GRAV.AD` | Shaded spheres bouncing under gravity with parabolic trails |
| **Hard Rain** | `RAIN.AD` | Expanding circular raindrops eroding the captured desktop |
| **Lasers** | `LASER.AD` | Sweeping multi-color laser fans over black or live desktop |
| **Logo** | `LOGO.AD` | Bouncing `ADLOGO.BMP` with custom `.BMP` picture picker (`Picture..`) |
| **Magic** | `MAGIC.AD` | Classic 1991 geometric ribbon line-art trails |
| **Messages** | `MESSAGE.AD` | Bouncing styled messages reading/writing authentic 1968-byte `MESSAGES.ADS` (`Edit...`) |
| **Mondrian** | `MONDRIAN.AD` | Overlapping rectangular XOR/color patches over live desktop |
| **Mountains** | `MOUNTAIN.AD` | Triangle-midpoint fractal landscapes, all 9 solar-system planets, 5 views & shoreline clipping |
| **Nocturnes** | `NOCTURNE.AD` | Original 102×80 `EYES` sprite sheet (5 styles × 3 blink frames) & `DSPDxax` blink state machine |
| **Punch Out** | `PUNCH.AD` | Circular desktop chunks punched out and bounced around the screen |
| **Puzzle** | `PUZZLE.AD` | Live desktop sliced into a classic sliding-tile puzzle |
| **Rain Storm** | `RAINSTOR.AD` | Wind-driven slanted rain streaks with occasional lightning flashes |
| **Randomizer** | `AD.EXE SEG 16` | Random / In Order cycling across all modules with `Choose..` dialog & `RANDOM.ADS` |
| **Rose** | `ROSE.AD` | Parametric rainbow mathematical rose curves |
| **Satori** | `SATORI.AD` | All 13 color themes, 5 interference fields + checkerboard Mix, spiral refinement, live palette cycling & Caps Lock flip |
| **Shapes** | `SHAPES.AD` | Overlapping colorful rectangles, ellipses, and polygons |
| **Slide Show** | `SLIDESHO.AD` | Transitioning `.BMP` slide deck (`ADLOGO`, `BSILOGO`, `EARTH`, or any folder via `Pictures..`) |
| **Spheres** | `SPHERES.AD` | Smoothly shaded 3D spheres popping onto the screen |
| **Spiral Gyra** | `SGYRA.AD` | Symmetric harmonic spirograph line-art webs |
| **Spotlight** | `SPOTLITE.AD` | Circular spotlight sweeping across the live desktop (preserves desktop during live Demo resize) |
| **Stained Glass** | `STAINED.AD` | 8-way kaleidoscopic quilt generator with random walkers, motifs & 3×3 / checkerboard / ring tiling |
| **Starry Night** | `AD.EXE SEG 2` | Twinkling starfield, `HRGN` city skyline, lit windows, aircraft beacon, shooting stars & lightning |
| **String Theory** | `STRING.AD` | Sweeping multi-point Bézier string-art curves |
| **Vertigo** | `VERTIGO.AD` | Shrinking concentric spirals, cones & shapes with 256-color `AnimatePalette` cycling |
| **Warp!** | `WARP.AD` | 3D spiraling warp-speed starfield |
| **Wrap Around** | `WRAP.AD` | Rainbow figure-8 Lissajous wrapping ribbons |
| **Zot!** | `ZOT.AD` | Forked single- & double-flash jagged lightning bolts |

### Collection 2: The Simpsons After Dark (1994 — 15 Modules)

| Module | ID | Highlights |
|---|---|---|
| **Chalkboard** | `simpsons_chalkboard` | Bart writes authentic episode chalkboard gags (`CSTR #1300`) in bitmap handwriting font (`RLEP #1500`) with chalk dust & eraser |
| **Simpsons Files** | `simpsons_files` | Springfield File cabinet with animated character folders, classic episode lists, and character illustrations |
| **Grampa's Wisdom** | `simpsons_grampa` | Abe Simpson rambles from his purple armchair (`HSTR`)—standing up, sitting back down, reclining, drooling, and snoozing on a dream cloud with 21 digitized voice clips |
| **Grass Skirts** | `simpsons_grass_skirts` | Homer & Ned Flanders mow striped lawns in their finest dresses/muumuus, dodging weeds, Krusty dolls & plutonium rods while Maggie toddles by |
| **Homer Eats** | `simpsons_homer_eats` | Homer chomps bite holes out of your live desktop or an `"AH FUDGE!"` chocolate bar while Marge stops by with chocolate syrup, sugar & sighs |
| **How to Draw** | `simpsons_how_to_draw` | David Silverman's step-by-step animator's pencil & ink construction of Bart Simpson on a graph-paper drawing board |
| **Itchy & Scratchy** | `simpsons_itchy_scratchy` | Classic multi-frame cartoon chase and mayhem skits directly on your live desktop with the *Itchy & Scratchy* MIDI theme |
| **Krusty** | `simpsons_krusty` | Multi-layer theater spotlight routines: juggling dolls/pins/pies/balls, unicycle, shadow puppets, smoking/coughing, pie-in-face & heckling tomatoes (`"Hey hey, kids!"`) |
| **Lisa's Mood Swings** | `simpsons_lisa_moods` | Full-body spotlighted Lisa improvises Happy, Sad, and Angry saxophone solos surrounded by choreographed golden musical-note silhouettes (`LISA`) |
| **Mr. Burns** | `simpsons_mr_burns` | Montgomery Burns and Smithers emerge from hidden office vaults & security monitor banks across your desktop (`"Excellent..."`) |
| **Objets B'art** | `simpsons_objets_bart` | Bart sprays graffiti tags onto your live desktop and sprints away from Principal Skinner, with Milhouse and Nelson (`"HA-HA!"`) cameos |
| **Simpsons Physics** | `simpsons_physics` | 19 Springfield character heads (Barney through Smithers) bouncing and colliding under configurable gravity & attraction with animated head-turns and reactions |
| **Simpsons Clocks** | `simpsons_clocks` | 3 live animated wall clocks: **Duff Brewery** (Barney, Lenny, swinging doors, Homer recliner, neon & rat), **Krusty Pop-Eye** (forehead dial, roving/popping eyes, pendulum tongue & chime explosion), and **Maggie** (blinking eyes, pacifier & toddling Maggie) |
| **Snowball I** | `simpsons_snowball` | Angel Snowball I flaps smoothly on ping-pong wings over clouds and a starry night sky |
| **Simpsons Trivia** | `simpsons_trivia` | Sector 7-G **E-Fission-Cy 1000** Core Temperature Regulator hosting 300 authentic `QERY` trivia questions with live gauges, ceiling ooze, spider cameo, and reactor meltdown |

### Collection 3: Star Trek: The Next Generation After Dark (1994 — 13 Modules)

| Module | ID | Highlights |
|---|---|---|
| **Counselor Troi** | `tng_troi` | Deanna Troi offering empathetic psychological counsel on the *Enterprise-D* bridge & Ten Forward |
| **Data Dances** | `tng_data_dances` | Lt. Commander Data performing tap, rhumba, cha-cha, and foxtrot routines (`SONG_DATA_TAP`, `#10..#37`) on the Holodeck |
| **Encounters** | `tng_encounters` | USS *Enterprise* NCC-1701-D encountering Romulan Warbirds, Klingon Birds-of-Prey, and Borg Cubes in deep space |
| **Nanites** | `tng_nanites` | Evolving microscopic Nanites multiplying and assembling circuitry across your live desktop |
| **Officers' Review** | `tng_officers_review` | Interactive LCARS Starfleet Academy & Bridge Officer evaluation exam (`QERY`) with cast voice reactions |
| **Personnel Files** | `tng_personnel_files` | LCARS Starfleet Personnel Database displaying animated crew dossiers for Picard, Riker, Data, Worf, Troi, Crusher & La Forge |
| **Science Stations** | `tng_science_stations` | Animated LCARS bridge science consoles, Subspace/Warp Field telemetry, and sensor diagnostics |
| **Starbase** | `tng_starbase` | Orbital Starbase space dock with arriving and departing Starfleet vessels, shuttles, and running lights |
| **Starfleet Messages** | `tng_starfleet_messages` | Incoming LCARS Subspace Priority One transmissions and customizable Starfleet command dispatches |
| **Tachyon** | `tng_tachyon` | Geometric tachyon particle detection fields and quantum wave-front interference ribbons |
| **The Borg** | `tng_borg` | Borg drones beaming onto your live desktop with green assimilation lasers and *"Resistance is futile"* |
| **Warp Effect** | `tng_warp_effect` | Relativistic starlight streaking into warp drive as Picard orders *"Engage!"* |
| **Worf's Weapons** | `tng_worfs_weapons` | Lt. Worf & Alexander practicing with the Klingon bat'leth and phasers across your desktop |

### Collection 4: Looney Tunes After Dark (1995 — 12 Modules)

| Module | ID | Highlights |
|---|---|---|
| **ACME Home Shopping** | `looney_acme_shopping` | Wile E. Coyote's live TV shopping network pitching all 12 original ACME contraptions (`STR# #2000..#2006`) with Granny Operator Standing By |
| **Cartoons 101** | `looney_cartoons_101` | Blueprint university lecture board teaching the 10 Cartoon Laws of Physics (`STR# #11000`) with Wile E. Coyote & Road Runner |
| **Conducktor** | `looney_conducktor` | Maestro Daffy Duck conducting Verdi's *Anvil Chorus* (`#70`) and Tchaikovsky's *1812 Overture* (`#80`) with rising colored notes & falling safes |
| **Desquetoppe D'Amour** | `looney_desquetoppe` | Pepé Le Pew wooingly bounding after Penelope Pussycat across your real desktop with rising hearts and French romance |
| **Looney Messages** | `looney_messages` | Speedy Gonzales & Foghorn Leghorn delivering custom parchment banner messages (`"OUT TO LUNCH."`) in authentic `RLEP #7500` font |
| **Marvin's Invasion** | `looney_marvins_invasion` | Marvin the Martian and Duck Dodgers in the 24½th Century dueling with disintegrator pistols over your live desktop |
| **Michigan J. Frog** | `looney_michigan_frog` | Spotlighted theater stage where Michigan J. Frog performs *"Hello! Ma Baby"* (`#10`) in top hat & cane—and croaks when caught |
| **Putty Tat Splat!** | `looney_putty_tat` | Sylvester stalking Tweety Bird across your real desktop until Tweety drinks Dr. Jekyll's Formula and turns into Monster Tweety |
| **Rabbitron** | `looney_rabbitron` | Retro arcade high-score cabinet (`B BUNNY 989,000 / E FUDD 14`) where Bugs Bunny outsmarts Elmer Fudd through teleporting doors |
| **Taz Desktop** | `looney_taz_desktop` | Tasmanian Devil spinning across your real desktop, chewing bite holes directly out of the screen and spitting out scraps |
| **Wockets' Wed Gware** | `looney_wockets` | Bugs, Daffy, Elmer & Yosemite Sam silhouetted at the bottom of the screen watching Looney Tunes character fireworks burst in the night sky |
| **Yosemite Sam** | `looney_yosemite_sam` | Yosemite Sam bursting out of saloon doors directly onto your real desktop and blasting six-shooter bullet holes |

---

## Archive & Resource Extraction Pipeline (`tools/`)

- [tools/unpack_floppy.py](tools/unpack_floppy.py) — Reads the 1991 720 KB FAT12 `DISK01.IMG` and decompresses all 67 `.*$` payload files using the exact 9–14-bit MSB-first LZW algorithm reversed from `INSTALL.EXE` (`SEG 9`).
- [tools/ne_inspect.py](tools/ne_inspect.py) & [tools/dump_phase1.py](tools/dump_phase1.py) — Win16 NE (New Executable) parser, relocation/ordinal resolver, resource extractor, and Capstone 16-bit x86 disassembler.
- [tools/unpack_sit5.py](tools/unpack_sit5.py) — Pure-Python StuffIt 5 (`SIT5`) archive unpacker (Method 15 LZSS + Huffman) for *The Simpsons* and *Star Trek: TNG* Macintosh archives.
- [tools/unpack_sit_m13.py](tools/unpack_sit_m13.py) — Pure-Python StuffIt Deluxe (`SIT!`) Method 13 archive unpacker (37-symbol meta-Huffman + 5 static/dynamic LZSS+Huffman table sets + CRC-16 verification) for *Looney Tunes*.
- [tools/extract_simpsons.py](tools/extract_simpsons.py), [tools/extract_tng.py](tools/extract_tng.py) & [tools/extract_looney.py](tools/extract_looney.py) — Macintosh resource fork parsers that extract `RLEP`, `OFst`, `OFtb`, `clut`, `QERY`, `STR#`, `CSTR`, `HSTR`, `LISA`, `SCLO`, Apple MACE 3:1 ADPCM & uncompressed `snd ` clips (`PCM8`), and `SONG`/`MIDI` arrangements (`SMF Type 1`) into `assets/simpsons/simpsons.adp`, `assets/tng/tng.adp`, and `assets/looney/looney.adp`.

---

## Build, Verify & Run

```sh
cargo test --workspace
cargo build --workspace --release
./target/release/after-dark
```

`cargo test --workspace` executes automated rendering, audio/resource integrity, and pixel-variance checks across all **77 built-in modules** plus `AD_PREFS.INI`, `RANDOM.ADS`, `MESSAGES.ADS`, `GRAFSTAT.TXT`, and the software-GDI unit tests, and exports all 16 visual contact sheets (8 catalog sheets + 8 16:9 HD collection sheets) to `target/verify_snapshots/` — compare them with the Windows-rendered originals in `docs/screenshots/`.

Set `AFTER_DARK_DEBUG=1` to have `after-dark daemon` log its idle decisions to stderr.
