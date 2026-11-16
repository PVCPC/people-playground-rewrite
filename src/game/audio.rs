//! UnityEngine.AudioSource and AudioListener as game state. The scripts read isPlaying, time and clip.length, so
//! the sources live here and advance deterministically: each frame's audio update (PostLateUpdate.UpdateAudio,
//! between the delayed Destroys and the WaitForEndOfFrame coroutines) moves every playing voice on by the frame's
//! unscaled time times its pitch, the way FMOD's mixer clock runs in real time with the pitch Global keeps at
//! Time.timeScale. A one-shot leaves the source's list at the first update past its end (AudioSource::Update);
//! the main voice stops there unless it loops. What is heard is `crate::audio_out`'s, which mirrors this state in
//! the live game only.
//!
//! AudioSource::IsPlaying (180aac820): not paused and either a one-shot or the main channel sounding.
//! AudioSource::AddToManager (180aa25b0): an enabled source with playOnAwake plays when it is enabled;
//! RemoveFromManager (180aaf080) cleans every channel up when it is disabled or destroyed.

use super::{script, BareAnchor};
use crate::assets::db::ObjRef;
use crate::game_assets::{Curve, CurveCache};
use crate::unity::transform::Trs;
use std::collections::BTreeMap;
use std::sync::Arc;

/// An AudioClip: what the model needs (length) and where its samples are (`crate::assets::audio::decode`).
#[derive(Debug)]
pub struct Clip {
    pub asset: ObjRef,
    pub clip: crate::assets::classes::AudioClip,
    /// AudioClip.Create's samples (Utils.FileToAudioClip); None for an asset.
    pub pcm: Option<Arc<crate::assets::audio::Pcm>>,
    /// AudioClip.Create(..., stream: true, pcmreadercallback)'s stream.
    pub stream: Option<Arc<PcmStream>>,
}

/// A user stream: AudioClip.Create(name, lengthSamples, channels, frequency, stream: true, callback). AudioClip::CreateSound
/// (180ab69c0) opens it as an FMOD user sound (mode 0x4d2: a looping stream) with no decodebuffersize, so FMOD reads it in
/// its default 400 ms blocks, cut at the clip's end where the stream loops; ScriptPCMReadCallback (180ab8ef0) hands the C#
/// callback a float[] of the block's bytes / 4. Opening the stream reads its first block (FMOD fills the stream buffer
/// before createSound returns); the output reads the rest as it plays.
pub struct PcmStream {
    pub channels: usize,
    pub frequency: u32,
    state: std::sync::Mutex<StreamState>,
}

struct StreamState {
    read: Box<dyn FnMut(&mut [f32]) + Send>,
    /// The block read at open, played first.
    first: Option<Vec<f32>>,
    /// Sample frames into the clip, and its length.
    pos: usize,
    length: usize,
}

impl std::fmt::Debug for PcmStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PcmStream({} ch, {} Hz)", self.channels, self.frequency)
    }
}

impl PcmStream {
    fn read_block(s: &mut StreamState, channels: usize, frequency: u32) -> Vec<f32> {
        let frames = (frequency as usize * 400 / 1000).min(s.length - s.pos);
        let mut data = vec![0.0f32; frames * channels];
        (s.read)(&mut data);
        s.pos = (s.pos + frames) % s.length;
        data
    }

    /// The next block of interleaved samples.
    pub fn next_block(&self) -> Vec<f32> {
        let mut s = self.state.lock().unwrap();
        if let Some(b) = s.first.take() {
            return b;
        }
        Self::read_block(&mut s, self.channels, self.frequency)
    }
}

/// AudioClip.Create(name, lengthSamples, channels, frequency, stream: true, read): the clip, its first block read.
/// `id` is its instance id (the audio output's key).
pub fn create_stream_clip(id: i32, name: &str, length_samples: usize, channels: usize, frequency: u32, read: Box<dyn FnMut(&mut [f32]) + Send>) -> ClipRef {
    if length_samples == 0 {
        unimplemented!("mod-only: AudioClip.Create with no samples (ArgumentException)");
    }
    let mut state = StreamState { read, first: None, pos: 0, length: length_samples };
    state.first = Some(PcmStream::read_block(&mut state, channels, frequency));
    let clip = crate::assets::classes::AudioClip { name: name.into(), load_type: 0, channels: channels as i32, frequency: frequency as i32, length: length_samples as f32 / frequency as f32, resource: Default::default(), compression_format: 0 };
    Arc::new(Clip { asset: ObjRef { file: usize::MAX, path_id: id as i64 }, clip, pcm: None, stream: Some(Arc::new(PcmStream { channels, frequency, state: std::sync::Mutex::new(state) })) })
}

impl Clip {
    /// AudioClip.length
    pub fn length(&self) -> f32 {
        self.clip.length
    }
}

pub type ClipRef = Arc<Clip>;

/// Utils.FileToAudioClip(path): AudioClip.Create(path, samples, 1, rate) with the file's samples. Its asset key is
/// `ObjRef { file: usize::MAX, path_id: n }` (the audio output's cache key).
pub fn file_to_audio_clip(path: &std::path::Path, n: i64) -> Result<ClipRef, String> {
    let pcm = crate::assets::audio::file_to_pcm(path)?;
    let name = path.display().to_string();
    // AudioClip.length = samples / frequency
    let length = pcm.data.len() as f32 / pcm.frequency as f32;
    let clip = crate::assets::classes::AudioClip { name, load_type: 0, channels: 1, frequency: pcm.frequency as i32, length, resource: Default::default(), compression_format: 0 };
    Ok(Arc::new(Clip { asset: ObjRef { file: usize::MAX, path_id: n }, clip, pcm: Some(Arc::new(pcm)), stream: None }))
}

/// A clip asset as a ClipRef (None for a null PPtr), read once.
pub fn clip_ref(db: &crate::assets::db::AssetDb, cache: &mut std::collections::HashMap<ObjRef, ClipRef>, r: Option<ObjRef>) -> Result<Option<ClipRef>, String> {
    let Some(asset) = r else { return Ok(None) };
    if let Some(c) = cache.get(&asset) {
        return Ok(Some(c.clone()));
    }
    if db.class(asset) != 83 {
        return Err(format!("{asset:?} is no AudioClip"));
    }
    let c = Arc::new(Clip { asset, clip: crate::assets::classes::AudioClip::read(db.data(asset))?, pcm: None, stream: None });
    cache.insert(asset, c.clone());
    Ok(Some(c))
}

impl Audio {
    /// Records a script component's AudioClip and AudioClip[] fields (top level; a PPtr resolving to class 83).
    pub fn register_clip_fields(&mut self, db: &crate::assets::db::AssetDb, file: usize, id: i32, fields: &crate::assets::mono::Value) -> Result<(), String> {
        use crate::assets::mono::Value;
        let Value::Struct(fs) = fields else { return Ok(()) };
        for (name, v) in fs {
            let ptrs: Vec<_> = match v {
                Value::PPtr(p) => vec![*p],
                Value::Array(a) if a.first().is_some_and(|x| x.pptr().is_some()) => a.iter().filter_map(|x| x.pptr()).collect(),
                _ => continue,
            };
            let refs: Vec<_> = ptrs.iter().map(|&p| db.resolve(file, p)).collect();
            // Only fields whose every non-null element is an AudioClip (an empty or all-null array is skipped).
            if refs.iter().all(|r| r.is_none_or(|r| db.class(r) == 83)) && refs.iter().any(|r| r.is_some()) {
                let clips = refs.into_iter().map(|r| clip_ref(db, &mut self.clip_cache, r)).collect::<Result<_, _>>()?;
                self.clip_fields.insert((id, name.clone()), clips);
            }
        }
        Ok(())
    }

    /// Every AudioClip a UnityEvent persistent call of `v` passes (m_ObjectArgument), for `AudioCall`.
    pub fn register_event_clips(&mut self, db: &crate::assets::db::AssetDb, file: usize, v: &crate::assets::mono::Value) -> Result<(), String> {
        use crate::assets::mono::Value;
        match v {
            Value::Struct(fs) => {
                for (name, x) in fs {
                    match x {
                        Value::PPtr(p) if name == "m_ObjectArgument" && !p.is_null() => {
                            if let Some(r) = db.resolve(file, *p).filter(|&r| db.class(r) == 83) {
                                let c = clip_ref(db, &mut self.clip_cache, Some(r))?.unwrap();
                                self.event_clips.insert((file, *p), c);
                            }
                        }
                        _ => self.register_event_clips(db, file, x)?,
                    }
                }
            }
            Value::Array(a) => {
                for x in a {
                    self.register_event_clips(db, file, x)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Component `id`'s clip field `name` (an AudioClip[] or a single AudioClip); empty when the field was null or
    /// empty.
    pub fn clips(&self, id: i32, name: &str) -> &[Option<ClipRef>] {
        self.clip_fields.get(&(id, name.to_string())).map_or(&[], |v| v.as_slice())
    }

    /// Component `id`'s AudioSource field `name` (None: null or outside the clone).
    pub fn source_field(&self, id: i32, name: &str) -> Option<i32> {
        self.source_fields.get(&(id, name.to_string())).and_then(|v| v.first().copied().flatten())
    }

    /// Component `id`'s AudioSource[] field `name`.
    pub fn source_fields(&self, id: i32, name: &str) -> &[Option<i32>] {
        self.source_fields.get(&(id, name.to_string())).map_or(&[], |v| v.as_slice())
    }

    /// GetComponent<AudioSource>() on the object of component `id` (None when it has none).
    pub fn object_source(&self, id: i32) -> Option<i32> {
        self.object_sources.get(&id).and_then(|v| v.first().copied())
    }

    /// The single AudioClip field `name` of component `id`.
    pub fn clip(&self, id: i32, name: &str) -> Option<ClipRef> {
        self.clips(id, name).first().cloned().flatten()
    }
}

/// The groups of Default.mixer: Master and its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MixerGroup {
    Master,
    SoundEffects,
    Ambience,
    UserInterface,
}

impl MixerGroup {
    pub fn from_name(name: &str) -> Self {
        match name {
            "Master" => Self::Master,
            "Sound effects" => Self::SoundEffects,
            "Ambience" => Self::Ambience,
            "User interface" => Self::UserInterface,
            _ => unimplemented!("mod-only: an AudioSource routed to mixer group {name:?} (not in Default.mixer)"),
        }
    }
}

/// A sounding channel: the main one (Play) or a one-shot.
#[derive(Clone, Debug)]
pub struct Voice {
    /// Unique per voice (the output's handle on it).
    pub id: u64,
    pub clip: ClipRef,
    /// Seconds into the clip.
    pub time: f64,
    /// PlayOneShot's volumeScale (1 for the main voice).
    pub volume_scale: f32,
    /// PlayDelayed's wait left (unscaled seconds; the clip starts when it runs out).
    pub delay: f64,
    /// The audio clock (unscaled seconds) when the voice was started (after any PlayDelayed wait): the DSP block the
    /// channel's first samples are mixed in (`audio_spectrum`).
    pub born: f64,
    /// The channel's place in the DSP mix (`audio_spectrum`).
    pub mix: super::audio_spectrum::MixState,
}

/// UnityEngine.AudioSource.
#[derive(Clone, Debug)]
pub struct AudioSource {
    /// Where it sits: the transform chain down from a physical or node (`Game::anchored_world`).
    pub anchor: BareAnchor,
    pub chain: Vec<Trs>,
    /// The Transform ids of its object and every ancestor (GetComponentsInChildren from any of them finds it).
    pub ancestors: Vec<i32>,
    /// GameObject.activeInHierarchy and Behaviour.enabled.
    pub active: bool,
    pub enabled: bool,
    pub clip: Option<ClipRef>,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub play_on_awake: bool,
    pub mute: bool,
    pub priority: i32,
    pub doppler_level: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub pan_stereo: f32,
    /// 0 Logarithmic, 1 Linear, 2 Custom.
    pub rolloff_mode: i32,
    pub rolloff_curve: Curve,
    pub spatial_blend_curve: Curve,
    pub spread_curve: Curve,
    pub group: Option<MixerGroup>,
    pub ignore_listener_volume: bool,
    pub ignore_listener_pause: bool,
    pub bypass_effects: bool,
    /// With no output mixer group: FMOD's NoFX channel groups, past the listener's AudioReverbFilter (ConfigureFMODGroups).
    pub bypass_listener_effects: bool,
    pub main: Option<Voice>,
    /// AudioSource.time set while no main voice sounds: where the next Play starts.
    pub start_time: f64,
    pub paused: bool,
    pub one_shots: Vec<Voice>,
    /// AudioSourceParameterCache's Doppler parts.
    pub doppler: Doppler,
}

/// Project Settings > Audio > Doppler Factor (ProjectSettings/AudioManager.asset `Doppler Factor: 0`).
pub const DOPPLER_FACTOR: f32 = 0.0;

/// The Doppler parts of an AudioSource's m_ParameterCache: its position at the last CachePositional (180aa4bc0), the
/// dopplerLevel CacheFrameParameters (180aa4520) last copied, and the dopplerPitch SoundChannelInstance::UpdatePitch
/// (180acd890) multiplies into the channel's frequency.
#[derive(Clone, Copy, Debug)]
pub struct Doppler {
    /// None until the first update: AwakeFromLoad's CacheFrameParameters caches the source's position and level there,
    /// which only reach the pitch through DOPPLER_FACTOR (0), so the first update takes them as they are then.
    pub position: Option<[f32; 3]>,
    pub level: f32,
    pub pitch: f32,
}

impl Default for Doppler {
    fn default() -> Self {
        Self { position: None, level: 0.0, pitch: 1.0 }
    }
}

fn constant(v: f32) -> Curve {
    Curve { keys: vec![[0.0, v, 0.0, 0.0]], pre: crate::game_assets::CLAMP, post: crate::game_assets::CLAMP }
}

impl Default for AudioSource {
    /// AddComponent<AudioSource>(): Unity's defaults.
    fn default() -> Self {
        Self {
            anchor: BareAnchor::Root,
            chain: Vec::new(),
            ancestors: Vec::new(),
            active: true,
            enabled: true,
            clip: None,
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            play_on_awake: true,
            mute: false,
            priority: 128,
            doppler_level: 1.0,
            min_distance: 1.0,
            max_distance: 500.0,
            pan_stereo: 0.0,
            rolloff_mode: 0,
            // The default custom rolloff (AudioSource::Reset): 1 at 0 falling to 0 at 1.
            rolloff_curve: Curve { keys: vec![[0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]], pre: crate::game_assets::CLAMP, post: crate::game_assets::CLAMP },
            spatial_blend_curve: constant(0.0),
            spread_curve: constant(0.0),
            group: None,
            ignore_listener_volume: false,
            ignore_listener_pause: false,
            bypass_effects: false,
            bypass_listener_effects: false,
            main: None,
            start_time: 0.0,
            paused: false,
            one_shots: Vec::new(),
            doppler: Doppler::default(),
        }
    }
}

impl AudioSource {
    /// A serialized AudioSource (prefab or scene) under `anchor`/`chain`, active as its GameObject is.
    pub fn from_component(s: &crate::assets::components::AudioSource, clip: Option<ClipRef>, group: Option<&str>, anchor: BareAnchor, chain: Vec<Trs>, ancestors: Vec<i32>, active: bool) -> Self {
        Self {
            anchor,
            chain,
            ancestors,
            active,
            enabled: s.enabled,
            clip,
            volume: s.volume,
            pitch: s.pitch,
            looping: s.looping,
            play_on_awake: s.play_on_awake,
            mute: s.mute,
            priority: s.priority,
            doppler_level: s.doppler_level,
            min_distance: s.min_distance,
            max_distance: s.max_distance,
            pan_stereo: s.pan_2d,
            rolloff_mode: s.rolloff_mode,
            rolloff_curve: s.rolloff_curve.clone(),
            spatial_blend_curve: s.spatial_blend.clone(),
            spread_curve: s.spread.clone(),
            group: group.map(MixerGroup::from_name),
            bypass_effects: s.bypass[0],
            bypass_listener_effects: s.bypass[1],
            ..Default::default()
        }
    }

    /// Behaviour.isActiveAndEnabled
    pub fn live(&self) -> bool {
        self.active && self.enabled
    }

    /// AudioSource.spatialBlend (the curve's first key, as the getter reads it).
    pub fn spatial_blend(&self) -> f32 {
        self.spatial_blend_curve.keys.first().map_or(0.0, |k| k[1])
    }

    /// AudioSource.spatialBlend = v: the curve becomes that one constant.
    pub fn set_spatial_blend(&mut self, v: f32) {
        self.spatial_blend_curve = constant(v.clamp(0.0, 1.0));
    }

    /// AudioSource.spread = v (degrees; the curve holds v / 360).
    pub fn set_spread(&mut self, v: f32) {
        self.spread_curve = constant(v.clamp(0.0, 360.0) / 360.0);
    }

    /// AudioSource.isPlaying
    pub fn is_playing(&self) -> bool {
        !self.paused && (self.main.is_some() || !self.one_shots.is_empty())
    }

    /// AudioSource.time: the main voice's position (or where the next Play starts).
    pub fn time(&self) -> f32 {
        self.main.as_ref().map_or(self.start_time, |v| v.time) as f32
    }

    /// The source's rolloff at `distance` (AudioSource::CacheFrameParameters, 180aa4520): Logarithmic is
    /// minDistance / distance (rolloff scale 1), Linear falls from 1 at minDistance to 0 at maxDistance, Custom reads
    /// the curve at distance / maxDistance; clamped to 0..1. The 3D attenuation then blends in by spatialBlend.
    pub fn rolloff(&self, distance: f32) -> f32 {
        let g = match self.rolloff_mode {
            0 => self.min_distance / distance.max(1e-6),
            1 => {
                let range = self.max_distance - self.min_distance;
                if range <= 0.0 { 1.0 } else { (self.max_distance - distance) / range }
            }
            _ if self.max_distance > 0.0 => self.rolloff_curve.evaluate(&mut CurveCache::default(), distance / self.max_distance),
            _ => 1.0,
        };
        g.clamp(0.0, 1.0)
    }

    /// The spatial blend at `distance` (the panLevel curve over distance / maxDistance), clamped to 0..1.
    pub fn spatial_blend_at(&self, distance: f32) -> f32 {
        let c = &self.spatial_blend_curve;
        let v = if c.keys.len() != 1 && self.max_distance > 0.0 { c.evaluate(&mut CurveCache::default(), distance / self.max_distance) } else { self.spatial_blend() };
        v.clamp(0.0, 1.0)
    }

    /// The spread in degrees at `distance`.
    pub fn spread_at(&self, distance: f32) -> f32 {
        let c = &self.spread_curve;
        let v = if c.keys.len() != 1 && self.max_distance > 0.0 { c.evaluate(&mut CurveCache::default(), distance / self.max_distance) } else { c.keys.first().map_or(0.0, |k| k[1]) };
        v.clamp(0.0, 1.0) * 360.0
    }
}

/// Every AudioSource, by component instance id, and the AudioListener.
#[derive(Clone, Debug)]
pub struct Audio {
    pub sources: BTreeMap<i32, AudioSource>,
    /// AudioListener.volume
    pub listener_volume: f32,
    /// AudioListener.pause
    pub listener_pause: bool,
    /// The AudioSources Global.AddAudioSource registered (their pitch follows Time.timeScale).
    pub time_scaled: std::collections::BTreeSet<i32>,
    /// Every script component's AudioClip and AudioClip[] fields, by (component id, field name): what the
    /// scripts' PlayOneShot(Clips.PickRandom()) and `clip = ...` read.
    pub clip_fields: std::collections::HashMap<(i32, String), Vec<Option<ClipRef>>>,
    /// Every script component's AudioSource and AudioSource[] fields that point into its own clone, by (component
    /// id, field name).
    pub source_fields: std::collections::HashMap<(i32, String), Vec<Option<i32>>>,
    /// The AudioSources on each component's GameObject, by the component's id (GetComponent<AudioSource>()).
    pub object_sources: std::collections::HashMap<i32, Vec<i32>>,
    /// Each AudioClip asset read once.
    pub clip_cache: std::collections::HashMap<ObjRef, ClipRef>,
    /// The AudioClip object arguments of UnityEvent persistent calls, by (file, PPtr).
    pub event_clips: std::collections::HashMap<(usize, crate::assets::classes::PPtr), ClipRef>,
    /// Resources.LoadAll<AudioClip>(folder), read once per folder.
    pub resource_clips: std::collections::HashMap<String, Vec<ClipRef>>,
    next_voice: u64,
    /// The audio thread's clock (unscaled seconds the audio update has run): its DSP blocks complete at this rate.
    pub clock: f64,
    /// Each source's dry channel group as `audio_spectrum` models it, by source id.
    pub dry: BTreeMap<i32, super::audio_spectrum::DryGroup>,
    /// The sources whose m_dryGroup exists: AudioSource::CreateFMODGroups (180aa70f0) makes it at the first Play / PlayOneShot
    /// of an active, enabled source and nothing but the component's end releases it; GetSpectrumData (180aaaff0) on a
    /// source without one zero-fills the array.
    pub dry_groups: std::collections::BTreeSet<i32>,
    /// The decoded samples of each AudioClip asset a spectrum read needed.
    pub pcm_cache: std::collections::HashMap<ObjRef, Option<Arc<crate::assets::audio::Pcm>>>,
    /// The AudioListener's m_LastPosition (AddToManager 180ab60c0 sets it; None until the first update, which takes the
    /// position then: it only reaches a pitch through DOPPLER_FACTOR) and its velocity.
    pub listener_last: Option<[f32; 3]>,
    pub listener_velocity: [f32; 3],
    /// The AudioClips whose sample data is not loaded (AudioClip.loadState, a recording's AudioClipsNotLoaded), by name:
    /// PlayOneShot of one gets no channel. Empty: all loaded. (Names with underscores for spaces, as recorded.)
    pub not_loaded: std::collections::HashSet<String>,
    /// gapC r11: every PlayOneShot(clip, volumeScale) with a clip, as `<source id>:<clip name, _ for spaces>:<volumeScale bits>`
    /// (the fn-check's one-shot compare against the recorder's FnResult shots=; it clears this before the call).
    pub shot_log: Vec<String>,
}

impl Default for Audio {
    fn default() -> Self {
        Self { sources: BTreeMap::new(), listener_volume: 1.0, listener_pause: false, time_scaled: Default::default(), clip_fields: Default::default(), source_fields: Default::default(), object_sources: Default::default(), clip_cache: Default::default(), event_clips: Default::default(), resource_clips: Default::default(), next_voice: 1, clock: 0.0, dry: Default::default(), dry_groups: Default::default(), pcm_cache: Default::default(), listener_last: None, listener_velocity: [0.0; 3], not_loaded: Default::default(), shot_log: Vec::new() }
    }
}

impl Audio {
    fn voice(&mut self, clip: ClipRef, time: f64, volume_scale: f32) -> Voice {
        self.next_voice += 1;
        let mix = super::audio_spectrum::MixState::new(self.block_of(self.clock), super::audio_spectrum::frame_pos(&clip, time));
        Voice { id: self.next_voice, clip, time, volume_scale, delay: 0.0, born: self.clock, mix }
    }

    fn source(&mut self, id: i32) -> &mut AudioSource {
        self.sources.get_mut(&id).unwrap_or_else(|| panic!("AudioSource {id}: not a source the rewrite made"))
    }

    pub fn get(&self, id: i32) -> &AudioSource {
        self.sources.get(&id).unwrap_or_else(|| panic!("AudioSource {id}: not a source the rewrite made"))
    }

    pub fn get_mut(&mut self, id: i32) -> &mut AudioSource {
        self.source(id)
    }

    /// A new AudioSource component (a spawned prefab's or AddComponent's). It plays at once when it is live with
    /// playOnAwake (AddToManager).
    pub fn add(&mut self, id: i32, s: AudioSource) {
        let play = s.live() && s.play_on_awake;
        self.sources.insert(id, s);
        if play {
            self.play(id);
        }
    }

    /// The component is destroyed (RemoveFromManager, then gone).
    pub fn remove(&mut self, id: i32) {
        self.sources.remove(&id);
        self.dry.remove(&id);
        self.dry_groups.remove(&id);
        self.time_scaled.remove(&id);
        self.clip_fields.retain(|k, _| k.0 != id);
        self.source_fields.retain(|k, _| k.0 != id);
        self.object_sources.remove(&id);
    }

    /// GameObject.activeInHierarchy changed: AddToManager (playOnAwake) or RemoveFromManager (every channel stops).
    pub fn set_active(&mut self, id: i32, active: bool) {
        let Some(s) = self.sources.get_mut(&id) else { return };
        let was = s.live();
        s.active = active;
        self.changed(id, was);
    }

    /// Behaviour.enabled
    pub fn set_enabled(&mut self, id: i32, enabled: bool) {
        let s = self.source(id);
        let was = s.live();
        s.enabled = enabled;
        self.changed(id, was);
    }

    fn changed(&mut self, id: i32, was: bool) {
        let s = self.source(id);
        match (was, s.live()) {
            (false, true) if s.play_on_awake => self.play(id),
            (true, false) => self.stop(id),
            _ => {}
        }
    }

    /// AudioSource.Play(): the clip from the start (or from a `time` set while stopped); nothing without a clip or
    /// on a source that is not active and enabled.
    pub fn play(&mut self, id: i32) {
        let s = self.source(id);
        let Some(clip) = s.clip.clone() else { return };
        if !s.live() {
            return;
        }
        let start = std::mem::take(&mut s.start_time);
        self.dry_groups.insert(id);
        let v = self.voice(clip, start, 1.0);
        let s = self.source(id);
        s.main = Some(v);
        s.paused = false;
    }

    /// AudioSource.PlayDelayed(delay): Play, with the clip starting `delay` seconds of the audio clock (not the
    /// source's pitch) later; isPlaying while it waits.
    pub fn play_delayed(&mut self, id: i32, delay: f32) {
        self.play(id);
        let clock = self.clock;
        let d = delay.max(0.0) as f64;
        let block = self.block_of(clock + d);
        if let Some(v) = &mut self.source(id).main {
            v.delay = d;
            v.born += d;
            v.mix.block = block;
        }
    }

    /// AudioSource.PlayOneShot(clip, volumeScale); a null clip or a source that is not active and enabled plays nothing.
    pub fn play_one_shot(&mut self, id: i32, clip: Option<&ClipRef>, volume_scale: f32) {
        if let Some(c) = clip {
            self.shot_log.push(format!("{id}:{}:{:08x}", c.clip.name.replace(' ', "_"), volume_scale.to_bits()));
        }
        self.play_one_shot_at(id, clip, volume_scale, 0.0);
    }

    /// PlayOneShot already `time` seconds into the clip (a loaded snapshot's one-shot).
    pub fn play_one_shot_at(&mut self, id: i32, clip: Option<&ClipRef>, volume_scale: f32, time: f64) {
        let Some(clip) = clip else { return };
        if !self.source(id).live() {
            return;
        }
        // PlayOneShot (180aad970): CreateFMODGroups before the channel is allocated.
        self.dry_groups.insert(id);
        // PlayOneShot (180aad970) links the one-shot only when SampleClip::AllocateChannel (180ab6120) gives a channel:
        // none for a clip whose FMOD sound is not loaded.
        if self.not_loaded.contains(&clip.clip.name.replace(' ', "_")) {
            return;
        }
        let v = self.voice(clip.clone(), time, volume_scale);
        self.source(id).one_shots.push(v);
    }

    /// AudioSource.Stop(): the main voice and the one-shots.
    pub fn stop(&mut self, id: i32) {
        let s = self.source(id);
        s.main = None;
        s.one_shots.clear();
        s.paused = false;
        s.start_time = 0.0;
    }

    pub fn pause(&mut self, id: i32) {
        self.source(id).paused = true;
    }

    pub fn unpause(&mut self, id: i32) {
        self.source(id).paused = false;
    }

    pub fn is_playing(&self, id: i32) -> bool {
        self.get(id).is_playing()
    }

    /// AudioSource.time = t (seconds, clamped to the clip).
    pub fn set_time(&mut self, id: i32, t: f32) {
        let s = self.source(id);
        let len = s.clip.as_ref().map_or(0.0, |c| c.length()) as f64;
        let t = (t as f64).clamp(0.0, len);
        match &mut s.main {
            Some(v) => {
                // The channel jumps (Channel::setPosition): the mixer moves it at the next block.
                v.mix.seek = Some(super::audio_spectrum::frame_pos(&v.clip, t));
                v.time = t;
            }
            None => s.start_time = t,
        }
    }

    /// AudioSource.clip = c: a sounding main voice stops (AudioSource::SetAudioClip).
    pub fn set_clip(&mut self, id: i32, clip: Option<ClipRef>) {
        let s = self.source(id);
        if s.clip.as_ref().map(|c| c.asset) != clip.as_ref().map(|c| c.asset) {
            s.main = None;
        }
        s.clip = clip;
    }

    /// AudioSourceTimeScaleBehaviour.Start on the object with Transform `transform`: Global.AddAudioSource of every
    /// AudioSource in its children (GetComponentsInChildren: active objects only).
    pub fn time_scale_below(&mut self, transform: i32, time_scale: f32, ambience: bool) {
        let ids: Vec<i32> = self.sources.iter().filter(|(_, s)| s.active && s.ancestors.contains(&transform)).map(|(&id, _)| id).collect();
        for id in ids {
            self.add_time_scaled(id, time_scale, ambience);
        }
    }

    /// Global.AddAudioSource(a, isAmbience): pitch = Time.timeScale and, without a group, Ambience or Sound effects.
    pub fn add_time_scaled(&mut self, id: i32, time_scale: f32, ambience: bool) {
        self.time_scaled.insert(id);
        let s = self.source(id);
        s.pitch = time_scale;
        if s.group.is_none() {
            s.group = Some(if ambience { MixerGroup::Ambience } else { MixerGroup::SoundEffects });
        }
    }

    /// Global.UpdatePitch(timeScale)
    pub fn update_pitch(&mut self, time_scale: f32) {
        for id in self.time_scaled.clone() {
            if let Some(s) = self.sources.get_mut(&id) {
                s.pitch = time_scale;
            }
        }
    }

    /// AudioManager::Update's Doppler: AudioListener::DoUpdate (180ab6db0) for the listener at `listener`, then
    /// AudioSource::DoUpdate (180aa7820) for every source with channels (`positions`, the sources' world positions):
    /// CachePositional and CacheFrameParameters. `dt` is Time.deltaTime (TimeManager's m_InvDeltaTime is 1 / dt, 1 when
    /// dt is 1e-5 or less). The velocity update mode is Auto, Dynamic without a 3D Rigidbody.
    pub fn doppler_update(&mut self, listener: [f32; 3], dt: f32, positions: &BTreeMap<i32, [f32; 3]>) {
        let inv = if dt > 1e-5 { 1.0 / dt } else { 1.0 };
        let last = self.listener_last.unwrap_or(listener);
        let lv = [(listener[0] - last[0]) * inv, (listener[1] - last[1]) * inv, (listener[2] - last[2]) * inv];
        self.listener_velocity = lv;
        self.listener_last = Some(listener);
        for (id, &p) in positions {
            let s = self.sources.get_mut(id).unwrap_or_else(|| panic!("AudioSource {id}: not a source the rewrite made"));
            let dir = [p[0] - listener[0], p[1] - listener[1], p[2] - listener[2]];
            let distance = (dir[1] * dir[1] + dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
            let blend = s.spatial_blend_at(distance);
            let doppler_level = s.doppler_level;
            let c = &mut s.doppler;
            let prev = c.position.unwrap_or(p);
            let level = if c.position.is_some() { c.level } else { doppler_level };
            let v = [(p[0] - prev[0]) * inv, (p[1] - prev[1]) * inv, (p[2] - prev[2]) * inv];
            c.position = Some(p);
            let rel = [v[0] - lv[0], v[1] - lv[1], v[2] - lv[2]];
            c.pitch = if 0.0 < doppler_level && 0.0 < blend {
                let n = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
                let vr = if 0.0 < n { (dir[1] * rel[1] + dir[0] * rel[0] + dir[2] * rel[2]) / n } else { 0.0 };
                let p = ((340.0 - vr * DOPPLER_FACTOR * level) / 340.0).max(1e-6);
                p * blend + (1.0 - blend)
            } else {
                1.0
            };
            c.level = doppler_level;
        }
    }

    /// The frame's audio update: every voice moves on by `dt` (unscaled seconds) times its source's pitch (Doppler
    /// included); a finished one-shot leaves, a finished main voice loops or stops.
    pub fn update(&mut self, dt: f32) {
        self.clock += dt as f64;
        let listener_pause = self.listener_pause;
        for s in self.sources.values_mut() {
            if s.paused || (listener_pause && !s.ignore_listener_pause) {
                continue;
            }
            let pitch = s.doppler.pitch * s.pitch;
            let step = dt as f64 * pitch as f64;
            if step < 0.0 {
                unimplemented!("mod-only: an AudioSource playing backwards (negative pitch)");
            }
            s.one_shots.retain_mut(|v| {
                v.time += step;
                v.time < v.clip.length() as f64
            });
            let looping = s.looping;
            if let Some(v) = &mut s.main {
                if v.delay > 0.0 {
                    v.delay -= dt as f64;
                    if v.delay > 0.0 {
                        continue;
                    }
                    // The part of the frame after the wait plays.
                    v.time += -v.delay * pitch as f64;
                    v.delay = 0.0;
                } else {
                    v.time += step;
                }
                let len = v.clip.length() as f64;
                if v.time >= len {
                    if looping && len > 0.0 {
                        v.time %= len;
                    } else {
                        s.main = None;
                    }
                }
            }
        }
        self.advance_mix();
    }
}

/// UISoundBehaviour.Main: its AudioSource and DefaultClip, WarningClip, ErrorClip, ScrollClip, UpClip, DownClip.
/// UISoundBehaviour.Start sets AudioSource.bypassListenerEffects (1.28.41): the mix applies no listener filter (audio_out.rs).
#[derive(Clone, Debug)]
pub struct UiSound {
    pub source: i32,
    pub clips: [Option<ClipRef>; 6],
    #[allow(non_snake_case)]
    pub timeOfLastScrollClip: f32,
}

/// A UISoundBehaviour method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiSoundKind {
    Blip,
    Warning,
    Error,
    Scroll,
    Up,
    Down,
    /// SliderCallback: Scroll when ScrollClip.length * 0.2f has passed since the last one or the source is quiet.
    Slider,
}

impl UiSound {
    /// UISoundBehaviour.Blip, UISoundBehaviour.Warning, UISoundBehaviour.Error, UISoundBehaviour.Scroll, UISoundBehaviour.Up, UISoundBehaviour.Down and UISoundBehaviour.SliderCallback, `now` being Time.unscaledTime.
    /// Each is AudioSource.PlayOneShot of its clip; Scroll first sets timeOfLastScrollClip = Time.unscaledTime, and
    /// SliderCallback scrolls when `Time.unscaledTime - timeOfLastScrollClip > ScrollClip.length * 0.2f` (in double) or
    /// the source is quiet.
    pub fn play(&mut self, audio: &mut Audio, now: f32, kind: UiSoundKind) {
        let kind = match kind {
            UiSoundKind::Slider => {
                let len = self.clips[3].as_ref().unwrap_or_else(|| super::menu::throw("NullReferenceException: UISoundBehaviour.ScrollClip")).length();
                if (super::d(now) - self.timeOfLastScrollClip).0 > (super::d(len) * 0.2f32).0 || !audio.is_playing(self.source) {
                    UiSoundKind::Scroll
                } else {
                    return;
                }
            }
            k => k,
        };
        let k = match kind {
            UiSoundKind::Blip => 0,
            UiSoundKind::Warning => 1,
            UiSoundKind::Error => 2,
            UiSoundKind::Scroll => {
                self.timeOfLastScrollClip = now;
                3
            }
            UiSoundKind::Up => 4,
            UiSoundKind::Down | UiSoundKind::Slider => 5,
        };
        audio.play_one_shot(self.source, self.clips[k].as_ref(), 1.0);
    }
}

/// The world position of a scene object under the root (`chain`: its Transform first, then each ancestor's).
pub fn root_position(chain: &[Trs]) -> [f32; 3] {
    let mut v = [0.0; 4];
    for n in chain {
        v = crate::unity::transform::global_position(n, v);
    }
    [v[0], v[1], v[2]]
}

/// The scripts that only drive an AudioSource from the camera: WaterSoundBehaviour (Update) and
/// RectangleAudioSourceBehaviour (LateUpdate).
#[derive(Clone, Debug)]
pub enum AudioScript {
    /// AudioRect (center, extents) and SoftRange.
    WaterSound { id: i32, anchor: BareAnchor, chain: Vec<Trs>, center: [f32; 3], extents: [f32; 3], soft_range: f32 },
    /// MaxVolume, MinVolume, Rectangle (x, y, width, height), MaxDistance, AutoPlayAndStopBasedOnDistance.
    Rectangle { id: i32, anchor: BareAnchor, chain: Vec<Trs>, max_volume: f32, min_volume: f32, rect: [f32; 4], max_distance: f32, auto: bool },
}

impl super::Game {
    /// Resources.LoadAll<AudioClip>(folder), in the ResourceManager's order (empty without the game's data).
    pub fn resource_clips(&mut self, folder: &str) -> Vec<ClipRef> {
        if let Some(v) = self.audio.resource_clips.get(folder) {
            return v.clone();
        }
        let Some(db) = self.assets.as_deref() else { return Vec::new() };
        let v: Vec<ClipRef> = db.resources_in(folder, 83).unwrap_or_else(|e| panic!("Resources.LoadAll({folder}): {e}")).into_iter().map(|r| clip_ref(db, &mut self.audio.clip_cache, Some(r)).expect("an AudioClip").unwrap()).collect();
        self.audio.resource_clips.insert(folder.to_string(), v.clone());
        v
    }

    /// `Resources.LoadAll<AudioClip>(folder).PickRandom()`: the draw over the `n` clips the game holds there (the
    /// rewrite's count when it has no data), and that clip.
    pub fn pick_resource_clip(&mut self, folder: &str, n: usize) -> Option<ClipRef> {
        let k = self.pick_random(n)?;
        self.resource_clips(folder).get(k).cloned()
    }

    /// UISoundBehaviour.Main's methods (`UiSound::play`) at Time.unscaledTime.
    pub fn ui_sound(&mut self, kind: UiSoundKind) {
        let now = self.time.unscaledTimeAsDouble as f32;
        if let Some(u) = self.ui_sound.as_mut() {
            u.play(&mut self.audio, now, kind);
        }
    }

    /// WaterSoundBehaviour.Update of each live one: AudioSource.volume = Clamp01((SoftRange - distance) * num²), the
    /// distance from the camera (z = 0) to the closest point of AudioRect in world space and num the camera's local
    /// height above AudioRect.min.y over its height.
    pub fn water_sound_update(&mut self) {
        let camera = self.camera_position();
        for s in self.audio_scripts.clone() {
            let AudioScript::WaterSound { id, anchor, chain, center, extents, soft_range } = s else { continue };
            if self.destroyed_components.contains(&id) {
                continue;
            }
            let source = self.audio.source_field(id, "AudioSource").unwrap_or_else(|| super::menu::throw("NullReferenceException: WaterSoundBehaviour.AudioSource"));
            let nodes = self.anchored_nodes(anchor, &chain);
            let position = [camera[0], camera[1], 0.0];
            // base.transform.InverseTransformPoint(position): each Transform's inverse from the root down.
            let mut point = [position[0], position[1], position[2], 0.0];
            for n in nodes.iter().rev() {
                point = crate::unity::transform::inverse(n, point, [0.0, 0.0, 0.0, 1.0]).0;
            }
            let closest = crate::unity::transform::bounds_closest_point([point[0], point[1], point[2]], center, extents).0;
            let mut w = [closest[0], closest[1], closest[2], 0.0];
            for n in &nodes {
                w = crate::unity::transform::global_position(n, w);
            }
            let dv = [w[0] - position[0], w[1] - position[1], w[2] - position[2]];
            let magnitude = super::mathf::sqrt(dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]);
            let min_y = center[1] - extents[1];
            let mut num = super::mathf::max(0.0, (min_y - point[1]).abs()) / (extents[1] * 2.0);
            num *= num;
            self.audio.get_mut(source).volume = super::mathf::clamp01((soft_range - magnitude) * num);
        }
    }

    /// RectangleAudioSourceBehaviour.LateUpdate of each live one: MapRange(0, 1, MinVolume, MaxVolume,
    /// GetNormalisedDistanceFactor()); with AutoPlayAndStopBasedOnDistance it stops at no volume and plays otherwise.
    pub fn rectangle_audio_late_update(&mut self) {
        self.rectangle_audio_late_update_of(None);
    }

    /// RectangleAudioSourceBehaviour.LateUpdate of the live one `only` (all of them for None).
    pub fn rectangle_audio_late_update_of(&mut self, only: Option<i32>) {
        let camera = self.camera_position();
        for s in self.audio_scripts.clone() {
            let AudioScript::Rectangle { id, anchor, chain, max_volume, min_volume, rect, max_distance, auto } = s else { continue };
            if self.destroyed_components.contains(&id) || only.is_some_and(|o| o != id) {
                continue;
            }
            let source = self.audio.source_field(id, "AudioSource").unwrap_or_else(|| super::menu::throw("NullReferenceException: RectangleAudioSourceBehaviour.AudioSource"));
            // GetNormalisedDistanceFactor: CameraPosition - transform.position against Rectangle.
            let at = self.anchored_world(anchor, &chain).0;
            let v = [camera[0] - at[0], camera[1] - at[1]];
            let (x_min, y_min, x_max, y_max) = (rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]);
            let factor = if v[0] >= x_min && v[0] < x_max && v[1] >= y_min && v[1] < y_max {
                1.0
            } else if max_distance <= f32::EPSILON {
                0.0
            } else {
                let c = [super::mathf::clamp(v[0], x_min, x_max) - v[0], super::mathf::clamp(v[1], y_min, y_max) - v[1]];
                1.0 - super::mathf::sqrt(c[0] * c[0] + c[1] * c[1]) / max_distance
            };
            // Utils.MapRange(0, 1, MinVolume, MaxVolume, factor)
            let num = min_volume + (max_volume - min_volume) * factor;
            if auto {
                if num <= f32::EPSILON {
                    if self.audio.is_playing(source) {
                        self.audio.stop(source);
                    }
                    continue;
                }
                if !self.audio.is_playing(source) {
                    self.audio.play(source);
                }
            }
            self.audio.get_mut(source).volume = num;
        }
    }

    /// The anchor and Transform ancestor ids of a component's object: its physical's, or else its transform node's.
    pub fn anchor_ancestors(&self, physical: Option<usize>, node: Option<usize>) -> (super::BareAnchor, Vec<i32>) {
        match (physical, node) {
            (Some(p), _) => (super::BareAnchor::Physical(p), self.physical_ancestors(p)),
            (None, Some(n)) => {
                let ancestors = std::iter::successors(Some(n), |&i| self.transform_nodes[i].parent).map(|i| self.transform_nodes[i].id).collect();
                (super::BareAnchor::Node(n), ancestors)
            }
            (None, None) => unimplemented!("mod-only: an AudioSource added to an object that is neither a physical nor a transform node"),
        }
    }

    /// `MuteBehaviour.Muted` of a script's cached MuteBehaviour (the component id; None: null, which throws).
    pub fn muted(&self, mute: Option<i32>) -> bool {
        let mute = mute.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let k = self.mute_ids.iter().position(|&id| id == mute).unwrap_or_else(|| unimplemented!("mod-only: a MuteBehaviour that never started (no PhysicalBehaviour)"));
        self.mutes[k]
    }

    /// MuteBehaviour.SetMute(v): Muted = v; each of Sources gets enabled = !v (a null one throws).
    pub fn mute_set(&mut self, m: usize, v: bool) {
        self.mutes[m] = v;
        let id = self.mute_ids[m];
        for s in self.audio.source_fields(id, "Sources").to_vec() {
            let s = s.unwrap_or_else(|| super::menu::throw("NullReferenceException: MuteBehaviour.Sources has a null entry"));
            self.audio.set_enabled(s, !v);
        }
    }

    /// AudioSourceTimeScaleBehaviour.Start (see `Audio::time_scale_below`).
    pub fn audio_time_scale_below(&mut self, transform: i32, ambience: bool) {
        let t = self.time.timeScale;
        self.audio.time_scale_below(transform, t, ambience);
    }

    /// AudioSource.isPlaying of a source the rewrite made.
    pub fn audio_is_playing(&self, id: i32) -> bool {
        self.audio.is_playing(id)
    }

    /// PostLateUpdate.UpdateAudio
    pub fn audio_update(&mut self) {
        // Destroyed components' sources are gone (their hierarchy's deactivation stopped them).
        let gone = &self.destroyed_components;
        self.audio.sources.retain(|id, _| !gone.contains(id));
        self.audio.time_scaled.retain(|id| !gone.contains(id));
        self.audio.clip_fields.retain(|k, _| !gone.contains(&k.0));
        self.audio.source_fields.retain(|k, _| !gone.contains(&k.0));
        self.audio.object_sources.retain(|k, _| !gone.contains(k));
        let listener = self.audio_listener();
        let positions: BTreeMap<i32, [f32; 3]> = self.audio.sources.iter().filter(|(_, s)| s.live() && (s.main.is_some() || !s.one_shots.is_empty())).map(|(&id, s)| (id, self.audio_position(s))).collect();
        self.audio.doppler_update(listener, self.time.deltaTime, &positions);
        let dt = self.time.unscaledDeltaTime;
        self.audio.update(dt);
        self.antenna_radio_sync();
    }

    /// The AudioSource field `name` of component `id`; a null one throws the NullReferenceException.
    pub fn audio_src(&self, id: i32, name: &str) -> i32 {
        self.audio.source_field(id, name).unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// `source.PlayOneShot(clip)` with the source in field `source` and the clip in field `clip` of component `id`.
    pub fn audio_one_shot(&mut self, id: i32, source: &str, clip: &str) {
        let s = self.audio_src(id, source);
        let c = self.audio.clip(id, clip);
        self.audio.play_one_shot(s, c.as_ref(), 1.0);
    }

    /// LoopSoundBehaviour.StartLoop
    pub fn loop_sound_start(&mut self, i: usize) {
        let id = self.loop_sounds[i].id;
        let s = self.audio_src(id, "AudioSource");
        self.audio.play(s);
        let clip = self.audio.clip(id, "StartClip");
        self.audio.play_one_shot(s, clip.as_ref(), 1.0);
        self.loop_sounds[i].IsPlaying = true;
    }

    /// LoopSoundBehaviour.StopLoop
    pub fn loop_sound_stop(&mut self, i: usize) {
        let id = self.loop_sounds[i].id;
        let s = self.audio_src(id, "AudioSource");
        self.audio.stop(s);
        let clip = self.audio.clip(id, "EndClip");
        self.audio.play_one_shot(s, clip.as_ref(), 1.0);
        self.loop_sounds[i].IsPlaying = false;
    }

    /// JointMotorSoundBehaviour.FixedUpdate (a HingeJoint2D or SliderJoint2D; a WheelJoint2D stops at load).
    pub fn joint_motor_sound_fixed_update(&mut self, i: usize) {
        let (hinge, lp, threshold) = (self.joint_motor_sounds[i].joint, self.joint_motor_sounds[i].loop_sound, self.joint_motor_sounds[i].SpeedThreshold);
        let Some(lp) = lp else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let slider = self.joint_motor_sounds[i].slider;
        // Joint == null: a null reference or a destroyed joint; a SliderJoint2D's get_motor reads the stored speed.
        let speed = match (hinge, slider) {
            (Some(h), _) if !self.hinges[h].destroyed => Some(self.hinges[h].motor[0]),
            (None, Some(f)) if !self.fixed_joints[f].destroyed => Some(self.fixed_joints[f].slider.as_ref().expect("a SliderJoint2D").motor_speed),
            _ => None,
        };
        let Some(speed) = speed else {
            if self.loop_sounds[lp].IsPlaying {
                self.loop_sound_stop(lp);
            }
            return;
        };
        let flag = speed.abs() > threshold;
        if flag && !self.loop_sounds[lp].IsPlaying {
            self.loop_sound_start(lp);
        } else if !flag && self.loop_sounds[lp].IsPlaying {
            self.loop_sound_stop(lp);
        }
    }

    /// `source.PlayOneShot(source.clip)`.
    pub fn audio_one_shot_own(&mut self, id: i32, source: &str) {
        let s = self.audio_src(id, source);
        let c = self.audio.get(s).clip.clone();
        self.audio.play_one_shot(s, c.as_ref(), 1.0);
    }

    /// `source.PlayOneShot(clips.PickRandom())` with the draw over `n` clips.
    pub fn audio_one_shot_pick(&mut self, id: i32, source: &str, clips: &str, n: usize) {
        let c = self.pick_clip(id, clips, n);
        let s = self.audio_src(id, source);
        self.audio.play_one_shot(s, c.as_ref(), 1.0);
    }

    /// The world position of a source (its Transform).
    pub fn audio_position(&self, s: &AudioSource) -> [f32; 3] {
        self.anchored_world(s.anchor, &s.chain).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(length: f32) -> ClipRef {
        Arc::new(Clip {
            asset: ObjRef { file: 0, path_id: 1 },
            clip: crate::assets::classes::AudioClip { name: "c".into(), load_type: 0, channels: 1, frequency: 44100, length, resource: Default::default(), compression_format: 0 },
            pcm: None,
            stream: None,
        })
    }

    #[test]
    fn voices_run_on_the_frame_clock() {
        let mut a = Audio::default();
        a.add(2, AudioSource { clip: Some(clip(0.1)), play_on_awake: false, ..Default::default() });
        assert!(!a.is_playing(2));
        a.play_one_shot(2, Some(&clip(0.05)), 1.0);
        a.play(2);
        assert!(a.is_playing(2));
        a.update(0.06);
        assert!(a.get(2).one_shots.is_empty() && a.is_playing(2));
        a.update(0.06);
        assert!(!a.is_playing(2));
        // Pitch 0 (Time.timeScale 0 through Global) holds a voice.
        a.add_time_scaled(2, 0.0, false);
        a.play(2);
        a.update(1.0);
        assert!(a.is_playing(2));
        // Disabling stops; enabling plays on awake.
        a.set_enabled(2, false);
        assert!(!a.is_playing(2));
        a.get_mut(2).play_on_awake = true;
        a.set_enabled(2, true);
        assert!(a.is_playing(2));
        assert_eq!(a.get(2).group, Some(MixerGroup::SoundEffects));
    }

    /// UISoundBehaviour.SliderCallback: Scroll only when ScrollClip.length * 0.2f has passed since the last Scroll or the
    /// source is quiet; Scroll stamps timeOfLastScrollClip.
    #[test]
    fn ui_sound_slider_waits_for_the_scroll_clip() {
        let mut a = Audio::default();
        a.add(5, AudioSource { clip: Some(clip(5.0)), play_on_awake: false, ..Default::default() });
        let mut ui = UiSound { source: 5, clips: std::array::from_fn(|_| Some(clip(1.0))), timeOfLastScrollClip: 0.0 };
        ui.play(&mut a, 10.0, UiSoundKind::Slider);
        assert_eq!((ui.timeOfLastScrollClip, a.get(5).one_shots.len()), (10.0, 1));
        a.play(5);
        ui.play(&mut a, 10.1, UiSoundKind::Slider);
        assert_eq!((ui.timeOfLastScrollClip, a.get(5).one_shots.len()), (10.0, 1));
        ui.play(&mut a, 10.25, UiSoundKind::Slider);
        assert_eq!((ui.timeOfLastScrollClip, a.get(5).one_shots.len()), (10.25, 2));
        ui.play(&mut a, 10.3, UiSoundKind::Up);
        assert_eq!((ui.timeOfLastScrollClip, a.get(5).one_shots.len()), (10.25, 3));
    }

    #[test]
    fn rolloff_modes() {
        let mut s = AudioSource { min_distance: 2.0, max_distance: 10.0, ..Default::default() };
        assert_eq!(s.rolloff(1.0), 1.0);
        assert_eq!(s.rolloff(4.0), 0.5);
        s.rolloff_mode = 1;
        assert_eq!(s.rolloff(6.0), 0.5);
        assert_eq!(s.rolloff(20.0), 0.0);
        s.rolloff_mode = 2;
        assert_eq!(s.rolloff(5.0), 0.5);
    }
}

script! {
    LoopSound {}
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// IsPlaying (a private setter, not serialised).
        IsPlaying: bool = false,
    }
}

script! {
    JointMotorSound {
        SpeedThreshold: f32 = 0.5,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Joint: a `Game::hinges` index (None: null or a slider).
        joint: Option<usize> = None,
        /// Joint: a SliderJoint2D, as its (object, component) in the prefab and, once Awake adopted it, its `Game::fixed_joints` index.
        slider_at: Option<(usize, usize)> = None,
        slider: Option<usize> = None,
        /// LoopSoundBehaviour: a `Game::loop_sounds` index (None: null).
        loop_sound: Option<usize> = None,
    }
}
