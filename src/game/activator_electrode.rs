//! ActivatorElectrodeBehaviour (the Activator Electrode). Use and UseContinuous look for the physical objects at its
//! emission point (an OverlapCircle of EmissionRadius on LayersToConsider) and pass the activation on to them as a branch
//! through the electrode's root: BroadcastMessage("Use"/"UseContinuous") to the object, then OnSingleUse/OnContinuousUse.
//! Use also lights the emitter glow and plays the lights and StartUseSound.

// ActivatorElectrodeBehaviour.Awake: phys at link; it adds ProcessEmitContinuous, an empty method, to OnContinuousUpdate.
// ActivatorElectrodeBehaviour.OnDestroy: removes that listener (an empty method: no effect to undo).
// ActivatorElectrodeBehaviour.ProcessEmitContinuous: an empty method.
#![allow(non_snake_case)]

use super::{script, Game};
use crate::b2::math::Vec2 as V;

pub use super::signal::Act;

script! {
    ActivatorElectrode {
        EmissionPoint: [f32; 2] = [0.1, 0.0],
        EmissionRadius: f32 = 0.05,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>() (Awake)
        physical: usize = 0,
        /// LayersToConsider (LayerMask bits)
        LayersToConsider: u32 = 0,
        /// Lights (visual.rs indexes) and EmitterGlow.
        lights: Vec<usize> = Vec::new(),
        EmitterGlow: Option<super::visual::SpriteAt> = None,
    }
}

/// Physics2D.OverlapCircleNonAlloc's buffer
pub(crate) const BUFFER: usize = 4;

impl Game {
    /// base.transform.TransformPoint(EmissionPoint)
    fn electrode_center(&mut self, e: usize) -> V {
        let s = &self.activator_electrodes[e];
        let (p, point) = (s.physical, s.EmissionPoint);
        let c = self.transform_point3(p, [point[0], point[1], 0.0]);
        V::new(c[0], c[1])
    }

    /// `collider.gameObject != base.gameObject`, `PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform)` and
    /// `!activation.Contains(base.transform.root)`: the physical to pass the activation on to.
    fn electrode_target(&self, e: usize, hit: Option<usize>, act: &Act) -> Option<usize> {
        let me = self.activator_electrodes[e].physical;
        let v = hit.filter(|&v| v != me && self.physicals[v].in_world)?;
        (!self.signal_contains(act, self.root_id(me))).then_some(v)
    }

    /// ActivatorElectrodeBehaviour.Use
    pub fn activator_electrode_use(&mut self, e: usize, act: &Act) {
        if !self.activator_electrodes[e].enabled {
            return;
        }
        let center = self.electrode_center(e);
        let (radius, layers, me) = (self.activator_electrodes[e].EmissionRadius, self.activator_electrodes[e].LayersToConsider, self.activator_electrodes[e].physical);
        let hits = self.overlap_circle(center, radius, layers);
        for (physical, _) in hits.into_iter().take(BUFFER) {
            let Some(value) = self.electrode_target(e, physical, act) else { continue };
            let root = self.root_id(me);
            let branch = self.signal_branch(act, root);
            self.broadcast_use(value, &branch);
            self.activations_send_once(value);
        }
        self.activator_electrode_glow(e);
        self.activator_electrode_start_lights(e);
        // phys.PlayClipOnce(StartUseSound)
        let (id, p) = (self.activator_electrodes[e].id, self.activator_electrodes[e].physical);
        let clip = self.audio.clip(id, "StartUseSound");
        self.play_clip_once(p, clip, 1.0);
    }

    /// ActivatorElectrodeBehaviour.UseContinuous: the first collider of Physics2D.OverlapCircle.
    pub fn activator_electrode_use_continuous(&mut self, e: usize, act: &Act) {
        if !self.activator_electrodes[e].enabled {
            return;
        }
        let center = self.electrode_center(e);
        let (radius, layers, me) = (self.activator_electrodes[e].EmissionRadius, self.activator_electrodes[e].LayersToConsider, self.activator_electrodes[e].physical);
        // Glow runs whether or not a collider was hit.
        let first = self.overlap_circle(center, radius, layers).first().map(|h| h.0);
        if let Some(value) = first.and_then(|physical| self.electrode_target(e, physical, act)) {
            let root = self.root_id(me);
            let branch = self.signal_branch(act, root);
            self.broadcast_use_continuous(value, &branch);
            self.activations_send_continuous(value);
        }
        self.activator_electrode_glow(e);
    }

    fn emitter_glow(&self, e: usize) -> super::visual::SpriteAt {
        self.activator_electrodes[e].EmitterGlow.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// ActivatorElectrodeBehaviour.Start: StopLights, EmitterGlow.enabled = false.
    pub fn activator_electrode_start(&mut self, e: usize) {
        self.activator_electrode_stop_lights(e);
        let g = self.emitter_glow(e);
        self.set_sprite_enabled(g, false);
    }

    /// ActivatorElectrodeBehaviour.Glow: EmitterGlow enabled at full alpha.
    pub(crate) fn activator_electrode_glow(&mut self, e: usize) {
        let g = self.emitter_glow(e);
        self.set_sprite_enabled(g, true);
        let mut c = self.sprite_color(g);
        c[3] = 1.0;
        self.set_sprite_color(g, c);
    }

    /// ActivatorElectrodeBehaviour.Update: the enabled glow fades by 4 a second, then disables.
    pub fn activator_electrode_update(&mut self, e: usize) {
        let g = self.emitter_glow(e);
        if self.sprite_enabled(g) {
            let mut c = self.sprite_color(g);
            c[3] = (super::d(c[3]) - super::d(self.time.deltaTime) * 4.0f32).f();
            self.set_sprite_color(g, c);
            if c[3] <= super::mathf::EPSILON {
                self.set_sprite_enabled(g, false);
            }
        }
    }

    /// ActivatorElectrodeBehaviour.StopLights
    pub fn activator_electrode_stop_lights(&mut self, e: usize) {
        for v in self.activator_electrodes[e].lights.clone() {
            self.trivial_animation_stop(v);
            self.trivial_force_set_progress(v, 0.0);
        }
    }

    /// ActivatorElectrodeBehaviour.StartLights
    pub fn activator_electrode_start_lights(&mut self, e: usize) {
        for v in self.activator_electrodes[e].lights.clone() {
            self.trivial_force_set_progress(v, 0.0);
            self.trivial_animation_play(v);
        }
    }
}
