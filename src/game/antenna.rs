//! AntennaReceiverBehaviour (the Jukebox's radio): the antenna count and signal strength the EnergyWires feed it,
//! Update's closeness to TargetPosition on TargetMap and the SGNR stat, and its streamed radio noise (Awake's
//! AudioSource and AudioClip.Create, ProvideAudioData, GetNextFloat). FuseBoxBehaviour's streamed hum is here too.

use super::{d, mathf, Game};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

/// AntennaReceiverBehaviour.globalStartAudioPos (static).
static GLOBAL_START_AUDIO_POS: AtomicI32 = AtomicI32::new(0);

/// What the radio stream's callback reads of its receiver: SignalStrength, audioInformation and currentAudioPosition
/// (the game's values, copied in each frame's audio update; the callback runs on the audio thread, as FMOD's does).
#[derive(Debug, Default)]
pub struct Radio {
    pub SignalStrength: f32,
    pub OldFactor: f32,
    pub CurrentFactor: f32,
    pub currentAudioPosition: i32,
}

/// AntennaReceiverBehaviour.GetNextFloat(index, count) with its rand.
pub(crate) fn get_next_float(r: &Radio, rand: &mut super::utils::NetRandom, index: i32, count: i32) -> f32 {
    let t = (index as f32 as f64 / (count as f32 as f64 - 1.0)) as f32;
    // 0 / 0 (count 1, index 0) is x86's default NaN 0xffc00000 on every platform (ARM makes +NaN); Lerp carries it out.
    let t = if t.is_nan() { f32::from_bits(0xffc0_0000) } else { t };
    if rand.next_double() > r.SignalStrength as f64 {
        return 0.0;
    }
    let a = rand.next_double() as f32;
    let num = (r.currentAudioPosition as f32 as f64 / 44100.0) as f32;
    let noise = crate::unity::perlin_noise((num as f64 % 60.0 * 0.754578f32 as f64) as f32, 3498.346);
    let num2 = mathf::lerp(100.0, 3000.0, noise);
    let b = mathf::sin((num as f64 * 2.0 * std::f32::consts::PI as f64 * num2 as f64) as f32);
    let num3 = mathf::lerp(r.OldFactor, r.CurrentFactor, t);
    mathf::lerp(a, b, (num3 as f64 * num3 as f64) as f32)
}

/// AntennaReceiverBehaviour.ProvideAudioData: each sample GetNextFloat(i, data.Length), then currentAudioPosition++ and
/// globalStartAudioPos = currentAudioPosition.
fn antenna_provide(data: &mut [f32], r: &mut Radio, rand: &mut super::utils::NetRandom) {
    let count = data.len();
    for (k, x) in data.iter_mut().enumerate() {
        *x = get_next_float(r, rand, k as i32, count as i32);
        r.currentAudioPosition = r.currentAudioPosition.wrapping_add(1);
        GLOBAL_START_AUDIO_POS.store(r.currentAudioPosition, Ordering::Relaxed);
    }
}

/// FuseBoxBehaviour.d: Encoding.ASCII.GetString(Utils.FBSF(...)) (bytes past 0x7f read '?').
pub(crate) static FUSE_BOX_D: std::sync::LazyLock<Vec<u8>> = std::sync::LazyLock::new(|| {
    super::utils::fbsf("MDAxMDExMDExMTEwMTAwMDExMDAxMTAxMDAwMDExMDExMDExMDAwMDEwMTEwMTEwMTEwMDAwMTAxMDAxMTAxMTExMDExMDEwMDExMTEwMDAwMTAwMDEwMTAxMDAwMTEwMDExMDAwMDEwMDAxMDEwMTEwMTAwMTExMTAxMTExMDAxMDExMDAxMTExMDEwMTEwMTEwMDAxMTAwMTAxMDAwMTEwMDExMDExMDExMTAwMTEwMDExMTAwMDEwMTAw")
        .into_iter()
        .map(|b| if b > 0x7f { b'?' } else { b })
        .collect()
});

/// FuseBoxBehaviour.ProvideAudioData: a 300 Hz tone keyed by d's bits at 0.05 bits a second, rough (times NextDouble)
/// on '0', every other pass through d silent.
fn fuse_box_provide(data: &mut [f32], pos: &mut i64, rand: &mut super::utils::NetRandom) {
    let d = &*FUSE_BOX_D;
    for x in data.iter_mut() {
        let num = (*pos as f32 as f64 / 44100.0) as f32;
        let num2 = (num as f64 * 0.05f32 as f64) as f32;
        let c = d[(num2 as f64 % 1.0 * d.len() as f32 as f64) as i32 as usize];
        let arg = (num as f64 * 2.0 * std::f32::consts::PI as f64 * 300.0) as f32;
        let num3 = (mathf::sin(arg) as f64 * rand.next_double() as f32 as f64) as f32;
        let num4 = mathf::sin(arg);
        *x = if !((num2 as f64 % 2.0) < 1.0) { 0.0 } else if c == b'0' { num3 } else { num4 };
        *pos += 1;
        if *pos > 4410000 {
            *pos = 0;
        }
    }
}

#[derive(Clone, Debug, Default)]
#[allow(non_snake_case)]
pub struct AntennaReceiver {
    pub id: i32,
    pub enabled: bool,
    /// The PhysicalBehaviour on its object (EnergyWireBehaviour's receiver.phys).
    pub physical: usize,
    pub ConnectedAntennae: i32,
    pub SignalStrength: f32,
    pub TargetPosition: [f32; 2],
    /// TargetMap (the Map asset; null when unset).
    pub target_map: Option<crate::assets::db::ObjRef>,
    pub correctMap: bool,
    /// audioInformation.OldFactor and CurrentFactor.
    pub OldFactor: f32,
    pub CurrentFactor: f32,
    pub timeSpentNearTarget: f32,
    /// The "antenna" child's AudioSource (Awake).
    pub audioSource: i32,
    pub radio: Option<Arc<Mutex<Radio>>>,
}

impl Game {
    /// AntennaReceiverBehaviour.Awake: correctMap = MapLoaderBehaviour.CurrentMap == TargetMap; new GameObject("antenna")
    /// at the receiver (GameObject, Transform), AddComponent<Optout>, AddComponent<AudioSource> (spatialBlend 1, no
    /// playOnAwake, looping, Linear from 4 to 50), Global.AddAudioSource, then the radioClip stream (one second, mono,
    /// 44100 Hz, ProvideAudioData) as its clip: five instance ids.
    pub(crate) fn antenna_receiver_awake(&mut self, i: usize) {
        self.antenna_receivers[i].correctMap = self.current_map_asset == self.antenna_receivers[i].target_map;
        let _game_object = self.allocate_instance_id();
        let transform = self.allocate_instance_id();
        let _optout = self.allocate_instance_id();
        let src = self.allocate_instance_id();
        let clip_id = self.allocate_instance_id();
        let p = self.antenna_receivers[i].physical;
        let ancestors = std::iter::once(transform).chain(self.physical_ancestors(p)).collect();
        let mut s = super::audio::AudioSource { anchor: super::BareAnchor::Physical(p), ancestors, play_on_awake: false, looping: true, min_distance: 4.0, max_distance: 50.0, rolloff_mode: 1, ..Default::default() };
        s.set_spatial_blend(1.0);
        self.audio.add(src, s);
        let ts = self.time.timeScale;
        self.audio.add_time_scaled(src, ts, false);
        let radio = Arc::new(Mutex::new(Radio::default()));
        let r = radio.clone();
        // The receiver's private System.Random.
        let mut rand = super::utils::NetRandom::new();
        let read = Box::new(move |data: &mut [f32]| antenna_provide(data, &mut r.lock().unwrap(), &mut rand));
        let clip = super::audio::create_stream_clip(clip_id, "radioClip", 44100, 1, 44100, read);
        self.audio.set_clip(src, Some(clip));
        let x = &mut self.antenna_receivers[i];
        x.audioSource = src;
        x.radio = Some(radio);
    }

    /// The radio streams' view of the receivers (PostLateUpdate.UpdateAudio).
    pub(super) fn antenna_radio_sync(&mut self) {
        for x in &self.antenna_receivers {
            if let Some(r) = &x.radio {
                let mut r = r.lock().unwrap();
                (r.SignalStrength, r.OldFactor, r.CurrentFactor) = (x.SignalStrength, x.OldFactor, x.CurrentFactor);
            }
        }
    }

    /// AntennaReceiverBehaviour.Connect, when enabled: the JukeboxBehaviour's StopMusic, then unless the radio plays,
    /// currentAudioPosition = globalStartAudioPos and Play.
    pub(crate) fn antenna_receiver_connect(&mut self, i: usize) {
        if !self.antenna_receivers[i].enabled {
            return;
        }
        let r = self.antenna_receivers[i].physical;
        if let Some(j) = self.jukeboxes.iter().position(|j| j.physical == r) {
            let s = self.jukebox_source(j);
            self.audio.stop(s);
        }
        let src = self.antenna_receivers[i].audioSource;
        if !self.audio.is_playing(src) {
            if let Some(radio) = &self.antenna_receivers[i].radio {
                radio.lock().unwrap().currentAudioPosition = GLOBAL_START_AUDIO_POS.load(Ordering::Relaxed);
            }
            self.audio.play(src);
        }
    }

    /// AntennaReceiverBehaviour.OnEnable: Play with an antenna.
    pub(crate) fn antenna_receiver_on_enable(&mut self, i: usize) {
        if self.antenna_receivers[i].ConnectedAntennae > 0 {
            self.audio.play(self.antenna_receivers[i].audioSource);
        }
    }

    /// AntennaReceiverBehaviour.OnDisable: Stop while playing.
    pub(crate) fn antenna_receiver_on_disable(&mut self, i: usize) {
        let src = self.antenna_receivers[i].audioSource;
        // An OnDestroy already destroyed the "antenna" child with its AudioSource: `audioSource.isPlaying` throws (fngb10).
        if !self.audio.sources.contains_key(&src) {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        if self.audio.is_playing(src) {
            self.audio.stop(src);
        }
    }

    /// AntennaReceiverBehaviour.OnDestroy: Global.RemoveAudioSource, then Destroy of the child (its AudioSource) and the clip.
    pub(crate) fn antenna_receiver_on_destroy(&mut self, i: usize) {
        let src = self.antenna_receivers[i].audioSource;
        self.audio.remove(src);
        self.antenna_receivers[i].radio = None;
    }

    /// FuseBoxBehaviour.Awake: audioProvider = AudioClip.Create("radioClip", 44100, 1, 44100, stream: true,
    /// ProvideAudioData) (one instance id), then Source.clip = audioProvider.
    pub(crate) fn fuse_box_awake(&mut self, id: i32) {
        let clip_id = self.allocate_instance_id();
        self.fuse_box_clips.insert(id, clip_id);
        let mut pos = 0i64;
        let mut rand = super::utils::NetRandom::new();
        let read = Box::new(move |data: &mut [f32]| fuse_box_provide(data, &mut pos, &mut rand));
        let clip = super::audio::create_stream_clip(clip_id, "radioClip", 44100, 1, 44100, read);
        let src = self.audio_src(id, "Source");
        self.audio.set_clip(src, Some(clip));
    }

    /// FuseBoxBehaviour.OnDestroy: Object.Destroy(audioProvider), queued (the destroy pass takes a clip id as gone: no script runs).
    pub(crate) fn fuse_box_on_destroy(&mut self, id: i32) {
        if let Some(&clip) = self.fuse_box_clips.get(&id) {
            self.tr_destroy_component(clip, 0.0);
        }
    }

    /// AntennaReceiverBehaviour.Update
    pub fn antenna_receiver_update(&mut self, i: usize) {
        if !self.antenna_receivers[i].enabled || self.antenna_receivers[i].ConnectedAntennae == 0 {
            return;
        }
        let pos = self.physicals[self.antenna_receivers[i].physical].transform.position;
        let r = &mut self.antenna_receivers[i];
        r.OldFactor = r.CurrentFactor;
        r.CurrentFactor = if r.correctMap {
            // (Vector2)transform.position - TargetPosition: Vector2's operator- rounds each component.
            let v = [(d(pos[0]) - r.TargetPosition[0]).f(), (d(pos[1]) - r.TargetPosition[1]).f()];
            mathf::pow(mathf::clamp01((d(10.0f32) / mathf::max(0.1, mathf::sqr_magnitude(v))).f()), 2.0)
        } else {
            0.0
        };
        let flag = r.CurrentFactor > 0.9;
        if flag {
            r.timeSpentNearTarget = (d(r.timeSpentNearTarget) + self.time.deltaTime).f();
        }
        if (r.timeSpentNearTarget > 3.0) & flag && super::stats::STEAMWORKS_INITIALISED {
            super::stats::increment_integer(super::stats::SGNR, 1);
        }
    }

    /// AntennaReceiverBehaviour.FixedUpdate: the SignalStrength decay; with no antenna, Stop while playing.
    pub fn antenna_receiver_fixed_update(&mut self, i: usize) {
        if !self.antenna_receivers[i].enabled {
            return;
        }
        let t = (d(self.time.fixedDeltaTime) * 5.0f32).f();
        let r = &mut self.antenna_receivers[i];
        if r.ConnectedAntennae > 0 {
            r.SignalStrength = mathf::lerp(r.SignalStrength, 0.0, t);
        } else if self.audio.is_playing(r.audioSource) {
            let src = r.audioSource;
            self.audio.stop(src);
        }
    }

    /// The AntennaReceiverBehaviour on a physical's object, if any.
    pub(super) fn antenna_receiver_of(&self, p: usize) -> Option<usize> {
        self.antenna_receivers.iter().position(|r| r.physical == p)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{d, mathf};

    /// Update's factor: 10 / max(0.1, sqr distance), clamped, squared.
    fn factor(v: [f32; 2]) -> f32 {
        mathf::pow(mathf::clamp01((d(10.0f32) / mathf::max(0.1, mathf::sqr_magnitude(v))).f()), 2.0)
    }

    /// The radio: silent without signal; with full signal and factor, a 100-3000 Hz sine; d's bits key the hum.
    #[test]
    fn radio_and_hum() {
        let mut rand = super::super::utils::NetRandom::with_seed(1);
        let r = super::Radio { SignalStrength: 0.0, ..Default::default() };
        assert!((0..100).all(|k| super::get_next_float(&r, &mut rand, k, 100) == 0.0));
        let r = super::Radio { SignalStrength: 1.0, OldFactor: 1.0, CurrentFactor: 1.0, currentAudioPosition: 0 };
        assert_eq!(super::get_next_float(&r, &mut rand, 0, 100), 0.0);
        let d = &*super::FUSE_BOX_D;
        assert!(d.len() > 100 && d.iter().all(|&c| c == b'0' || c == b'1'), "{:?}", String::from_utf8_lossy(d));
        let mut data = vec![0.0; 44100];
        let mut pos = 0;
        super::fuse_box_provide(&mut data, &mut pos, &mut rand);
        assert_eq!(pos, 44100);
        assert!(data.iter().any(|&x| x != 0.0) && data.iter().all(|&x| x.abs() <= 1.0));
    }

    #[test]
    fn factor_near_and_far() {
        assert_eq!(factor([0.0, 0.0]), 1.0);
        assert_eq!(factor([3.0, 1.0]), 1.0); // sqr 10
        assert_eq!(factor([4.0, 0.0]), (10.0f32 / 16.0) * (10.0f32 / 16.0));
        // CurrentFactor > 0.9 needs sqr distance < 10 / sqrt(0.9)
        assert!(factor([3.2, 0.0]) > 0.9);
        assert!(factor([3.3, 0.0]) < 0.9);
    }
}
