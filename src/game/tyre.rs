//! TyreDeflationBehaviour and the native CircleCollider2D.radius / Collider2D.offset setters it calls.
#![allow(non_snake_case)]

use super::spawn::Instance;
use super::{script, Game};
use crate::assets::components::ColliderShape;
use crate::assets::db::Component;

/// DeflationParticlePrefabPath
const PARTICLE_PREFAB: &str = "Prefabs/Tyre deflation effect poolable";

script! {
    TyreDeflationBehaviour {
        Deflated: bool = false,
        Indestructible: bool = false,
        TemperatureThreshold: f32 = 398.0,
        ShotDamageThreshold: f32 = 8.0,
        FragmentForceThreshold: f32 = 15.0,
        VisualSagIntensity: f32 = 5.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Phys
        Phys: usize = 0,
        /// Collider as (object, component) of its clone.
        collider: (usize, usize) = (0, 0),
        /// DeflatedColliderSettings
        DeflatedRadius: f32 = 0.0,
        DeflatedOffset: [f32; 2] = [0.0; 2],
        /// initial, set by Awake.
        initialRadius: f32 = 0.0,
        initialOffset: [f32; 2] = [0.0; 2],
        /// TyreDeflationSound.Length
        sounds: usize = 0,
        /// pooledDeflationParticles as (`Game::splash_pools` index, item).
        pooled: Option<(usize, usize)> = None,
        SharedDeflatedMaterial: Option<crate::assets::db::ObjRef> = None,
        /// previousMat and props, set by Start.
        previousMat: Option<crate::assets::db::ObjRef> = None,
        props: super::props::PropBlock = Default::default(),
        sagVector: [f32; 2] = [0.0; 2],
    }
}

/// The circle a CircleCollider2D currently has: its radius and offset.
pub(super) fn circle_of(inst: &Instance, (o, c): (usize, usize)) -> (f32, [f32; 2]) {
    if let Some(&x) = inst.circles.get(&(o, c)) {
        return x;
    }
    let Component::Collider2D(col) = &inst.prefab.objects[o].components[c].1 else { unreachable!() };
    let ColliderShape::Circle { radius } = col.shape else { unreachable!("TyreDeflationBehaviour.Collider is a CircleCollider2D") };
    (radius, col.offset)
}

impl Game {
    /// CircleCollider2D.radius = v (CircleCollider2D::SetRadius 180c15c30): unchanged within 1e-6, else clamped to
    /// 0.0001..1e6 and the collider re-created.
    pub(super) fn circle_set_radius(&mut self, inst: &mut Instance, key: (usize, usize), v: f32) {
        let (radius, offset) = circle_of(inst, key);
        if 0.000_001f32 >= (radius - v).abs() {
            return;
        }
        let v = if 0.0001f32 > v { 0.0001 } else { f32::min(1_000_000.0, v) };
        inst.circles.insert(key, (v, offset));
        self.circle_recreate(inst, key);
    }

    /// Collider2D.offset = v (Collider2D::SetOffset 180c159e0): unchanged within 1e-5, else stored and the
    /// collider re-created.
    fn circle_set_offset(&mut self, inst: &mut Instance, key: (usize, usize), v: [f32; 2]) {
        let (radius, offset) = circle_of(inst, key);
        let (dx, dy) = (v[0] - offset[0], v[1] - offset[1]);
        let eps = 0.000_01f32;
        if eps * eps >= dx * dx + dy * dy {
            return;
        }
        inst.circles.insert(key, (radius, v));
        self.circle_recreate(inst, key);
    }

    /// The RecreateCollider(null) the setters end with; a collider of an inactive object has no fixtures to remake.
    fn circle_recreate(&mut self, inst: &mut Instance, (o, c): (usize, usize)) {
        if inst.objects[o].active {
            let prefab = inst.prefab.clone();
            self.collider2d_recreate(inst, &prefab, o, c);
        }
    }

    // TyreDeflationBehaviour.Unstabbed: stab.rs sends it as tyre_pop(Some(stabbing.point)).
    // TyreDeflationBehaviour.Break: ballistics.rs's Break message is tyre_pop(None).
    /// TyreDeflationBehaviour.Update: a deflated tyre with its material eases sagVector (SmoothApproach, speed 12) toward the
    /// first contact's local point (Rigidbody2D.GetContacts into a 1-entry buffer) times VisualSagIntensity times how far
    /// that point sits inside the initial radius, or toward zero without contacts. Utils.DrawCross is editor-only.
    pub fn tyre_update(&mut self, t: usize) {
        let s = &self.tyres[t];
        if !s.Deflated || s.SharedDeflatedMaterial.is_none() {
            return;
        }
        let p = s.Phys;
        let dt = self.time.deltaTime;
        let target = match self.rigidbody_get_contacts(p).first() {
            None => [0.0; 2],
            Some(c) => {
                let (pp, pq, ls) = self.anchored_world(super::BareAnchor::Physical(p), &[]);
                let inv = bevy::math::Quat::from_array(pq).inverse();
                let d = inv * bevy::math::Vec3::new(c.point.x - pp[0], c.point.y - pp[1], 0.0 - pp[2]);
                let v = [d.x / ls[0], d.y / ls[1], if ls[2] == 0.0 { 0.0 } else { d.z / ls[2] }];
                let magnitude = ((v[0] as f64).powi(2) + (v[1] as f64).powi(2) + (v[2] as f64).powi(2)).sqrt() as f32;
                let num = (super::d(self.tyres[t].initialRadius) - magnitude).f().max(0.0);
                let k = (super::d(self.tyres[t].VisualSagIntensity) * num).f();
                [(super::d(v[0]) * k).f(), (super::d(v[1]) * k).f()]
            }
        };
        let past = self.tyres[t].sagVector;
        let e = (-((dt * 12.0) as f64)).exp() as f32;
        let v = [0, 1].map(|i| ((super::d(past[i]) - target[i]) * e + super::d(target[i])).f());
        self.tyres[t].sagVector = if v.iter().any(|x| x.is_nan()) { [0.0; 2] } else { v };
    }

    /// TyreDeflationBehaviour.OnWillRenderObject: a deflated tyre with its material sets `_Direction` = sagVector on props and
    /// hands the renderer the block.
    pub fn tyres_will_render(&mut self) {
        for t in 0..self.tyres.len() {
            let s = &self.tyres[t];
            if !self.will_render_dispatch(s.id, !self.physicals[s.Phys].destroyed) || !s.Deflated || s.SharedDeflatedMaterial.is_none() {
                continue;
            }
            let mut block = std::mem::take(&mut self.tyres[t].props);
            let v = self.tyres[t].sagVector;
            block.set_vector("_Direction", [v[0], v[1], 0.0, 0.0]);
            self.set_property_block(super::visual::SpriteAt::Physical(self.tyres[t].Phys), &block);
            self.tyres[t].props = block;
        }
    }

    /// Phys.spriteRenderer.material = `m`.
    fn tyre_set_material(&mut self, t: usize, m: Option<crate::assets::db::ObjRef>) {
        if self.tyres[t].SharedDeflatedMaterial.is_some() {
            let p = self.tyres[t].Phys;
            self.physicals[p].sprite_material = m;
        }
    }

    /// TyreDeflationBehaviour.Awake
    pub fn tyre_awake(&mut self, t: usize, inst: &Instance) {
        let (radius, offset) = circle_of(inst, self.tyres[t].collider);
        self.tyres[t].initialRadius = radius;
        self.tyres[t].initialOffset = offset;
    }

    /// TyreDeflationBehaviour.Repair: Deflated = false; Collider.radius = initial.Radius; Collider.offset = initial.Offset;
    /// with SharedDeflatedMaterial, the sprite takes previousMat back.
    pub fn tyre_repair(&mut self, t: usize) {
        self.tyres[t].Deflated = false;
        self.tyre_set_material(t, self.tyres[t].previousMat);
        let s = &self.tyres[t];
        let (id, key, radius, offset) = (s.id, s.collider, s.initialRadius, s.initialOffset);
        self.with_component_instance(id, |g, inst, _| {
            g.circle_set_radius(inst, key, radius);
            g.circle_set_offset(inst, key, offset);
        });
    }

    /// TyreDeflationBehaviour.Start: previousMat = the sprite's sharedMaterial, props = its property block; a Deflated tyre
    /// takes the deflated circle and material; then the context menu buttons.
    pub fn tyre_start(&mut self, t: usize) {
        let p = self.tyres[t].Phys;
        self.tyres[t].previousMat = self.physicals[p].sprite_material;
        self.tyres[t].props = self.get_property_block(super::visual::SpriteAt::Physical(p));
        if self.tyres[t].Deflated {
            self.tyre_deflated_circle(t);
            self.tyre_set_material(t, self.tyres[t].SharedDeflatedMaterial);
        }
        use super::menu::{Cond, Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.tyres[t].Phys].menu;
        menu.push(B { condition: Cond::TyreDeflated(t), ..B::new("repairMachinery", "Repair", crate::il!(str "TyreDeflationBehaviour::Start" 2), A::TyreRepair(t)) });
        menu.push(B { label: Label::TyreIndestructible(t), ..B::new("machineryIndestructible", "", crate::il!(str "TyreDeflationBehaviour::Start" 4), A::TyreToggle(t)) });
    }

    /// Collider.radius = DeflatedColliderSettings.Radius; Collider.offset = DeflatedColliderSettings.Offset;
    fn tyre_deflated_circle(&mut self, t: usize) {
        let s = &self.tyres[t];
        let (id, key, radius, offset) = (s.id, s.collider, s.DeflatedRadius, s.DeflatedOffset);
        self.with_component_instance(id, |g, inst, _| {
            g.circle_set_radius(inst, key, radius);
            g.circle_set_offset(inst, key, offset);
        });
    }

    /// TyreDeflationBehaviour.FixedUpdate
    pub fn tyre_fixed_update(&mut self, t: usize) {
        let s = &self.tyres[t];
        let p = &self.physicals[s.Phys];
        if !s.Indestructible && !s.Deflated && p.SimulateTemperature && p.Temperature >= s.TemperatureThreshold {
            let (position, key, id) = (p.transform.position, s.collider, s.id);
            let radius = self.with_component_instance(id, |_, inst, _| circle_of(inst, key).0);
            let c = self.random.inside_unit_circle();
            self.tyre_pop(t, Some([position[0] + c[0] * radius, position[1] + c[1] * radius]));
        }
    }

    /// TyreDeflationBehaviour.PopTyre
    pub fn tyre_pop(&mut self, t: usize, point: Option<[f32; 2]>) {
        let s = &self.tyres[t];
        if s.Indestructible || s.Deflated {
            return;
        }
        if point.is_some() {
            // Collider.ClosestPoint(point) starts with the AutoSyncTransforms; the effect's pose (the closest point,
            // right = away from the tyre, parent = the tyre) is rendering.
            self.sync_pending();
            let mut db = self.assets.take().expect("TyreDeflationBehaviour.PopTyre without the game's data");
            let pool = db.resource(PARTICLE_PREFAB, 1).and_then(|r| r.ok_or_else(|| format!("Resources.Load({PARTICLE_PREFAB}) found no GameObject"))).and_then(|r| self.splash_pool_for(&mut db, r));
            self.assets = Some(db);
            let pool = pool.unwrap_or_else(|e| panic!("TyreDeflationBehaviour.PopTyre: {e}"));
            if !self.splash_pools[pool].poolable {
                unimplemented!("mod-only: TyreDeflationBehaviour.PopTyre: a deflation effect prefab not tagged Poolable");
            }
            let Some(item) = self.splash_request(pool) else { crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
            self.tyres[t].pooled = Some((pool, item));
            // At Collider.ClosestPoint(point), transform.right = (that - tyre position).normalized, then
            // SetParent(tyre transform) with the world pose kept.
            let ph = self.tyres[t].Phys;
            let (id, key) = (self.tyres[t].id, self.tyres[t].collider);
            let collider = self.with_component_instance(id, |_, inst, _| inst.objects[key.0].components[key.1]);
            let q = point.unwrap();
            let at = self.world.collider_closest_point(collider, crate::b2::math::Vec2::new(q[0], q[1]));
            let at = [at.x, at.y];
            let c = self.physicals[ph].transform.position;
            let z = (super::d(crate::game::mathf::atan2((at[1] - c[1]), (at[0] - c[0]))) * 57.29578f32).f();
            let rotation = super::physical::z_rotation(z);
            let source = self.splash_pools[pool].source;
            self.log_effect(source, [at[0], at[1], 0.0], rotation, 1.0, None, None);
            self.tag_pooled(super::Pooled::Splash(pool, item));
            if !self.effects_off {
                let (pp, pq, ls) = self.anchored_world(super::BareAnchor::Physical(ph), &[]);
                let inv = bevy::math::Quat::from_array(pq).inverse();
                let d = inv * bevy::math::Vec3::new(at[0] - pp[0], at[1] - pp[1], 0.0 - pp[2]);
                let local = [d.x / ls[0], d.y / ls[1], if ls[2] == 0.0 { 0.0 } else { d.z / ls[2] }];
                let turn = (inv * bevy::math::Quat::from_array(rotation)).to_array();
                self.effect_log.last_mut().expect("the clone's effect").parent = Some(super::EffectParent { physical: ph, position: local, rotation: turn, sprite_shape: false });
            }
        }
        // Phys.PlayClipOnce(TyreDeflationSound.PickRandom()).
        let clip = self.pick_clip(self.tyres[t].id, "TyreDeflationSound", self.tyres[t].sounds);
        self.play_clip_once(self.tyres[t].Phys, clip, 1.0);
        self.tyre_deflated_circle(t);
        self.tyres[t].Deflated = true;
        self.tyre_set_material(t, self.tyres[t].SharedDeflatedMaterial);
    }

    /// TyreDeflationBehaviour.Shot
    pub fn tyre_shot(&mut self, t: usize, damage: f32, point: [f32; 2]) {
        let s = &self.tyres[t];
        if !s.Indestructible && damage >= s.ShotDamageThreshold {
            self.tyre_pop(t, Some(point));
        }
    }

    /// TyreDeflationBehaviour.OnFragmentHit
    pub fn tyre_on_fragment_hit(&mut self, t: usize, force: f32) {
        let s = &self.tyres[t];
        if !s.Indestructible && force >= s.FragmentForceThreshold {
            self.tyre_pop(t, None);
        }
    }

    /// TyreDeflationBehaviour.OnDestroy: the effect leaves the tyre and returns to its pool.
    pub fn tyre_destroy(&mut self, cid: i32) {
        let Some(t) = self.tyres.iter().position(|x| x.id == cid) else { return };
        let Some((pool, item)) = self.tyres[t].pooled else { return };
        // (bool)pooledDeflationParticles: the pool's Clear destroyed it.
        if self.splash_pools[pool].items[item].destroyed {
            return;
        }
        // ObjectPoolBehaviour.Return: SetActive(false), availableObjects.Push.
        let p = &mut self.splash_pools[pool];
        p.items[item].active = false;
        p.pool.available.push(item);
        self.pooled_deactivated.push(super::Pooled::Splash(pool, item));
    }
}
