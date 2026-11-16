//! AudioSource.GetSpectrumData: AudioSource::GetSpectrumData (180aaaff0) hands the array to FMOD::ChannelGroup::getSpectrum
//! (1810b7660) of the source's dry group, which reads the last 2 * N frames its head DSP mixed (the DSP's history
//! buffer, started by the first call: DSPFilter::startBuffering 1810ead00, 0x4000 frames long, DSPFilter::read 1810ea600
//! appends every block it mixes) through FMOD::DSPFFT::getSpectrum (1810f5880) and DSPFFT::process (1810f63e0).
//!
//! The audio thread's block is not deterministic in the real game: this models its DSP clock. The game's AudioManager
//! (ProjectSettings/AudioManager.asset) asks FMOD for 1024-frame blocks (m_DSPBufferSize, System::setDSPBufferSize in
//! AudioManager::InitNormal 180aabd80) at the software rate (m_SampleRate 0: InitNormal skips the custom rate and sets the software format FMOD::SystemI::SystemI
//! (1810bf400) started with, mOutputRate 48000 with the linear resampler; the device's rate is only the fallback when
//! setSoftwareFormat refuses, which 48000 never does; `PP_AUDIO_RATE` overrides), the stereo speaker mode (Default Speaker Mode 2). A block completes every 1024 / rate seconds of
//! the audio clock (`Audio::clock`); `PP_AUDIO_DSP_OFFSET` (DSP blocks, fractional allowed) shifts that clock against the
//! game's to line a run up with a recording. A channel started at clock t first sounds in the block being mixed at t.
//!
//! The dry group (one per AudioSource: AudioSource::CreateFMODGroups 180aa70f0, created at Play / PlayOneShot, every channel
//! of the source gets it by SoundChannelInstance::setChannelGroup; SoundChannelInstance's constructor 180ac50e0 starts
//! m_AmbientVolume, m_PlayableVolume, m_ChannelVolume, m_OneShotVolume at 1.0 and the pan at 0) sums every channel's:
//! - DSPResampler::read (1810eb270) at speed (i64)((freq / outRate) * 4.2949673e9) in 32.32 fixed point, freq =
//!   dopplerPitch * pitch * clip frequency (SoundChannelInstance::UpdatePitch 180acd890); FMOD_Resampler_Linear
//!   (18113b350: s[hi] * 3.0517578e-05 * (1 - frac) + s[hi + 1] * 3.0517578e-05 * frac, frac = lo * 2.3283064e-10) with the
//!   default linear method, FMOD_Resampler_NoInterp (18113a660) at speed exactly 1.0;
//! - the connection into the group head (DSPConnectionI::setPan 1810d8130 with the stereo speaker mode): a mono clip
//!   gets sqrt(1 - 0.5) on each side, a stereo clip 1.0 on the left; times the channel volume
//!   (SoundChannelInstance::UpdateVolume 180acd980: rolloff * channel volume; at spatial blend 0 the rolloff is 1;
//!   AudioSource::CacheFrameParameters 180aa4520).
//! Positions advance block by block at the pitch in force (`Audio::advance_mix`), a seek moves a channel at the next block.
//! Left channel (the spectrum's channel 0) is what the jukebox reads.

use super::audio::{Audio, Clip};
use crate::assets::audio::Pcm;
use std::sync::{Arc, OnceLock};

/// Frames per DSP block (AudioManager m_DSPBufferSize).
pub const DSP_BLOCK: u64 = 1024;
/// DSPFilter's history buffer length in frames (DSPFilter::getHistoryBuffer).
const HISTORY: usize = 0x4000;
/// System::mMaxOutputChannels with the stereo speaker mode.
const OUT_CHANNELS: usize = 2;

/// The software mixing rate: `PP_AUDIO_RATE`, else FMOD's default 48000 (AudioManager.asset m_SampleRate 0).
pub fn out_rate() -> f32 {
    static R: OnceLock<f32> = OnceLock::new();
    *R.get_or_init(|| std::env::var("PP_AUDIO_RATE").ok().and_then(|v| v.parse().ok()).filter(|r: &f32| *r > 0.0).unwrap_or(48000.0))
}

/// `PP_AUDIO_DSP_OFFSET`: how many DSP blocks the audio thread's clock runs ahead of the game's.
pub fn dsp_offset() -> f64 {
    static R: OnceLock<f64> = OnceLock::new();
    *R.get_or_init(|| std::env::var("PP_AUDIO_DSP_OFFSET").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0))
}

/// FMOD_DSP_FFT_WINDOW (UnityEngine.FFTWindow has the same values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    Rectangular = 0,
    Triangle = 1,
    Hamming = 2,
    Hanning = 3,
    Blackman = 4,
    BlackmanHarris = 5,
}

impl Window {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => Window::Triangle,
            2 => Window::Hamming,
            3 => Window::Hanning,
            4 => Window::Blackman,
            5 => Window::BlackmanHarris,
            // DSPFFT::getSpectrum's final else: every other value is the rectangular window.
            _ => Window::Rectangular,
        }
    }
}

/// DSPFFT::mCosTab: cosf(i * 2 pi / 32768) for the first quarter turn (DSPFFT::DSPFFT 1810f5810).
fn cos_tab() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        let step = f32::from_bits(0x39490fdb);
        (0..0x2000).map(|i| (i as f32 * step).cos()).collect()
    })
}

/// The table lookup DSPFFT::getSpectrum and process inline: cos(2 pi x) from the quarter table (x in turns).
pub fn cos_turns(x: f32) -> f32 {
    let t = cos_tab();
    let u = ((x * 32768.0) as i32).unsigned_abs() & 0x7fff;
    match u >> 13 {
        0 => t[u as usize],
        1 => -t[(0x3fff - u) as usize],
        2 => -t[(u - 0x4000) as usize],
        _ => t[(0x7fff - u) as usize],
    }
}

fn bit_reverse(mut v: u32, bits: u32) -> u32 {
    let mut r = 0;
    for _ in 0..bits {
        r = r * 2 | (v & 1);
        v >>= 1;
    }
    r
}

/// DSPFFT::process (1810f63e0): the decimation-in-frequency butterflies over `buf` (bit-reversed result).
pub fn fft_process(buf: &mut [[f32; 2]], bits: u32) {
    let n = 1usize << bits;
    let (mut half, mut groups) = (n / 2, 1usize);
    for _ in 0..bits {
        let mut start = 0usize;
        for _ in 0..groups {
            let k = (start / half) as u32;
            let f = bit_reverse(k, bits) as f32 * (1.0f32 / n as f32);
            let c = cos_turns(f);
            let s = -cos_turns(f - 0.25);
            for i in start..start + half {
                let [ar, ai] = buf[i];
                let [br, bi] = buf[i + half];
                let tr = br * c - bi * s;
                let ti = bi * c + br * s;
                buf[i] = [tr + ar, ti + ai];
                buf[i + half] = [ar - tr, ai - ti];
            }
            start += half * 2;
        }
        half /= 2;
        groups *= 2;
    }
}

/// DSPFFT::getSpectrum (1810f5880): `n` samples (a power of two) from the history ring `history` (`nch` interleaved
/// channels, `len` frames) starting at frame `pos`, channel `channel`, windowed, transformed; `out` gets the
/// n / 2 - 1 first magnitudes * 2.5 clamped to 1 (the rest is left alone).
pub fn dspfft_get_spectrum(history: &[f32], mut pos: usize, len: usize, out: &mut [f32], n: usize, channel: usize, nch: usize, window: Window) {
    let bits = n.ilog2();
    let inv = 1.0f32 / n as f32;
    let mut buf = vec![[0.0f32; 2]; n];
    for (i, b) in buf.iter_mut().enumerate() {
        let x = history[pos * nch + channel];
        let fi = i as f32 * inv;
        match window {
            Window::Triangle => {
                // (1 - |(i/n + i/n) - 1|) * x * (1/n)
                b[0] = (1.0 - ((fi + fi) - 1.0).abs()) * x * inv;
            }
            Window::Hamming => {
                b[0] = (0.54 - cos_turns(fi) * 0.46) * x * inv;
            }
            Window::Hanning => {
                b[0] = (1.0 - cos_turns(fi)) * 0.5 * x * inv;
            }
            Window::Blackman => {
                b[0] = ((0.42 - cos_turns(fi) * 0.5) + cos_turns(fi + fi) * 0.08) * x * inv;
            }
            Window::BlackmanHarris => {
                let (c1, c2, c3) = (cos_turns(fi), cos_turns(fi + fi), cos_turns(fi * 3.0));
                b[0] = (((0.35875 - c1 * 0.48829) + c2 * 0.14128) - c3 * 0.01168) * x * inv;
            }
            Window::Rectangular => {
                b[0] = inv * x;
            }
        }
        b[1] = 1e-8;
        pos += 1;
        if pos >= len {
            pos = 0;
        }
    }
    fft_process(&mut buf, bits);
    for (i, o) in out.iter_mut().enumerate().take((n / 2).saturating_sub(1)) {
        let [re, im] = buf[bit_reverse(i as u32, bits) as usize];
        let m = (im * im + re * re).sqrt() * 2.5;
        *o = if 1.0 <= m { 1.0 } else { m };
    }
}

/// FMOD_Resampler_Linear / NoInterp for the PCM16 clip: the left channel's output sample at 32.32 position `pos`.
/// A looping clip reads on past its end from its start, else silence.
fn resample_left(pcm: &Pcm, pos: u64, interpolate: bool, looping: bool) -> f32 {
    let frames = pcm.frames() as u64;
    let (hi, lo) = (pos >> 32, pos & 0xffff_ffff);
    let at = |i: u64| -> f32 {
        let i = if looping { i % frames } else { i };
        if i >= frames {
            return 0.0;
        }
        pcm.data[i as usize * pcm.channels.max(1)] as i32 as f32
    };
    if !interpolate {
        return at(hi) * 3.0517578e-05;
    }
    let frac = lo as f32 * 2.3283064e-10;
    at(hi) * 3.0517578e-05 * (1.0 - frac) + at(hi + 1) * 3.0517578e-05 * frac
}

/// What one channel did in one DSP block: the clip position (32.32 frames, never wrapped) it started the block at and
/// the resampler speed in force.
#[derive(Clone, Copy, Debug)]
pub struct BlockRec {
    pub block: i64,
    pub pos: u64,
    pub speed: u64,
    /// False while the channel (or the listener) is paused: nothing is read.
    pub audible: bool,
}

/// A channel's place in the DSP mix: advanced once per DSP block with the pitch in force (`Audio::advance_mix`), as the
/// resampler advances its position (DSPResampler::read 1810eb270).
#[derive(Clone, Copy, Debug)]
pub struct MixState {
    /// The next block to advance (the channel's first sounds in it).
    pub block: i64,
    /// The clip position at the start of `block`.
    pub pos: u64,
    /// The latest block advanced.
    pub rec: Option<BlockRec>,
    /// Channel::setPosition: the clip position the next block starts at.
    pub seek: Option<u64>,
}

impl MixState {
    pub fn new(block: i64, pos: u64) -> Self {
        Self { block, pos, rec: None, seek: None }
    }
}

/// The 32.32 clip position of `seconds` into the clip.
pub fn frame_pos(clip: &Clip, seconds: f64) -> u64 {
    ((seconds * clip.clip.frequency as f64).max(0.0) as u64) << 32
}

/// DSPResampler::setFrequency (1810ebc90): the speed in 32.32 fixed point.
pub fn resample_speed(clip_frequency: f32, pitch: f32) -> u64 {
    ((clip_frequency * pitch / out_rate()) * 4.2949673e9f32) as i64 as u64
}

/// One channel's left output sample at frame `frame` of the block `rec` describes.
pub fn voice_sample(pcm: &Pcm, rec: &BlockRec, frame: u64, looping: bool, volume: f32) -> f32 {
    let frames = pcm.frames() as u64;
    if !rec.audible || frames == 0 {
        return 0.0;
    }
    let pos = rec.pos.wrapping_add(frame.wrapping_mul(rec.speed));
    let pos = if looping { ((pos >> 32) % frames) << 32 | (pos & 0xffff_ffff) } else { pos };
    if (pos >> 32) >= frames {
        // The channel finished: DSPResampler::read fills the rest with silence.
        return 0.0;
    }
    let s = resample_left(pcm, pos, rec.speed != 1u64 << 32, looping);
    // DSPConnectionI::mix: in * level, the level being mVolume * (sqrt(1 - 0.5) for a mono clip, 1 for stereo's left).
    let level = if pcm.channels == 1 { volume * 0.5f32.sqrt() } else { volume };
    s * level
}

/// A source's dry channel group: the history DSPFilter::startBuffering began.
#[derive(Clone, Debug)]
pub struct DryGroup {
    /// The DSP blocks completed when the first getSpectrum started the history.
    first_block: i64,
    /// The history ring (frames * OUT_CHANNELS interleaved floats).
    history: Vec<f32>,
}

impl Audio {
    /// The DSP block an audio-clock time falls in.
    pub fn block_of(&self, clock: f64) -> i64 {
        ((clock * out_rate() as f64 / DSP_BLOCK as f64) + dsp_offset()).floor() as i64
    }

    /// DSP blocks the audio thread has completed.
    fn dsp_blocks(&self) -> i64 {
        self.block_of(self.clock)
    }

    /// The mixer's progress to the current block: every channel (main and one-shots alike; AudioSource::PlayOneShot
    /// 180aad970 puts them in the same m_dryGroup) moves on by one block's frames per block done, at the pitch the source
    /// has now (UpdatePitch sets the frequency between blocks), paused ones standing still.
    pub(super) fn advance_mix(&mut self) {
        let blocks = self.dsp_blocks();
        let listener_pause = self.listener_pause;
        for s in self.sources.values_mut() {
            let silent = s.paused || (listener_pause && !s.ignore_listener_pause);
            let pitch = s.doppler.pitch * s.pitch;
            for v in s.main.iter_mut().chain(s.one_shots.iter_mut()) {
                let speed = resample_speed(v.clip.clip.frequency as f32, pitch);
                let m = &mut v.mix;
                if m.block >= blocks {
                    continue;
                }
                if let Some(p) = m.seek.take() {
                    m.pos = p;
                }
                // Blocks before the last one only move the position.
                let skipped = (blocks - 1 - m.block) as u64;
                if !silent {
                    m.pos = m.pos.wrapping_add(speed.wrapping_mul(skipped * DSP_BLOCK));
                }
                m.rec = Some(BlockRec { block: blocks - 1, pos: m.pos, speed, audible: !silent });
                if !silent {
                    m.pos = m.pos.wrapping_add(speed.wrapping_mul(DSP_BLOCK));
                }
                m.block = blocks;
            }
        }
    }

    /// The clip's samples: a Utils.FileToAudioClip clip's own, an asset's decoded from the game's data folder (once).
    fn clip_pcm(&mut self, clip: &Clip) -> Option<Arc<Pcm>> {
        if let Some(p) = &clip.pcm {
            return Some(p.clone());
        }
        self.pcm_cache
            .entry(clip.asset)
            .or_insert_with(|| {
                let game = crate::game_assets::find_game(None).map_err(|e| eprintln!("GetSpectrumData: {e}")).ok()?;
                crate::assets::audio::decode(&clip.clip, &game.join("People Playground_Data")).map_err(|e| eprintln!("GetSpectrumData: {}: {e}", clip.clip.name)).ok().map(Arc::new)
            })
            .clone()
    }

    /// AudioSource.GetSpectrumData(array, channel, window): AudioSource_CUSTOM_GetSpectrumDataHelper (180ad6060) checks the
    /// length (a power of two from 64 to 8192) and the channel (below the output's channel count), then
    /// FMOD::ChannelGroup::getSpectrum fills the array; Err is the exception or logged error.
    pub fn get_spectrum_data(&mut self, id: i32, samples: usize, channel: i32, window: Window) -> Result<Vec<f32>, String> {
        if !samples.is_power_of_two() || !(64..=8192).contains(&samples) {
            return Err("AudioSource.GetSpectrumData failed; Length of sample buffer must be a power of two between 64 and 8192.".into());
        }
        if channel < 0 || channel as usize >= OUT_CHANNELS {
            return Err("ArgumentException: AudioSource.GetSpectrumData failed; invalid channel".into());
        }
        // No m_dryGroup (never played; a loaded voice means the game's source had played): the array stays zeroed.
        let s = self.get(id);
        if !self.dry_groups.contains(&id) && s.main.is_none() && s.one_shots.is_empty() {
            return Ok(vec![0.0; samples]);
        }
        self.dry_groups.insert(id);
        let blocks = self.dsp_blocks();
        let last = blocks - 1;
        // Every channel the last block mixed into the group: (clip, its record, looping, its volume).
        let mut voices: Vec<(Arc<Clip>, BlockRec, bool, f32)> = Vec::new();
        {
            let s = self.get(id);
            // UpdateVolume (180acd980): rolloff * channel volume * ambient (1) * playable (1), times the one-shot volume;
            // the rolloff is 1 at spatial blend 0 (all the jukebox's reads that matter).
            let base = if s.mute { 0.0 } else { s.volume };
            let has_main = s.main.is_some();
            for (i, v) in s.main.iter().chain(s.one_shots.iter()).enumerate() {
                if let Some(r) = v.mix.rec.filter(|r| r.block == last) {
                    let main = has_main && i == 0;
                    let vol = if main { base } else { base * v.volume_scale };
                    voices.push((v.clip.clone(), r, main && s.looping, vol));
                }
            }
        }
        let pcms: Vec<Option<Arc<Pcm>>> = voices.iter().map(|(c, ..)| self.clip_pcm(c)).collect();
        // ChannelGroupI::getSpectrum: startBuffering the first time.
        let g = self.dry.entry(id).or_insert_with(|| DryGroup { first_block: blocks, history: vec![0.0; HISTORY * OUT_CHANNELS] });
        let done = blocks - g.first_block;
        if done > 0 {
            // The block mixed last lands in the history right before its position.
            let pos = (done as u64 * DSP_BLOCK) as usize % HISTORY;
            let start = (pos + HISTORY - DSP_BLOCK as usize) % HISTORY;
            for f in 0..DSP_BLOCK as usize {
                let mut l = 0.0f32;
                for ((_, r, looping, vol), pcm) in voices.iter().zip(&pcms) {
                    if let Some(pcm) = pcm {
                        l += voice_sample(pcm, r, f as u64, *looping, *vol);
                    }
                }
                // The right side is not read; a mono clip's right level equals the left's.
                g.history[(start + f) * OUT_CHANNELS] = l;
                g.history[(start + f) * OUT_CHANNELS + 1] = l;
            }
        }
        // History position after the blocks mixed so far; the window is the 2 * N frames before it.
        let pos = (done.max(0) as u64 * DSP_BLOCK) as usize % HISTORY;
        let mut from = pos as i64 - 2 * samples as i64;
        if from < 0 {
            from += HISTORY as i64;
        }
        let mut out = vec![0.0f32; samples];
        dspfft_get_spectrum(&g.history, from as usize, HISTORY, &mut out, samples * 2, channel as usize, OUT_CHANNELS, window);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAU: f64 = std::f64::consts::TAU;

    /// The window of DSPFFT::getSpectrum for a constant 1.0 input: the fft's DC bin (output 0) times 2.5 / (1/n)... is the window's mean,
    /// so check the window itself through the buffer the function builds: with n samples of 1.0 the DC magnitude is
    /// sum(w) / n * 2.5 (clamped at 1).
    fn dc(window: Window, n: usize, x: f32) -> f32 {
        let hist = vec![x; n * 2];
        let mut out = vec![0.0; n / 2];
        dspfft_get_spectrum(&hist, 0, n, &mut out, n, 0, 2, window);
        out[0]
    }

    /// Hand sums of the decompiled formulas: the mean of each window over n = 512 points (w(i) from i / n), times 2.5.
    #[test]
    fn windows_match_their_formulas() {
        let n = 512usize;
        let mean = |w: &dyn Fn(f64) -> f64| (0..n).map(|i| w(i as f64 / n as f64)).sum::<f64>() / n as f64;
        let c = |f: f64| (TAU * f).cos();
        let cases: [(Window, f64); 6] = [
            (Window::Rectangular, 1.0),
            (Window::Triangle, mean(&|f| 1.0 - (2.0 * f - 1.0).abs())),
            (Window::Hamming, mean(&|f| 0.54 - 0.46 * c(f))),
            (Window::Hanning, mean(&|f| (1.0 - c(f)) * 0.5)),
            (Window::Blackman, mean(&|f| 0.42 - 0.5 * c(f) + 0.08 * c(2.0 * f))),
            (Window::BlackmanHarris, mean(&|f| 0.35875 - 0.48829 * c(f) + 0.14128 * c(2.0 * f) - 0.01168 * c(3.0 * f))),
        ];
        for (w, m) in cases {
            // x = 0.1 keeps the 2.5-scaled magnitude below the clamp; the cosine lookup is exact to ~2e-4 per point.
            let got = dc(w, n, 0.1) as f64;
            assert!((got - m * 0.1 * 2.5).abs() < 2e-4, "{w:?}: {got} vs {}", m * 0.25);
        }
        // Well-known values: Hann's mean is 0.5, Hamming's 0.54, Blackman's 0.42, Triangle's 0.5.
        assert!((dc(Window::Hanning, n, 0.1) - 0.125).abs() < 2e-4);
        assert!((dc(Window::Hamming, n, 0.1) - 0.135).abs() < 2e-4);
    }

    #[test]
    fn cosine_table_lookup() {
        assert_eq!(cos_turns(0.0), 1.0);
        // A quarter turn is table entry 0x3fff negated (the lookup's own off-by-one): -cos(16383 * 2 pi / 32768 - ...).
        assert!((cos_turns(0.25) - (-(8191.0 * TAU / 32768.0).cos() as f32)).abs() < 1e-6);
        assert!((cos_turns(0.5) + 1.0).abs() < 1e-3);
        assert!((cos_turns(-0.125) - (TAU * 0.125).cos() as f32).abs() < 1e-3);
    }

    /// A whole-bin sine of amplitude A gives A / 2 * 2.5 in its bin and ~0 elsewhere; DC gives x * 2.5.
    #[test]
    fn fft_of_a_sine_and_dc() {
        let n = 512usize;
        let hist: Vec<f32> = (0..n).flat_map(|i| { let v = (0.4 * (TAU * 8.0 * i as f64 / n as f64).sin()) as f32; [v, v] }).collect();
        let mut out = vec![0.0; n / 2];
        dspfft_get_spectrum(&hist, 0, n, &mut out, n, 0, 2, Window::Rectangular);
        assert!((out[8] - 0.5).abs() < 2e-3, "{}", out[8]);
        for (k, v) in out.iter().enumerate().take(n / 2 - 1) {
            if k != 8 {
                assert!(*v < 2e-3, "bin {k}: {v}");
            }
        }
        assert_eq!(out[255], 0.0);
        assert!((dc(Window::Rectangular, n, 0.2) - 0.5).abs() < 1e-4);
        // Silence: the 1e-8 imaginary parts alone give the DC bin 512e-8 * 2.5.
        assert!((dc(Window::Rectangular, n, 0.0) - 1.28e-5).abs() < 1e-7);
    }

    #[test]
    fn linear_resampler_and_levels() {
        let pcm = Pcm { channels: 1, frequency: 24000, data: (0..4000).map(|i| (i * 8) as i16).collect() };
        // clip 24000 Hz at 48000: speed 0.5 frames per output frame; the left level of a mono clip is volume * sqrt(0.5).
        let speed = resample_speed(24000.0, 1.0);
        assert_eq!(speed, 1u64 << 31);
        let r = BlockRec { block: 3, pos: 0, speed, audible: true };
        // Output frame 3 is clip position 1.5: (8 + 16) / 2 = 12 -> 12 / 32768 * 0.5 * sqrt(0.5).
        let want = 12.0 / 32768.0 * 0.5 * 0.5f32.sqrt();
        let got = voice_sample(&pcm, &r, 3, false, 0.5);
        assert!((got - want).abs() < 1e-9, "{got} vs {want}");
        // Past the end of a non-looping clip: silence; a looping one wraps; a paused channel reads nothing.
        let late = BlockRec { pos: 8001u64 << 32, ..r };
        assert_eq!(voice_sample(&pcm, &late, 0, false, 0.5), 0.0);
        assert!(voice_sample(&pcm, &late, 0, true, 0.5) != 0.0);
        assert_eq!(voice_sample(&pcm, &BlockRec { audible: false, ..r }, 3, false, 0.5), 0.0);
    }

    fn test_clip() -> super::super::audio::ClipRef {
        let pcm = Pcm { channels: 1, frequency: 48000, data: (0..48000).map(|i| (i % 1000) as i16).collect() };
        let clip = crate::assets::classes::AudioClip { name: "t".into(), load_type: 0, channels: 1, frequency: 48000, length: 1.0, resource: Default::default(), compression_format: 0 };
        Arc::new(Clip { asset: crate::assets::db::ObjRef { file: usize::MAX, path_id: 77 }, clip, pcm: Some(Arc::new(pcm)), stream: None })
    }

    /// The mixer follows the pitch block by block, not in closed form: 10 blocks at pitch 1 then 10 at pitch 2 end at
    /// 30 blocks' frames, and a seek moves the channel at the next block.
    #[test]
    fn mix_position_follows_the_pitch_per_block() {
        let mut a = Audio::default();
        let mut s = super::super::audio::AudioSource::default();
        s.active = true;
        s.enabled = true;
        s.clip = Some(test_clip());
        a.sources.insert(1, s);
        a.play(1);
        let block = DSP_BLOCK as f64 / out_rate() as f64;
        for _ in 0..10 {
            a.update(block as f32);
        }
        a.sources.get_mut(&1).unwrap().pitch = 2.0;
        for _ in 0..10 {
            a.update(block as f32);
        }
        let v = a.get(1).main.as_ref().unwrap();
        let frames = v.mix.pos >> 32;
        // 10 blocks at 1.0 + 10 at 2.0 = 30 blocks of 1024 frames (the clock's f32 steps may cost one block).
        assert!((frames as i64 - 30 * 1024).abs() <= 2 * 1024, "{frames}");
        let before = v.mix.pos;
        a.set_time(1, 0.5);
        a.update(block as f32);
        let v = a.get(1).main.as_ref().unwrap();
        assert!(v.mix.pos < before && (v.mix.pos >> 32) >= 24000, "{}", v.mix.pos >> 32);
    }

    /// One-shots mix into the source's dry group with the group of the main voice.
    #[test]
    fn one_shots_reach_the_spectrum() {
        let mut a = Audio::default();
        let mut s = super::super::audio::AudioSource::default();
        s.active = true;
        s.enabled = true;
        a.sources.insert(1, s);
        let c = test_clip();
        let block = DSP_BLOCK as f64 / out_rate() as f64;
        assert!(a.get_spectrum_data(1, 256, 0, Window::Rectangular).unwrap().iter().all(|v| *v < 1e-4));
        a.play_one_shot(1, Some(&c), 1.0);
        // No group before the play: the first read after it starts the buffering (DSPFilter::startBuffering).
        a.get_spectrum_data(1, 256, 0, Window::Rectangular).unwrap();
        for _ in 0..4 {
            a.update(block as f32);
        }
        let out = a.get_spectrum_data(1, 256, 0, Window::Rectangular).unwrap();
        assert!(out.iter().any(|v| *v > 1e-3), "{:?}", &out[..8]);
    }
}
