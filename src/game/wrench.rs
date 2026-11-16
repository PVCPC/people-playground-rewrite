//! WrenchBehaviour's OnCollisionEnter2D: a hit at or over MinImpulse draws a 1 in 4 roll for SendMessageUpwards("Repair")
//! to the hit object and its ancestors. The limb heal is only for androids. Rewritten: the draw and the no-receiver,
//! non-android path. A Repair receiver anywhere above the hit, a bare collider, and an android limb stop with
//! unimplemented!, since their repairs are not rewritten.
#![allow(non_snake_case)]

use crate::b2::contacts2d::ScriptCollision;
use super::{d, mathf, script, Behaviour, Game};
use super::container::Owner;
use crate::assets::db::Component;

script! {
    Wrench {
        MinImpulse: f32 = 3.0,
        HealingPower: f32 = 0.2,
        DamagePointHealingPower: f32 = 0.2,
        BulletHoleHealingPower: f32 = 0.2,
        BurnProgressHealingPower: f32 = 0.2,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour: the collisions it gets are its physical's.
        physical: usize = 0,
        /// The component's index on its GameObject, and whether PhysicalBehaviour comes before it (collision order).
        index: usize = 0,
        after_physical: bool = false,
    }
}

/// The classes with a Repair() method (Messages.IRepair): SendMessageUpwards("Repair") reaches them.
const REPAIR_RECEIVERS: &[&str] = &["CathodeLightBehaviour", "DamagableMachineryBehaviour", "LiquidContainerLeakingBehaviour", "TelevisionBehaviour", "TyreDeflationBehaviour"];

impl Game {
    /// WrenchBehaviour.OnCollisionEnter2D
    pub(crate) fn wrench_on_collision_enter(&mut self, w: usize, c: &ScriptCollision) {
        if !self.wrenches[w].enabled {
            return;
        }
        let first = c.contacts.first().unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array."));
        if first.normal_impulse < self.wrenches[w].MinImpulse {
            return;
        }
        if self.random.value() > 0.75 {
            self.wrench_send_repair_upwards(c.collider);
        }
        // collision.transform.GetComponent<LimbBehaviour>(): an android's limb is healed (not rewritten).
        if let Some(p) = self.physical_of_collider(c.collider) {
            if let Some(l) = self.limbs.iter().position(|l| l.PhysicalBehaviour == p) {
                if self.limbs[l].IsAndroid {
                    self.wrench_heal_android(w, l);
                }
            }
        }
    }

    /// WrenchBehaviour.OnCollisionEnter2D, the android LimbBehaviour half (SkinMaterialHandler.Sync is rendering).
    pub(crate) fn wrench_heal_android(&mut self, w: usize, l: usize) {
        let (healing, point, burn) = (self.wrenches[w].HealingPower, self.wrenches[w].DamagePointHealingPower, self.wrenches[w].BurnProgressHealingPower);
        self.limbs[l].Health = mathf::lerp(self.limbs[l].Health, self.limbs[l].InitialHealth, healing);
        self.limb_heal_bone(l);
        self.heal_bleeding(l);
        self.clear_liquid(Owner::Circulation(l));
        let blood = self.blood_type(l);
        self.add_liquid(Owner::Circulation(l), blood, 1.0);
        let c = &mut self.circulations[l];
        c.BloodFlow = 1.0;
        c.IsPump = c.WasInitiallyPumping;
        let person = self.limbs[l].Person;
        if self.limbs[l].HasBrain {
            self.people[person].Braindead = false;
            self.people[person].BrainDamaged = false;
        }
        self.people[person].Consciousness = mathf::lerp(self.people[person].Consciousness, 1.0, 0.5);
        let p = self.limbs[l].PhysicalBehaviour;
        self.physicals[p].BurnProgress = (d(self.physicals[p].BurnProgress) * (d(1.0) - burn)).f();
        for p in self.skins[l].damagePoints.iter_mut() {
            p[2] = (d(p[2]) - point).f();
        }
    }

    /// SendMessageUpwards("Repair") from the hit collider's object: each Repair receiver from the object up through its
    /// ancestors, in component order, gets Repair() (disabled ones too). Every receiver class (REPAIR_RECEIVERS)
    /// is rewritten.
    fn wrench_send_repair_upwards(&mut self, collider: i32) {
        let Some(p) = self.physical_of_collider(collider) else {
            unimplemented!("mod-only: WrenchBehaviour: SendMessageUpwards Repair from a collider without a PhysicalBehaviour (its ancestors are not checked)");
        };
        let id = self.physicals[p].id;
        let receivers = self.with_component_instance(id, |_, inst, o| {
            let mut out = Vec::new();
            let mut x = Some(o);
            while let Some(i) = x {
                for (k, (_, comp)) in inst.prefab.objects[i].components.iter().enumerate() {
                    if let Component::Script { class, .. } = comp {
                        if REPAIR_RECEIVERS.contains(&class.as_str()) {
                            out.push((class.clone(), inst.objects[i].components[k]));
                        }
                    }
                }
                x = inst.objects[i].parent;
            }
            out
        });
        // SendMessageUpwards invokes each receiver on its own: an exception in one is logged and the next still runs.
        for (class, cid) in receivers {
            self.unity_callback(|g| match g.behaviour_of.get(&cid).map(|&(_, b)| b) {
                Some(Behaviour::Television(t)) => g.television_repair(t),
                Some(Behaviour::Machinery(m)) => g.machinery_repair(m),
                Some(Behaviour::Tyre(t)) => g.tyre_repair(t),
                Some(Behaviour::CathodeLight(c)) => g.cathode_light_repair(c),
                _ if class == "LiquidContainerLeakingBehaviour" => {
                    let l = g.liquid_leaks.iter().position(|x| x.id == cid).expect("the object's LiquidContainerLeakingBehaviour");
                    g.liquid_leak_repair(l);
                }
                _ => unimplemented!("mod-only: WrenchBehaviour: SendMessageUpwards Repair to {class}.Repair (not rewritten)"),
            });
        }
    }
}
