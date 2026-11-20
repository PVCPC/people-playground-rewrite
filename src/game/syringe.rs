//! SyringeBehaviour and its 15 subclasses (the subclasses only name their liquid), SyringeExplosionBehaviour, and the
//! liquids the syringes carry (Acid, Adrenaline, Bone eating, Coagulation, Instant death, Freeze, Knockout, Life, Ultra
//! strength, Mending, Painkiller, Water breathing, Reanimation, Immortality, Osteomorphosis and every Pink syringe liquid).
//! Awake and OnWillRenderObject only touch the SpriteRenderer's property block (_LiquidColour, _Direction): rendering.
//! Start's AddButtons builds context menu buttons: UI.
#![allow(non_snake_case)]

use super::circulation::{liquid, LIQUIDS};
use super::container::{BloodContainer, Owner};
use super::coroutine::{Routine, Yield};
use super::explosion::DEFAULT_RAYCAST_LAYERS;
use super::flask::BLOOD_CONTAINERS;
use super::{d, mathf, script, Game};
use crate::b2::math::Vec2 as V;

/// SyringeBehaviour and its subclasses.
pub const CLASSES: &[&str] = &[
    "SyringeBehaviour", "AcidSyringe", "AdrenalineSyringe", "BoneEatingPoisonSyringe", "CoagulationSyringe", "DeathSyringe", "EmptySyringe",
    "FreezeSyringe", "KnockoutSyringe", "LifeSyringe", "MendingSyringe", "PainKillerSyringe", "PinkSyringe", "UltraStrengthSyringe",
    "WaterBreathingSyringe", "ZombieSyringe",
];

pub fn is_syringe(class: &str) -> bool {
    CLASSES.contains(&class)
}

/// AcidSyringe.GetLiquidID, AdrenalineSyringe.GetLiquidID, BoneEatingPoisonSyringe.GetLiquidID, CoagulationSyringe.GetLiquidID, DeathSyringe.GetLiquidID, EmptySyringe.GetLiquidID, FreezeSyringe.GetLiquidID, KnockoutSyringe.GetLiquidID, LifeSyringe.GetLiquidID, MendingSyringe.GetLiquidID, PainKillerSyringe.GetLiquidID, PinkSyringe.GetLiquidID, UltraStrengthSyringe.GetLiquidID, WaterBreathingSyringe.GetLiquidID, ZombieSyringe.GetLiquidID (SyringeBehaviour itself is abstract; EmptySyringe and PinkSyringe return null).
pub fn liquid_id(class: &str) -> Option<&'static str> {
    Some(match class {
        "AcidSyringe" => "ACID",
        "AdrenalineSyringe" => "ADRENALINE",
        "BoneEatingPoisonSyringe" => crate::il!(str "BoneEatingPoisonSyringe::GetLiquidID" 0),
        "CoagulationSyringe" => crate::il!(str "CoagulationSyringe::GetLiquidID" 0),
        "DeathSyringe" => crate::il!(str "DeathSyringe::GetLiquidID" 0),
        "FreezeSyringe" => crate::il!(str "FreezeSyringe::GetLiquidID" 0),
        "KnockoutSyringe" => crate::il!(str "KnockoutSyringe::GetLiquidID" 0),
        "LifeSyringe" => crate::il!(str "LifeSyringe::GetLiquidID" 0),
        "MendingSyringe" => crate::il!(str "MendingSyringe::GetLiquidID" 0),
        "PainKillerSyringe" => crate::il!(str "PainKillerSyringe::GetLiquidID" 0),
        "UltraStrengthSyringe" => crate::il!(str "UltraStrengthSyringe::GetLiquidID" 0),
        "WaterBreathingSyringe" => crate::il!(str "WaterBreathingSyringe::GetLiquidID" 0),
        "ZombieSyringe" => crate::il!(str "ZombieSyringe::GetLiquidID" 0),
        "EmptySyringe" | "PinkSyringe" => return None,
        c => unreachable!("{c} is not a syringe"),
    })
}

/// PressureDirection
const PUSH: i32 = 0;
const PULL: i32 = 1;
pub(super) const NONE: i32 = 2;

/// Limits.y
const CAPACITY: f32 = 1.4;

/// SyringeBehaviour.get_Limits: new Vector2(0f, 1.4f).
pub fn syringe_limits() -> [f32; 2] {
    [0.0, CAPACITY]
}

/// MendingSerum.get_RemovalChancePerSecond (nested in MendingSyringe): base.RemovalChancePerSecond (TemporaryBodyLiquid's 0.8f) * 0.8f.
pub fn mending_serum_removal_chance_per_second() -> f32 {
    0.8f32 * 0.8f32
}

/// OsteomorphosisAgent.get_RemovalChancePerSecond: base.RemovalChancePerSecond (TemporaryBodyLiquid's 0.8f) * 0.1f.
pub fn osteomorphosis_agent_removal_chance_per_second() -> f32 {
    0.8f32 * 0.1f32
}

script! {
    Syringe {
        Finite: bool = false,
        CanToggleInfinite: bool = true,
        PressureMode: i32 = PUSH,
        NewlySpawned: bool = true,
        TransferRate: f32 = 0.01,
        /// 1.28.45 (set by CrossbowBehaviour)
        IsAttachedToLauncher: bool = false,
    }
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// GetComponent<PhysicalBehaviour>()
        Phys: usize = 0,
        /// materialProperty, read from the SpriteRenderer (the physical's) in Awake.
        materialProperty: super::props::PropBlock = super::props::PropBlock::default(),
        /// GetLiquidID() as a `LIQUIDS` index (spawnLiquid once Start has run).
        liquid: Option<usize> = None,
        /// PinkSyringe (overrides Start and FixedUpdate).
        pink: bool = false,
        /// PinkSyringe.Fingerprint (empty: Start draws a new one) and fingerprintReadPos.
        Fingerprint: Vec<f32> = Vec::new(),
        fingerprintReadPos: i32 = 0,
        /// PinkSyringe.liquids
        liquids: Vec<(usize, f32)> = Vec::new(),
        /// pushTargets, a HashSet<BloodContainer>: .NET enumerates its entries in slot order, and an Add reuses the most
        /// recently freed slot first.
        pushTargets: Vec<Option<Owner>> = Vec::new(),
        pushFree: Vec<usize> = Vec::new(),
    }
}

script! {
    SyringeExplosion {
        Radius: f32 = 0.0,
        Amount: f32 = 0.1,
    }
    links {
        id: i32 = 0,
        /// transform.position (nothing moves it).
        Position: [f32; 3] = [0.0, 0.0, 0.0],
        /// Liquids = the syringe's LiquidDistribution.Keys, a live view of it.
        Syringe: Option<usize> = None,
        /// A recording's explosion (its syringe is gone): the recorded Liquids.
        Liquids: Vec<usize> = Vec::new(),
        /// Liquids is null (an explosion no syringe made: GenRun's prefab): the foreach over it throws.
        Liquids_null: bool = false,
    }
}

/// PinkSyringe.Start's liquids, each added when the next fingerprint value is above its threshold here: the liquid is
/// that method's k-th ldstr.
const PINK_THRESHOLDS: &[f32] = &[
    0.05, 0.6, 0.6, 0.6, 0.85, 0.85, 0.95, 0.995, 0.995, 0.9, 0.9, 0.6, 0.6, 0.8, 0.9, 0.95, 0.99, 0.4, 0.4, 0.95, 0.95,
];

/// The liquid classes written by hand here.
pub fn is_syringe_liquid(class: &str) -> bool {
    matches!(
        class,
        "AcidLiquid" | "InstantDeathPoisonLiquid" | "LifeSerumLiquid" | "UltraStrengthSerumLiquid" | "MendingSerum" | "PainKillerLiquid" | "MassManipulationPoison"
            | "ExplosionPoison" | "SizeManipulationPoison" | "CirculationPoison" | "TissueDeconstructionAgent" | "AdrenalineLiquid"
            | "BoneHurtingJuiceLiquid" | "CoagulationLiquid" | "FreezePoisonLiquid" | "KnockoutPoisonLiquid" | "WaterBreathingSerum"
            | "ZombiePoisonLiquid" | "ImmortalitySerum" | "OsteomorphosisAgent" | "CombustionAgent" | "CrushingPoison" | "DurabilitySerum"
            | "MuscleEnhancementSerum" | "MusclePoison" | "NumbingPoison" | "PinkDormantLiquid" | "ReflectionPoison" | "RegenerationSerum"
            | "TransparencyPoison" | "VestibularPoison"
    )
}

impl Game {
    /// Unity's `(bool)container`: the component is not destroyed.
    fn container_alive(&self, o: Owner) -> bool {
        !self.destroyed_components.contains(&self.container_id(o))
    }

    /// The BloodContainer component's instance id.
    pub(super) fn container_id(&self, o: Owner) -> i32 {
        match o {
            Owner::Circulation(l) => self.limbs[l].id,
            Owner::Flask(f) => self.flasks[f].id,
            Owner::Syringe(s) => self.syringes[s].id,
            Owner::Valve(v) => self.valves[v].id,
            Owner::BloodTank(t) => self.blood_tanks[t].id,
            Owner::LiquidCanister(c) => self.liquid_canisters[c].id,
            Owner::LiquidFilter(f) => self.liquid_filters[f].id,
            Owner::Liquidentifier(l) => self.liquidentifiers[l].id,
            Owner::Centrifuge(c) => self.centrifuges[c].id,
            Owner::LiquidPump(l) => self.liquid_pumps[l].id,
            Owner::LiquidDuplicator(l) => self.liquid_duplicators[l].id,
            Owner::LiquidOutlet(l) => self.liquid_outlets[l].id,
            Owner::ReactorConduit(c) => self.reactor_conduits[c].id,
        }
    }

    /// physical.TryGetComponent<BloodContainer>(): a limb's CirculationBehaviour, a flask or a syringe.
    pub(super) fn physical_container(&mut self, q: usize) -> Option<Owner> {
        if let Some(l) = self.limbs.iter().position(|x| x.PhysicalBehaviour == q) {
            return Some(Owner::Circulation(l));
        }
        if let Some(f) = self.flasks.iter().position(|x| x.Phys == Some(q)) {
            return Some(Owner::Flask(f));
        }
        if let Some(s) = self.syringes.iter().position(|x| x.Phys == q) {
            return Some(Owner::Syringe(s));
        }
        // LiquidDuplicatorBehaviour (liquid_duplicator.rs).
        if let Some(l) = self.liquid_duplicators.iter().position(|x| x.physical == q) {
            return Some(Owner::LiquidDuplicator(l));
        }
        // LiquidPumpBehaviour (liquid_pump.rs).
        if let Some(l) = self.liquid_pumps.iter().position(|x| x.physical == q) {
            return Some(Owner::LiquidPump(l));
        }
        // CentrifugeBehaviour (centrifuge.rs).
        if let Some(c) = self.centrifuges.iter().position(|x| x.physical == q) {
            return Some(Owner::Centrifuge(c));
        }
        // LiquidentifierBehaviour (liquidentifier.rs).
        if let Some(l) = self.liquidentifiers.iter().position(|x| x.physical == q) {
            return Some(Owner::Liquidentifier(l));
        }
        // LiquidCanisterBehaviour (liquid_canister.rs).
        if let Some(c) = self.liquid_canisters.iter().position(|x| x.physical == q) {
            return Some(Owner::LiquidCanister(c));
        }
        // LiquidFilterBehaviour (liquid_filter.rs).
        if let Some(f) = self.liquid_filters.iter().position(|x| x.physical == q) {
            return Some(Owner::LiquidFilter(f));
        }
        // BloodTankBehaviour (blood_tank.rs).
        if let Some(t) = self.blood_tanks.iter().position(|x| x.physical == q) {
            return Some(Owner::BloodTank(t));
        }
        // LiquidOutletBehaviour (liquid_outlet.rs).
        if let Some(l) = self.liquid_outlets.iter().position(|x| x.physical == q) {
            return Some(Owner::LiquidOutlet(l));
        }
        // ReactorConduitBehaviour (reactor.rs).
        if let Some(c) = self.reactor_conduits.iter().position(|x| x.physical == q) {
            return Some(Owner::ReactorConduit(c));
        }
        // ValveBehaviour (valve.rs).
        if let Some(v) = self.valves.iter().position(|x| x.physical == q) {
            return Some(Owner::Valve(v));
        }
        let class = self.with_component_instance(self.physicals[q].id, |_, inst, o| {
            inst.prefab.objects[o].components.iter().find_map(|(_, c)| match c {
                crate::assets::db::Component::Script { class, .. } if BLOOD_CONTAINERS.contains(&class.as_str()) => Some(class.clone()),
                _ => None,
            })
        });
        if let Some(class) = class {
            unimplemented!("mod-only: SyringeBehaviour pushing into a {class}");
        }
        None
    }

    /// pushTargets.Add
    fn push_target_add(&mut self, s: usize, o: Owner) {
        let x = &mut self.syringes[s];
        if x.pushTargets.contains(&Some(o)) {
            return;
        }
        match x.pushFree.pop() {
            Some(i) => x.pushTargets[i] = Some(o),
            None => x.pushTargets.push(Some(o)),
        }
    }

    /// pushTargets.Remove
    fn push_target_remove(&mut self, s: usize, o: Owner) {
        let x = &mut self.syringes[s];
        if let Some(i) = x.pushTargets.iter().position(|t| *t == Some(o)) {
            x.pushTargets[i] = None;
            x.pushFree.push(i);
        }
    }

    /// SyringeBehaviour.AddButtons (the first thing both Starts do).
    pub(crate) fn syringe_add_buttons(&mut self, s: usize) {
        use super::menu::{Cond, Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.syringes[s].Phys].menu;
        for (id, label, desc, mode) in [("setToPush", crate::il!(str "SyringeBehaviour::AddButtons" 1), crate::il!(str "SyringeBehaviour::AddButtons" 2), PUSH), ("setToPull", crate::il!(str "SyringeBehaviour::AddButtons" 4), crate::il!(str "SyringeBehaviour::AddButtons" 5), PULL), ("setToIdle", crate::il!(str "SyringeBehaviour::AddButtons" 7), crate::il!(str "SyringeBehaviour::AddButtons" 8), NONE)] {
            menu.push(B { condition: Cond::SyringeModeIsNot(s, mode), ..B::new(id, label, desc, A::SyringeMode(s, mode)) });
        }
        menu.push(B { label: Label::SyringeInfinite(s), condition: Cond::SyringeCanToggle(s), ..B::new("toggleInifniteSyringe", "", crate::il!(str "SyringeBehaviour::AddButtons" 10), A::SyringeInfinite(s)) });
    }

    /// SyringeBehaviour.Start; the pink syringe's own Start is pink_syringe_start.
    pub fn syringe_start(&mut self, s: usize) {
        if self.syringes[s].pink {
            self.pink_syringe_start(s);
            return;
        }
        self.syringe_add_buttons(s);
        let o = Owner::Syringe(s);
        if let Some(spawn) = self.syringes[s].liquid {
            if self.syringes[s].NewlySpawned {
                self.add_liquid(o, spawn, CAPACITY);
            }
        }
        let p = self.syringes[s].Phys;
        for pen in self.physicals[p].penetrations.clone() {
            let victim = self.penetration_arena[pen].Victim.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            if let Some(target) = self.physical_container(victim) {
                self.push_target_add(s, target);
            }
        }
        self.syringes[s].NewlySpawned = false;
    }

    /// PinkSyringe.GetFingerprintValue
    pub(crate) fn fingerprint_value(&mut self, s: usize) -> f32 {
        let x = &mut self.syringes[s];
        // Fingerprint[fingerprintReadPos]: an int index, out of range (negative, or past the end) throws.
        let Some(&result) = usize::try_from(x.fingerprintReadPos).ok().and_then(|i| x.Fingerprint.get(i)) else {
            crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.")
        };
        x.fingerprintReadPos += 1;
        if x.fingerprintReadPos as usize >= x.Fingerprint.len() {
            x.fingerprintReadPos = 0;
        }
        result
    }

    /// PinkSyringe.CreateNewFingerprint: 64 draws of Random.Range(-1, 1), read from the start.
    pub(crate) fn pink_syringe_create_new_fingerprint(&mut self, s: usize) {
        self.syringes[s].fingerprintReadPos = 0;
        let mut fingerprint = Vec::with_capacity(64);
        for _ in 0..64 {
            fingerprint.push(self.random.range(-1.0, 1.0));
        }
        self.syringes[s].Fingerprint = fingerprint;
    }

    /// PinkSyringe.Start (it does not call SyringeBehaviour.Start: no pushTargets from the penetrations).
    pub(crate) fn pink_syringe_start(&mut self, s: usize) {
        let o = Owner::Syringe(s);
        if self.syringes[s].Fingerprint.is_empty() {
            self.pink_syringe_create_new_fingerprint(s);
        }
        // PinkSyringe.Start overrides SyringeBehaviour.Start: AddButtons after the fingerprint.
        self.syringe_add_buttons(s);
        let v = self.fingerprint_value(s);
        let item = mathf::map_range(-1.0, 1.0, 0.05, 0.4, v);
        let mut num = 0;
        let mut total = 0.0f32;
        while total < CAPACITY - mathf::EPSILON && num < 5000 {
            for (k, &threshold) in PINK_THRESHOLDS.iter().enumerate() {
                let name = crate::assets::il::get().str("PinkSyringe::Start", k);
                if self.fingerprint_value(s) > threshold {
                    // addToList
                    let l = liquid(name);
                    total += item;
                    self.syringes[s].liquids.push((l, item));
                    if self.syringes[s].NewlySpawned {
                        self.add_liquid(o, l, item);
                    }
                }
            }
            num += 1;
        }
        self.syringes[s].NewlySpawned = false;
    }

    /// SyringeBehaviour.FixedUpdate, after PinkSyringe.FixedUpdate's top-up.
    pub fn syringe_fixed_update(&mut self, s: usize) {
        let o = Owner::Syringe(s);
        if self.syringes[s].pink && !self.syringes[s].Finite {
            let num = CAPACITY - self.bc(o).TotalLiquidAmount;
            let num2 = self.syringes[s].TransferRate * num;
            for (l, amount) in self.syringes[s].liquids.clone() {
                self.add_liquid(o, l, amount * num2);
            }
        }
        let finite = self.syringes[s].Finite;
        let spawn = self.syringes[s].liquid;
        if !finite {
            if let Some(spawn) = spawn {
                let amount = CAPACITY - self.get_amount(o, spawn);
                self.add_liquid(o, spawn, amount);
            }
        }
        let rate = self.syringes[s].TransferRate;
        for other in self.syringes[s].pushTargets.iter().flatten().copied().collect::<Vec<_>>() {
            if !self.container_alive(other) {
                continue;
            }
            self.average_pressure(self.time.fixedDeltaTime, o, other);
            if !finite || self.bc(o).MeasuredPressure > self.bc(other).MeasuredPressure {
                // push()
                if !(self.bc(o).TotalLiquidAmount <= self.container_limits(o)[0]) && !(self.bc(other).TotalLiquidAmount >= self.upper_limit(other)) {
                    match (spawn, other) {
                        (Some(spawn), Owner::Circulation(_)) => {
                            if self.get_amount(other, spawn) < 0.5 {
                                self.transfer_to(o, rate, other);
                            }
                        }
                        _ => {
                            self.transfer_to(o, rate, other);
                        }
                    }
                }
            } else {
                // pull()
                if !(self.bc(other).TotalLiquidAmount <= self.container_limits(other)[0]) && !(self.bc(o).TotalLiquidAmount >= self.upper_limit(o)) {
                    self.transfer_to(other, rate, o);
                }
            }
        }
    }

    /// The syringe's copy of BloodWireBehaviour.AveragePressure (a closed ValveBehaviour does not allow pressure transfer).
    pub(crate) fn average_pressure(&mut self, rate: f32, from: Owner, to: Owner) {
        if !(self.container_allows_pressure(from) && self.container_allows_pressure(to)) {
            return;
        }
        let (a, b) = (self.bc(from).MeasuredPressure, self.bc(to).MeasuredPressure);
        let mean = ((d(a) + b) * 0.5f32).f();
        self.bc_mut(from).MeasuredPressure = mathf::lerp(a, mean, rate);
        self.bc_mut(to).MeasuredPressure = mathf::lerp(b, mean, rate);
    }

    /// SyringeBehaviour.Update is BloodContainer.Update (the Push and Pull pressure is `container_pressure`).
    pub fn syringe_update(&mut self, s: usize) {
        self.blood_container_update(Owner::Syringe(s));
    }

    /// SyringeBehaviour.Use
    pub fn syringe_use(&mut self, s: usize, channel: i32) {
        let x = &mut self.syringes[s];
        // 1.28.45: the use that comes while attached to a launcher only clears IsAttachedToLauncher.
        if crate::assets::il::is_1_28_45() && x.IsAttachedToLauncher {
            x.IsAttachedToLauncher = false;
            return;
        }
        match channel {
            1 => x.PressureMode = PUSH,
            2 => {
                if x.Finite {
                    x.PressureMode = PULL;
                }
            }
            _ => x.PressureMode = if x.PressureMode == PUSH { PULL } else { PUSH },
        }
    }

    /// SyringeBehaviour.Lodged
    pub fn syringe_lodged(&mut self, s: usize, victim: usize) {
        if !self.physicals[victim].destroyed {
            if let Some(target) = self.physical_container(victim) {
                self.push_target_add(s, target);
            }
        }
    }

    /// SyringeBehaviour.Dislodged
    pub fn syringe_dislodged(&mut self, s: usize, pen: usize) {
        if let Some(victim) = self.penetration_arena[pen].Victim.filter(|&v| !self.physicals[v].destroyed) {
            if let Some(target) = self.physical_container(victim) {
                self.push_target_remove(s, target);
            }
        }
    }

    // SyringeBehaviour.System.Collections.IEnumerator.get_Current: the WaitAFrame iterator's plumbing (SyringeBehaviour.Shot).
    // SyringeBehaviour.System.Collections.Generic.IEnumerator<System.Object>.get_Current: likewise.
    // SyringeBehaviour.System.Collections.IEnumerator.Reset: likewise.
    // SyringeBehaviour.System.IDisposable.Dispose: likewise.
    // SyringeBehaviour.MoveNext: the Shot coroutine WaitAFrame, WaitForSeconds(0.05) then BreakSyringe (Step::SyringeBreak).
    /// SyringeBehaviour.Awake: materialProperty is read from the SpriteRenderer (the physical's).
    pub fn syringe_awake(&mut self, s: usize) {
        self.syringes[s].materialProperty = self.get_property_block(super::visual::SpriteAt::Physical(self.syringes[s].Phys));
    }

    /// SyringeBehaviour.GetColor: GetComputedColor() with alpha Clamp01(ScaledLiquidAmount).
    pub fn syringe_get_color(&mut self, s: usize) -> [f32; 4] {
        let o = super::container::Owner::Syringe(s);
        let fallback = self.bc(o).cachedComputedColour;
        let mut c = self.get_computed_color(o, fallback);
        c[3] = super::mathf::clamp01(self.scaled_liquid_amount(o));
        c
    }

    /// SyringeBehaviour.OnWillRenderObject: `_LiquidColour` = GetColor(), `_Direction` 1 for Push, -1 for Pull, else 0, on the block.
    pub fn syringe_will_render(&mut self, s: usize) {
        let c = self.syringe_get_color(s);
        let dir = match self.syringes[s].PressureMode {
            0 => 1.0,
            1 => -1.0,
            _ => 0.0,
        };
        let mut block = std::mem::take(&mut self.syringes[s].materialProperty);
        block.set_color("_LiquidColour", c);
        block.set_float("_Direction", dir);
        self.set_property_block(super::visual::SpriteAt::Physical(self.syringes[s].Phys), &block);
        self.syringes[s].materialProperty = block;
    }
    /// SyringeBehaviour.Shot: StartCoroutine(WaitAFrame()), which breaks it after WaitForSeconds(0.05).
    pub fn syringe_shot(&mut self, s: usize) {
        let id = self.syringes[s].id;
        self.start_wait(id, 0.05, super::destroyable::Step::SyringeBreak(s));
    }

    /// SyringeBehaviour.ExitShot(shot): BreakSyringe().
    pub fn syringe_exit_shot(&mut self, s: usize) {
        self.break_syringe(s);
    }

    /// SyringeBehaviour.OnFragmentHit(force): BreakSyringe().
    pub fn syringe_on_fragment_hit(&mut self, s: usize) {
        self.break_syringe(s);
    }

    /// SyringeBehaviour.BreakSyringe. The explosion's particle colour (GetComputedColor) is rendering.
    pub fn break_syringe(&mut self, s: usize) {
        let o = Owner::Syringe(s);
        let total = self.bc(o).TotalLiquidAmount;
        let count = self.bc(o).LiquidDistribution.len();
        if total > mathf::EPSILON && count > 0 {
            // component.Colour = GetComputedColor(): the colour is the particles' (rendering); the call fills the cache.
            let cached = self.bc(o).cachedComputedColour;
            self.get_computed_color(o, cached);
            let position = self.physicals[self.syringes[s].Phys].transform.position;
            let before = self.syringe_explosions.len();
            if self.is_recording() {
                // A recording has no prefabs or clones: the explosion's script (Radius from the prefab) and the
                // item's object.
                self.syringe_explosions.push(SyringeExplosion { Radius: 3.0, Position: position, Syringe: Some(s), Amount: total / count as f32, ..Default::default() });
                self.pending_starts.push(super::misc::PendingStart::SyringeExplosion(before));
                let p = self.syringes[s].Phys;
                self.behaviours.remove(super::Behaviour::Syringe(s));
                self.collider_disable(p);
                self.physical_on_disable(p);
                self.destroy_object(self.physicals[p].id);
                return;
            }
            self.spawn_resource("Prefabs/SyringeExplosion", position);
            assert_eq!(self.syringe_explosions.len(), before + 1, "the SyringeExplosion prefab has one SyringeExplosionBehaviour");
            let e = &mut self.syringe_explosions[before];
            e.Position = position;
            e.Syringe = Some(s);
            e.Amount = total / count as f32;
            let id = self.syringes[s].id;
            self.destroy_object(id);
        }
    }

    /// SyringeExplosionBehaviour.Start: the particles' colour and Play are rendering.
    pub fn syringe_explosion_start(&mut self, e: usize) {
        let x = &self.syringe_explosions[e];
        let (radius, amount, position) = (x.Radius, x.Amount, x.Position);
        let liquids: Vec<usize> = match x.Syringe {
            Some(s) => self.syringes[s].base.LiquidDistribution.keys(),
            None => x.Liquids.clone(),
        };
        let mut hits = self.overlap_circle(V::new(position[0], position[1]), radius, DEFAULT_RAYCAST_LAYERS);
        // Mathf.Min(results.Length, OverlapCircleNonAlloc(...)): the query's first 64 colliders (180bf4620).
        hits.truncate(SYRINGE_EXPLOSION_RESULTS);
        let num = hits.len();
        let amount = (d(amount) / num as f32 * 3.0f32).f();
        for (physical, collider) in hits {
            // results[i].GetComponent<LimbBehaviour>(): a LimbBehaviour on the collider's own GameObject.
            let Some(l) = physical.and_then(|p| self.limbs.iter().position(|x| x.PhysicalBehaviour == p)) else { continue };
            // A recording's limb has one collider, on its own object.
            let on_limb = self.is_recording() || self.world.collider_game_object(collider) == Some(self.limb_object(l).0);
            if !on_limb || self.limbs[l].IsAndroid {
                continue;
            }
            if self.syringe_explosions[e].Syringe.is_none() && self.syringe_explosions[e].Liquids_null {
                crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
            }
            for &liquid in &liquids {
                self.add_liquid(Owner::Circulation(l), liquid, amount);
            }
        }
    }

    // ---- Liquid's virtual members for the liquids above ----

    /// CirculationPoison.OnEnterContainer, ExplosionPoison.OnEnterContainer, InstantDeathPoisonLiquid.OnEnterContainer, LifeSerumLiquid.OnEnterContainer, MassManipulationPoison.OnEnterContainer, MendingSerum.OnEnterContainer, SizeManipulationPoison.OnEnterContainer, UltraStrengthSerumLiquid.OnEnterContainer, Liquid.OnEnterContainer, AdrenalineLiquid.OnEnterContainer, BoneHurtingJuiceLiquid.OnEnterContainer, CoagulationLiquid.OnEnterContainer, FreezePoisonLiquid.OnEnterContainer, KnockoutPoisonLiquid.OnEnterContainer, WaterBreathingSerum.OnEnterContainer, ZombiePoisonLiquid.OnEnterContainer, ImmortalitySerum.OnEnterContainer, OsteomorphosisAgent.OnEnterContainer, CombustionAgent.OnEnterContainer, CrushingPoison.OnEnterContainer, DurabilitySerum.OnEnterContainer, MuscleEnhancementSerum.OnEnterContainer, MusclePoison.OnEnterContainer, NumbingPoison.OnEnterContainer, PinkDormantLiquid.OnEnterContainer, ReflectionPoison.OnEnterContainer, RegenerationSerum.OnEnterContainer, TransparencyPoison.OnEnterContainer, VestibularPoison.OnEnterContainer: empty in every one (PainKillerLiquid.OnEnterContainer too).
    pub(crate) fn syringe_liquid_on_enter_container(&mut self, _liquid: usize, _o: Owner) {}

    /// CirculationPoison.OnExitContainer, TissueDeconstructionAgent.OnExitContainer, ExplosionPoison.OnExitContainer, InstantDeathPoisonLiquid.OnExitContainer, LifeSerumLiquid.OnExitContainer, MassManipulationPoison.OnExitContainer, MendingSerum.OnExitContainer, SizeManipulationPoison.OnExitContainer, UltraStrengthSerumLiquid.OnExitContainer, Liquid.OnExitContainer, AdrenalineLiquid.OnExitContainer, BoneHurtingJuiceLiquid.OnExitContainer, CoagulationLiquid.OnExitContainer, FreezePoisonLiquid.OnExitContainer, KnockoutPoisonLiquid.OnExitContainer, WaterBreathingSerum.OnExitContainer, ZombiePoisonLiquid.OnExitContainer, ImmortalitySerum.OnExitContainer, OsteomorphosisAgent.OnExitContainer, CombustionAgent.OnExitContainer, CrushingPoison.OnExitContainer, DurabilitySerum.OnExitContainer, MuscleEnhancementSerum.OnExitContainer, MusclePoison.OnExitContainer, NumbingPoison.OnExitContainer, PinkDormantLiquid.OnExitContainer, ReflectionPoison.OnExitContainer, RegenerationSerum.OnExitContainer, TransparencyPoison.OnExitContainer, VestibularPoison.OnExitContainer: empty in every one (PainKillerLiquid.OnExitContainer too).
    pub(crate) fn syringe_liquid_on_exit_container(&mut self, _liquid: usize, _o: Owner) {}

    /// Liquid.OnEnterLimb
    pub(crate) fn syringe_liquid_on_enter_limb(&mut self, liquid: usize, l: usize) {
        match LIQUIDS[liquid].1 {
            // GorseBlood.OnEnterLimb, inherited.
            "AcidLiquid" => {
                if self.limbs[l].SpeciesIdentity != "Gorse" {
                    self.acid_routine(liquid, l);
                }
            }
            // InstantDeathPoisonLiquid.OnEnterLimb: `_ = limb.SpeciesIdentity == "Android"`
            "InstantDeathPoisonLiquid" => {}
            "LifeSerumLiquid" => self.life_serum_on_enter_limb(liquid, l),
            "UltraStrengthSerumLiquid" => self.ultra_strength_on_enter_limb(l),
            "MendingSerum" => self.mending_serum_on_enter_limb(l),
            // PainKillerLiquid.OnEnterLimb: KillPain(limb).
            "PainKillerLiquid" => self.kill_pain(liquid, l),
            // AdrenalineLiquid.OnEnterLimb: PushAdrenaline(limb).
            "AdrenalineLiquid" => self.push_adrenaline(liquid, l),
            "ImmortalitySerum" => self.immortality_serum_on_enter_limb(l),
            // BoneHurtingJuiceLiquid.OnEnterLimb: empty.
            // CoagulationLiquid.OnEnterLimb: empty.
            // FreezePoisonLiquid.OnEnterLimb: empty.
            // KnockoutPoisonLiquid.OnEnterLimb: empty.
            // WaterBreathingSerum.OnEnterLimb: empty.
            // PinkDormantLiquid.OnEnterLimb: empty.
            "BoneHurtingJuiceLiquid" | "CoagulationLiquid" | "FreezePoisonLiquid" | "KnockoutPoisonLiquid" | "WaterBreathingSerum"
            | "PinkDormantLiquid" => {}
            // The rest test `limb.SpeciesIdentity == "Android"` first and do nothing for one.
            _ if self.limbs[l].SpeciesIdentity == "Android" => {}
            "ZombiePoisonLiquid" => self.zombie_poison_on_enter_limb(l),
            "OsteomorphosisAgent" => self.osteomorphosis_agent_on_enter_limb(l),
            // CombustionAgent.OnEnterLimb
            "CombustionAgent" => {
                if self.random.value() > 0.95 {
                    self.ignite(self.limbs[l].PhysicalBehaviour, true);
                }
            }
            // CrushingPoison.OnEnterLimb
            "CrushingPoison" => {
                if self.random.value() > 0.5 {
                    self.limb_crush(l);
                }
            }
            // DurabilitySerum.OnEnterLimb
            "DurabilitySerum" => {
                let initial = (d(self.limbs[l].InitialHealth) + d(self.random.value()) * 5.0f32).f();
                self.limbs[l].InitialHealth = initial;
                self.limbs[l].Health += initial;
            }
            "MuscleEnhancementSerum" => self.muscle_enhancement_serum_on_enter_limb(l),
            // MusclePoison.OnEnterLimb
            "MusclePoison" => self.limbs[l].BloodMuscleStrengthRatio += self.random.range_int(-1, 1) as f32,
            // NumbingPoison.OnEnterLimb
            "NumbingPoison" => self.limbs[l].Numbness += self.random.range_int(-1, 1) as f32,
            // ReflectionPoison.OnEnterLimb
            "ReflectionPoison" => self.physicals[self.limbs[l].PhysicalBehaviour].ReflectsLasers = true,
            // RegenerationSerum.OnEnterLimb
            "RegenerationSerum" => self.limbs[l].RegenerationSpeed += 1.0,
            // TransparencyPoison.OnEnterLimb
            "TransparencyPoison" => self.physicals[self.limbs[l].PhysicalBehaviour].AbsorbsLasers = false,
            "VestibularPoison" => self.vestibular_poison_on_enter_limb(l),
            // MassManipulationPoison.OnEnterLimb
            "MassManipulationPoison" => {
                let rb = self.physicals[self.limbs[l].PhysicalBehaviour].rigidbody.clone();
                let mass = rb.mass(&self.world) * self.random.range_int(-1, 1) as f32;
                let lp = self.limbs[l].PhysicalBehaviour;
                self.physical_set_mass(lp, mass);
                let scale = rb.gravity_scale(&self.world) * self.random.range_int(-1, 1) as f32;
                self.physical_set_gravity_scale(lp, scale);
            }
            // ExplosionPoison.OnEnterLimb
            "ExplosionPoison" => {
                let position = self.physicals[self.limbs[l].PhysicalBehaviour].transform.position;
                let under_water = self.is_point_under_water([position[0], position[1]]);
                let big = self.random.value() > 0.9;
                let dismember = self.random.value();
                self.create_explosion_with_water(
                    under_water,
                    super::explosion::ExplosionParameters {
                        FragmentationRayCount: 32,
                        Position: position,
                        Range: 9.0,
                        FragmentForce: 25.0,
                        CreateParticlesAndSound: true,
                        EffectSize: if big { super::explosion::EffectSize::Large } else { super::explosion::EffectSize::Medium },
                        DismemberChance: dismember,
                        BallisticShrapnelCount: 0,
                    },
                );
            }
            // SizeManipulationPoison.OnEnterLimb
            "SizeManipulationPoison" => {
                let p = self.limbs[l].PhysicalBehaviour;
                let x = self.random.range(0.952381, 1.05) * self.physicals[p].transform.localScale[0];
                let y = self.random.range(0.952381, 1.05) * self.physicals[p].transform.localScale[1];
                // new Vector3(x, y): z is 0.
                self.physical_set_local_scale(p, [x, y, 0.0]);
            }
            // CirculationPoison.OnEnterLimb
            "CirculationPoison" => {
                let r = self.random.range_int(-1, 1).abs() as f32;
                self.circulations[l].BleedingRate *= r;
                let r = self.random.range_int(-1, 1) as f32;
                self.circulations[l].BloodLossRateMultiplier += r * 0.2;
                let r = self.random.range_int(-1, 1) as f32;
                self.circulations[l].IsPump ^= r > 0.9;
                let r = self.random.range_int(-1, 1) as f32;
                self.circulations[l].WasInitiallyPumping ^= r > 0.7;
                let r = self.random.range_int(0, 8) as f32;
                self.circulations[l].InternalBleedingIntensity *= r;
                let r = (self.random.range_int(-1, 1) * 8) as f32;
                self.limbs[l].Vitality *= r;
                let r = (self.random.range_int(-1, 1) * 8) as f32;
                self.limbs[l].ImpactPainMultiplier *= r;
                let r = self.random.range_int(-1, 1) as f32;
                let acid = self.skins[l].acidProgress + r * 0.05;
                self.skins[l].set_acid(acid);
                let r = self.random.range_int(-1, 1) as f32;
                self.skins[l].intensityMultiplier += r * 0.5;
                let r = self.random.range_int(-1, 1) as f32;
                let rotten = self.skins[l].rottenProgress + r * 0.05;
                self.skins[l].set_rotten(rotten);
                // (double)Random.value > 0.5
                let r = self.random.value();
                self.limbs[l].HasLungs ^= d(r).0 > 0.5;
                let r = self.random.value();
                self.limbs[l].LungsPunctured ^= d(r).0 > 0.5;
            }
            // TissueDeconstructionAgent.OnEnterLimb: the Android test is the arm above; then Random.value > 0.8 slices the limb.
            "TissueDeconstructionAgent" => {
                if self.random.value() > 0.8 {
                    self.limb_slice(l);
                }
            }
            // is_syringe_liquid admits exactly the classes matched above.
            c => unreachable!("{c} is no syringe liquid"),
        }
    }

    /// Liquid.OnUpdate
    pub(crate) fn syringe_liquid_on_update(&mut self, liquid: usize, o: Owner) {
        match LIQUIDS[liquid].1 {
            // Blood.OnUpdate, inherited from GorseBlood.
            "AcidLiquid" => self.blood_on_update(liquid, o),
            // InstantDeathPoisonLiquid.OnUpdate
            "InstantDeathPoisonLiquid" => {
                self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1));
                if let Owner::Circulation(l) = o {
                    self.limbs[l].Health = 0.0;
                    self.circulations[l].IsPump = false;
                }
            }
            "LifeSerumLiquid" | "UltraStrengthSerumLiquid" => self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1)),
            "MendingSerum" => self.temporary_body_liquid_on_update(liquid, o, mending_serum_removal_chance_per_second(), should_call_on_enter_every_update(LIQUIDS[liquid].1)),
            // PainKillerLiquid.OnUpdate: base.OnUpdate, then KillPain for a limb's circulation.
            "PainKillerLiquid" => {
                self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1));
                if let Owner::Circulation(l) = o {
                    self.kill_pain(liquid, l);
                }
            }
            // TissueDeconstructionAgent.get_ShouldCallOnEnterEveryUpdate: false, so only the removal.
            // TissueDeconstructionAgent.OnEnterContainer, TissueDeconstructionAgent.OnExitContainer and TissueDeconstructionAgent.constructor: empty / the colour (`liquid_color`).
            // ShouldCallOnEnterEveryUpdate is false: only the removal.
            "MassManipulationPoison" | "ExplosionPoison" | "SizeManipulationPoison" | "CirculationPoison" | "TissueDeconstructionAgent" => {
                self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1))
            }
            // CrushingPoison.get_ShouldCallOnEnterEveryUpdate, DurabilitySerum.get_ShouldCallOnEnterEveryUpdate,
            // MuscleEnhancementSerum.get_ShouldCallOnEnterEveryUpdate, MusclePoison.get_ShouldCallOnEnterEveryUpdate,
            // NumbingPoison.get_ShouldCallOnEnterEveryUpdate, ReflectionPoison.get_ShouldCallOnEnterEveryUpdate,
            // RegenerationSerum.get_ShouldCallOnEnterEveryUpdate, TransparencyPoison.get_ShouldCallOnEnterEveryUpdate,
            // VestibularPoison.get_ShouldCallOnEnterEveryUpdate: false, so OnUpdate only removes.
            "CrushingPoison" | "DurabilitySerum" | "MuscleEnhancementSerum" | "MusclePoison" | "NumbingPoison" | "ReflectionPoison"
            | "RegenerationSerum" | "TransparencyPoison" | "VestibularPoison" => self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1)),
            // OsteomorphosisAgent.get_ShouldCallOnEnterEveryUpdate: true.
            "OsteomorphosisAgent" => self.temporary_body_liquid_on_update(liquid, o, osteomorphosis_agent_removal_chance_per_second(), should_call_on_enter_every_update(LIQUIDS[liquid].1)),
            // PinkDormantLiquid.get_ShouldCallOnEnterEveryUpdate: true.
            "PinkDormantLiquid" => self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1)),
            // AdrenalineLiquid.OnUpdate: base.OnUpdate (which already pushes through OnEnterLimb), then PushAdrenaline again.
            "AdrenalineLiquid" => {
                self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1));
                if let Owner::Circulation(l) = o {
                    self.push_adrenaline(liquid, l);
                }
            }
            // BoneHurtingJuiceLiquid.OnUpdate
            "BoneHurtingJuiceLiquid" => {
                self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1));
                if let Owner::Circulation(l) = o {
                    if self.limbs[l].SpeciesIdentity != "Android" {
                        let num = mathf::clamp01(self.get_amount(o, liquid) * 250.0);
                        if self.random.value() < num {
                            self.break_bone(l);
                        }
                    }
                }
            }
            // CoagulationLiquid.OnUpdate
            "CoagulationLiquid" => {
                self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1));
                if let Owner::Circulation(l) = o {
                    if self.limbs[l].SpeciesIdentity != "Android" {
                        self.heal_bleeding(l);
                    }
                }
            }
            // FreezePoisonLiquid.OnUpdate
            "FreezePoisonLiquid" => {
                self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1));
                if let Owner::Circulation(l) = o {
                    if self.limbs[l].SpeciesIdentity != "Android" {
                        let num = mathf::clamp01(self.get_amount(o, liquid) * 750.0);
                        if self.random.value() < num {
                            self.limbs[l].Frozen = true;
                        }
                    }
                }
            }
            // KnockoutPoisonLiquid.OnUpdate
            "KnockoutPoisonLiquid" => {
                self.temporary_body_liquid_on_update(liquid, o, 0.8, should_call_on_enter_every_update(LIQUIDS[liquid].1));
                if let Owner::Circulation(l) = o {
                    if self.limbs[l].SpeciesIdentity != "Android" {
                        let num = mathf::clamp01(self.get_amount(o, liquid) * 350.0);
                        self.limbs[l].Numbness = mathf::max(num, self.limbs[l].Numbness);
                        if self.limbs[l].RoughClassification == 0 {
                            let p = &mut self.people[self.limbs[l].Person];
                            p.Consciousness = mathf::min(1.0 - num, p.Consciousness);
                        }
                    }
                }
            }
            // WaterBreathingSerum.OnUpdate: base.OnUpdate (Liquid's, empty), then a breathing person's oxygen stays full.
            "WaterBreathingSerum" => {
                if let Owner::Circulation(l) = o {
                    if self.limbs[l].SpeciesIdentity != "Android" && self.limbs[l].RoughClassification == 0 {
                        self.people[self.limbs[l].Person].OxygenLevel = 1.0;
                    }
                }
            }
            // ZombiePoisonLiquid.OnUpdate: Liquid's, empty.
            "ZombiePoisonLiquid" => {}
            // ImmortalitySerum.OnUpdate: base.OnUpdate (Liquid's, empty), then OnEnterLimb every update.
            "ImmortalitySerum" => {
                if let Owner::Circulation(l) = o {
                    self.immortality_serum_on_enter_limb(l);
                }
            }
            // CombustionAgent.OnUpdate: a living limb heats up, then base.OnUpdate (Liquid's, empty).
            "CombustionAgent" => {
                if let Owner::Circulation(l) = o {
                    if self.limbs[l].SpeciesIdentity != "Android" {
                        self.physicals[self.limbs[l].PhysicalBehaviour].Temperature += 10.0;
                    }
                }
            }
            c => unreachable!("{c} is no syringe liquid"),
        }
    }

    /// TemporaryBodyLiquid.OnUpdate (`rcps` is get_RemovalChancePerSecond, `call_on_enter_limb`
    /// get_ShouldCallOnEnterEveryUpdate).
    pub(crate) fn temporary_body_liquid_on_update(&mut self, liquid: usize, o: Owner, rcps: f32, call_on_enter_limb: bool) {
        let Owner::Circulation(l) = o else { return };
        if (self.random.value() as f64) < (rcps as f64) * (rcps as f64) {
            let amount = self.get_amount(o, liquid);
            self.remove_liquid(o, liquid, amount);
        }
        if call_on_enter_limb {
            self.liquid_on_enter_limb(liquid, l);
        }
    }

    /// AdrenalineLiquid.PushAdrenaline
    pub(crate) fn push_adrenaline(&mut self, liquid: usize, l: usize) {
        if self.limbs[l].SpeciesIdentity == "Android" {
            return;
        }
        let num = mathf::clamp01(self.get_amount(Owner::Circulation(l), liquid) * 450.0);
        self.limbs[l].Numbness = 0.0;
        if num > 1.4 {
            let damage = 10.0 * self.random.value();
            self.limb_damage(l, damage);
        }
        if self.limbs[l].RoughClassification == 0 {
            let p = &mut self.people[self.limbs[l].Person];
            p.AdrenalineLevel += 1.0 * num;
            p.Consciousness = mathf::max(num, p.Consciousness);
            p.PainLevel = mathf::max(0.0, p.PainLevel - num);
            p.ShockLevel = mathf::max(0.0, p.ShockLevel - num);
        }
    }

    /// ImmortalitySerum.OnEnterLimb
    pub(crate) fn immortality_serum_on_enter_limb(&mut self, l: usize) {
        if self.limbs[l].SpeciesIdentity == "Android" {
            if self.random.value() > 0.95 {
                let position = self.physicals[self.limbs[l].PhysicalBehaviour].transform.position;
                self.explosion_creator_explode_at(position, 5.0);
            }
            return;
        }
        self.limbs[l].Health = self.limbs[l].InitialHealth;
        self.limb_heal_bone(l);
        let limb = &mut self.limbs[l];
        limb.DiscomfortingHeatTemperature = f32::MAX;
        limb.IsLethalToBreak = false;
        limb.Vitality = 0.0;
        self.heal_bleeding(l);
        let limb = &mut self.limbs[l];
        limb.LungsPunctured = false;
        limb.ShotDamageMultiplier = 0.0;
        limb.ImpactPainMultiplier = 0.0;
        limb.InternalTemperature = limb.BodyTemperature;
        limb.BreakingThreshold = f32::MAX;
        limb.Numbness = 0.0;
        let acid = self.skins[l].acidProgress - 0.5;
        self.skins[l].set_acid(acid);
        self.limbs[l].RegenerationSpeed = 0.5;
        let c = &mut self.circulations[l];
        c.BleedingRate = 0.0;
        c.IsPump = c.WasInitiallyPumping;
        let blood = self.amount_of_blood(l);
        if blood < 1.0 {
            let id = self.blood_type(l);
            self.add_liquid(Owner::Circulation(l), id, 1.0 - blood);
        }
        if self.limbs[l].HasBrain {
            let p = &mut self.people[self.limbs[l].Person];
            (p.PainLevel, p.ShockLevel, p.Consciousness, p.OxygenLevel, p.Braindead, p.BrainDamaged) = (0.0, 0.0, 1.0, 1.0, false, false);
        }
    }

    /// ZombiePoisonLiquid.OnEnterLimb (the Android test is the caller's)
    pub(crate) fn zombie_poison_on_enter_limb(&mut self, l: usize) {
        let limb = &mut self.limbs[l];
        limb.IsZombie = true;
        limb.Health = limb.InitialHealth;
        limb.Numbness = 0.0;
        let id = self.blood_type(l);
        let amount = self.get_amount(Owner::Circulation(l), id);
        if amount < 0.1 {
            self.add_liquid(Owner::Circulation(l), id, 1.0 - amount);
        }
        self.circulations[l].BloodFlow = 1.0;
        self.heal_bleeding(l);
        let c = &mut self.circulations[l];
        c.BleedingRate = 0.0;
        c.IsPump = c.WasInitiallyPumping;
        if self.limbs[l].RoughClassification == 0 {
            let p = &mut self.people[self.limbs[l].Person];
            (p.Braindead, p.BrainDamaged, p.Consciousness, p.ShockLevel, p.PainLevel, p.OxygenLevel, p.AdrenalineLevel) =
                (false, false, 1.0, 0.0, 0.0, 1.0, 1.0);
        }
    }

    /// OsteomorphosisAgent.OnEnterLimb (the Android test is the caller's)
    pub(crate) fn osteomorphosis_agent_on_enter_limb(&mut self, l: usize) {
        let acid = (d(self.skins[l].acidProgress) + d(0.1f32) * self.random.value()).f();
        self.skins[l].set_acid(acid);
        if self.nodes[l].IsConnectedToRoot {
            self.add_pain(self.limbs[l].Person, 5.0);
        }
    }

    /// MuscleEnhancementSerum.OnEnterLimb (the Android test is the caller's)
    pub(crate) fn muscle_enhancement_serum_on_enter_limb(&mut self, l: usize) {
        self.limbs[l].BreakingThreshold += self.random.value();
        self.limbs[l].BaseStrength += self.random.range_int(0, 8) as f32;
        self.limbs[l].RegenerationSpeed += self.random.range(0.0, 0.5);
    }

    /// VestibularPoison.OnEnterLimb (the Android test is the caller's)
    pub(crate) fn vestibular_poison_on_enter_limb(&mut self, l: usize) {
        self.limbs[l].BalanceMuscleMovement += self.random.range_int(-1, 1) as f32;
        self.limbs[l].DoBalanceJerk ^= self.random.range_int(-1, 1) as f32 > 0.7;
        self.limbs[l].DoStumble ^= self.random.range_int(-1, 1) as f32 > 0.7;
        if self.limbs[l].HasBrain {
            self.circulations[l].InternalBleedingIntensity += self.random.value();
        }
    }

    /// LimbBehaviour.HealBone
    pub fn limb_heal_bone(&mut self, l: usize) {
        self.limbs[l].Broken = false;
        if let Some(h) = self.limbs[l].Joint.filter(|&h| !self.hinges[h].destroyed && self.hinges[h].useLimits) {
            self.limbs[l].JointStress = 0.0;
            let [min, max] = self.limbs[l].originalJointLimits;
            self.hinges[h].set_limits(&mut self.world, min, max);
        }
    }

    /// The limb's object: its GameObject id and its CirculationBehaviour's instance id (StartCoroutine's owner).
    pub(super) fn limb_object(&mut self, l: usize) -> (i32, i32) {
        self.with_component_instance(self.limbs[l].id, |_, inst, o| {
            let k = inst.prefab.objects[o].components.iter().position(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "CirculationBehaviour"));
            (inst.objects[o].game_object, inst.objects[o].components[k.expect("a limb's CirculationBehaviour")])
        })
    }

    pub(super) fn circulation_id(&mut self, l: usize) -> i32 {
        // A recording has no clone: its record's id.
        self.circulations[l].recorded_id.unwrap_or_else(|| self.limb_object(l).1)
    }

    /// GorseBlood.AcidRoutine: the coroutine on the limb's CirculationBehaviour.
    pub(crate) fn acid_routine(&mut self, liquid: usize, l: usize) {
        let owner = self.circulation_id(l);
        let r = Self::acid_routine_body(liquid, l);
        self.start_coroutine(owner, owner, r);
    }

    /// GorseBlood.AcidRoutine's IEnumerator (unstarted).
    pub(crate) fn acid_routine_body(liquid: usize, l: usize) -> super::coroutine::Routine {
        let (mut resumed, mut concentration) = (false, 0.0f32);
        Box::new(move |g| {
            if resumed && concentration <= mathf::EPSILON {
                return Yield::Done;
            }
            if !(g.skins[l].acidProgress < 0.99) {
                g.limbs[l].Health = 0.0;
                g.circulations[l].IsPump = false;
                g.limbs[l].BreakingThreshold = 0.0;
                return Yield::Done;
            }
            concentration = mathf::clamp01((d(g.get_amount(Owner::Circulation(l), liquid)) * 500.0f32).f());
            if g.limbs[l].SpeciesIdentity == "Android" {
                concentration = (d(concentration) * 0.1f32).f();
            }
            let acid = (d(g.skins[l].acidProgress) + d(0.002f32) * concentration).f();
            g.skins[l].set_acid(acid);
            let intensity = (d(150.0f32) * concentration * g.skins[l].acidProgress).f();
            g.limb_wince(l, intensity);
            if g.skins[l].acidProgress > 0.5 {
                if g.nodes[l].IsConnectedToRoot {
                    let pain = (d(0.1f32) * concentration * g.skins[l].acidProgress).f();
                    let person = g.limbs[l].Person;
                    g.add_pain(person, pain);
                }
                let bleeding = (d(g.circulations[l].InternalBleedingIntensity) + d(0.005f32) * concentration).f();
                g.circulations[l].InternalBleedingIntensity = bleeding;
                if g.skins[l].acidProgress > 0.9 {
                    g.break_bone(l);
                }
            }
            resumed = true;
            Yield::WaitForSeconds(0.03)
        })
    }

    /// LifeSerumLiquid.OnEnterLimb (in LifeSyringe)
    pub(crate) fn life_serum_on_enter_limb(&mut self, liquid: usize, l: usize) {
        if self.limbs[l].SpeciesIdentity == "Android" {
            return;
        }
        let person = self.limbs[l].Person;
        if self.limbs[l].HasBrain {
            let p = &mut self.people[person];
            (p.Braindead, p.BrainDamaged, p.BrainDamagedTime, p.SeizureTime) = (false, false, 0.0, 0.0);
        }
        self.life_serum_general_healing(l);
        // StartCoroutine of each.
        let owner = self.circulation_id(l);
        for r in [self.heal_burn(liquid, l), self.heal_wounds(liquid, l), self.heal_rotten_flesh(liquid, l), self.heal_acid_flesh(liquid, l)] {
            self.start_coroutine(owner, owner, r);
        }
        self.limbs[l].LungsPunctured = false;
    }

    /// LifeSerumLiquid.GeneralHealing
    pub(crate) fn life_serum_general_healing(&mut self, l: usize) {
        let person = self.limbs[l].Person;
        self.limb_heal_bone(l);
        self.limbs[l].Health = self.limbs[l].InitialHealth;
        self.limbs[l].Numbness = 0.0;
        self.heal_bleeding(l);
        self.circulations[l].IsPump = self.circulations[l].WasInitiallyPumping;
        self.circulations[l].BloodFlow = 1.0;
        let blood = self.blood_type(l);
        let amount = mathf::max(0.0, 1.0 - self.get_amount(Owner::Circulation(l), blood));
        self.add_liquid(Owner::Circulation(l), blood, amount);
        self.limbs[l].BruiseCount = 0;
        self.circulations[l].GunshotWoundCount = 0;
        self.circulations[l].StabWoundCount = 0;
        if self.limbs[l].RoughClassification == 0 {
            let p = &mut self.people[person];
            (p.Consciousness, p.ShockLevel, p.PainLevel, p.OxygenLevel, p.AdrenalineLevel) = (1.0, 0.0, 0.0, 1.0, 1.0);
        }
    }

    /// LifeSerumLiquid.HealBurn (the coroutine)
    pub(crate) fn heal_burn(&mut self, liquid: usize, l: usize) -> Routine {
        let p = self.limbs[l].PhysicalBehaviour;
        let mut resumed = false;
        Box::new(move |g| {
            if !resumed {
                g.physicals[p].Temperature = g.limbs[l].BodyTemperature;
            } else if g.get_amount(Owner::Circulation(l), liquid) <= mathf::EPSILON {
                return Yield::Done;
            }
            if g.physicals[p].BurnProgress > 0.1 {
                g.physicals[p].BurnProgress = (d(g.physicals[p].BurnProgress) - d(g.time.deltaTime) * 0.16f32).f();
                resumed = true;
                return Yield::WaitForEndOfFrame;
            }
            g.physicals[p].BurnProgress = 0.0;
            Yield::Done
        })
    }

    /// LifeSerumLiquid.HealAcidFlesh
    pub(crate) fn heal_acid_flesh(&mut self, liquid: usize, l: usize) -> Routine {
        let mut resumed = false;
        Box::new(move |g| {
            if resumed && g.get_amount(Owner::Circulation(l), liquid) <= mathf::EPSILON {
                return Yield::Done;
            }
            if g.skins[l].acidProgress > 0.1 {
                let v = (d(g.skins[l].acidProgress) - d(g.time.deltaTime) * 0.8f32).f();
                g.skins[l].set_acid(v);
                resumed = true;
                return Yield::WaitForEndOfFrame;
            }
            g.skins[l].set_acid(0.0);
            Yield::Done
        })
    }

    /// LifeSerumLiquid.HealRottenFlesh
    pub(crate) fn heal_rotten_flesh(&mut self, liquid: usize, l: usize) -> Routine {
        let mut resumed = false;
        Box::new(move |g| {
            if resumed && g.get_amount(Owner::Circulation(l), liquid) <= mathf::EPSILON {
                return Yield::Done;
            }
            if g.skins[l].rottenProgress > 0.1 {
                let v = (d(g.skins[l].rottenProgress) - d(g.time.deltaTime) * 0.8f32).f();
                g.skins[l].set_rotten(v);
                resumed = true;
                return Yield::WaitForEndOfFrame;
            }
            g.skins[l].set_rotten(0.0);
            Yield::Done
        })
    }

    /// LifeSerumLiquid.HealWounds (the coroutine): every damage point's intensity (z) halves per second until all are gone; Sync is rendering.
    pub(crate) fn heal_wounds(&mut self, liquid: usize, l: usize) -> Routine {
        let factor = mathf::pow(0.5, self.time.deltaTime);
        let mut resumed = false;
        Box::new(move |g| {
            if resumed && g.get_amount(Owner::Circulation(l), liquid) <= mathf::EPSILON {
                return Yield::Done;
            }
            if !g.skins[l].damagePoints.iter().any(|p| p[2] > 0.0) {
                return Yield::Done;
            }
            for p in g.skins[l].damagePoints.iter_mut() {
                p[2] *= factor;
            }
            resumed = true;
            Yield::WaitForEndOfFrame
        })
    }

    /// UltraStrengthSerumLiquid.OnEnterLimb (in UltraStrengthSyringe)
    pub(crate) fn ultra_strength_on_enter_limb(&mut self, l: usize) {
        if self.limbs[l].SpeciesIdentity == "Android" {
            return;
        }
        let limb = &mut self.limbs[l];
        limb.Vitality *= 0.1;
        limb.RegenerationSpeed += 8.0;
        limb.ImpactPainMultiplier *= 0.1;
        limb.InitialHealth += 350.0;
        limb.Health = limb.InitialHealth;
        limb.BreakingThreshold += 20.0;
        limb.BaseStrength = mathf::min(15.0, limb.BaseStrength + 5.0);
        limb.ShotDamageMultiplier *= 0.05;
        let c = &mut self.circulations[l];
        c.BloodLossRateMultiplier *= 0.5;
        c.BloodRegenerationPerSecond += 5.0;
        c.BleedingRate *= 0.2;
        c.InternalBleedingIntensity *= 0.2;
        self.limb_wince(l, 5.0);
        let p = self.limbs[l].PhysicalBehaviour;
        let value = self.random.value();
        let ph = &mut self.physicals[p];
        ph.BulletDeflectChanceOverride = if ph.BulletDeflectChanceOverride.is_nan() { value } else { ph.BulletDeflectChanceOverride + value };
        if self.limbs[l].Color[1] > 0.95 && self.random.value() < 0.1 {
            // limb.Color *= (Color)new Vector4(0.95f, 0.92f, 1f, 1f)
            let mut color = self.limbs[l].Color;
            for (c, k) in color.iter_mut().zip([0.95, 0.92, 1.0, 1.0]) {
                *c *= k;
            }
            self.limb_set_color(l, color);
        }
    }

    /// MendingSerum.OnEnterLimb (nested in MendingSyringe)
    pub(crate) fn mending_serum_on_enter_limb(&mut self, l: usize) {
        if self.limbs[l].SpeciesIdentity == "Android" {
            return;
        }
        if self.limbs[l].HasBrain {
            let person = self.limbs[l].Person;
            self.people[person].Braindead = false;
            if self.random.value() > 0.995 {
                self.people[person].BrainDamaged = false;
            }
        }
        self.limbs[l].Health = self.limbs[l].InitialHealth;
        self.circulations[l].BleedingRate *= 0.8;
        self.circulations[l].InternalBleedingIntensity *= 0.8;
        let rotten = self.skins[l].rottenProgress * 0.8;
        self.skins[l].set_rotten(rotten);
        self.limbs[l].LungsPunctured = false;
        self.limb_heal_bone(l);
    }

    /// PainKillerLiquid.KillPain (nested in PainKillerSyringe)
    pub(crate) fn kill_pain(&mut self, liquid: usize, l: usize) {
        if self.limbs[l].SpeciesIdentity == "Android" {
            return;
        }
        self.limbs[l].Numbness += 0.1;
        if !self.nodes[l].IsConnectedToRoot {
            return;
        }
        let person = self.limbs[l].Person;
        let p = &mut self.people[person];
        p.PainLevel *= 0.1;
        p.ShockLevel *= 0.1;
        p.SeizureTime *= 0.5;
        let amount = self.get_amount(Owner::Circulation(l), liquid);
        if amount > 0.4 {
            self.influence_motor_speed(l, 0.0, 0.5);
            let intensity = self.random.value() * 300.0;
            self.person_wince(person, intensity);
            if amount > 0.5 {
                let acid = self.skins[l].acidProgress + 0.05;
                self.skins[l].set_acid(acid);
                let damage = (d(self.limbs[l].Health) * 0.5f32 + 0.1f32).f();
                self.limb_damage(l, damage);
            }
        }
    }
}

#[cfg(test)]
mod small_tests {
    use super::*;

    #[test]
    fn limits_and_mending_removal() {
        assert_eq!(syringe_limits(), [0.0, 1.4]);
        assert_eq!(mending_serum_removal_chance_per_second(), 0.8f32 * 0.8f32);
        assert_eq!(osteomorphosis_agent_removal_chance_per_second(), 0.8f32 * 0.1f32);
    }
}

/// TemporaryBodyLiquid.get_ShouldCallOnEnterEveryUpdate: true; OsteomorphosisAgent's too. PinkSyringe's VestibularPoison,
/// MusclePoison, NumbingPoison, ReflectionPoison, TransparencyPoison, CrushingPoison, MassManipulationPoison, ExplosionPoison,
/// DurabilitySerum, RegenerationSerum, SizeManipulationPoison, CirculationPoison, TissueDeconstructionAgent and
/// MuscleEnhancementSerum override it to false.
pub fn should_call_on_enter_every_update(class: &str) -> bool {
    !matches!(
        class,
        "VestibularPoison" | "MusclePoison" | "NumbingPoison" | "ReflectionPoison" | "TransparencyPoison" | "CrushingPoison"
            | "MassManipulationPoison" | "ExplosionPoison" | "DurabilitySerum" | "RegenerationSerum" | "SizeManipulationPoison"
            | "CirculationPoison" | "TissueDeconstructionAgent" | "MuscleEnhancementSerum"
    )
}

/// SyringeExplosionBehaviour.results: static Collider2D[64] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const SYRINGE_EXPLOSION_RESULTS: usize = 64;
