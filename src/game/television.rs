//! TelevisionBehaviour (the Television). Awake keeps MyRenderer's sprite and draws the glow's seed; Update breaks the set
//! when its PhysicalBehaviour is hotter than TemperatureThreshold; OnCollisionEnter2D breaks it when the first contact's
//! normal impulse exceeds BreakingTreshold; Shot breaks it; Break (Broken, unless Indestructible) draws one of
//! BreakSounds. Use toggles Activated while not broken. Start (the date's Christmas clip, the 0-3 am sprite and clip),
//! UpdateActivation, Play, Stop, the special-position clip swap and the glow colour are rewritten with the VideoPlayer's
//! clock (`VideoPlayer`); play.rs decodes and shows the frames (video.rs). The AudioSource is the audio model's
//! (Audio.clip, Play, Stop, time). The context menu's Make indestructible and Repair are UI.
#![allow(non_snake_case)]

use super::visual::{Gradient, SpriteAt};
use super::audio::ClipRef;
use super::{mathf, script, Game};
use crate::assets::db::ObjRef;
use crate::b2::contacts2d::ScriptCollision;

/// A VideoClip the set can play: the asset and VideoClip.length (`frame_count / frame_rate`).
#[derive(Clone, Copy, Debug)]
pub struct VideoRef {
    pub clip: ObjRef,
    pub length: f64,
}

/// UnityEngine.Video.VideoPlayer as the set drives it: the clock runs while playing, at `speed`, and loops (the
/// prefab's isLooping).
#[derive(Clone, Debug)]
pub struct VideoPlayer {
    pub playing: bool,
    pub time: f64,
    pub speed: f32,
}

/// The video clips by index: 0 the VideoPlayer's own, 1 MessageVideo, 2 B2Video, 3 B3Video.
pub const MESSAGE: usize = 1;
pub const B2: usize = 2;
pub const B3: usize = 3;

/// fn-check's fake clock: the game's DateTime.Now at the case's call (FnTiny " clk="), which local_now returns while set,
/// so a case that reads the wall clock (TelevisionBehaviour.Start's `alt`, the 5 December clip) checks the same way at any hour.
pub static FAKE_NOW: std::sync::Mutex<Option<(u16, u16, f64)>> = std::sync::Mutex::new(None);

/// DateTime.Now: (month, day, time of day in seconds), on the local clock (FAKE_NOW while fn-check sets it).
pub fn local_now() -> (u16, u16, f64) {
    FAKE_NOW.lock().unwrap().unwrap_or_else(real_now)
}

#[cfg(windows)]
fn real_now() -> (u16, u16, f64) {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    (t.wMonth, t.wDay, t.wHour as f64 * 3600.0 + t.wMinute as f64 * 60.0 + t.wSecond as f64 + t.wMilliseconds as f64 / 1000.0)
}

#[cfg(not(windows))]
fn real_now() -> (u16, u16, f64) {
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        rest: [i64; 6],
    }
    unsafe extern "C" {
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let mut tm: Tm = unsafe { std::mem::zeroed() };
    unsafe { localtime_r(&(secs as i64), &mut tm) };
    ((tm.mon + 1) as u16, tm.mday as u16, tm.hour as f64 * 3600.0 + tm.min as f64 * 60.0 + tm.sec as f64 + secs.fract())
}

script! {
    Television {
        Broken: bool = false,
        Activated: bool = false,
        BreakingTreshold: f32 = 2.0,
        Indestructible: bool = false,
        TemperatureThreshold: f32 = 300.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour (GetComponent in Awake)
        physical: usize = 0,
        /// The component's index in its object (its order against the PhysicalBehaviour).
        index: usize = 0,
        /// Whether the PhysicalBehaviour is before it in the object's components (collision order).
        after_physical: bool = false,
        /// BreakSounds' length (PickRandom draws only when it is non-empty).
        break_sounds: usize = 0,
        /// MyRenderer, ScreenRenderer and Glow.
        my_renderer: Option<SpriteAt> = None,
        screen_renderer: Option<SpriteAt> = None,
        glow: Option<SpriteAt> = None,
        /// GlowColours
        glow_colours: Gradient = Gradient::default(),
        /// BrokenTelly, AltTelly and AltBrokenTelly; originalSprite is MyRenderer's at Awake.
        broken_telly: Option<ObjRef> = None,
        alt_telly: Option<ObjRef> = None,
        alt_broken_telly: Option<ObjRef> = None,
        original_sprite: Option<ObjRef> = None,
        /// The clips by index (own, MESSAGE, B2, B3).
        clips: Vec<VideoRef> = Vec::new(),
        /// B2Pos, and B2Map (the Map asset).
        B2Pos: [f32; 2] = [0.0; 2],
        b2_map: Option<ObjRef> = None,
        /// Video.clip as an index into `clips`; prevVid and prevAud (Audio.clip is the AudioSource's).
        video_clip: usize = 0,
        prev_video: Option<usize> = None,
        prev_audio: Option<ClipRef> = None,
        /// `alt`: the 0-3 am set.
        alt: bool = false,
        /// videoTime, and Awake's `seed`.
        videoTime: f64 = 0.0,
        seed: f32 = 0.0,
        video: VideoPlayer = VideoPlayer { playing: false, time: 0.0, speed: 1.0 },
    }
}

impl Game {
    /// SpriteRenderer.sprite
    pub fn sprite_ref(&self, s: SpriteAt) -> ObjRef {
        match s {
            SpriteAt::Physical(p) => self.physicals[p].sprite.expect("a physical sprite").0,
            SpriteAt::Renderer(r) => self.renderers[r].sprite.0,
        }
    }

    fn television_set_sprite(&mut self, t: usize, sprite: Option<ObjRef>) {
        let Some(sprite) = sprite else { unimplemented!("mod-only: TelevisionBehaviour: a null sprite on MyRenderer") };
        match self.televisions[t].my_renderer.expect("TelevisionBehaviour.MyRenderer") {
            SpriteAt::Physical(p) => self.physicals[p].sprite.as_mut().expect("a physical sprite").0 = sprite,
            SpriteAt::Renderer(r) => self.renderers[r].sprite.0 = sprite,
        }
    }

    /// TelevisionBehaviour.Audio
    fn television_audio(&self, t: usize) -> i32 {
        self.audio.source_field(self.televisions[t].id, "Audio").unwrap_or_else(|| super::menu::throw("NullReferenceException: TelevisionBehaviour.Audio"))
    }

    /// Audio.clip = the clip field `name` (MessageAudio, B2Audio or B3Audio).
    fn television_set_audio_clip(&mut self, t: usize, name: &str) {
        let (src, clip) = (self.television_audio(t), self.audio.clip(self.televisions[t].id, name));
        self.audio.set_clip(src, clip);
    }

    /// TelevisionBehaviour.Awake: originalSprite = MyRenderer.sprite; `seed = Random.value * 1000f` (the glow's noise).
    pub fn television_awake(&mut self, t: usize) {
        self.televisions[t].original_sprite = self.televisions[t].my_renderer.map(|s| self.sprite_ref(s));
        self.televisions[t].seed = self.random.value() * 1000.0;
    }

    /// TelevisionBehaviour.Start: the context menu buttons, UpdateActivation, `Video.time = Math.Min(Video.length,
    /// videoTime)`, then the date's clip (5 December) or the 0-3 am set (strictly between midnight and 03:00).
    pub fn television_start(&mut self, t: usize) {
        use super::menu::{Cond, Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.televisions[t].physical].menu;
        menu.push(B { label: Label::TelevisionIndestructible(t), ..B::new("machineryIndestructible", "", crate::il!(str "TelevisionBehaviour::Start" 1), A::TelevisionToggle(t)) });
        menu.push(B { condition: Cond::TelevisionBroken(t), ..B::new("repairMachinery", "Repair", crate::il!(str "TelevisionBehaviour::Start" 4), A::TelevisionRepair(t)) });
        self.television_update_activation(t);
        let x = &mut self.televisions[t];
        x.video.time = x.clips[x.video_clip].length.min(x.videoTime);
        let (month, day, secs) = local_now();
        if day == 5 && month == 12 {
            x.video_clip = B3;
            self.television_set_audio_clip(t, "B3Audio");
            return;
        }
        x.alt = secs > 0.0 && secs < 3.0 * 3600.0;
        if x.alt {
            let alt = x.alt_telly;
            x.video_clip = MESSAGE;
            self.television_set_audio_clip(t, "MessageAudio");
            self.television_set_sprite(t, alt);
        }
    }

    /// TelevisionBehaviour.Repair: Broken = false, UpdateActivation.
    pub fn television_repair(&mut self, t: usize) {
        self.televisions[t].Broken = false;
        self.television_update_activation(t);
    }

    /// TelevisionBehaviour.Use: `if (!Broken)` Activated flips, UpdateActivation.
    pub fn television_use(&mut self, t: usize) {
        if !self.televisions[t].Broken {
            self.televisions[t].Activated = !self.televisions[t].Activated;
            self.television_update_activation(t);
        }
    }

    /// TelevisionBehaviour.IsInSpecialPos: the Map of B2Pos is the current one and the set is within 8 of B2Pos.
    pub(crate) fn television_in_special_pos(&self, t: usize) -> bool {
        let x = &self.televisions[t];
        if self.current_map_asset != x.b2_map {
            return false;
        }
        let at = self.physicals[x.physical].transform.position;
        // Vector2.Distance: (a - b).magnitude, the components rounded, then (float)Math.Sqrt in double.
        super::mathf::magnitude2([at[0] - x.B2Pos[0], at[1] - x.B2Pos[1]]) < 8.0
    }

    /// What both clip swaps do while the set is on: Stop, Audio.time = 0, Video.time = 0, Activated = true, UpdateActivation.
    fn television_restart(&mut self, t: usize) {
        self.television_stop(t);
        let src = self.television_audio(t);
        self.audio.set_time(src, 0.0);
        self.televisions[t].video.time = 0.0;
        self.televisions[t].Activated = true;
        self.television_update_activation(t);
    }

    /// TelevisionBehaviour.Update: videoTime = Video.time; Video.playbackSpeed = Time.timeScale; while the screen shows,
    /// Glow.color = GlowColours.Evaluate(Clamp01(PerlinNoise(videoTime * 8, seed)) % 1); a hot set breaks; the special
    /// position swaps in the B2 clips and back. The VideoPlayer then runs its clock for the frame.
    pub fn television_update(&mut self, t: usize) {
        self.televisions[t].videoTime = self.televisions[t].video.time;
        self.televisions[t].video.speed = self.time.timeScale;
        let screen_on = self.televisions[t].screen_renderer.is_some_and(|s| self.sprite_enabled(s));
        if screen_on {
            let x = &self.televisions[t];
            let num = mathf::clamp01(mathf::perlin(x.videoTime as f32 * 8.0, x.seed));
            let color = x.glow_colours.evaluate(num % 1.0);
            let glow = x.glow.expect("TelevisionBehaviour.Glow");
            self.set_sprite_color(glow, color);
        }
        let (p, threshold) = (self.televisions[t].physical, self.televisions[t].TemperatureThreshold);
        if !self.televisions[t].Broken && self.physicals[p].Temperature > threshold {
            self.television_break(t);
        }
        if self.television_in_special_pos(t) {
            if self.televisions[t].prev_video.is_none() || self.televisions[t].prev_audio.is_none() {
                let src = self.television_audio(t);
                let x = &mut self.televisions[t];
                (x.prev_video, x.prev_audio) = (Some(x.video_clip), self.audio.get(src).clip.clone());
                x.video_clip = B2;
                self.television_set_audio_clip(t, "B2Audio");
                if self.televisions[t].Activated {
                    self.television_restart(t);
                }
            }
        } else if self.televisions[t].prev_video.is_some() && self.televisions[t].prev_audio.is_some() {
            let src = self.television_audio(t);
            let x = &mut self.televisions[t];
            x.video_clip = x.prev_video.take().unwrap();
            self.audio.set_clip(src, x.prev_audio.take());
            if x.Activated {
                self.television_restart(t);
            }
        }
        let x = &mut self.televisions[t];
        if x.video.playing {
            x.video.time += (self.time.deltaTime * x.video.speed) as f64;
            let length = x.clips[x.video_clip].length;
            if x.video.time >= length {
                x.video.time %= length;
            }
        }
    }

    /// TelevisionBehaviour.UpdateActivation: the screen and glow show while on and whole; Play or Stop; the sprite is the
    /// broken one (the alt set's own) or the original. RefreshOutline is UI.
    pub(crate) fn television_update_activation(&mut self, t: usize) {
        let x = &self.televisions[t];
        let on = x.Activated && !x.Broken;
        let (screen, glow) = (x.screen_renderer.expect("TelevisionBehaviour.ScreenRenderer"), x.glow.expect("TelevisionBehaviour.Glow"));
        self.set_sprite_enabled(screen, on);
        self.set_sprite_enabled(glow, on);
        if on {
            self.television_play(t);
        } else {
            self.television_stop(t);
        }
        let x = &self.televisions[t];
        let sprite = if x.Broken {
            if x.alt { x.alt_broken_telly } else { x.broken_telly }
        } else {
            x.original_sprite
        };
        self.television_set_sprite(t, sprite);
    }

    /// TelevisionBehaviour.Play: Video.Play, Audio.Play, Audio.time = (float)videoTime.
    pub(crate) fn television_play(&mut self, t: usize) {
        let src = self.television_audio(t);
        self.televisions[t].video.playing = true;
        self.audio.play(src);
        self.audio.set_time(src, self.televisions[t].videoTime as f32);
    }

    /// TelevisionBehaviour.Stop: Video.Stop (back to the first frame), Audio.Stop.
    pub(crate) fn television_stop(&mut self, t: usize) {
        let src = self.television_audio(t);
        self.televisions[t].video.playing = false;
        self.televisions[t].video.time = 0.0;
        self.audio.stop(src);
    }

    /// TelevisionBehaviour.OnCollisionEnter2D: `GetContact(0).normalImpulse > BreakingTreshold` breaks it. Unity delivers
    /// no collision to a disabled behaviour.
    pub(crate) fn television_on_collision_enter(&mut self, t: usize, c: &ScriptCollision) {
        if !self.televisions[t].enabled {
            return;
        }
        let first = c.contacts.first().unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array."));
        if first.normal_impulse > self.televisions[t].BreakingTreshold {
            self.television_break(t);
        }
    }

    /// TelevisionBehaviour.Shot: Break(default).
    pub fn television_shot(&mut self, t: usize) {
        self.television_break(t);
    }

    /// TelevisionBehaviour.OnEMPHit: Break(default).
    pub fn television_on_emp_hit(&mut self, t: usize) {
        self.television_break(t);
    }

    /// TelevisionBehaviour.Break(velocity): `if (!Broken && !Indestructible)` the set breaks, its particles start, and
    /// PlayClipOnce(BreakSounds.PickRandom()), and UpdateActivation runs.
    pub fn television_break(&mut self, t: usize) {
        if !self.televisions[t].Broken && !self.televisions[t].Indestructible {
            self.televisions[t].Broken = true;
            // BrokenParticles.Play(withChildren: true)
            let sid = self.televisions[t].id;
            self.ps_field_call(sid, "BrokenParticles", true, super::PsCall::Play);
            let (n, id, p) = (self.televisions[t].break_sounds, self.televisions[t].id, self.televisions[t].physical);
            let clip = self.pick_clip(id, "BreakSounds", n);
            self.play_clip_once(p, clip, 1.0);
            self.television_update_activation(t);
        }
    }
}
