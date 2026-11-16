//! What the live game sounds like: `game::audio`'s AudioSources mixed to the default output device the way Unity's
//! FMOD plays them. Each frame the voices are read from the model (a recording's replay stays silent); the output
//! thread runs each voice on at its pitch and keeps it in step with the model's clock.
//!
//! Per voice (AudioSource::CacheFrameParameters, 180aa4520): the source's rolloff at its distance from the listener
//! (the main camera) blended in by the spatial blend, volume × PlayOneShot's volumeScale × AudioListener.volume
//! (unless ignoreListenerVolume), and FMOD's speaker levels from the 2D stereo pan and the 3D direction. Then Default.mixer: the
//! group's volume (Sound effects, Ambience after its high-pass, User interface) and Master, as MixerControllerBehaviour.Sync
//! sets them from the preferences (Lerp(-80, 0, p^0.3) dB).
//! Then the channel groups (AudioManager::InitNormal): the mixer's output and the sources without a mixer group into
//! FX_UseVolume and FX_IgnoreVolume, whose head is the listener's AudioReverbFilter (`crate::sfx_reverb`, run on
//! FMOD's 1024-frame mix blocks), then the bypassListenerEffects sources (NoFX) beside it into the master. The pitch
//! includes the Doppler pitch (`game::audio::Doppler`).

use crate::assets::audio::Pcm;
use crate::assets::db::ObjRef;
use crate::game::audio::MixerGroup;
use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

/// MixerControllerBehaviour.ScaleVolume: Mathf.Lerp(-80, 0, Mathf.Pow(p, 0.3)).
pub fn scale_volume(p: f32) -> f32 {
    crate::game::mathf::lerp(-80.0, 0.0, crate::game::mathf::pow(p, 0.3))
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// MixerControllerBehaviour.Sync (and MixerControllerBehaviour.Start, which calls it): Default.mixer's Master, Sound
/// effects, User interface and Ambience volumes at ScaleVolume of the preferences and the ambience cutoff. Every
/// writer of those preferences calls Sync right after (SliderUserPreferenceBehaviour.Save, SettingsGeneratorBehaviour
/// .ApplySetting, Global.SetPausedMenu), so the mixer always holds the current ones: read here each frame.
fn mixer_sync<'a>(out: &'a mut AudioOut, settings: &crate::ui::Settings) -> std::sync::MutexGuard<'a, Mix> {
    let cutoff = settings.get("AmbienceHighpassCutoff");
    let rehp = cutoff != out.cutoff;
    out.cutoff = cutoff;
    let mut m = out.mix.lock().unwrap();
    m.master = db_to_gain(scale_volume(settings.get("MasterVolume")));
    m.groups = [1.0, db_to_gain(scale_volume(settings.get("SfxVolume"))), db_to_gain(scale_volume(settings.get("AmbienceVolume"))), db_to_gain(scale_volume(settings.get("UserInterfaceVolume")))];
    if rehp {
        m.hp = highpass(cutoff, m.out_rate);
    }
    m
}

/// What a voice plays: a clip's samples, or a user stream's blocks as it reads them (`buf`, `pos` frames into it).
#[derive(Clone)]
enum Src {
    Pcm(Arc<Pcm>),
    Stream(Arc<crate::game::audio::PcmStream>),
}

impl Src {
    fn channels(&self) -> usize {
        match self {
            Src::Pcm(p) => p.channels,
            Src::Stream(s) => s.channels,
        }
    }
    fn frequency(&self) -> u32 {
        match self {
            Src::Pcm(p) => p.frequency,
            Src::Stream(s) => s.frequency,
        }
    }
}

/// A voice as the output plays it.
struct Voice {
    src: Src,
    /// A stream's samples read and not yet played.
    buf: std::collections::VecDeque<f32>,
    /// Sample frames into the clip (into `buf` for a stream).
    pos: f64,
    /// Clip frames per output frame.
    rate: f64,
    /// Output speaker (left, right) by clip channel, volume included.
    gain: [[f32; 2]; 2],
    /// 0 Master, 1 Sound effects, 2 Ambience, 3 User interface; 4 no mixer group, 5 no mixer group bypassing the
    /// listener's effects.
    group: usize,
    looping: bool,
    /// Present in the last sync.
    seen: bool,
}

struct Mix {
    voices: HashMap<u64, Voice>,
    /// Each group's gain (Master's applies after).
    groups: [f32; 4],
    master: f32,
    /// The ambience high-pass (RBJ biquad) coefficients and state per channel.
    hp: [f32; 5],
    hp_state: [[f32; 4]; 2],
    out_rate: f32,
    /// The listener's AudioReverbFilter DSP.
    reverb: Option<crate::sfx_reverb::SfxReverb>,
    /// Mixed frames not yet handed out (FMOD mixes in blocks).
    pending: std::collections::VecDeque<[f32; 2]>,
}

/// FMOD's DSP block length (Unity's default DSP buffer size, 1024 frames).
const BLOCK: usize = 1024;

/// The output's handle: the shared mix and the decoded clips.
#[derive(Resource)]
pub struct AudioOut {
    mix: Arc<Mutex<Mix>>,
    pcm: Arc<Mutex<HashMap<ObjRef, Option<Arc<Pcm>>>>>,
    decode: mpsc::Sender<(ObjRef, crate::assets::classes::AudioClip)>,
    /// The game's data folder, for the decoder (set once the game has its data).
    data_dir: Arc<Mutex<Option<std::path::PathBuf>>>,
    cutoff: f32,
}

/// Adds the output when there is an output device (none: the game runs silent).
pub fn add(app: &mut App) {
    let mix = Arc::new(Mutex::new(Mix { voices: HashMap::new(), groups: [1.0; 4], master: 1.0, hp: [1.0, 0.0, 0.0, 0.0, 0.0], hp_state: [[0.0; 4]; 2], out_rate: 48000.0, reverb: None, pending: Default::default() }));
    let (ready_tx, ready_rx) = mpsc::channel();
    let m = mix.clone();
    // The stream lives on its own thread (cpal streams are not Send).
    std::thread::spawn(move || {
        let stream = (|| {
            let device = cpal::default_host().default_output_device().ok_or("no output device")?;
            let config = device.default_output_config().map_err(|e| e.to_string())?;
            if config.sample_format() != cpal::SampleFormat::F32 {
                return Err(format!("output format {:?}", config.sample_format()));
            }
            let config = config.config();
            let channels = config.channels as usize;
            m.lock().unwrap().out_rate = config.sample_rate as f32;
            let m2 = m.clone();
            let stream = device
                .build_output_stream(&config, move |out: &mut [f32], _: &cpal::OutputCallbackInfo| render(&mut m2.lock().unwrap(), out, channels), |e| eprintln!("audio: {e}"), None)
                .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            Ok::<_, String>(stream)
        })();
        let ok = stream.is_ok();
        if let Err(e) = &stream {
            eprintln!("audio: no sound ({e})");
        }
        let _ = ready_tx.send(ok);
        if ok {
            loop {
                std::thread::park();
            }
        }
    });
    if !ready_rx.recv().unwrap_or(false) {
        return;
    }
    let pcm = Arc::new(Mutex::new(HashMap::new()));
    let (tx, rx) = mpsc::channel::<(ObjRef, crate::assets::classes::AudioClip)>();
    let (p, dir) = (pcm.clone(), Arc::new(Mutex::new(None::<std::path::PathBuf>)));
    let dir2 = dir.clone();
    std::thread::spawn(move || {
        for (r, clip) in rx {
            let Some(d) = dir2.lock().unwrap().clone() else { continue };
            let decoded = crate::assets::audio::decode(&clip, &d).map_err(|e| eprintln!("audio: {}: {e}", clip.name)).ok().map(Arc::new);
            p.lock().unwrap().insert(r, decoded);
        }
    });
    app.insert_resource(AudioOut { mix, pcm, decode: tx, data_dir: dir, cutoff: f32::NAN });
    app.add_systems(PostUpdate, sync);
}

/// The voices' gains and pan for this frame, from the model.
/// The Main scene's sources when a game runs, else the Menu scene's (`ui::MenuAudio`, all under the scene root).
fn sync(mut out: ResMut<AudioOut>, play: Option<Res<crate::play::Play>>, menu: Option<Res<crate::ui::MenuAudio>>, settings: Res<crate::ui::Settings>) {
    type Position<'a> = Box<dyn Fn(&crate::game::audio::AudioSource) -> [f32; 3] + 'a>;
    let (audio, listener, axes, data_dir, position, reverb): (_, _, _, _, Position, _) = if let Some(play) = &play {
        let g = &play.game;
        let cam = g.camera_position();
        // The AudioListener sits on the camera: its right axis turns with the shake's roll.
        let axes = [crate::game::mathf::rotate(g.camera.rotation, [1.0, 0.0, 0.0]), crate::game::mathf::rotate(g.camera.rotation, [0.0, 0.0, 1.0])];
        (&g.audio, [cam[0], cam[1], g.camera.z], axes, g.assets.as_ref().map(|db| &db.data_dir), Box::new(move |s| g.audio_position(s)), g.camera.reverb.filter(|_| g.camera.reverb_enabled))
    } else if let Some(m) = &menu {
        (&m.audio, m.listener, [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], Some(&m.data_dir), Box::new(|s| crate::game::audio::root_position(&s.chain)), None)
    } else {
        let mut m = out.mix.lock().unwrap();
        m.voices.clear();
        m.reverb = None;
        return;
    };
    if let Some(d) = data_dir {
        out.data_dir.lock().unwrap().get_or_insert_with(|| d.clone());
    }
    let listener_volume = audio.listener_volume;
    let mut wanted = Vec::new();
    {
        let mut pcm = out.pcm.lock().unwrap();
        for s in audio.sources.values() {
            if !s.live() || s.paused || (audio.listener_pause && !s.ignore_listener_pause) {
                continue;
            }
            let p = position(s);
            let d = ((p[0] - listener[0]).powi(2) + (p[1] - listener[1]).powi(2) + (p[2] - listener[2]).powi(2)).sqrt();
            let blend = s.spatial_blend_at(d);
            let mut volume = s.volume * (1.0 + (s.rolloff(d) - 1.0) * blend);
            // Through a mixer group the output is FX_UseVolume; without one ignoreListenerVolume picks FX_IgnoreVolume.
            if s.group.is_some() || !s.ignore_listener_volume {
                volume *= listener_volume;
            }
            if s.mute {
                volume = 0.0;
            }
            let pan = Pan { dir: listener_direction([p[0] - listener[0], p[1] - listener[1], p[2] - listener[2]], axes), level: blend, pan: s.pan_stereo, spread: s.spread_at(d) };
            let pitch = s.doppler.pitch * s.pitch;
            let group = match s.group {
                None if s.bypass_listener_effects => 5,
                None => 4,
                Some(MixerGroup::Master) => 0,
                Some(MixerGroup::SoundEffects) => 1,
                Some(MixerGroup::Ambience) => 2,
                Some(MixerGroup::UserInterface) => 3,
            };
            for (v, main) in s.main.iter().filter(|v| v.delay <= 0.0).map(|v| (v, true)).chain(s.one_shots.iter().map(|v| (v, false))) {
                if let Some(c) = &v.clip.stream {
                    wanted.push((v.id, Src::Stream(c.clone()), v.time, pitch, volume * v.volume_scale, pan, group, main && s.looping));
                    continue;
                }
                if let Some(c) = &v.clip.pcm {
                    wanted.push((v.id, Src::Pcm(c.clone()), v.time, pitch, volume * v.volume_scale, pan, group, main && s.looping));
                    continue;
                }
                match pcm.get(&v.clip.asset) {
                    Some(Some(c)) => wanted.push((v.id, Src::Pcm(c.clone()), v.time, pitch, volume * v.volume_scale, pan, group, main && s.looping)),
                    Some(None) => {}
                    None => {
                        pcm.insert(v.clip.asset, None);
                        let _ = out.decode.send((v.clip.asset, v.clip.clip.clone()));
                    }
                }
            }
        }
    }
    let mut m = mixer_sync(&mut *out, &settings);
    // The Main Camera's filter: its DSP made with the scene (AudioReverbFilter::AddToManager), every parameter set
    // each frame (each property setter runs AudioReverbFilter::Update).
    match reverb {
        Some(p) => {
            if m.reverb.is_none() || play.as_ref().is_some_and(|p| p.is_added()) {
                m.reverb = Some(crate::sfx_reverb::SfxReverb::new(m.out_rate as i32));
            }
            let r = m.reverb.as_mut().unwrap();
            for (i, v) in p.into_iter().enumerate() {
                r.set_parameter(i, v);
            }
        }
        None => m.reverb = None,
    }
    for v in m.voices.values_mut() {
        v.seen = false;
    }
    let out_rate = m.out_rate as f64;
    for (id, src, time, pitch, volume, pan, group, looping) in wanted {
        let freq = src.frequency() as f64;
        let rate = pitch as f64 * freq / out_rate;
        let at = time * freq;
        let stream = matches!(src, Src::Stream(_));
        let gain = pan.levels(src.channels()).map(|o| o.map(|x| x * volume));
        let v = m.voices.entry(id).or_insert_with(|| Voice { src, buf: Default::default(), pos: if stream { 0.0 } else { at }, rate, gain, group, looping, seen: true });
        // Kept in step with the model (AudioSource.time set, a hitch): more than a tenth of a second apart, it jumps.
        // A stream plays on from whatever it reads.
        if !stream && (v.pos - at).abs() > 0.1 * freq {
            v.pos = at;
        }
        (v.rate, v.gain, v.group, v.looping, v.seen) = (rate, gain, group, looping, true);
    }
    m.voices.retain(|_, v| v.seen);
}

/// What FMOD's channel is given: the listener-space direction (set2DFreqVolumePanFor3D's x on the listener's right and
/// z on its forward axis), the 3D pan level (spatialBlend), the 2D pan (panStereo, ChannelI::setPan) and the 3D spread in
/// degrees.
#[derive(Clone, Copy)]
struct Pan {
    dir: [f32; 2],
    level: f32,
    pan: f32,
    spread: f32,
}

/// The start of ChannelRealManual3D::set2DFreqVolumePanFor3D (181164e40): the source's offset from the listener,
/// normalized, on the listener's right and forward axes, clamped to [-1, 1].
fn listener_direction(d: [f32; 3], [right, forward]: [[f32; 3]; 2]) -> [f32; 2] {
    let n = (d[1] * d[1] + d[0] * d[0] + d[2] * d[2]).sqrt();
    let d = if 0.0 < n {
        let r = 1.0 / n;
        [d[0] * r, d[1] * r, d[2] * r]
    } else {
        [0.0; 3]
    };
    let dot = |a: [f32; 3]| (d[1] * a[1] + d[0] * a[0] + d[2] * a[2]).clamp(-1.0, 1.0);
    [dot(right), dot(forward)]
}

/// FMOD's angle code of an XZ direction (SystemI::set3DSpeakerPosition, prepareSpeakerPairs): 0 for none, else round
/// the square: front 1, right 3, back 5, left 7.
fn xz_angle(x: f32, z: f32) -> f32 {
    if x == 0.0 && z == 0.0 {
        0.0
    } else if z.abs() < x.abs() {
        let a = 3.0 - z / x.abs();
        if x < 0.0 { 10.0 - a } else { a }
    } else {
        let a = x / z.abs() + 1.0;
        if z < 0.0 { 6.0 - a } else { a }
    }
}

impl Pan {
    /// ChannelRealManual3D::set2DFreqVolumePanFor3D (181164e40) in FMOD_SPEAKERMODE_STEREO (SystemI::setSpeakerMode: front
    /// left (-1, 0) and front right (1, 0) active; sortSpeakerList orders them right (angle 3) then left (7), and
    /// prepareSpeakerPairs leaves both pairs without VBAP): the output speakers' (left, right) levels by clip channel, the
    /// 3D pair levels blended with the 2D pan's by the 3D pan level (pan mode setPan). A mono clip with a spread plays as
    /// two channels turned +spread/2 and -spread/2, averaged back to one.
    fn levels(self, channels: usize) -> [[f32; 2]; 2] {
        let [x, z] = self.dir;
        let channels = channels.clamp(1, 2);
        let split = self.spread != 0.0 && channels == 1;
        let n = if split { 2 } else { channels };
        // [speaker][channel]
        let mut m = [[0.0f32; 2]; 2];
        if 0.0 < self.level {
            for c in 0..n {
                let (x, z) = if self.spread == 0.0 || n == 1 {
                    (x, z)
                } else {
                    let a = ((c as f32 + c as f32) / (n - 1) as f32 - 1.0) * -0.5 * (self.spread * 0.017453292);
                    let (s, k) = (a.sin(), a.cos());
                    (k * x - s * z, k * z + s * x)
                };
                if x == 0.0 && z == 0.0 {
                    m[0][c] = 1.4142135 / 2.0;
                    m[1][c] = 1.4142135 / 2.0;
                    continue;
                }
                let o = xz_angle(x, z);
                // The pair (right, left) spans angles [3, 7), (left, right) [7, 11) going round.
                let (a, b, nx) = if (3.0..7.0).contains(&o) {
                    (1, 0, 1.0f32)
                } else if (7.0..11.0).contains(&o) || (7.0..11.0).contains(&(o + 8.0)) {
                    (0, 1, -1.0)
                } else {
                    continue;
                };
                let f = x * nx + z * 0.0;
                let s = (2.0 - f * f).sqrt();
                m[a][c] = (s + f) * 0.5;
                m[b][c] = (s - f) * 0.5;
            }
        }
        let l = self.level;
        if n < 2 {
            if l < 1.0 {
                let p = (self.pan + 1.0) * 0.5;
                m[0][0] = (1.0 - p).sqrt() * (1.0 - l) + l * m[0][0];
                m[1][0] = p.sqrt() * (1.0 - l) + l * m[1][0];
            }
            return m;
        }
        if l < 1.0 {
            // DSPI::calculateSpeakerLevels (1810bb080), two channels into stereo: channel 0 to the left at the balance's
            // left level, channel 1 to the right.
            let p = (self.pan + 1.0) * 0.5;
            let (bl, br) = if 0.5 < p { ((1.0 - p) + (1.0 - p), 1.0) } else { (1.0, p + p) };
            let flat = [[bl, 0.0], [0.0, br]];
            for s in 0..2 {
                for c in 0..2 {
                    m[s][c] = (1.0 - l) * flat[s][c] + l * m[s][c];
                }
            }
        }
        if split {
            for s in &mut m {
                *s = [(s[1] + s[0]) * 0.5, 0.0];
            }
        }
        m
    }
}

/// RBJ high-pass at `cutoff` Hz, resonance (Q) 1: [b0, b1, b2, a1, a2] over a0.
fn highpass(cutoff: f32, rate: f32) -> [f32; 5] {
    let w = 2.0 * std::f32::consts::PI * cutoff / rate;
    let alpha = w.sin() / 2.0;
    let c = w.cos();
    let a0 = 1.0 + alpha;
    [(1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0, -2.0 * c / a0, (1.0 - alpha) / a0]
}

/// Fills an interleaved output buffer from the mix blocks.
fn render(m: &mut Mix, out: &mut [f32], channels: usize) {
    let frames = out.len() / channels.max(1);
    while m.pending.len() < frames {
        mix_block(m);
    }
    for f in 0..frames {
        let o = m.pending.pop_front().unwrap();
        for c in 0..channels {
            out[f * channels + c] = o[c.min(1)].clamp(-1.0, 1.0);
        }
    }
}

/// One FMOD mix block: the voices into their buses, the mixer, the channel groups and the listener's reverb.
fn mix_block(m: &mut Mix) {
    let frames = BLOCK;
    let mut buses = vec![[0.0f32; 2]; frames * 6];
    m.voices.retain(|_, v| {
        let pcm = match &v.src {
            Src::Pcm(p) => p.clone(),
            Src::Stream(s) => {
                let s = s.clone();
                let ch = s.channels;
                for f in 0..frames {
                    // The frame and the next one, read as the stream gives them.
                    while v.buf.len() < (v.pos as usize + 2) * ch {
                        v.buf.extend(s.next_block());
                    }
                    let i = v.pos as usize;
                    let t = (v.pos - i as f64) as f32;
                    let at = |k: usize, c: usize| v.buf[k * ch + c.min(ch - 1)];
                    let l = at(i, 0) + (at(i + 1, 0) - at(i, 0)) * t;
                    let r = at(i, 1) + (at(i + 1, 1) - at(i, 1)) * t;
                    let b = &mut buses[v.group * frames + f];
                    b[0] += l * v.gain[0][0] + r * v.gain[0][1];
                    b[1] += l * v.gain[1][0] + r * v.gain[1][1];
                    v.pos += v.rate;
                    let done = v.pos as usize;
                    v.buf.drain(..done * ch);
                    v.pos -= done as f64;
                }
                return true;
            }
        };
        let n = pcm.frames();
        let ch = pcm.channels;
        for f in 0..frames {
            if v.pos >= n as f64 {
                if !v.looping || n == 0 {
                    return false;
                }
                v.pos %= n as f64;
            }
            let i = v.pos as usize;
            let t = (v.pos - i as f64) as f32;
            let j = if i + 1 < n { i + 1 } else if v.looping { 0 } else { i };
            let s = |k: usize, c: usize| pcm.data[k * ch + c.min(ch - 1)] as f32 / 32768.0;
            let l = s(i, 0) + (s(j, 0) - s(i, 0)) * t;
            let r = s(i, 1) + (s(j, 1) - s(i, 1)) * t;
            let b = &mut buses[v.group * frames + f];
            b[0] += l * v.gain[0][0] + r * v.gain[0][1];
            b[1] += l * v.gain[1][0] + r * v.gain[1][1];
            v.pos += v.rate;
        }
        true
    });
    let hp = m.hp;
    // FX_IgnoreVolume's input: the mixer's output and the no-mixer FX sources.
    let mut fx = vec![0.0f32; frames * 2];
    for f in 0..frames {
        let mut o = [0.0f32; 2];
        for g in 0..4 {
            let mut x = buses[g * frames + f];
            if g == 2 {
                for c in 0..2 {
                    let st = &mut m.hp_state[c];
                    let y = hp[0] * x[c] + hp[1] * st[0] + hp[2] * st[1] - hp[3] * st[2] - hp[4] * st[3];
                    *st = [x[c], st[0], y, st[2]];
                    x[c] = y;
                }
            }
            o[0] += x[0] * m.groups[g];
            o[1] += x[1] * m.groups[g];
        }
        let n = buses[4 * frames + f];
        fx[2 * f] = o[0] * m.master + n[0];
        fx[2 * f + 1] = o[1] * m.master + n[1];
    }
    if let Some(r) = &mut m.reverb {
        // DSPSfxReverb::updateInternal before the block's read.
        r.update();
        let input = fx.clone();
        r.process(&input, &mut fx, 2);
    }
    for f in 0..frames {
        let n = buses[5 * frames + f];
        m.pending.push_back([fx[2 * f] + n[0], fx[2 * f + 1] + n[1]]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mixer_volume() {
        assert_eq!(super::scale_volume(1.0), 0.0);
        assert_eq!(super::scale_volume(0.0), -80.0);
    }

    #[test]
    fn fmod_stereo_levels() {
        let pan = |dir, level, pan, spread| super::Pan { dir, level, pan, spread };
        let h = 0.5f32.sqrt();
        // 2D: setPan's laws.
        assert_eq!(pan([1.0, 0.0], 0.0, 0.0, 0.0).levels(1), [[h, 0.0], [h, 0.0]]);
        assert_eq!(pan([1.0, 0.0], 0.0, 0.5, 0.0).levels(2), [[0.5, 0.0], [0.0, 1.0]]);
        // 3D: right of the listener all right; ahead or behind both halves of the pair law.
        assert_eq!(pan([1.0, 0.0], 1.0, 0.0, 0.0).levels(1), [[0.0, 0.0], [1.0, 0.0]]);
        assert_eq!(pan([-1.0, 0.0], 1.0, 0.0, 0.0).levels(2), [[1.0, 1.0], [0.0, 0.0]]);
        let s = 2.0f32.sqrt() * 0.5;
        assert_eq!(pan([0.0, 1.0], 1.0, 0.0, 0.0).levels(1), [[s, 0.0], [s, 0.0]]);
        assert_eq!(pan([0.0, -1.0], 1.0, 0.0, 0.0).levels(1), [[s, 0.0], [s, 0.0]]);
        assert_eq!(pan([0.0, 0.0], 1.0, 0.0, 0.0).levels(1), [[1.4142135 / 2.0, 0.0], [1.4142135 / 2.0, 0.0]]);
        // Half blended: the mean of the 2D and 3D levels.
        assert_eq!(pan([1.0, 0.0], 0.5, 0.0, 0.0).levels(1), [[h * 0.5, 0.0], [h * 0.5 + 0.5, 0.0]]);
        // A 180 degree spread ahead: the two halves at the right and the left, averaged.
        let w = pan([0.0, 1.0], 1.0, 0.0, 180.0).levels(1);
        assert!((w[0][0] - 0.5).abs() < 1e-6 && (w[1][0] - 0.5).abs() < 1e-6 && w[0][1] == 0.0, "{w:?}");
        assert_eq!((super::xz_angle(-1.0, 0.0), super::xz_angle(1.0, 0.0), super::xz_angle(0.0, -1.0)), (7.0, 3.0, 5.0));
    }
}
