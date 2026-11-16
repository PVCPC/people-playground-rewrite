//! The old PoisonSpreadBehaviour family ([Obsolete]; OldSyringeBehaviour.Lodged adds one to the stabbed limb through
//! `poison_add`): AcidPoison, AdrenalinePoison, CoagulationPoison, DeathPoison, FreezePoison, KnockoutPoison,
//! LifePoison, PinkPoison, SlowZombiePoison, UltraStrengthPoison and ZombiePoison. Each is a component on a limb's
//! GameObject: Start applies its effect, Update counts LifeTime, spreads once to the Source and PushesTo limbs after
//! 1 / SpreadSpeed seconds, and destroys itself after Lifespan.
#![allow(non_snake_case)]

use super::container::Owner;
use super::coroutine::{Routine, Yield};
use super::explosion::{EffectSize, ExplosionParameters};
use super::{d, mathf, Behaviour, Game};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PoisonKind {
    #[default]
    Acid,
    Adrenaline,
    Coagulation,
    Death,
    Freeze,
    Knockout,
    Life,
    Pink,
    SlowZombie,
    UltraStrength,
    Zombie,
}

impl PoisonKind {
    /// The class name.
    pub fn class(self) -> &'static str {
        match self {
            PoisonKind::Acid => "AcidPoison",
            PoisonKind::Adrenaline => "AdrenalinePoison",
            PoisonKind::Coagulation => "CoagulationPoison",
            PoisonKind::Death => "DeathPoison",
            PoisonKind::Freeze => "FreezePoison",
            PoisonKind::Knockout => "KnockoutPoison",
            PoisonKind::Life => "LifePoison",
            PoisonKind::Pink => "PinkPoison",
            PoisonKind::SlowZombie => "SlowZombiePoison",
            PoisonKind::UltraStrength => "UltraStrengthPoison",
            PoisonKind::Zombie => "ZombiePoison",
        }
    }

    /// PoisonSpreadBehaviour.get_SpreadSpeed, AcidPoison.get_SpreadSpeed, AdrenalinePoison.get_SpreadSpeed, DeathPoison.get_SpreadSpeed, PinkPoison.get_SpreadSpeed, UltraStrengthPoison.get_SpreadSpeed, ZombiePoison.get_SpreadSpeed, SlowZombiePoison.get_SpreadSpeed (1 unless overridden).
    pub fn spread_speed(self) -> f32 {
        match self {
            PoisonKind::Acid => 5.0,
            PoisonKind::Adrenaline => 20.0,
            PoisonKind::Death => 10.0,
            PoisonKind::Pink => 1.4,
            PoisonKind::UltraStrength => 1.4,
            PoisonKind::Zombie => 500.0,
            _ => 1.0,
        }
    }

    /// PoisonSpreadBehaviour.get_Lifespan, AcidPoison.get_Lifespan, KnockoutPoison.get_Lifespan (5 unless overridden).
    pub fn lifespan(self) -> f32 {
        match self {
            PoisonKind::Acid => 60.0,
            _ => 5.0,
        }
    }

    /// GetComponent(typeof(requested)) finds this kind: the same class, or a subclass (SlowZombiePoison is a ZombiePoison).
    fn is_a(self, requested: PoisonKind) -> bool {
        self == requested || (requested == PoisonKind::Zombie && self == PoisonKind::SlowZombie)
    }
}

/// A PoisonSpreadBehaviour component.
#[derive(Default, Clone, Debug)]
pub struct Poison {
    pub id: i32,
    pub kind: PoisonKind,
    pub Spreads: bool,
    /// LimbBehaviour (`Game::limbs` index).
    pub Limb: usize,
    pub LifeTime: f32,
    pub HasSpread: bool,
    /// Fingerprint (None: null, as an AddComponent'd poison nobody gave one).
    pub Fingerprint: Option<Vec<f32>>,
    pub fingerprintReadPos: usize,
    /// ZombiePoison.FillBlood
    pub FillBlood: bool,
}

impl Poison {
    /// The kind's class constructed: PoisonSpreadBehaviour's Spreads = true and ZombiePoison's FillBlood = true.
    pub fn new(kind: PoisonKind) -> Self {
        Poison { kind, Spreads: true, FillBlood: true, ..Default::default() }
    }
}

impl Game {
    /// PoisonSpreadBehaviour..ctor, ZombiePoison..ctor: limb.gameObject.AddComponent<kind>() with Limb = limb (the callers set Fingerprint): a new instance id, Update from
    /// the next frame, Start with the pending Starts. Returns the `Game::poisons` index.
    pub fn poison_add(&mut self, limb: usize, kind: PoisonKind) -> usize {
        let id = self.allocate_instance_id();
        self.poisons.push(Poison { id, Limb: limb, ..Poison::new(kind) });
        let i = self.poisons.len() - 1;
        let b = Behaviour::Poison(i);
        self.behaviour_of.insert(id, (0, b));
        self.behaviours.add(0, b);
        self.pending_starts.push(super::misc::PendingStart::Poison(i));
        i
    }

    /// PoisonSpreadBehaviour.GetFingerprintValue
    pub(crate) fn poison_fingerprint_value(&mut self, i: usize) -> f32 {
        let x = &mut self.poisons[i];
        // An AddComponent'd poison nobody gave a Fingerprint (only Poison's OnPoisonOther copies one): the game's float[] is null.
        let Some(fp) = x.Fingerprint.as_ref() else {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        };
        let Some(&result) = fp.get(x.fingerprintReadPos) else {
            crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
        };
        x.fingerprintReadPos += 1;
        if x.fingerprintReadPos >= fp.len() {
            x.fingerprintReadPos = 0;
        }
        result
    }

    /// AcidPoison.Start, AdrenalinePoison.Start, CoagulationPoison.Start, DeathPoison.Start, FreezePoison.Start, KnockoutPoison.Start, LifePoison.Start, PinkPoison.Start, UltraStrengthPoison.Start, UltraStrengthPoison.AddStrengthBonus, ZombiePoison.Start: the Start of every kind (PoisonSpreadBehaviour.Start is abstract).
    pub fn poison_start(&mut self, i: usize) {
        let (l, kind) = (self.poisons[i].Limb, self.poisons[i].kind);
        let head = self.limbs[l].RoughClassification == 0;
        let person = self.limbs[l].Person;
        match kind {
            // AcidPoison.Start
            PoisonKind::Acid => {
                if !(self.limbs[l].SpeciesIdentity == "Gorse") {
                    self.poison_acid(i);
                }
            }
            // AdrenalinePoison.Start
            PoisonKind::Adrenaline => {
                if head {
                    let p = &mut self.people[person];
                    (p.AdrenalineLevel, p.Consciousness, p.PainLevel, p.ShockLevel) = (20.0, 1.0, 0.0, 0.0);
                }
            }
            // CoagulationPoison.Start
            PoisonKind::Coagulation => self.heal_bleeding(l),
            // DeathPoison.Start
            PoisonKind::Death => self.circulations[l].IsPump = false,
            // FreezePoison.Start
            PoisonKind::Freeze => self.limbs[l].Frozen = true,
            // KnockoutPoison.Start
            PoisonKind::Knockout => {
                self.limbs[l].Numbness = 1.0;
                if head {
                    self.people[person].Consciousness = 0.0;
                }
            }
            // LifePoison.Start
            PoisonKind::Life => {
                self.stop_all_coroutines(self.poisons[i].id);
                self.poison_general_healing(l);
                self.poison_heal_burn(i, l);
                self.poison_heal_wounds(i, l);
                self.poison_heal_rotten_flesh(i, l);
                self.poison_heal_acid_flesh(i, l);
            }
            PoisonKind::Pink => self.poison_pink(i),
            // ZombiePoison.Start (SlowZombiePoison inherits it)
            PoisonKind::Zombie | PoisonKind::SlowZombie => {
                self.limbs[l].IsZombie = true;
                self.limbs[l].Health = self.limbs[l].InitialHealth;
                self.limbs[l].Numbness = 0.0;
                if self.poisons[i].FillBlood {
                    let blood = self.blood_type(l);
                    self.add_liquid(Owner::Circulation(l), blood, 5.0);
                    self.circulations[l].BloodFlow = 5.0;
                }
                self.heal_bleeding(l);
                self.circulations[l].BleedingRate = 0.0;
                self.circulations[l].IsPump = self.circulations[l].WasInitiallyPumping;
                if head {
                    let p = &mut self.people[person];
                    (p.Consciousness, p.ShockLevel, p.PainLevel, p.OxygenLevel, p.AdrenalineLevel) = (1.0, 0.0, 0.0, 1.0, 1.0);
                }
            }
            // UltraStrengthPoison.Start, then AddStrengthBonus
            PoisonKind::UltraStrength => {
                let limb = &mut self.limbs[l];
                limb.Vitality *= 0.1;
                limb.RegenerationSpeed += 5.0;
                limb.ImpactPainMultiplier *= 0.1;
                limb.InitialHealth += 150.0;
                limb.Health = limb.InitialHealth;
                limb.BreakingThreshold += 10.0;
                limb.BaseStrength = mathf::min(15.0, limb.BaseStrength + 5.0);
                limb.ShotDamageMultiplier *= 0.1;
                self.heal_bleeding(l);
                self.limb_wince(l, 10.0);
            }
        }
    }

    /// PoisonSpreadBehaviour.Update, PoisonSpreadBehaviour.DestroyIfExpired
    pub fn poison_update(&mut self, i: usize) {
        let (id, l) = (self.poisons[i].id, self.poisons[i].Limb);
        // The limb's GameObject was destroyed: the component goes with it.
        if self.physicals[self.limbs[l].PhysicalBehaviour].destroyed {
            if self.destroyed_components.insert(id) {
                self.poison_destroy(i);
            }
            return;
        }
        self.poisons[i].LifeTime += self.time.deltaTime;
        self.poison_spread(i);
        // DestroyIfExpired
        if self.poisons[i].LifeTime > self.poisons[i].kind.lifespan() {
            self.tr_destroy_component(id, 0.0);
        }
    }

    /// PoisonSpreadBehaviour.Spread
    pub(crate) fn poison_spread(&mut self, i: usize) {
        let (l, kind) = (self.poisons[i].Limb, self.poisons[i].kind);
        if self.poisons[i].HasSpread || !self.poisons[i].Spreads || !(self.poisons[i].LifeTime > 1.0 / mathf::max(0.0001, kind.spread_speed())) {
            return;
        }
        // shouldPoison: !cir.PhysicalBehaviour.isDisintegrated
        let should = |g: &Game, o: usize| !g.physicals[g.limbs[o].PhysicalBehaviour].isDisintegrated;
        let alive = |g: &Game, o: usize| !g.physicals[g.limbs[o].PhysicalBehaviour].destroyed;
        if let Some(s) = self.circulations[l].Source.filter(|&s| alive(self, s)) {
            if !self.circulations[l].IsDisconnected && should(self, s) {
                self.poison_poison(i, s);
            }
        }
        for o in self.circulations[l].PushesTo.clone() {
            if alive(self, o) && !self.circulations[o].IsDisconnected && should(self, o) {
                self.poison_poison(i, o);
            }
        }
        self.poisons[i].HasSpread = true;
    }

    /// PoisonSpreadBehaviour.Poison, PoisonSpreadBehaviour.get_AllowMultipleActivations, PoisonSpreadBehaviour.OnPoisonOther: the limb's own component of this class gets Start again (AllowMultipleActivations is
    /// true everywhere), else a new one takes the limb and the Fingerprint (OnPoisonOther is empty everywhere).
    pub fn poison_poison(&mut self, i: usize, limb: usize) {
        let kind = self.poisons[i].kind;
        let existing = (0..self.poisons.len()).find(|&k| self.poisons[k].Limb == limb && self.poisons[k].kind.is_a(kind) && !self.destroyed_components.contains(&self.poisons[k].id));
        match existing {
            None => {
                let n = self.poison_add(limb, kind);
                self.poisons[n].Fingerprint = self.poisons[i].Fingerprint.clone();
            }
            Some(k) if poison_allow_multiple_activations() => self.poison_start(k),
            Some(_) => {}
        }
    }

    /// StartCoroutine(WaitAndDelete(seconds)) on the component.
    pub fn poison_wait_and_delete(&mut self, i: usize, seconds: f32) {
        let id = self.poisons[i].id;
        let go = self.circulation_id(self.poisons[i].Limb);
        self.start_coroutine(id, go, Self::poison_wait_and_delete_routine(i, seconds));
    }

    /// PoisonSpreadBehaviour.WaitAndDelete's IEnumerator (unstarted): Destroy(this) after `seconds`. Its first MoveNext takes
    /// poison `i`'s component (`this`).
    pub(crate) fn poison_wait_and_delete_routine(i: usize, seconds: f32) -> Routine {
        let mut id = None;
        Box::new(move |g| {
            let Some(id) = id else {
                id = Some(g.poisons[i].id);
                return Yield::WaitForSeconds(seconds);
            };
            g.tr_destroy_component(id, 0.0);
            Yield::Done
        })
    }

    /// The delayed Destroy of a poison (false: `id` is not one): its coroutines stop and it leaves the update loop.
    pub fn poison_run_destroy(&mut self, id: i32) -> bool {
        let Some(i) = self.poisons.iter().position(|x| x.id == id) else { return false };
        self.component_destroys.retain(|&(_, k)| k != id);
        if self.destroyed_components.insert(id) {
            self.poison_destroy(i);
        }
        true
    }

    fn poison_destroy(&mut self, i: usize) {
        let id = self.poisons[i].id;
        self.stop_all_coroutines(id);
        if let Some((_, b)) = self.behaviour_of.remove(&id) {
            self.behaviours.remove(b);
        }
    }

    /// AcidPoison.Acid
    pub(crate) fn poison_acid(&mut self, i: usize) {
        let (id, l) = (self.poisons[i].id, self.poisons[i].Limb);
        let go = self.circulation_id(l);
        self.start_coroutine(id, go, Box::new(move |g| g.acid_step(l)));
    }

    /// One MoveNext of AcidPoison.Acid on limb `l`.
    pub(crate) fn acid_step(&mut self, l: usize) -> Yield {
        if !(self.skins[l].acidProgress < 0.99) {
            self.limbs[l].Health = 0.0;
            self.circulations[l].IsPump = false;
            self.force_set_all_liquid(Owner::Circulation(l), 0.0);
            self.limbs[l].BreakingThreshold = 0.0;
            return Yield::Done;
        }
        let acid = (d(self.skins[l].acidProgress) + d(self.time.deltaTime) * 0.2f32).f();
        self.skins[l].set_acid(acid);
        let intensity = (d(self.skins[l].acidProgress) * 150.0f32).f();
        self.limb_wince(l, intensity);
        if self.skins[l].acidProgress > 0.5 {
            self.break_bone(l);
        }
        Yield::WaitForSeconds(0.03)
    }

    /// LifePoison.GeneralHealing
    pub(crate) fn poison_general_healing(&mut self, l: usize) {
        self.limb_heal_bone(l);
        self.limbs[l].Health = self.limbs[l].InitialHealth;
        self.limbs[l].Numbness = 0.0;
        self.heal_bleeding(l);
        self.circulations[l].IsPump = self.circulations[l].WasInitiallyPumping;
        self.circulations[l].BloodFlow = 5.0;
        let blood = self.blood_type(l);
        self.add_liquid(Owner::Circulation(l), blood, 5.0);
        self.limbs[l].BruiseCount = 0;
        self.circulations[l].GunshotWoundCount = 0;
        self.circulations[l].StabWoundCount = 0;
        if self.limbs[l].RoughClassification == 0 {
            let p = &mut self.people[self.limbs[l].Person];
            (p.Consciousness, p.ShockLevel, p.PainLevel, p.OxygenLevel, p.AdrenalineLevel) = (1.0, 0.0, 0.0, 1.0, 1.0);
        }
    }

    /// LifePoison.HealBurn
    pub(crate) fn poison_heal_burn(&mut self, i: usize, l: usize) {
        let r = self.heal_burn_routine(l);
        let (id, go) = (self.poisons[i].id, self.circulation_id(l));
        self.start_coroutine(id, go, r);
    }
    /// LifePoison.HealBurn's iterator (fn-check runs its first MoveNext).
    pub(crate) fn heal_burn_routine(&self, l: usize) -> Routine {
        let p = self.limbs[l].PhysicalBehaviour;
        let mut first = true;
        Box::new(move |g| {
            if std::mem::take(&mut first) {
                g.physicals[p].Temperature = g.limbs[l].BodyTemperature;
            }
            if g.physicals[p].BurnProgress > 0.1 {
                g.physicals[p].BurnProgress = (d(g.physicals[p].BurnProgress) - d(g.time.deltaTime) * 0.16f32).f();
                return Yield::WaitForEndOfFrame;
            }
            g.physicals[p].BurnProgress = 0.0;
            Yield::Done
        })
    }

    /// LifePoison.HealAcidFlesh
    pub(crate) fn poison_heal_acid_flesh(&mut self, i: usize, l: usize) {
        let (id, go) = (self.poisons[i].id, self.circulation_id(l));
        self.start_coroutine(id, go, Self::heal_acid_flesh_routine(l));
    }
    pub(crate) fn heal_acid_flesh_routine(l: usize) -> Routine {
        Box::new(move |g| {
            if g.skins[l].acidProgress > 0.1 {
                let v = (d(g.skins[l].acidProgress) - d(g.time.deltaTime) * 0.8f32).f();
                g.skins[l].set_acid(v);
                return Yield::WaitForEndOfFrame;
            }
            g.skins[l].set_acid(0.0);
            Yield::Done
        })
    }

    /// LifePoison.HealRottenFlesh
    pub(crate) fn poison_heal_rotten_flesh(&mut self, i: usize, l: usize) {
        let (id, go) = (self.poisons[i].id, self.circulation_id(l));
        self.start_coroutine(id, go, Self::heal_rotten_flesh_routine(l));
    }
    pub(crate) fn heal_rotten_flesh_routine(l: usize) -> Routine {
        Box::new(move |g| {
            if g.skins[l].rottenProgress > 0.1 {
                let v = (d(g.skins[l].rottenProgress) - d(g.time.deltaTime) * 0.8f32).f();
                g.skins[l].set_rotten(v);
                return Yield::WaitForEndOfFrame;
            }
            g.skins[l].set_rotten(0.0);
            Yield::Done
        })
    }

    /// LifePoison.HealWounds: every damage point's intensity (z) falls by deltaTime * 0.5 until none is above 0; Sync is rendering.
    pub(crate) fn poison_heal_wounds(&mut self, i: usize, l: usize) {
        let (id, go) = (self.poisons[i].id, self.circulation_id(l));
        self.start_coroutine(id, go, Self::heal_wounds_routine(l));
    }
    pub(crate) fn heal_wounds_routine(l: usize) -> Routine {
        Box::new(move |g| {
            if !g.skins[l].damagePoints.iter().map(|p| p[2]).fold(f32::NEG_INFINITY, f32::max).gt(&0.0) {
                return Yield::Done;
            }
            let dt = g.time.deltaTime;
            for p in g.skins[l].damagePoints.iter_mut() {
                p[2] -= dt * 0.5;
            }
            Yield::WaitForEndOfFrame
        })
    }

    /// PinkPoison.WhatTheFuck (Start): every field the Fingerprint's next values perturb, in the original order.
    pub(crate) fn poison_pink(&mut self, i: usize) {
        let l = self.poisons[i].Limb;
        let p = self.limbs[l].PhysicalBehaviour;
        macro_rules! fp {
            () => {
                self.poison_fingerprint_value(i)
            };
        }
        let v = fp!();
        self.limbs[l].BalanceMuscleMovement += v * 0.5;
        let v = fp!();
        self.limbs[l].BloodMuscleStrengthRatio += v * 0.5;
        self.limbs[l].DoBalanceJerk ^= fp!() > 0.7;
        self.limbs[l].DoStumble ^= fp!() > 0.7;
        self.limbs[l].Numbness += fp!();
        let v = fp!();
        self.limbs[l].ShotDamageMultiplier += v * 5.0;
        if fp!() > 0.95 {
            self.break_bone(l);
        }
        if fp!() > 0.95 {
            self.limb_crush(l);
        }
        let v = fp!();
        let acid = self.skins[l].acidProgress + v * 0.05;
        self.skins[l].set_acid(acid);
        let v = fp!();
        self.skins[l].intensityMultiplier += v * 0.5;
        let v = fp!();
        let rotten = self.skins[l].rottenProgress + v * 0.05;
        self.skins[l].set_rotten(rotten);
        if fp!() > 0.9 {
            let position = self.physicals[p].transform.position;
            let direction = self.random.inside_unit_circle();
            self.cut(l, [position[0], position[1]], direction);
        }
        let v = fp!();
        self.circulations[l].BleedingRate *= v.abs();
        let v = fp!();
        self.circulations[l].BloodLossRateMultiplier += v * 0.2;
        self.circulations[l].IsPump ^= fp!() > 0.9;
        self.circulations[l].WasInitiallyPumping ^= fp!() > 0.7;
        self.physicals[p].AbsorbsLasers ^= fp!() > 0.0;
        self.physicals[p].ReflectsLasers ^= fp!() > 0.7;
        // rigidbody.mass += fp * mass * 0.5f
        let rb = self.physicals[p].rigidbody.clone();
        let mass = rb.mass(&self.world);
        let v = fp!();
        self.physical_set_mass(p, (d(mass) + d(v) * mass * 0.5f32).f());
        if fp!() > 0.99 {
            let num = (d(fp!()) * 25.0f32).f();
            let charge = self.physicals[p].Charge();
            self.physicals[p].charge = (d(charge) + d(num) * num * num).f();
        }
        if fp!() > 0.95 {
            self.ignite(p, true);
        }
        if fp!() > 0.9 {
            let x = fp!();
            let y = fp!();
            let s = self.physicals[p].transform.localScale;
            // new Vector3(x, y) * 0.2f: z is 0.
            self.physical_set_local_scale(p, [s[0] + x * 0.2, s[1] + y * 0.2, s[2] + 0.0 * 0.2]);
        }
        let gravity = rb.gravity_scale(&self.world);
        let add = if fp!() > 0.9 { fp!() } else { 0.0 };
        self.physical_set_gravity_scale(p, gravity + add);
        let v = fp!();
        self.limbs[l].Vitality *= v * 8.0;
        let v = fp!();
        self.limbs[l].RegenerationSpeed += v * 8.0;
        self.limbs[l].ImpactPainMultiplier += fp!();
        self.limbs[l].InitialHealth += fp!();
        self.limbs[l].Health = self.limbs[l].InitialHealth;
        self.limbs[l].BreakingThreshold += fp!();
        let v = fp!();
        self.limbs[l].BaseStrength += v * 8.0;
        if fp!() > 0.99 {
            let position = self.physicals[p].transform.position;
            let under_water = self.is_point_under_water([position[0], position[1]]);
            let range = fp!().abs() * 8.0;
            let force = fp!() * 25.0;
            let fx = fp!() < 0.5;
            let big = fp!() > 0.9;
            self.create_explosion_with_water(
                under_water,
                ExplosionParameters::new(32, position, range, force, fx, if big { EffectSize::Large } else { EffectSize::Medium }, 0.0, 0),
            );
        }
        self.limbs[l].Frozen = fp!() > 0.95;
        let v = fp!();
        self.limb_wince(l, v);
    }
}

impl Game {
    /// OldSyringeExplosionBehaviour.Start: the particles' startColor and Play are rendering. The limbs under the first 256 colliders
    /// OverlapCircleNonAlloc(position, Radius) finds, androids left out, each get the PoisonType component's Start again when it
    /// has one (AllowMultipleActivations is true everywhere), else a new one with the explosion's Fingerprint. `position` is
    /// transform.position; `kind` the PoisonType and `fingerprint` the Fingerprint BreakSyringe set.
    pub fn old_syringe_explosion_start(&mut self, position: [f32; 3], radius: f32, kind: PoisonKind, fingerprint: &[f32]) {
        use crate::b2::math::Vec2 as V;
        let mut hits = self.overlap_circle(V::new(position[0], position[1]), radius, super::explosion::DEFAULT_RAYCAST_LAYERS);
        // Mathf.Min(array.Length, OverlapCircleNonAlloc(...)): the query's first 256 colliders.
        hits.truncate(256);
        for (physical, collider) in hits {
            // array[i].GetComponent<LimbBehaviour>(): a LimbBehaviour on the collider's own GameObject.
            let Some(l) = physical.and_then(|p| self.limbs.iter().position(|x| x.PhysicalBehaviour == p)) else { continue };
            let on_limb = self.is_recording() || self.world.collider_game_object(collider) == Some(self.limb_object(l).0);
            if !on_limb || self.limbs[l].IsAndroid {
                continue;
            }
            // component2.TryGetComponent(PoisonType, out component3)
            let existing = (0..self.poisons.len()).find(|&k| self.poisons[k].Limb == l && self.poisons[k].kind.is_a(kind) && !self.destroyed_components.contains(&self.poisons[k].id));
            match existing {
                Some(k) if poison_allow_multiple_activations() => self.poison_start(k),
                Some(_) => {}
                None => {
                    let n = self.poison_add(l, kind);
                    self.poisons[n].Fingerprint = Some(fingerprint.to_vec());
                }
            }
        }
    }
}

/// PoisonSpreadBehaviour.get_AllowMultipleActivations: true (no subclass overrides it).
pub(crate) fn poison_allow_multiple_activations() -> bool {
    true
}
