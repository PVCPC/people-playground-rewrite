//! LiquidContainerLeakingBehaviour and LiquidContainerHoleBehaviour. Shots and wounding unstabs make a Hole clone under the
//! object; each hole sprays FancyBloodSplat particles and drains its Source. Repair destroys them. The "Repair holes" context
//! button is UI, and the holes' LiquidParticles and WaterPuffParticles are presentation.
#![allow(non_snake_case)]

use super::container::Owner;
use super::destroyable::AttachParent;
use super::shatter::ShatterHost;
use super::{mathf, script, Game};
use crate::assets::db::ObjRef;

script! {
    /// LiquidContainerLeakingBehaviour
    LiquidLeak {
        DrainIntensityMultiplier: f32 = 0.1,
        VisualPressureMultiplier: f32 = 1.0,
        HasHoles: bool = false,
    }
    links {
        id: i32 = 0,
        /// PhysicalBehaviour
        physical: usize = 0,
        HolePrefab: ObjRef = ObjRef { file: 0, path_id: 0 },
        Source: Owner = Owner::LiquidCanister(0),
        /// The holes' `Game::liquid_holes` indices (GetComponentsInChildren<LiquidContainerHoleBehaviour>).
        holes: Vec<usize> = Vec::new(),
        /// The `holes` list of Hole structs: (LocalPos, LocalDir) as [x, y, x, y] and Intensity, in creation order.
        hole_data: Vec<[f32; 5]> = Vec::new(),
        /// Hole[] SerialisableHoles: filled by OnBeforeSerialise or a load, emptied by Start.
        SerialisableHoles: Option<Vec<[f32; 5]>> = None,
    }
}

/// LiquidContainerHoleBehaviour
#[derive(Clone, Debug)]
pub struct LiquidHole {
    pub id: i32,
    /// The clone root's Transform id.
    pub root: i32,
    pub DrainRate: f32,
    pub ForceMultiplier: f32,
    /// Set right after Instantiate.
    pub Source: Option<Owner>,
    /// PhysicalParticles: the `Game::splats` index of its FancyBloodSplatController.
    pub splat: usize,
    /// WaterPuffParticles' Transform id.
    pub water_puff: i32,
}

impl Default for LiquidHole {
    fn default() -> Self {
        Self {
            id: Default::default(),
            root: Default::default(),
            DrainRate: 0.1,
            ForceMultiplier: 0.001,
            Source: Default::default(),
            splat: Default::default(),
            water_puff: Default::default(),
        }
    }
}

// LiquidContainerHoleBehaviour.Awake: PhysicalParticles.OnCollide += OnParticleCollide, the splat's `on_hole` link.
// LiquidContainerLeakingBehaviour.Start: the "Repair holes" button (Cond::LiquidLeakHoles in menu.rs), then liquid_leak_start_holes.

impl Game {
    /// The leak on physical `p`.
    pub(crate) fn leak_of(&self, p: usize) -> Option<usize> {
        self.liquid_leaks.iter().position(|x| x.physical == p)
    }

    /// LiquidContainerLeakingBehaviour.Shot. rigidbody.ClosestPoint (PhysicsQuery2D::ClosestPoint, 180bea340) is
    /// the nearest over the attached colliders, like Utils.ClosestPointFix.
    pub fn liquid_leak_shot(&mut self, l: usize, normal: [f32; 2], point: [f32; 2], damage: f32) {
        self.liquid_leak_hole_at(l, point, normal, mathf::clamp(damage, 0.2, 5.0));
    }

    /// LiquidContainerLeakingBehaviour.ExitShot: Shot(shot).
    pub fn liquid_leak_exit_shot(&mut self, l: usize, normal: [f32; 2], point: [f32; 2], damage: f32) {
        self.liquid_leak_shot(l, normal, point, damage);
    }

    /// LiquidContainerLeakingBehaviour.Unstabbed, when stabbing.stabber.StabCausesWound.
    pub fn liquid_leak_unstabbed(&mut self, p: usize, point: [f32; 2], normal: [f32; 2]) {
        if let Some(l) = self.leak_of(p) {
            self.liquid_leak_hole_at(l, point, normal, 5.0);
        }
    }

    fn liquid_leak_hole_at(&mut self, l: usize, point: [f32; 2], normal: [f32; 2], intensity: f32) {
        let p = self.liquid_leaks[l].physical;
        let at = [point[0] + normal[0] * 0.1, point[1] + normal[1] * 0.1];
        let closest = self.closest_point_fix(p, at);
        let local = Game::inverse_transform_point(self, p, [closest[0], closest[1], 0.0]);
        let dir = self.anchored_inverse_direction(p, [normal[0], normal[1], 0.0]);
        self.liquid_leak_create_hole(l, [local[0], local[1]], [dir[0], dir[1]], intensity);
    }

    /// LiquidContainerLeakingBehaviour.CreateHole
    pub(crate) fn liquid_leak_create_hole(&mut self, l: usize, local_pos: [f32; 2], local_dir: [f32; 2], intensity: f32) {
        if intensity < 0.1 {
            return;
        }
        let s = &self.liquid_leaks[l];
        let (p, prefab, source, visual, multiplier) = (s.physical, s.HolePrefab, s.Source, s.VisualPressureMultiplier, s.DrainIntensityMultiplier);
        let at = self.physicals[p].transform.position;
        let root = self.spawn_prefab(prefab, at);
        self.attach_clone(root, AttachParent::Physical(p), at);
        let n = mathf::normalized2(local_dir);
        let local = crate::unity::transform::Trs {
            t: [local_pos[0] + n[0] * 0.03, local_pos[1] + n[1] * 0.03, 0.0, 0.0],
            q: super::physical::z_rotation((super::d((crate::game::mathf::atan2_f64(local_dir[1] as f64, local_dir[0] as f64)) as f32) * 57.29578f32 + 90.0f32).f()),
            s: [1.0, 1.0, 1.0, 0.0],
        };
        let key = self.physicals[p].transform_id;
        self.attached_clones.get_mut(&key).and_then(|v| v.last_mut()).expect("the clone just attached").local = local;
        let h = self.liquid_holes.iter().rposition(|x| x.root == root).expect("the Hole prefab's LiquidContainerHoleBehaviour");
        let hole = &mut self.liquid_holes[h];
        hole.Source = Some(source);
        hole.ForceMultiplier *= visual;
        hole.DrainRate = (super::d(intensity) * 0.05f32 * multiplier).f();
        let leak = &mut self.liquid_leaks[l];
        leak.HasHoles = true;
        leak.hole_data.push([local_pos[0], local_pos[1], local_dir[0], local_dir[1], intensity]);
        leak.holes.push(h);
    }

    /// LiquidContainerLeakingBehaviour.OnBeforeSerialise: SerialisableHoles = holes.ToArray().
    pub(crate) fn liquid_leak_on_before_serialise(&mut self, l: usize) {
        let s = &mut self.liquid_leaks[l];
        s.SerialisableHoles = Some(s.hole_data.clone());
    }

    /// LiquidContainerLeakingBehaviour.Start after the button: a saved leak makes its holes again; SerialisableHoles is then null.
    pub(super) fn liquid_leak_start_holes(&mut self, l: usize) {
        if self.liquid_leaks[l].HasHoles {
            for h in self.liquid_leaks[l].SerialisableHoles.clone().unwrap_or_default() {
                self.liquid_leak_create_hole(l, [h[0], h[1]], [h[2], h[3]], h[4]);
            }
        }
        self.liquid_leaks[l].SerialisableHoles = None;
    }

    /// LiquidContainerLeakingBehaviour.Repair
    pub fn liquid_leak_repair(&mut self, l: usize) {
        self.liquid_leaks[l].HasHoles = false;
        self.liquid_leaks[l].hole_data.clear();
        for h in std::mem::take(&mut self.liquid_leaks[l].holes) {
            self.destroy_object(self.liquid_holes[h].root);
        }
    }

    /// LiquidContainerHoleBehaviour.FixedUpdate. A hole without a Source (a spawned one, not CreateHole's) throws, as the game.
    pub fn liquid_hole_fixed_update(&mut self, h: usize) {
        let o = self.liquid_holes[h].Source.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let num = (super::d(self.bc(o).MeasuredPressure) * self.liquid_holes[h].ForceMultiplier + 0.5f32).f();
        let s = self.liquid_holes[h].splat;
        self.splats[s].SpawningChance = 0.0;
        // LiquidContainerHoleBehaviour.IsParticleSubmerged: water_puff_submerged.
        if !self.water_puff_submerged(self.liquid_holes[h].water_puff) {
            let fallback = self.bc(o).cachedComputedColour;
            let colour = self.get_computed_color(o, fallback);
            let splat = &mut self.splats[s];
            splat.DecalColourMultiplier = colour;
            splat.EjectionForce = [0.25f32 * num, 0.3f32 * num];
        }
        let rate = self.liquid_holes[h].DrainRate;
        if self.bc(o).TotalLiquidAmount > rate {
            self.create_splat_particle(s, 0);
            self.drain(o, rate);
        }
    }

    /// LiquidContainerHoleBehaviour.OnParticleCollide: CopyTo(DrainRate, hit's BloodContainer) unless it hit Source's object.
    pub fn liquid_hole_on_particle_collide(&mut self, h: usize, collider: i32) {
        if self.destroyed_components.contains(&self.liquid_holes[h].id) {
            return;
        }
        let o = self.liquid_holes[h].Source.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        if self.physical_of_collider(collider) == Some(self.container_physical(o)) {
            return;
        }
        if let Some(target) = self.particle_hit_container(collider) {
            self.copy_to(o, self.liquid_holes[h].DrainRate, target, true);
        }
    }
}
