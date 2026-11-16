//! DetectorBehaviour with Utils.GetLaserEndPoint and MaterialAwareRaycast. The beam is the Line's positions (Game::lines);
//! the glow is `_GlowIntensity` on the property block (Jaap/GlowingSprite, custom_material.rs). IsolatedActivation and IsolatedContinuousActivation reach only the
//! components on the detector's own GameObject, and only a UseWireBehaviour receives them (link.rs refuses one).
#![allow(non_snake_case)]

use super::explosion::RayHit2D;
use super::{d, mathf, script, Game};
use crate::b2::math::Vec2 as V;

script! {
    Detector {
        RelativeVelocityThreshold: f32 = 0.0,
        HasToBeAlive: bool = false,
        HasToBeOnFire: bool = false,
        TriggerOnExit: bool = true,
        ShowBeam: bool = true,
        Range: f32 = 5.0,
        Activated: bool = false,
        BeamOffset: f32 = 0.05,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// PhysicalBehaviour
        Phys: usize = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// LayersToDetect (LayerMask bits)
        LayersToDetect: u32 = 0,
        /// PropertyFilter, as PhysicalProperties arena indices
        PropertyFilter: Vec<usize> = Vec::new(),
        isDetecting: bool = false,
        /// Line (Game::lines)
        line: usize = 0,
        /// EnabledLight
        light: Option<super::visual::SpriteAt> = None,
        /// propertyBlock, read from the sprite in Awake.
        block: super::props::PropBlock = super::props::PropBlock(Default::default()),
    }
}

/// Utils.LaserHit
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct LaserHit {
    pub(super) hit: Option<RayHit2D>,
    pub(super) physical: Option<usize>,
    pub(super) point: [f32; 2],
    pub(super) rayDepth: i32,
    pub(super) totalDistance: f32,
}

impl LaserHit {
    /// LaserHit..ctor(hit, point, physicalBehaviour, rayDepth): totalDistance 0.
    pub(super) fn new(hit: Option<RayHit2D>, point: [f32; 2], physical: Option<usize>, ray_depth: i32) -> Self {
        LaserHit { hit, physical, point, rayDepth: ray_depth, totalDistance: 0.0 }
    }
}

const MAX_STEPS: i32 = 8;
const MAX_ITERATIONS: usize = 64;

impl Game {
    /// DetectorBehaviour.Awake: Activated, renderer.GetPropertyBlock(propertyBlock), and its ContinuousActivationBehaviour listener.
    pub fn detector_awake(&mut self, i: usize) {
        self.detectors[i].Activated = true;
        self.detectors[i].block = self.get_property_block(super::visual::SpriteAt::Physical(self.detectors[i].Phys));
        self.continuous.listeners.push(super::continuous::Listener::Detector(i));
    }

    /// DetectorBehaviour.OnContinuousUpdate: IsolatedContinuousActivation on each channel to its own object.
    pub(crate) fn detector_continuous_update(&mut self, i: usize) {
        if self.detectors[i].isDetecting && self.detectors[i].enabled {
            self.send_isolated(self.detectors[i].Phys, true);
        }
    }

    /// DetectorBehaviour.Start: UpdateActivated, Line.useWorldSpace = true and SetPositions({right * BeamOffset, right *
    /// Range}), then the context menu buttons.
    pub fn detector_start(&mut self, i: usize) {
        self.detector_update_activated(i);
        let (line, offset, range) = (self.detectors[i].line, self.detectors[i].BeamOffset, self.detectors[i].Range);
        self.lines[line].use_world_space = true;
        self.line_set_positions(line, &[[offset, 0.0, 0.0], [range, 0.0, 0.0]]);
        use super::menu::{Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.detectors[i].Phys].menu;
        menu.push(B { label: Label::DetectorTrigger(i), ..B::new("toggleTriggerBehaviour", "", crate::il!(str "DetectorBehaviour::Start" 1), A::DetectorTrigger(i)) });
        menu.push(B::new("setDetectorRange", crate::il!(str "DetectorBehaviour::Start" 3), crate::il!(str "DetectorBehaviour::Start" 4), A::Entry(super::menu::Entry::DetectorRange(i))));
        menu.push(B { label: Label::DetectorBeam(i), ..B::new("toggleDetectorBeam", "", crate::il!(str "DetectorBehaviour::Start" 6), A::DetectorBeam(i)) });
    }

    /// DetectorBehaviour.Use
    pub fn detector_use(&mut self, i: usize) {
        if self.detectors[i].enabled {
            let s = &mut self.detectors[i];
            s.Activated = !s.Activated;
            self.detector_update_activated(i);
        }
    }

    /// DetectorBehaviour.UpdateActivated: FixedUpdate, Line.enabled = Activated && ShowBeam, EnabledLight.enabled = Activated.
    pub(crate) fn detector_update_activated(&mut self, i: usize) {
        self.detector_fixed_update(i);
        let s = &self.detectors[i];
        let (line, on, light) = (s.line, s.Activated, s.light.expect("DetectorBehaviour.EnabledLight null (NullReferenceException in UpdateActivated)"));
        self.lines[line].enabled = on && s.ShowBeam;
        self.set_sprite_enabled(light, on);
    }

    /// DetectorBehaviour.PassesFilter: whether the object's Properties is one of the PropertyFilter entries.
    pub(crate) fn detector_passes_filter(&self, i: usize, phys: usize) -> bool {
        self.detectors[i].PropertyFilter.contains(&self.physicals[phys].Properties)
    }

    /// DetectorBehaviour.Update: while neither paused nor in the pause menu, `_GlowIntensity` is 1 while detecting, else 0.
    pub fn detector_update(&mut self, i: usize) {
        if !self.paused && !self.paused_menu {
            let v = if self.detectors[i].isDetecting { 1.0 } else { 0.0 };
            self.detectors[i].block.set_float("_GlowIntensity", v);
        }
    }

    /// DetectorBehaviour.OnWillRenderObject for each detector whose sprite the camera draws: SetPropertyBlock(propertyBlock).
    pub fn detectors_will_render(&mut self) {
        for i in 0..self.detectors.len() {
            let at = super::visual::SpriteAt::Physical(self.detectors[i].Phys);
            if !self.will_render_dispatch(self.detectors[i].id, self.detectors[i].enabled && self.sprite_drawn(at)) {
                continue;
            }
            let block = std::mem::take(&mut self.detectors[i].block);
            self.set_property_block(at, &block);
            self.detectors[i].block = block;
        }
    }

    /// DetectorBehaviour.OnDisable: Activated = false, Line.enabled = false, `_GlowIntensity` 0 set on the sprite and
    /// EnabledLight.enabled = false.
    pub fn detector_on_disable(&mut self, i: usize) {
        self.detectors[i].Activated = false;
        let line = self.detectors[i].line;
        self.lines[line].enabled = false;
        let mut block = std::mem::take(&mut self.detectors[i].block);
        block.set_float("_GlowIntensity", 0.0);
        self.set_property_block(super::visual::SpriteAt::Physical(self.detectors[i].Phys), &block);
        self.detectors[i].block = block;
        let light = self.detectors[i].light.expect("DetectorBehaviour.EnabledLight null (NullReferenceException in OnDisable)");
        self.set_sprite_enabled(light, false);
    }

    /// DetectorBehaviour.FixedUpdate
    pub fn detector_fixed_update(&mut self, i: usize) {
        if !self.detectors[i].Activated {
            self.detectors[i].isDetecting = false;
            return;
        }
        let s = &self.detectors[i];
        let p = s.Phys;
        let maximum_distance = self.physicals[p].Charge() + s.Range;
        let layers = s.LayersToDetect;
        // rigidbody.position and rigidbody.GetRelativeVector(Vector3.right) * (IsFlipped ? -1 : 1)
        self.sync_pending();
        let xf = self.world.body(self.physicals[p].rigidbody.body).xf;
        let sign = if detector_is_flipped(self.physicals[p].transform.localScale) { -1.0 } else { 1.0 };
        let dir = [xf.q.c * sign, xf.q.s * sign];
        let mut hits = Vec::new();
        self.get_laser_end_point([xf.p.x, xf.p.y], dir, &mut hits, layers, maximum_distance, MAX_ITERATIONS);
        // Line.positionCount = hits.Count + 1 and SetPositions(rigidbody.position, then each hit's point).
        let line = self.detectors[i].line;
        let positions: Vec<[f32; 3]> = std::iter::once([xf.p.x, xf.p.y, 0.0]).chain(hits.iter().map(|h| [h.point[0], h.point[1], 0.0])).collect();
        self.line_set_position_count(line, positions.len());
        self.line_set_positions(line, &positions);
        let fits = self.detector_fits_criteria(i, hits.last().copied());
        if !self.detectors[i].isDetecting || fits {
            return;
        }
        if self.detectors[i].TriggerOnExit {
            self.send_isolated(self.detectors[i].Phys, false);
        }
        self.detectors[i].isDetecting = false;
    }

    /// DetectorBehaviour.FitsCriteria
    pub(crate) fn detector_fits_criteria(&mut self, i: usize, hit: Option<LaserHit>) -> bool {
        // LastOrDefault of an empty list is a default LaserHit: no PhysicalBehaviour.
        let Some(hit) = hit else { return false };
        let Some(p) = hit.physical else { return false };
        if hit.hit.is_none() {
            return false;
        }
        let s = &self.detectors[i];
        let (me, threshold, alive, fire) = (s.Phys, s.RelativeVelocityThreshold, s.HasToBeAlive, s.HasToBeOnFire);
        if alive {
            // AliveByTransform.TryGetValue(value.transform.root)
            let Some(person) = self.person_of_root(p) else { return false };
            if !self.person_is_alive(person) {
                return false;
            }
        }
        if threshold > 0.01 {
            self.sync_pending();
            let a = self.physicals[me].rigidbody.point_velocity(&self.world, hit.point);
            let b = self.physicals[p].rigidbody.point_velocity(&self.world, hit.point);
            if mathf::sqr_magnitude([a[0] - b[0], a[1] - b[1]]) < threshold {
                return false;
            }
        }
        if fire && !self.physicals[p].OnFire {
            return false;
        }
        if !self.detectors[i].PropertyFilter.is_empty() && !self.detector_passes_filter(i, p) {
            return false;
        }
        if !self.detectors[i].isDetecting {
            self.send_isolated(self.detectors[i].Phys, false);
        }
        self.detectors[i].isDetecting = true;
        true
    }

    /// Utils.MaterialAwareRaycast
    pub(super) fn material_aware_raycast(&mut self, origin: [f32; 2], dir: [f32; 2], max_distance: f32, layers: u32, max_steps: i32, depth: i32, mut current_distance: f32) -> LaserHit {
        if max_steps <= 0 || max_distance <= 0.0 {
            return LaserHit::new(None, origin, None, depth);
        }
        let Some(hit) = self.raycast(V::new(origin[0], origin[1]), dir, max_distance, layers, f32::NEG_INFINITY, f32::INFINITY) else {
            current_distance += max_distance;
            // origin + dir * maxDistance: each Vector2 operator's result is stored (rounded) before the next.
            let point = [dir[0] * max_distance + origin[0], dir[1] * max_distance + origin[1]];
            return LaserHit { totalDistance: current_distance, ..LaserHit::new(None, point, None, depth) };
        };
        let point = [hit.point.x, hit.point.y];
        // PhysicalObjectsInWorldByTransform.TryGetValue(raycastHit2D.transform)
        let Some(p) = self.physical_of_collider(hit.collider).filter(|&p| self.physicals[p].in_world) else {
            return LaserHit { totalDistance: current_distance + hit.distance, ..LaserHit::new(Some(hit), point, None, depth) };
        };
        if self.physicals[p].AbsorbsLasers {
            return LaserHit { totalDistance: current_distance + hit.distance, ..LaserHit::new(Some(hit), point, Some(p), depth) };
        }
        let next = [point[0] + dir[0] * 0.01f32, point[1] + dir[1] * 0.01f32];
        self.material_aware_raycast(next, dir, max_distance - hit.distance, layers, max_steps - 1, depth + 1, current_distance + hit.distance)
    }

    /// Utils.GetLaserEndPoint
    pub(super) fn get_laser_end_point(&mut self, origin: [f32; 2], dir: [f32; 2], list: &mut Vec<LaserHit>, layers: u32, maximum_distance: f32, max_iterations: usize) {
        if list.len() >= max_iterations {
            return;
        }
        let laser = self.material_aware_raycast(origin, dir, maximum_distance, layers, MAX_STEPS, 0, 0.0);
        let add_to_list = |list: &mut Vec<LaserHit>| {
            let n = mathf::normalized2(dir);
            let point = [n[0] * maximum_distance + origin[0], n[1] * maximum_distance + origin[1]];
            list.push(LaserHit::new(None, point, None, list.len() as i32));
        };
        let Some(value) = laser.hit else { return add_to_list(list) };
        let point = [value.point.x, value.point.y];
        let Some(p) = laser.physical else {
            list.push(LaserHit::new(Some(value), point, None, list.len() as i32));
            return;
        };
        // MaterialAwareRaycast only returns a PhysicalBehaviour that AbsorbsLasers.
        if !self.physicals[p].AbsorbsLasers {
            return add_to_list(list);
        }
        list.push(LaserHit::new(Some(value), point, Some(p), list.len() as i32));
        if self.physicals[p].ReflectsLasers {
            // Vector2.Reflect(dir, normal): -2 * Dot(normal, dir) scales the normal.
            let n = [value.normal.x, value.normal.y];
            let num = (d(-2.0) * (d(n[0]) * dir[0] + d(n[1]) * dir[1]).f()).f();
            let reflected = [(d(num) * n[0] + dir[0]).f(), (d(num) * n[1] + dir[1]).f()];
            // The recursive call takes maxIterations' default.
            self.get_laser_end_point(point, reflected, list, layers, maximum_distance - laser.totalDistance, MAX_ITERATIONS);
        }
    }
}

/// DetectorBehaviour.IsFlipped: transform.localScale.x < 0.
pub(crate) fn detector_is_flipped(local_scale: [f32; 3]) -> bool {
    local_scale[0] < 0.0
}
