//! Lightweight Win16 `SOUND.DRV` synthesizer, 8-bit PCM voice/SFX player,
//! and Standard MIDI File (`MThd`/`MTrk`) player for Linux.
//!
//! - 1991 After Dark modules (`CLOCKS.AD`) play retro square-wave tones.
//! - 1994 *The Simpsons* modules play authentic digitized 8-bit Mac `snd ` voice/SFX clips
//!   simultaneously with Standard MIDI music.
//!
//! Everything is rendered by one lazily-spawned background mixer thread at 44.1 kHz stereo
//! s16 in ~10 ms blocks:
//!
//! - SFX/tones are resampled from their source rate with linear interpolation.
//! - MIDI is scheduled in the sample domain and rendered with a SoundFont synthesizer
//!   (`rustysynth`), or with a tiny built-in square/triangle/noise synth if no `.sf2` is found.
//! - Output goes to PulseAudio (`libpulse-simple.so.0`) or ALSA (`libasound.so.2`), both loaded
//!   at runtime. If neither is available, sound silently does nothing.
//! - After ~1 s of silence the device is closed and the thread blocks until the next request.

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread;

use libloading::Library;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

static SFX_VOLUME_PCT: AtomicI32 = AtomicI32::new(85);
static MIDI_VOLUME_PCT: AtomicI32 = AtomicI32::new(80);

/// Mixer output rate (Hz).
const OUT_RATE: u32 = 44_100;
/// Frames per mixer block (~10 ms).
const BLOCK_FRAMES: usize = 441;
/// Silent blocks before the output device is closed (~1 s).
const IDLE_CLOSE_BLOCKS: u32 = 100;
/// Gap between MIDI loop repeats (and release tail after the final repeat), ~400 ms.
const MIDI_GAP_FRAMES: usize = (OUT_RATE as usize * 400) / 1000;

/// A single queued tone (matching Win16 `SetVoiceSound(voice, freq_hz_fixed16, duration_ms)` or `SetVoiceNote`).
#[derive(Debug, Clone, Copy)]
pub struct Tone {
    /// Frequency in Hz (`0.0` = silence/rest).
    pub freq_hz: f32,
    /// Duration in milliseconds.
    pub duration_ms: u32,
    /// Volume `0.0..=1.0`.
    pub volume: f32,
}

/// Per-handle stop counters (shared by clones of one [`AdSound`]).
struct Epochs {
    wave: AtomicU64,
    midi: AtomicU64,
}

enum SfxReq {
    Tones(Vec<Tone>),
    Pcm8 {
        samples: &'static [u8],
        sample_rate: u32,
        volume: f32,
    },
}

enum Cmd {
    Sfx {
        lane: u64,
        epochs: Arc<Epochs>,
        epoch: u64,
        req: SfxReq,
    },
    Midi {
        lane: u64,
        epochs: Arc<Epochs>,
        epoch: u64,
        smf: &'static [u8],
        loops: u32, // 0 = infinite
    },
}

/// Non-blocking retro audio handle (PC-speaker tones + 8-bit PCM SFX/voices + MIDI music).
#[derive(Clone)]
pub struct AdSound {
    lane: u64,
    epochs: Arc<Epochs>,
}

impl AdSound {
    pub fn new() -> Self {
        static NEXT_LANE: AtomicU64 = AtomicU64::new(1);
        Self {
            lane: NEXT_LANE.fetch_add(1, Ordering::Relaxed),
            epochs: Arc::new(Epochs {
                wave: AtomicU64::new(1),
                midi: AtomicU64::new(1),
            }),
        }
    }

    fn send(&self, cmd: Cmd) {
        if let Some(tx) = mixer_sender() {
            let _ = tx.send(cmd);
        }
    }

    fn send_sfx(&self, req: SfxReq) {
        let epoch = self.epochs.wave.load(Ordering::Relaxed);
        self.send(Cmd::Sfx {
            lane: self.lane,
            epochs: Arc::clone(&self.epochs),
            epoch,
            req,
        });
    }

    /// Play a sequence of retro tones asynchronously.
    pub fn play_tones(&self, tones: Vec<Tone>) {
        if !tones.is_empty() {
            self.send_sfx(SfxReq::Tones(tones));
        }
    }

    /// Play a single frequency for `duration_ms` (matches Win16 `SetVoiceSound`).
    pub fn play_freq(&self, freq_hz: f32, duration_ms: u32) {
        self.play_tones(vec![Tone {
            freq_hz,
            duration_ms,
            volume: 0.16,
        }]);
    }

    /// Play an uncompressed 8-bit unsigned PCM voice/SFX clip (Mac Format-1/2 `snd `).
    pub fn play_pcm_u8(&self, samples: &'static [u8], sample_rate: u32) {
        if !samples.is_empty() && sample_rate >= 4000 {
            self.send_sfx(SfxReq::Pcm8 {
                samples,
                sample_rate,
                volume: 0.58,
            });
        }
    }

    /// Play a Standard MIDI File (`MThd`/`MTrk`) asynchronously, replacing any playing song.
    /// `loops == 0` loops continuously until `stop_midi()` / `stop_all()`.
    pub fn play_midi(&self, smf: &'static [u8], loops: u32) {
        if smf.len() >= 14 {
            let epoch = self.epochs.midi.fetch_add(1, Ordering::Relaxed) + 1;
            self.send(Cmd::Midi {
                lane: self.lane,
                epochs: Arc::clone(&self.epochs),
                epoch,
                smf,
                loops,
            });
        }
    }

    /// Immediately stop any currently playing MIDI song.
    pub fn stop_midi(&self) {
        self.epochs.midi.fetch_add(1, Ordering::Relaxed);
    }

    /// Immediately silence all active voices, SFX, tones, and MIDI music.
    pub fn stop_all(&self) {
        self.epochs.wave.fetch_add(1, Ordering::Relaxed);
        self.epochs.midi.fetch_add(1, Ordering::Relaxed);
    }

    /// Set global Voice/SFX volume (`0..=100`) and MIDI Music volume (`0..=100`).
    pub fn set_volumes(&self, sfx_pct: i32, midi_pct: i32) {
        SFX_VOLUME_PCT.store(sfx_pct.clamp(0, 100), Ordering::Relaxed);
        MIDI_VOLUME_PCT.store(midi_pct.clamp(0, 100), Ordering::Relaxed);
    }

    /// Get global `(sfx_pct, midi_pct)` volumes (`0..=100`).
    pub fn get_volumes(&self) -> (i32, i32) {
        (
            SFX_VOLUME_PCT.load(Ordering::Relaxed),
            MIDI_VOLUME_PCT.load(Ordering::Relaxed),
        )
    }

    /// Convert a Win16 `SetVoiceNote(voice, nValue, ...)` note number (1..=84, where 1 = C1, 37 = C4)
    /// into Hz using equal temperament (A4 = note 46 = 440 Hz).
    pub fn win16_note_to_hz(note: i32) -> f32 {
        if note <= 0 {
            return 0.0;
        }
        // In Windows 3.0 SOUND.DRV: note 1 = C1 (32.7 Hz), note 46 = A4 (440 Hz).
        440.0 * 2.0f32.powf((note - 46) as f32 / 12.0)
    }
}

impl Default for AdSound {
    fn default() -> Self {
        Self::new()
    }
}

/// Crate-internal mixer counters (for diagnostics/tests; never printed by the library).
pub(crate) mod diag {
    use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8};

    /// Currently open output: 0 = none, 1 = PulseAudio, 2 = ALSA.
    pub(crate) static BACKEND: AtomicU8 = AtomicU8::new(0);
    /// Number of successful output-device opens.
    pub(crate) static OPENS: AtomicU32 = AtomicU32::new(0);
    /// Number of blocks written to an output device.
    pub(crate) static BLOCKS: AtomicU64 = AtomicU64::new(0);
    /// Largest absolute MIDI sample (i16 scale) rendered since last reset.
    pub(crate) static MIDI_PEAK: AtomicU32 = AtomicU32::new(0);
    /// Largest absolute SFX sample (i16 scale) rendered since last reset.
    pub(crate) static SFX_PEAK: AtomicU32 = AtomicU32::new(0);
    /// MIDI engine of the last started song: 0 = none, 1 = SoundFont, 2 = built-in.
    pub(crate) static MIDI_ENGINE: AtomicU8 = AtomicU8::new(0);
    /// Slowest block render time (microseconds).
    pub(crate) static MAX_RENDER_US: AtomicU32 = AtomicU32::new(0);
}

// ---------------------------------------------------------------------------------------------
// Mixer thread
// ---------------------------------------------------------------------------------------------

fn mixer_sender() -> Option<&'static Sender<Cmd>> {
    static TX: OnceLock<Option<Sender<Cmd>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Cmd>();
        thread::Builder::new()
            .name("ad-sound-mixer".to_string())
            .spawn(move || mixer_main(rx))
            .ok()
            .map(|_| tx)
    })
    .as_ref()
}

fn mixer_main(rx: Receiver<Cmd>) {
    let mut mixer = Mixer::default();
    let mut out: Option<Output> = None;
    let mut buf = vec![0i16; BLOCK_FRAMES * 2];
    let mut idle_blocks = 0u32;

    loop {
        if out.is_none() {
            // Device closed: block until there is something to play.
            match rx.recv() {
                Ok(cmd) => mixer.handle(cmd),
                Err(_) => return,
            }
            while let Ok(cmd) = rx.try_recv() {
                mixer.handle(cmd);
            }
            if !mixer.has_work() {
                continue;
            }
            out = Output::open();
            if out.is_none() {
                mixer.clear();
                continue;
            }
            idle_blocks = 0;
        }

        while let Ok(cmd) = rx.try_recv() {
            mixer.handle(cmd);
        }

        let t0 = std::time::Instant::now();
        let active = mixer.render(&mut buf);
        let us = t0.elapsed().as_micros().min(u32::MAX as u128) as u32;
        diag::MAX_RENDER_US.fetch_max(us, Ordering::Relaxed);
        idle_blocks = if active { 0 } else { idle_blocks + 1 };

        let Some(dev) = out.as_mut() else { continue };
        if !dev.write(&buf) {
            out = None;
            mixer.clear();
            continue;
        }
        diag::BLOCKS.fetch_add(1, Ordering::Relaxed);

        if idle_blocks >= IDLE_CLOSE_BLOCKS
            && let Some(dev) = out.take()
        {
            dev.close();
        }
    }
}

struct SfxVoice {
    pcm: Vec<i16>,
    pos: f64,
    step: f64,
    epoch: u64,
}

struct Lane {
    id: u64,
    epochs: Arc<Epochs>,
    sfx: Option<SfxVoice>,
    pending: Option<(u64, SfxReq)>,
    midi: Option<MidiSong>,
}

impl Lane {
    fn has_work(&self) -> bool {
        self.sfx.is_some() || self.pending.is_some() || self.midi.is_some()
    }
}

#[derive(Default)]
struct Mixer {
    lanes: Vec<Lane>,
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
    tmp_l: Vec<f32>,
    tmp_r: Vec<f32>,
}

impl Mixer {
    fn lane(&mut self, id: u64, epochs: Arc<Epochs>) -> &mut Lane {
        let idx = match self.lanes.iter().position(|l| l.id == id) {
            Some(i) => i,
            None => {
                self.lanes.push(Lane {
                    id,
                    epochs,
                    sfx: None,
                    pending: None,
                    midi: None,
                });
                self.lanes.len() - 1
            }
        };
        &mut self.lanes[idx]
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Sfx {
                lane,
                epochs,
                epoch,
                req,
            } => {
                // The playing SFX is never interrupted; only the newest backed-up request survives.
                self.lane(lane, epochs).pending = Some((epoch, req));
            }
            Cmd::Midi {
                lane,
                epochs,
                epoch,
                smf,
                loops,
            } => {
                let current = epochs.midi.load(Ordering::Relaxed) == epoch;
                let lane = self.lane(lane, epochs);
                lane.midi = if current {
                    MidiSong::new(smf, loops, epoch)
                } else {
                    None
                };
            }
        }
        self.lanes.retain(Lane::has_work);
    }

    fn has_work(&self) -> bool {
        self.lanes.iter().any(Lane::has_work)
    }

    fn clear(&mut self) {
        self.lanes.clear();
    }

    /// Render one block into `out` (interleaved stereo). Returns whether anything is playing.
    fn render(&mut self, out: &mut [i16]) -> bool {
        let frames = out.len() / 2;
        for v in [&mut self.mix_l, &mut self.mix_r, &mut self.tmp_l, &mut self.tmp_r] {
            v.clear();
            v.resize(frames, 0.0);
        }
        let mut sfx_peak = 0.0f32;
        let mut midi_peak = 0.0f32;

        for lane in &mut self.lanes {
            let wave_epoch = lane.epochs.wave.load(Ordering::Relaxed);
            let midi_epoch = lane.epochs.midi.load(Ordering::Relaxed);

            // --- SFX lane ---
            if lane.sfx.as_ref().is_some_and(|v| v.epoch != wave_epoch) {
                lane.sfx = None;
            }
            let mut i = 0;
            while i < frames {
                if lane.sfx.is_none() {
                    match lane.pending.take() {
                        Some((epoch, req)) if epoch == wave_epoch => {
                            lane.sfx = start_sfx(req, epoch);
                            continue;
                        }
                        Some(_) => continue,
                        None => break,
                    }
                }
                let Some(v) = lane.sfx.as_mut() else { break };
                let n = v.pcm.len();
                while i < frames {
                    let idx = v.pos as usize;
                    if idx >= n {
                        break;
                    }
                    let frac = (v.pos - idx as f64) as f32;
                    let a = v.pcm[idx] as f32;
                    let b = if idx + 1 < n { v.pcm[idx + 1] as f32 } else { 0.0 };
                    let s = a + (b - a) * frac;
                    self.mix_l[i] += s;
                    self.mix_r[i] += s;
                    sfx_peak = sfx_peak.max(s.abs());
                    v.pos += v.step;
                    i += 1;
                }
                if v.pos as usize >= n {
                    lane.sfx = None;
                }
            }

            // --- MIDI ---
            if lane.midi.as_ref().is_some_and(|m| m.epoch != midi_epoch) {
                lane.midi = None;
            }
            if let Some(song) = lane.midi.as_mut() {
                self.tmp_l.fill(0.0);
                self.tmp_r.fill(0.0);
                let alive = song.render(&mut self.tmp_l, &mut self.tmp_r);
                for k in 0..frames {
                    let l = self.tmp_l[k] * 32767.0;
                    let r = self.tmp_r[k] * 32767.0;
                    midi_peak = midi_peak.max(l.abs()).max(r.abs());
                    self.mix_l[k] += l;
                    self.mix_r[k] += r;
                }
                if !alive {
                    lane.midi = None;
                }
            }
        }
        self.lanes.retain(Lane::has_work);

        for k in 0..frames {
            out[2 * k] = self.mix_l[k].clamp(-32767.0, 32767.0) as i16;
            out[2 * k + 1] = self.mix_r[k].clamp(-32767.0, 32767.0) as i16;
        }
        diag::SFX_PEAK.fetch_max(sfx_peak as u32, Ordering::Relaxed);
        diag::MIDI_PEAK.fetch_max(midi_peak as u32, Ordering::Relaxed);
        self.has_work()
    }
}

fn start_sfx(req: SfxReq, epoch: u64) -> Option<SfxVoice> {
    let (pcm, rate) = match req {
        SfxReq::Tones(tones) => (gen_tones(&tones), TONE_RATE),
        SfxReq::Pcm8 {
            samples,
            sample_rate,
            volume,
        } => (gen_pcm8(samples, sample_rate, volume), sample_rate),
    };
    if pcm.is_empty() || rate == 0 {
        return None;
    }
    Some(SfxVoice {
        pcm,
        pos: 0.0,
        step: rate as f64 / OUT_RATE as f64,
        epoch,
    })
}

// ---------------------------------------------------------------------------------------------
// Tone / PCM sample generation (source-rate mono s16)
// ---------------------------------------------------------------------------------------------

const TONE_RATE: u32 = 22_050;

fn gen_tones(tones: &[Tone]) -> Vec<i16> {
    let sfx_mult = (SFX_VOLUME_PCT.load(Ordering::Relaxed).clamp(0, 100) as f32) / 100.0;
    if sfx_mult <= 0.005 {
        return Vec::new();
    }
    const SAMPLE_RATE: u32 = TONE_RATE;
    let total_samples: usize = tones
        .iter()
        .map(|t| ((SAMPLE_RATE as u64 * t.duration_ms as u64) / 1000) as usize)
        .sum();
    if total_samples == 0 || total_samples > (SAMPLE_RATE as usize * 15) {
        return Vec::new();
    }

    let mut pcm = Vec::<i16>::with_capacity(total_samples);
    for t in tones {
        let count = ((SAMPLE_RATE as u64 * t.duration_ms as u64) / 1000) as usize;
        if count == 0 {
            continue;
        }
        let amp = t.volume.clamp(0.0, 1.0) * 16_000.0 * sfx_mult;
        let fade_len = (SAMPLE_RATE as usize / 200).min(count / 4).max(1);
        for i in 0..count {
            if t.freq_hz <= 1.0 {
                pcm.push(0);
            } else {
                let phase = (i as f32 * t.freq_hz) / SAMPLE_RATE as f32;
                let sq = if phase.fract() < 0.5 { 1.0f32 } else { -1.0f32 };
                let env = if i < fade_len {
                    i as f32 / fade_len as f32
                } else if i + fade_len >= count {
                    (count - 1 - i) as f32 / fade_len as f32
                } else {
                    1.0
                };
                pcm.push((sq * amp * env) as i16);
            }
        }
    }
    pcm
}

fn gen_pcm8(samples: &[u8], sample_rate: u32, volume: f32) -> Vec<i16> {
    let sfx_mult = (SFX_VOLUME_PCT.load(Ordering::Relaxed).clamp(0, 100) as f32) / 100.0;
    if sfx_mult <= 0.005 || samples.is_empty() || samples.len() > 2_000_000 {
        return Vec::new();
    }
    let scale = volume.clamp(0.0, 1.0) * 256.0 * sfx_mult;
    let fade = (sample_rate as usize / 250).min(samples.len() / 4).max(1);
    let mut pcm = Vec::<i16>::with_capacity(samples.len());
    for (i, &b) in samples.iter().enumerate() {
        let s = (b as i32 - 128) as f32 * scale;
        let env = if i < fade {
            i as f32 / fade as f32
        } else if i + fade >= samples.len() {
            (samples.len() - 1 - i) as f32 / fade as f32
        } else {
            1.0
        };
        pcm.push((s * env).clamp(-32767.0, 32767.0) as i16);
    }
    pcm
}

// ---------------------------------------------------------------------------------------------
// Standard MIDI File parsing
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum MidiEventKind {
    ShortMsg(u32),
    TempoUs(u32),
}

struct MidiEvent {
    tick: u64,
    order: u32,
    kind: MidiEventKind,
}

fn read_vlq(data: &[u8], pos: &mut usize) -> u32 {
    let mut val = 0u32;
    for _ in 0..4 {
        if *pos >= data.len() {
            break;
        }
        let b = data[*pos];
        *pos += 1;
        val = (val << 7) | (b & 0x7F) as u32;
        if (b & 0x80) == 0 {
            break;
        }
    }
    val
}

fn parse_smf(smf: &[u8]) -> Option<(u16, Vec<MidiEvent>)> {
    if smf.len() < 14 || &smf[0..4] != b"MThd" {
        return None;
    }
    let hlen = u32::from_be_bytes([smf[4], smf[5], smf[6], smf[7]]) as usize;
    if 8 + hlen > smf.len() || hlen < 6 {
        return None;
    }
    let division = u16::from_be_bytes([smf[12], smf[13]]);
    if division == 0 || (division & 0x8000) != 0 {
        return None;
    }

    let mut events = Vec::new();
    let mut order = 0u32;
    let mut pos = 8 + hlen;

    while pos + 8 <= smf.len() {
        let tag = &smf[pos..pos + 4];
        let tlen = u32::from_be_bytes([smf[pos + 4], smf[pos + 5], smf[pos + 6], smf[pos + 7]])
            as usize;
        pos += 8;
        let end = (pos + tlen).min(smf.len());
        if tag == b"MTrk" {
            let mut p = pos;
            let mut tick = 0u64;
            let mut running = 0u8;
            while p < end {
                let delta = read_vlq(smf, &mut p) as u64;
                tick += delta;
                if p >= end {
                    break;
                }
                let mut status = smf[p];
                if status < 0x80 {
                    if running == 0 {
                        break;
                    }
                    status = running;
                } else {
                    p += 1;
                    if status < 0xF0 {
                        running = status;
                    }
                }
                match status {
                    0x80..=0xEF => {
                        let kind_hi = status & 0xF0;
                        let d1 = if p < end {
                            let v = smf[p];
                            p += 1;
                            v
                        } else {
                            0
                        };
                        let d2 = if kind_hi == 0xC0 || kind_hi == 0xD0 {
                            0
                        } else if p < end {
                            let v = smf[p];
                            p += 1;
                            v
                        } else {
                            0
                        };
                        // Keep volume balanced nicely (~75% velocity on Note On)
                        let d2_adj = if kind_hi == 0x90 && d2 > 0 {
                            ((d2 as u32 * 3) / 4).max(1) as u8
                        } else {
                            d2
                        };
                        let msg = (status as u32) | ((d1 as u32) << 8) | ((d2_adj as u32) << 16);
                        events.push(MidiEvent {
                            tick,
                            order,
                            kind: MidiEventKind::ShortMsg(msg),
                        });
                        order = order.wrapping_add(1);
                    }
                    0xFF => {
                        if p >= end {
                            break;
                        }
                        let meta_type = smf[p];
                        p += 1;
                        let mlen = read_vlq(smf, &mut p) as usize;
                        if meta_type == 0x51 && mlen == 3 && p + 3 <= end {
                            let tempo = ((smf[p] as u32) << 16)
                                | ((smf[p + 1] as u32) << 8)
                                | (smf[p + 2] as u32);
                            events.push(MidiEvent {
                                tick,
                                order,
                                kind: MidiEventKind::TempoUs(tempo.max(50_000)),
                            });
                            order = order.wrapping_add(1);
                        }
                        p = (p + mlen).min(end);
                    }
                    0xF0 | 0xF7 => {
                        let slen = read_vlq(smf, &mut p) as usize;
                        p = (p + slen).min(end);
                    }
                    _ => {
                        break;
                    }
                }
            }
        }
        pos = end;
    }

    events.sort_by_key(|e| (e.tick, e.order));
    Some((division, events))
}

// ---------------------------------------------------------------------------------------------
// MIDI sequencing (sample domain)
// ---------------------------------------------------------------------------------------------

struct MidiSong {
    epoch: u64,
    division: u64,
    events: Vec<MidiEvent>,
    engine: MidiEngine,
    loops: u32,
    iter: u32,
    idx: usize,
    tempo_us: u64,
    last_tick: u64,
    /// Output frames until `events[idx]` is due.
    wait: f64,
    /// Frames remaining in the inter-loop gap / final release tail.
    gap: usize,
    finished: bool,
}

impl MidiSong {
    fn new(smf: &'static [u8], loops: u32, epoch: u64) -> Option<Self> {
        let (division, events) = parse_smf(smf)?;
        if events.is_empty() {
            return None;
        }
        let engine = MidiEngine::new();
        let mut song = Self {
            epoch,
            division: division as u64,
            events,
            engine,
            loops,
            iter: 1,
            idx: 0,
            tempo_us: 500_000,
            last_tick: 0,
            wait: 0.0,
            gap: 0,
            finished: false,
        };
        song.restart();
        Some(song)
    }

    fn restart(&mut self) {
        self.idx = 0;
        self.tempo_us = 500_000; // 120 BPM default
        self.last_tick = 0;
        self.wait = 0.0;
        self.schedule_next();
    }

    /// Add the delay from the previous event to `events[idx]` (at the current tempo) to `wait`.
    fn schedule_next(&mut self) {
        if let Some(ev) = self.events.get(self.idx) {
            let dt_ticks = ev.tick.saturating_sub(self.last_tick);
            if dt_ticks > 0 {
                let us = (dt_ticks * self.tempo_us) / self.division;
                self.wait += us as f64 * OUT_RATE as f64 / 1_000_000.0;
                self.last_tick = ev.tick;
            }
        }
    }

    fn dispatch(&mut self, kind: MidiEventKind) {
        match kind {
            MidiEventKind::ShortMsg(mut msg) => {
                let midi_pct = MIDI_VOLUME_PCT.load(Ordering::Relaxed).clamp(0, 100) as u32;
                let kind_hi = msg & 0xF0;
                if kind_hi == 0x90 {
                    let vel = (msg >> 16) & 0x7F;
                    if vel > 0 {
                        if midi_pct == 0 {
                            return;
                        }
                        let scaled = ((vel * midi_pct) / 100).clamp(1, 127);
                        msg = (msg & 0x0000_FFFF) | (scaled << 16);
                    }
                } else if kind_hi == 0xB0 && ((msg >> 8) & 0x7F) == 7 {
                    let cc_vol = (msg >> 16) & 0x7F;
                    let scaled = ((cc_vol * midi_pct) / 100).min(127);
                    msg = (msg & 0x0000_FFFF) | (scaled << 16);
                }
                self.engine.message(
                    (msg & 0x0F) as u8,
                    (msg & 0xF0) as u8,
                    ((msg >> 8) & 0x7F) as u8,
                    ((msg >> 16) & 0x7F) as u8,
                );
            }
            MidiEventKind::TempoUs(t) => self.tempo_us = t as u64,
        }
    }

    /// Render into `l`/`r` (pre-zeroed, accumulated). Returns `false` once the song has ended.
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) -> bool {
        let n = l.len();
        let mut pos = 0;
        while pos < n {
            if self.gap > 0 {
                let k = self.gap.min(n - pos);
                self.engine.render(&mut l[pos..pos + k], &mut r[pos..pos + k]);
                self.gap -= k;
                pos += k;
                if self.gap == 0 {
                    if self.finished {
                        return false;
                    }
                    self.restart();
                }
                continue;
            }

            while self.idx < self.events.len() && self.wait < 0.5 {
                let kind = self.events[self.idx].kind;
                self.dispatch(kind);
                self.idx += 1;
                self.schedule_next();
            }

            if self.idx >= self.events.len() {
                // End of pass: all notes off, then a short gap (repeat) or release tail (final).
                self.engine.all_notes_off();
                self.finished = !(self.loops == 0 || self.iter < self.loops);
                self.iter = self.iter.saturating_add(1);
                self.gap = MIDI_GAP_FRAMES;
                continue;
            }

            let k = (self.wait.round() as usize).clamp(1, n - pos);
            self.engine.render(&mut l[pos..pos + k], &mut r[pos..pos + k]);
            self.wait -= k as f64;
            pos += k;
        }
        true
    }
}

enum MidiEngine {
    SoundFont(Box<Synthesizer>),
    Basic(Box<BasicSynth>),
}

impl MidiEngine {
    fn new() -> Self {
        if let Some(sf) = soundfont() {
            let settings = SynthesizerSettings::new(OUT_RATE as i32);
            if let Ok(mut synth) = Synthesizer::new(sf, &settings) {
                // Default master volume (0.5) sits well below the SFX lane; match the old
                // GM-synth balance more closely.
                synth.set_master_volume(1.0);
                diag::MIDI_ENGINE.store(1, Ordering::Relaxed);
                return MidiEngine::SoundFont(Box::new(synth));
            }
        }
        diag::MIDI_ENGINE.store(2, Ordering::Relaxed);
        MidiEngine::Basic(Box::new(BasicSynth::new()))
    }

    fn message(&mut self, ch: u8, kind: u8, d1: u8, d2: u8) {
        match self {
            MidiEngine::SoundFont(s) => {
                s.process_midi_message(ch as i32, kind as i32, d1 as i32, d2 as i32)
            }
            MidiEngine::Basic(s) => s.message(ch, kind, d1, d2),
        }
    }

    fn all_notes_off(&mut self) {
        match self {
            MidiEngine::SoundFont(s) => s.note_off_all(false),
            MidiEngine::Basic(s) => s.all_notes_off(),
        }
    }

    /// Accumulate-free render: overwrites `l`/`r`.
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        match self {
            MidiEngine::SoundFont(s) => s.render(l, r),
            MidiEngine::Basic(s) => s.render(l, r),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// SoundFont discovery
// ---------------------------------------------------------------------------------------------

fn soundfont() -> Option<&'static Arc<SoundFont>> {
    static SF: OnceLock<Option<Arc<SoundFont>>> = OnceLock::new();
    SF.get_or_init(|| soundfont_candidates().iter().find_map(|p| load_soundfont(p)))
        .as_ref()
}

fn soundfont_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("AD_SOUNDFONT").filter(|p| !p.is_empty()) {
        out.push(PathBuf::from(p));
    }
    for name in ["default-GM.sf2", "FluidR3_GM.sf2", "TimGM6mb.sf2"] {
        out.push(Path::new("/usr/share/sounds/sf2").join(name));
    }
    let mut dirs = vec![PathBuf::from("/usr/share/soundfonts")];
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        dirs.push(Path::new(&home).join(".local/share/soundfonts"));
    }
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut found: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("sf2"))
            })
            .collect();
        found.sort();
        out.extend(found);
    }
    out
}

fn load_soundfont(path: &Path) -> Option<Arc<SoundFont>> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| SoundFont::new(&mut reader)))
        .ok()?
        .ok()
        .map(Arc::new)
}

// ---------------------------------------------------------------------------------------------
// Built-in fallback synth (no SoundFont available)
// ---------------------------------------------------------------------------------------------

const BASIC_MAX_VOICES: usize = 32;

#[derive(Clone, Copy, PartialEq)]
enum Wave {
    Square,
    Triangle,
    Noise,
}

struct BasicVoice {
    ch: u8,
    key: u8,
    wave: Wave,
    phase: f32,
    inc: f32,
    amp: f32,
    env: f32,
    /// Frames since note-on.
    age: u32,
    released: bool,
    /// Per-sample multiplicative decay for drums / released notes.
    decay: f32,
    /// One-pole low-pass state/coef for noise.
    lp: f32,
    lp_coef: f32,
}

struct BasicSynth {
    voices: Vec<BasicVoice>,
    program: [u8; 16],
    volume: [u8; 16],
    expression: [u8; 16],
    bend: [f32; 16],
    rng: u32,
}

impl BasicSynth {
    fn new() -> Self {
        Self {
            voices: Vec::with_capacity(BASIC_MAX_VOICES),
            program: [0; 16],
            volume: [100; 16],
            expression: [127; 16],
            bend: [1.0; 16],
            rng: 0x1234_5678,
        }
    }

    fn message(&mut self, ch: u8, kind: u8, d1: u8, d2: u8) {
        let ch = ch & 0x0F;
        let c = ch as usize;
        match kind {
            0x90 if d2 > 0 => self.note_on(ch, d1, d2),
            0x80 | 0x90 => {
                for v in self.voices.iter_mut().filter(|v| v.ch == ch && v.key == d1) {
                    v.released = true;
                }
            }
            0xB0 => match d1 {
                7 => self.volume[c] = d2,
                11 => self.expression[c] = d2,
                120 => self.voices.retain(|v| v.ch != ch),
                121 => {
                    self.volume[c] = 100;
                    self.expression[c] = 127;
                    self.bend[c] = 1.0;
                }
                123 => {
                    for v in self.voices.iter_mut().filter(|v| v.ch == ch) {
                        v.released = true;
                    }
                }
                _ => {}
            },
            0xC0 => self.program[c] = d1,
            0xE0 => {
                let raw = ((d2 as i32) << 7 | d1 as i32) - 8192;
                self.bend[c] = 2.0f32.powf(raw as f32 / 8192.0 * 2.0 / 12.0);
            }
            _ => {}
        }
    }

    fn note_on(&mut self, ch: u8, key: u8, vel: u8) {
        let c = ch as usize;
        let level = (vel as f32 / 127.0)
            * (self.volume[c] as f32 / 127.0)
            * (self.expression[c] as f32 / 127.0);
        let sr = OUT_RATE as f32;
        let voice = if ch == 9 {
            // Percussion: short filtered noise bursts; low keys darker/longer.
            let (decay_ms, lp_coef) = match key {
                35 | 36 => (90.0, 0.06),
                38 | 40 => (120.0, 0.45),
                42 | 44 => (45.0, 0.9),
                46 | 49 | 51 | 52 | 55 | 57 | 59 => (260.0, 0.85),
                _ => (100.0, 0.35),
            };
            BasicVoice {
                ch,
                key,
                wave: Wave::Noise,
                phase: 0.0,
                inc: 0.0,
                amp: level * 0.30,
                env: 1.0,
                age: 0,
                released: false,
                decay: (-1.0 / (decay_ms * 0.001 * sr)).exp(),
                lp: 0.0,
                lp_coef,
            }
        } else {
            let prog = self.program[c];
            // Leads, synth brass/pads, organs and chromatic percussion get the brighter square.
            let wave = if (16..24).contains(&prog) || prog >= 80 {
                Wave::Square
            } else {
                Wave::Triangle
            };
            let freq = 440.0 * 2.0f32.powf((key as f32 - 69.0) / 12.0);
            let amp = if wave == Wave::Square { 0.07 } else { 0.13 };
            BasicVoice {
                ch,
                key,
                wave,
                phase: 0.0,
                inc: freq / sr,
                amp: level * amp,
                env: 0.0,
                age: 0,
                released: false,
                decay: (-1.0 / (0.08 * sr)).exp(),
                lp: 0.0,
                lp_coef: 1.0,
            }
        };
        if self.voices.len() >= BASIC_MAX_VOICES {
            // Steal the oldest voice.
            if let Some(i) = self
                .voices
                .iter()
                .enumerate()
                .max_by_key(|(_, v)| v.age)
                .map(|(i, _)| i)
            {
                self.voices.swap_remove(i);
            }
        }
        self.voices.push(voice);
    }

    fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.released = true;
        }
    }

    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        l.fill(0.0);
        r.fill(0.0);
        let sr = OUT_RATE as f32;
        let attack = (0.004 * sr) as u32; // 4 ms
        let decay_len = 0.15 * sr; // 150 ms to sustain
        const SUSTAIN: f32 = 0.55;
        let mut rng = self.rng;
        for v in &mut self.voices {
            let bend = self.bend[v.ch as usize];
            for k in 0..l.len() {
                // Envelope
                if v.wave == Wave::Noise || v.released {
                    if v.wave != Wave::Noise && v.age < attack {
                        v.env = v.age as f32 / attack as f32;
                    }
                    v.env *= v.decay;
                } else if v.age < attack {
                    v.env = v.age as f32 / attack as f32;
                } else {
                    let t = ((v.age - attack) as f32 / decay_len).min(1.0);
                    v.env = 1.0 - (1.0 - SUSTAIN) * t;
                }
                v.age = v.age.saturating_add(1);

                let s = match v.wave {
                    Wave::Noise => {
                        rng ^= rng << 13;
                        rng ^= rng >> 17;
                        rng ^= rng << 5;
                        let white = (rng as i32) as f32 / i32::MAX as f32;
                        v.lp += v.lp_coef * (white - v.lp);
                        v.lp
                    }
                    Wave::Square => {
                        if v.phase < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    Wave::Triangle => 4.0 * (v.phase - 0.5).abs() - 1.0,
                };
                v.phase = (v.phase + v.inc * bend).fract();
                let out = s * v.amp * v.env;
                l[k] += out;
                r[k] += out;
            }
        }
        self.rng = rng;
        self.voices
            .retain(|v| v.env > 0.0005 || (v.wave != Wave::Noise && !v.released));
    }
}

// ---------------------------------------------------------------------------------------------
// Output backends (loaded at runtime)
// ---------------------------------------------------------------------------------------------

// --- PulseAudio simple API ---
const PA_SAMPLE_S16LE: c_int = 3;
const PA_STREAM_PLAYBACK: c_int = 1;

#[repr(C)]
struct PaSampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct PaBufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

type PaSimpleNew = unsafe extern "C" fn(
    server: *const c_char,
    name: *const c_char,
    dir: c_int,
    dev: *const c_char,
    stream_name: *const c_char,
    ss: *const PaSampleSpec,
    map: *const c_void,
    attr: *const PaBufferAttr,
    error: *mut c_int,
) -> *mut c_void;
type PaSimpleWrite =
    unsafe extern "C" fn(s: *mut c_void, data: *const c_void, bytes: usize, error: *mut c_int) -> c_int;
type PaSimpleDrain = unsafe extern "C" fn(s: *mut c_void, error: *mut c_int) -> c_int;
type PaSimpleFree = unsafe extern "C" fn(s: *mut c_void);

struct PulseLib {
    _lib: Library,
    new: PaSimpleNew,
    write: PaSimpleWrite,
    drain: PaSimpleDrain,
    free: PaSimpleFree,
}

impl PulseLib {
    fn get() -> Option<&'static PulseLib> {
        static LIB: OnceLock<Option<PulseLib>> = OnceLock::new();
        LIB.get_or_init(|| {
            // SAFETY: loading a well-known system library and resolving symbols with the
            // signatures documented in <pulse/simple.h>.
            unsafe {
                let lib = Library::new("libpulse-simple.so.0").ok()?;
                let new = *lib.get::<PaSimpleNew>(b"pa_simple_new\0").ok()?;
                let write = *lib.get::<PaSimpleWrite>(b"pa_simple_write\0").ok()?;
                let drain = *lib.get::<PaSimpleDrain>(b"pa_simple_drain\0").ok()?;
                let free = *lib.get::<PaSimpleFree>(b"pa_simple_free\0").ok()?;
                Some(PulseLib {
                    _lib: lib,
                    new,
                    write,
                    drain,
                    free,
                })
            }
        })
        .as_ref()
    }
}

// --- ALSA ---
const SND_PCM_STREAM_PLAYBACK: c_int = 0;
const SND_PCM_FORMAT_S16_LE: c_int = 2;
const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

type SndPcmOpen = unsafe extern "C" fn(
    pcm: *mut *mut c_void,
    name: *const c_char,
    stream: c_int,
    mode: c_int,
) -> c_int;
type SndPcmSetParams = unsafe extern "C" fn(
    pcm: *mut c_void,
    format: c_int,
    access: c_int,
    channels: c_uint,
    rate: c_uint,
    soft_resample: c_int,
    latency_us: c_uint,
) -> c_int;
type SndPcmWritei =
    unsafe extern "C" fn(pcm: *mut c_void, buffer: *const c_void, frames: c_ulong) -> c_long;
type SndPcmRecover = unsafe extern "C" fn(pcm: *mut c_void, err: c_int, silent: c_int) -> c_int;
type SndPcmClose = unsafe extern "C" fn(pcm: *mut c_void) -> c_int;

struct AlsaLib {
    _lib: Library,
    open: SndPcmOpen,
    set_params: SndPcmSetParams,
    writei: SndPcmWritei,
    recover: SndPcmRecover,
    close: SndPcmClose,
}

impl AlsaLib {
    fn get() -> Option<&'static AlsaLib> {
        static LIB: OnceLock<Option<AlsaLib>> = OnceLock::new();
        LIB.get_or_init(|| {
            // SAFETY: loading a well-known system library and resolving symbols with the
            // signatures documented in <alsa/pcm.h>.
            unsafe {
                let lib = Library::new("libasound.so.2").ok()?;
                let open = *lib.get::<SndPcmOpen>(b"snd_pcm_open\0").ok()?;
                let set_params = *lib.get::<SndPcmSetParams>(b"snd_pcm_set_params\0").ok()?;
                let writei = *lib.get::<SndPcmWritei>(b"snd_pcm_writei\0").ok()?;
                let recover = *lib.get::<SndPcmRecover>(b"snd_pcm_recover\0").ok()?;
                let close = *lib.get::<SndPcmClose>(b"snd_pcm_close\0").ok()?;
                Some(AlsaLib {
                    _lib: lib,
                    open,
                    set_params,
                    writei,
                    recover,
                    close,
                })
            }
        })
        .as_ref()
    }
}

/// An open blocking output stream (44.1 kHz, stereo, s16le). Lives on the mixer thread only.
enum Output {
    Pulse {
        lib: &'static PulseLib,
        s: *mut c_void,
    },
    Alsa {
        lib: &'static AlsaLib,
        pcm: *mut c_void,
    },
}

impl Output {
    fn open() -> Option<Output> {
        let out = Self::open_pulse().or_else(Self::open_alsa)?;
        diag::BACKEND.store(
            match out {
                Output::Pulse { .. } => 1,
                Output::Alsa { .. } => 2,
            },
            Ordering::Relaxed,
        );
        diag::OPENS.fetch_add(1, Ordering::Relaxed);
        Some(out)
    }

    fn open_pulse() -> Option<Output> {
        let lib = PulseLib::get()?;
        let spec = PaSampleSpec {
            format: PA_SAMPLE_S16LE,
            rate: OUT_RATE,
            channels: 2,
        };
        let bytes_per_ms = OUT_RATE * 4 / 1000;
        let attr = PaBufferAttr {
            maxlength: u32::MAX,
            tlength: bytes_per_ms * 50,
            prebuf: u32::MAX,
            minreq: bytes_per_ms * 10,
            fragsize: u32::MAX,
        };
        let mut err: c_int = 0;
        // SAFETY: all pointers are valid for the duration of the call; NULL server/device/map
        // select the defaults.
        let s = unsafe {
            (lib.new)(
                std::ptr::null(),
                c"After Dark".as_ptr(),
                PA_STREAM_PLAYBACK,
                std::ptr::null(),
                c"Screen saver sound".as_ptr(),
                &spec,
                std::ptr::null(),
                &attr,
                &mut err,
            )
        };
        // (Not `then_some`: that would build — and Drop/free — an Output around a NULL stream.)
        if s.is_null() {
            None
        } else {
            Some(Output::Pulse { lib, s })
        }
    }

    fn open_alsa() -> Option<Output> {
        let lib = AlsaLib::get()?;
        let mut pcm: *mut c_void = std::ptr::null_mut();
        // SAFETY: `pcm` is a valid out-pointer; the handle is closed on failure.
        unsafe {
            if (lib.open)(&mut pcm, c"default".as_ptr(), SND_PCM_STREAM_PLAYBACK, 0) < 0
                || pcm.is_null()
            {
                return None;
            }
            if (lib.set_params)(
                pcm,
                SND_PCM_FORMAT_S16_LE,
                SND_PCM_ACCESS_RW_INTERLEAVED,
                2,
                OUT_RATE,
                1,
                60_000,
            ) < 0
            {
                (lib.close)(pcm);
                return None;
            }
        }
        Some(Output::Alsa { lib, pcm })
    }

    /// Blocking write of interleaved stereo frames. Returns `false` on an unrecoverable error.
    fn write(&mut self, frames: &[i16]) -> bool {
        match *self {
            Output::Pulse { lib, s } => {
                let mut err: c_int = 0;
                // SAFETY: `s` is a live stream; the buffer is valid for its byte length.
                unsafe {
                    (lib.write)(
                        s,
                        frames.as_ptr().cast(),
                        std::mem::size_of_val(frames),
                        &mut err,
                    ) >= 0
                }
            }
            Output::Alsa { lib, pcm } => {
                let mut rest = frames;
                let mut retries = 0;
                while !rest.is_empty() {
                    // SAFETY: `pcm` is a live handle; the buffer holds `rest.len() / 2` frames.
                    let r = unsafe {
                        (lib.writei)(pcm, rest.as_ptr().cast(), (rest.len() / 2) as c_ulong)
                    };
                    if r < 0 {
                        retries += 1;
                        // SAFETY: as above.
                        if retries > 3 || unsafe { (lib.recover)(pcm, r as c_int, 1) } < 0 {
                            return false;
                        }
                    } else {
                        rest = &rest[(r as usize * 2).min(rest.len())..];
                    }
                }
                true
            }
        }
    }

    /// Let the queued audio finish, then close.
    fn close(self) {
        if let Output::Pulse { lib, s } = self {
            let mut err: c_int = 0;
            // SAFETY: `s` is a live stream (freed by Drop right after).
            unsafe {
                (lib.drain)(s, &mut err);
            }
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        // SAFETY: each handle is released exactly once, here.
        unsafe {
            match *self {
                Output::Pulse { lib, s } => (lib.free)(s),
                Output::Alsa { lib, pcm } => {
                    (lib.close)(pcm);
                }
            }
        }
        diag::BACKEND.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Format-0 SMF: tempo 500000, division 96; ch0 C4 for 96 ticks (0.5 s), ch9 kick.
    const TINY_SMF: &[u8] = &[
        b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0, 96, //
        b'M', b'T', b'r', b'k', 0, 0, 0, 23, //
        0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20, // tempo
        0x00, 0x90, 60, 100, // note on
        0x00, 0x99, 36, 100, // drum
        0x60, 0x80, 60, 0, // note off @96
        0x00, 0xFF, 0x2F, 0x00, // end of track
    ];

    fn render_song(song: &mut MidiSong, max_frames: usize) -> (usize, f32) {
        let mut l = vec![0.0; BLOCK_FRAMES];
        let mut r = vec![0.0; BLOCK_FRAMES];
        let mut frames = 0;
        let mut peak = 0.0f32;
        while frames < max_frames {
            let alive = song.render(&mut l, &mut r);
            peak = l.iter().chain(&r).fold(peak, |p, s| p.max(s.abs()));
            frames += BLOCK_FRAMES;
            if !alive {
                break;
            }
        }
        (frames, peak)
    }

    #[test]
    fn smf_parses_and_scales_velocity() {
        let (div, ev) = parse_smf(TINY_SMF).unwrap();
        assert_eq!(div, 96);
        assert_eq!(ev.len(), 4);
        match ev[1].kind {
            MidiEventKind::ShortMsg(m) => assert_eq!(m, 0x90 | (60 << 8) | (75 << 16)),
            _ => panic!(),
        }
    }

    #[test]
    fn builtin_synth_plays_and_loops_end() {
        let mut song = MidiSong::new(TINY_SMF, 2, 1).unwrap();
        song.engine = MidiEngine::Basic(Box::new(BasicSynth::new()));
        // Two 0.5 s passes + two 0.4 s gaps/tails = 1.8 s.
        let (frames, peak) = render_song(&mut song, OUT_RATE as usize * 10);
        assert!(peak > 0.01, "built-in synth is silent");
        let secs = frames as f32 / OUT_RATE as f32;
        assert!((1.75..1.85).contains(&secs), "song length {secs}");
    }

    #[test]
    fn soundfont_synth_is_non_silent() {
        let Some(mut song) = MidiSong::new(TINY_SMF, 1, 1) else { panic!() };
        if !matches!(song.engine, MidiEngine::SoundFont(_)) {
            assert!(
                !soundfont_candidates().iter().any(|p| p.is_file()),
                "a .sf2 exists but failed to load"
            );
            return; // no .sf2 on this machine
        }
        let (_, peak) = render_song(&mut song, OUT_RATE as usize * 2);
        assert!(peak > 0.01);
    }

    #[test]
    fn sfx_lane_does_not_interrupt_and_keeps_newest() {
        SFX_VOLUME_PCT.store(85, Ordering::Relaxed);
        let epochs = Arc::new(Epochs {
            wave: AtomicU64::new(1),
            midi: AtomicU64::new(1),
        });
        let tone = |f: f32, ms| {
            SfxReq::Tones(vec![Tone {
                freq_hz: f,
                duration_ms: ms,
                volume: 0.5,
            }])
        };
        let sfx = |req| Cmd::Sfx {
            lane: 7,
            epochs: Arc::clone(&epochs),
            epoch: 1,
            req,
        };
        let mut m = Mixer::default();
        let mut buf = vec![0i16; BLOCK_FRAMES * 2];
        m.handle(sfx(tone(500.0, 100)));
        assert!(m.render(&mut buf));
        m.handle(sfx(tone(600.0, 100)));
        m.handle(sfx(tone(700.0, 100)));
        let cur_len = |m: &Mixer| m.lanes[0].sfx.as_ref().map(|v| v.pcm.len());
        // Still playing the first one (100 ms @ 22050 = 2205 samples), newest pending.
        assert_eq!(cur_len(&m), Some(2205));
        assert!(matches!(m.lanes[0].pending, Some((1, SfxReq::Tones(ref t))) if t[0].freq_hz == 700.0));
        // stop_all drops current and pending immediately.
        epochs.wave.fetch_add(1, Ordering::Relaxed);
        m.render(&mut buf);
        assert!(!m.has_work());
        assert!(buf.iter().all(|&s| s == 0));
    }

    #[test]
    fn pcm_resampling_length() {
        SFX_VOLUME_PCT.store(85, Ordering::Relaxed);
        static CLIP: [u8; 11025] = [200; 11025];
        let v = start_sfx(
            SfxReq::Pcm8 {
                samples: &CLIP,
                sample_rate: 11025,
                volume: 0.58,
            },
            1,
        )
        .unwrap();
        let out_frames = (v.pcm.len() as f64 / v.step).ceil() as usize;
        assert_eq!(out_frames, OUT_RATE as usize);
    }
}
