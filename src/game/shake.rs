//! SoundOnShakeBehaviour (the bushes' and trees' branches): FixedUpdate compares the Rigidbody2D's velocity with the last
//! step's and, while its AudioSource is silent, a hard enough shake draws Random.value and plays one of Clips. The
//! AudioSource.isPlaying gates the draw (audio.rs).
#![allow(non_snake_case)]

use super::Game;

#[derive(Clone, Debug, Default)]
pub struct SoundOnShake {
    /// The component's instance id.
    pub id: i32,
    /// rb: the instance id of GetComponent<Rigidbody2D>() in Awake.
    pub rb: i32,
    /// AudioSource: its component id.
    pub AudioSource: i32,
    pub Clips: Vec<Option<super::audio::ClipRef>>,
    pub ShakeThreshold: f32,
    pub previousVelocity: [f32; 2],
    /// The AudioSource reference is null, or the object has no Rigidbody2D: FixedUpdate dereferences them.
    pub audio_null: bool,
    pub rb_null: bool,
}

/// EmitParticlesOnShakeBehaviour: Awake's GetComponent<Rigidbody2D> (its instance id) and its own ParticleSystem.
#[derive(Clone, Debug)]
pub struct EmitOnShake {
    pub id: i32,
    pub rb: Option<i32>,
    pub ShakeThreshold: f32,
    pub EmissionCount: f32,
    pub previousVelocity: [f32; 2],
}

impl Default for EmitOnShake {
    fn default() -> Self {
        Self {
            id: Default::default(),
            rb: Default::default(),
            ShakeThreshold: 3.0,
            EmissionCount: 1.0,
            previousVelocity: Default::default(),
        }
    }
}

/// ParticleSystemSerialisationHelper: IsPlaying.
#[derive(Clone, Debug, Default)]
pub struct SerialisationHelper {
    pub id: i32,
    pub IsPlaying: bool,
    /// ParticleSystems is null.
    pub null: bool,
}

impl Game {
    /// ParticleSystemSerialisationHelper.Start: Play if IsPlaying, else Stop (nothing on a null ParticleSystems).
    pub fn serialisation_helper_start(&mut self, h: usize) {
        let x = &self.serialisation_helpers[h];
        if !x.null {
            let (id, call) = (x.id, if x.IsPlaying { super::PsCall::Play } else { super::PsCall::Stop(self.time.timeAsDouble) });
            self.ps_call(|e| e.owner == id, call);
        }
    }

    /// ParticleSystemSerialisationHelper.Update: IsPlaying = ParticleSystems && ParticleSystems.isEmitting.
    pub fn serialisation_helper_update(&mut self, h: usize) {
        let x = &self.serialisation_helpers[h];
        let on = !x.null && crate::particles::is_emitting(self, x.id);
        self.serialisation_helpers[h].IsPlaying = on;
    }

    /// EmitParticlesOnShakeBehaviour.FixedUpdate: ps.Emit(min(CeilToInt(EmissionCount * (sqr / threshold^2)), 4)) on a
    /// shake harder than ShakeThreshold (at the system's pose now).
    pub fn emit_on_shake_fixed_update(&mut self, s: usize) {
        let Some(rb) = self.emit_on_shakes[s].rb else { crate::game::menu::npe() };
        let velocity = self.rigidbody_velocity_by_id(rb);
        let sh = &mut self.emit_on_shakes[s];
        // Vector2 operator- rounds each component; sqrMagnitude and the products are evaluated in double (Mono).
        let sqr_magnitude = super::mathf::sqr_magnitude([sh.previousVelocity[0] - velocity[0], sh.previousVelocity[1] - velocity[1]]);
        sh.previousVelocity = velocity;
        let num = (super::d(sh.ShakeThreshold) * sh.ShakeThreshold).f();
        if sqr_magnitude > num {
            let n = super::temperature::ceil_to_int((super::d(sh.EmissionCount) * (super::d(sqr_magnitude) / num)).f()).min(4);
            let id = sh.id;
            self.ps_emit(|e| e.owner == id && matches!(e.drive, super::EmbeddedDrive::EmitOnShake), n);
        }
    }

    /// SoundOnShakeBehaviour.FixedUpdate
    pub fn sound_on_shake_fixed_update(&mut self, s: usize) {
        if self.sound_on_shakes[s].audio_null || self.sound_on_shakes[s].rb_null {
            crate::game::menu::npe();
        }
        let sh = &self.sound_on_shakes[s];
        let velocity = self.rigidbody_velocity_by_id(sh.rb);
        if self.audio_is_playing(sh.AudioSource) {
            self.sound_on_shakes[s].previousVelocity = velocity;
            return;
        }
        // As EmitParticlesOnShakeBehaviour: per-component rounding, then double sums and products (Mono).
        let sqr_magnitude = super::mathf::sqr_magnitude([sh.previousVelocity[0] - velocity[0], sh.previousVelocity[1] - velocity[1]]);
        let num = (super::d(sh.ShakeThreshold) * sh.ShakeThreshold).f();
        self.sound_on_shakes[s].previousVelocity = velocity;
        if sqr_magnitude > num && self.random.value() > 0.5 {
            let (source, n) = (self.sound_on_shakes[s].AudioSource, self.sound_on_shakes[s].Clips.len());
            // AudioSource.PlayOneShot(Clips.PickRandom(), Mathf.Clamp01(sqrMagnitude / num - 0.5f)) (null when Clips is empty)
            let clip = self.pick_random(n).and_then(|k| self.sound_on_shakes[s].Clips[k].clone());
            self.audio.play_one_shot(source, clip.as_ref(), super::mathf::clamp01(sqr_magnitude / num - 0.5));
        }
    }
}
