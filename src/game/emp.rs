//! EMPBehaviour (the EMP Generator). Use, while enabled, runs Activate: every physical object in the world, in list
//! order, draws Random.value, and unless that draw is below DamageChance or the object lies outside Range it is sent
//! OnEMPHit. An android's limb loses all its Health (LimbBehaviour.OnEMPHit). Awake instantiates the Killzone under the
//! object (IgnoreParentSize, explosion.rs); the Effect clone is particles.
#![allow(non_snake_case)]

use super::destroyable::{AttachParent, AttachedClone};
use super::{script, Game};

script! {
    Emp {
        DamageChance: f32 = 0.25,
        Range: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// The GameObject's PhysicalBehaviour (its transform is base.transform).
        physical: usize = 0,
        /// Killzone: the clone's root Transform id.
        Killzone: i32 = 0,
        /// Effect is null: Activate's Instantiate throws an ArgumentException.
        effect_null: bool = false,
    }
}

impl Game {
    /// EMPBehaviour.Awake: Killzone = Instantiate(Prefabs/Killzone, transform.position, identity, transform), then its
    /// IgnoreParentSize.DesiredSize = 2 * Range * Vector2.one. Under the object the clone sits at local position zero
    /// with the inverse of the object's rotation.
    pub(crate) fn emp_awake(&mut self, mut db: Option<&mut crate::assets::db::AssetDb>, inst: &super::spawn::Instance, o: usize, i: usize) {
        let source = {
            let d = match db.as_deref() {
                Some(d) => d,
                None => self.assets.as_ref().expect("Instantiate without the game's data"),
            };
            d.resource("Prefabs/Killzone", 1).ok().flatten().expect("Resources.Load(Prefabs/Killzone) found no GameObject")
        };
        let id = self.spawn_prefab_with(db.as_deref_mut(), source, inst.position(o));
        let s = self.with_component_instance(id, |_, clone, x| clone.objects[x].trs.s);
        let q = inst.rotation(o);
        let local = crate::unity::transform::Trs { t: [0.0; 4], q: [-q[0], -q[1], -q[2], q[3]], s };
        let parent = AttachParent::Physical(self.emps[i].physical);
        self.attached_clones.entry(inst.objects[o].components[0]).or_default().push(AttachedClone { id, parent, local, driven: false });
        self.runtime_child_add(inst.objects[o].components[0], super::save::RuntimeKind::Clone(id), None);
        self.emps[i].Killzone = id;
        let k = self.killzone_ignore_parent_size(id);
        let r = self.emps[i].Range;
        self.ignore_parent_sizes[k].DesiredSize = [2.0 * r, 2.0 * r];
    }

    /// EMPBehaviour.SetRange(f): Range = f; Killzone's IgnoreParentSize.DesiredSize = 2 * Range * Vector2.one and its
    /// localScale = DesiredSize.
    #[allow(dead_code)]
    pub fn emp_set_range(&mut self, i: usize, f: f32) {
        self.emps[i].Range = f;
        let id = self.emps[i].Killzone;
        let k = self.killzone_ignore_parent_size(id);
        self.ignore_parent_sizes[k].DesiredSize = [2.0 * f, 2.0 * f];
        self.clone_set_local_scale(id, [2.0 * f, 2.0 * f, 0.0]);
    }

    /// EMPBehaviour.Use(activation): Activate while enabled.
    pub fn emp_use(&mut self, i: usize) {
        if self.emps[i].enabled {
            self.emp_activate(i);
        }
    }

    /// EMPBehaviour.Activate. The objects are the world's physical objects in index order (the order they entered the
    /// world); the draw comes before the test, so every object uses one draw.
    pub(crate) fn emp_activate(&mut self, i: usize) {
        if self.emps[i].effect_null {
            crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null.");
        }
        let (p, chance, range) = (self.emps[i].physical, self.emps[i].DamageChance, self.emps[i].Range);
        let position = self.physicals[p].transform.position;
        let sqr_range = range * range;
        let items: Vec<usize> = (0..self.physicals.len()).filter(|&q| self.physicals[q].in_world && !self.physicals[q].destroyed).collect();
        for item in items {
            let roll = self.random.value();
            let at = self.physicals[item].transform.position;
            let sqr = (position[0] - at[0]) * (position[0] - at[0]) + (position[1] - at[1]) * (position[1] - at[1]) + (position[2] - at[2]) * (position[2] - at[2]);
            if !(roll < chance) && sqr < sqr_range {
                self.emp_send_on_hit(item);
            }
        }
    }

    /// BulbBehaviour.OnEMPHit: the filament burns out.
    pub(crate) fn bulb_on_emp_hit(&mut self, b: usize) {
        self.bulbs[b].FilamentHealth = 0.0;
    }

    /// LimbBehaviour.OnEMPHit: an android's limb loses all its Health.
    pub(crate) fn limb_on_emp_hit(&mut self, l: usize) {
        if self.limbs[l].IsAndroid {
            self.limbs[l].Health = 0.0;
        }
    }

    /// SendMessage("OnEMPHit") on the item's GameObject, its own components only.
    pub(super) fn emp_send_on_hit(&mut self, p: usize) {
        if let Some(l) = self.limbs.iter().position(|x| x.PhysicalBehaviour == p) {
            self.limb_on_emp_hit(l);
        }
        // DamagableMachineryBehaviour.OnEMPHit on the same GameObject.
        if let Some(m) = self.machinery.iter().position(|x| x.PhysicalBehaviour == p) {
            self.machinery_on_emp_hit(m);
        }
        let id = self.physicals[p].id;
        // LaserTurretBehaviour.OnEMPHit on a turret of this GameObject: SetAsBroken (laser_turret.rs).
        let components = self.with_component_instance(id, |_, inst, o| inst.objects[o].components.clone());
        let lasers: Vec<usize> = (0..self.laser_turrets.len()).filter(|&l| components.contains(&self.laser_turrets[l].id)).collect();
        for l in lasers {
            self.laser_turret_set_as_broken(l);
        }
        // JavelinMissileBehaviour.OnEMPHit (javelin.rs).
        let missiles: Vec<usize> = (0..self.javelin_missiles.len()).filter(|&m| components.contains(&self.javelin_missiles[m].id)).collect();
        for m in missiles {
            self.javelin_missile_on_emp_hit(m);
        }
        // SentryTurretBehaviour.OnEMPHit: Explode (sentry_turret.rs).
        let sentries: Vec<usize> = (0..self.sentry_turrets.len()).filter(|&s| components.contains(&self.sentry_turrets[s].id)).collect();
        for s in sentries {
            self.sentry_turret_on_emp_hit(s);
        }
        // CarBehaviour.OnEMPHit
        for c in (0..self.cars.len()).filter(|&c| components.contains(&self.cars[c].id)).collect::<Vec<_>>() {
            self.car_on_emp_hit(c);
        }
        // BulbBehaviour.OnEMPHit
        for b in (0..self.bulbs.len()).filter(|&b| components.contains(&self.bulbs[b].id)).collect::<Vec<_>>() {
            self.bulb_on_emp_hit(b);
        }
        // CathodeLightBehaviour.OnEMPHit
        for c in (0..self.cathode_lights.len()).filter(|&c| components.contains(&self.cathode_lights[c].id)).collect::<Vec<_>>() {
            self.cathode_light_break(c);
        }
        // GlowtubeBehaviour.OnEMPHit
        for g in (0..self.glowtubes.len()).filter(|&g| components.contains(&self.glowtubes[g].id)).collect::<Vec<_>>() {
            self.glowtube_break(g);
        }
        // TelevisionBehaviour.OnEMPHit
        for t in (0..self.televisions.len()).filter(|&t| components.contains(&self.televisions[t].id)).collect::<Vec<_>>() {
            self.television_on_emp_hit(t);
        }
        // BatteryAttachmentBehaviour.OnEMPHit; without a firearm it throws and SendMessage goes on.
        for a in (0..self.firearm_attachments.len()).filter(|&a| components.contains(&self.firearm_attachments[a].id)).collect::<Vec<_>>() {
            if let (super::attachment::Kind::Battery, Some(f)) = (self.firearm_attachments[a].kind, self.firearm_attachments[a].firearm) {
                self.firearms[f].ApplyCharge = false;
            }
        }
        // StickyGrenadeBehaviour.OnEMPHit: Explosive.Disarm, ArmOnUse = false, StopAllCoroutines, light.enabled = false (the AudioSource.Stop is audio).
        for s in (0..self.sticky_grenades.len()).filter(|&s| self.sticky_grenades[s].physical == Some(p)).collect::<Vec<_>>() {
            self.sticky_grenade_on_emp_hit(s);
        }
    }

    /// StickyGrenadeBehaviour.OnEMPHit: Explosive.Disarm, ArmOnUse = false, StopAllCoroutines, light.enabled = false (the AudioSource.Stop is audio).
    pub(crate) fn sticky_grenade_on_emp_hit(&mut self, s: usize) {
        let e = self.sticky_grenades[s].explosive;
        self.explosive_disarm(e);
        self.explosives[e].ArmOnUse = false;
        let id = self.sticky_grenades[s].id;
        self.stop_all_coroutines(id);
        let light = self.sticky_grenades[s].light.expect("StickyGrenadeBehaviour.OnEMPHit: light null (NullReferenceException)");
        self.set_sprite_enabled(light, false);
    }
}
