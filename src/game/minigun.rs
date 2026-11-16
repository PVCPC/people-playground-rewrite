//! MinigunBehaviour: while the trigger is held past 0.2 s it fires its FirearmBehaviour (Shoot the first time, then
//! ShootContinuous); Use resets the firearm's shot index. rotationSpeed drives the barrel sound routine and the
//! fire loop plays on FireAudioSource. `rotation` and the temperature reach the barrel's Unlit/MinigunBarrelShader
//! (custom_material.rs) through its property block.
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    Minigun {
        UpperTemperatureBound: f32 = 500.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour (GetComponent in Awake).
        physical: usize = 0,
        /// `Game::firearms` index of the FirearmBehaviour field.
        firearm: usize = 0,
        /// timeHeld: seconds the trigger has been held (reset when it is released).
        timeHeld: f32 = 0.0,
        rotationSpeed: f32 = 0.0,
        fireLoopIsPlaying: bool = false,
        shouldPlayReflection: bool = false,
        /// SoundRoutine: 0 not running, 1 in WaitForSeconds(BarrelStart.length) (`sound_wait` left), 2 in
        /// WaitUntil(rotationSpeed < 0.06f).
        sound_stage: u8 = 0,
        sound_wait: f32 = 0.0,
        rotation: f32 = 0.0,
        /// propertyBlock, read from the SpriteRenderer (the physical's) in Awake.
        block: super::props::PropBlock = super::props::PropBlock::default(),
    }
}

impl Game {
    /// MinigunBehaviour.Use: FirearmBehaviour.ResetIndex(). Its StartCoroutine(SoundRoutine) is `minigun_sound_routine` below.
    pub fn minigun_use(&mut self, m: usize) {
        let f = self.miniguns[m].firearm;
        self.firearms[f].shotIndex = 0;
    }

    fn minigun_source(&self, m: usize, name: &str) -> i32 {
        self.audio.source_field(self.miniguns[m].id, name).unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: MinigunBehaviour AudioSource"))
    }

    /// MinigunBehaviour.FixedUpdate: rotationSpeed *= 0.98f while used continuously, else 0.92f.
    pub fn minigun_fixed_update(&mut self, m: usize) {
        let k = if self.used_continuously(self.miniguns[m].physical) { 0.98 } else { 0.92 };
        self.miniguns[m].rotationSpeed *= k;
    }

    /// MinigunBehaviour.Awake: physicalBehaviour and renderer are `physical`'s; the block is read from the sprite.
    pub fn minigun_awake(&mut self, m: usize) {
        self.miniguns[m].block = self.get_property_block(super::visual::SpriteAt::Physical(self.miniguns[m].physical));
    }

    /// MinigunBehaviour.OnWillRenderObject for each minigun whose sprite the camera draws: `_Rotation` = rotation, `_Progress` =
    /// Clamp01(Temperature / UpperTemperatureBound) squared.
    pub fn miniguns_will_render(&mut self) {
        for m in 0..self.miniguns.len() {
            let s = &self.miniguns[m];
            let at = super::visual::SpriteAt::Physical(s.physical);
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(at)) {
                continue;
            }
            let num = super::mathf::clamp01(self.physicals[s.physical].Temperature / s.UpperTemperatureBound);
            let mut block = std::mem::take(&mut self.miniguns[m].block);
            block.set_float("_Rotation", self.miniguns[m].rotation);
            block.set_float("_Progress", num * num);
            self.set_property_block(at, &block);
            self.miniguns[m].block = block;
        }
    }

    /// MinigunBehaviour.Update (`directlyUsed` is written only), then its SoundRoutine's resumption in the frame's coroutine pass.
    pub fn minigun_update(&mut self, m: usize) {
        let (f, p, id) = (self.miniguns[m].firearm, self.miniguns[m].physical, self.miniguns[m].id);
        // rotation += rotationSpeed * Time.deltaTime * 60f
        let mg = &mut self.miniguns[m];
        mg.rotation = (super::d(mg.rotation) + super::d(mg.rotationSpeed) * self.time.deltaTime * 60.0f32).f();
        if self.miniguns[m].timeHeld > 0.2 {
            if !self.miniguns[m].fireLoopIsPlaying {
                let s = self.minigun_source(m, "FireAudioSource");
                self.audio.play(s);
                self.miniguns[m].fireLoopIsPlaying = true;
            }
            if self.firearms[f].shotIndex == 0 {
                self.firearm_shoot(f);
            } else {
                self.firearm_shoot_continuous(f);
            }
        } else if self.miniguns[m].fireLoopIsPlaying {
            self.miniguns[m].fireLoopIsPlaying = false;
            let s = self.minigun_source(m, "FireAudioSource");
            self.audio.stop(s);
            if self.miniguns[m].shouldPlayReflection {
                let clip = self.audio.clip(id, "FireStop");
                self.audio.play_one_shot(s, clip.as_ref(), 1.0);
            }
            self.miniguns[m].shouldPlayReflection = false;
        }
        let dt = self.time.deltaTime;
        // StopAllCoroutines; StartCoroutine(SoundRoutine()) runs to its first yield.
        let started = self.started_used_continuously(p);
        if started {
            let s = self.minigun_source(m, "RattleAudioSource");
            self.audio.stop(s);
            let clip = self.audio.clip(id, "BarrelStart");
            // A replay has no game assets (no clips, muted).
            let len = match &clip {
                Some(c) => c.length(),
                None if self.assets.is_none() => 0.0,
                None => crate::game::menu::throw("NullReferenceException: MinigunBehaviour.BarrelStart"),
            };
            self.audio.set_clip(s, clip);
            self.audio.play(s);
            (self.miniguns[m].sound_stage, self.miniguns[m].sound_wait) = (1, len);
        }
        if self.used_continuously(p) {
            self.miniguns[m].timeHeld += dt;
            if self.miniguns[m].timeHeld > 1.0 {
                self.miniguns[m].shouldPlayReflection = true;
            }
            let r = self.miniguns[m].rotationSpeed;
            self.miniguns[m].rotationSpeed = super::mathf::lerp(r, 0.15, super::mathf::lerp_factor_dt(0.99, dt));
        } else {
            self.miniguns[m].timeHeld = 0.0;
        }
        if !started {
            self.minigun_sound_routine(m, dt);
        }
    }

    /// MinigunBehaviour.SoundRoutine after its first yield: BarrelLoop loops once BarrelStart has had its length, and
    /// BarrelStop plays once rotationSpeed < 0.06f.
    pub(crate) fn minigun_sound_routine(&mut self, m: usize, dt: f32) {
        let id = self.miniguns[m].id;
        if self.miniguns[m].sound_stage == 1 {
            self.miniguns[m].sound_wait -= dt;
            if self.miniguns[m].sound_wait > 0.0 {
                return;
            }
            let s = self.minigun_source(m, "RattleAudioSource");
            let clip = self.audio.clip(id, "BarrelLoop");
            self.audio.set_clip(s, clip);
            self.audio.get_mut(s).looping = true;
            self.audio.play(s);
            self.miniguns[m].sound_stage = 2;
        } else if self.miniguns[m].sound_stage == 2 && self.miniguns[m].rotationSpeed < 0.06 {
            let s = self.minigun_source(m, "RattleAudioSource");
            self.audio.get_mut(s).looping = false;
            let clip = self.audio.clip(id, "BarrelStop");
            self.audio.set_clip(s, clip);
            self.audio.play(s);
            self.miniguns[m].sound_stage = 0;
        }
    }
}
