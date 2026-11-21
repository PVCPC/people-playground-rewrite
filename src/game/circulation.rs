//! BloodContainer, CirculationBehaviour and the Liquid registry.
use super::container::{BloodContainer, Owner};
use super::{d, mathf, script, Game};
use super::misc::{BleedingParticle, FancyBloodSplat, FixedIntervalDistributor};

// Liquid.ClearLiquidRegistry: Global.Awake empties Registry and LiquidSet, then Register refills them in this order (a Game starts with exactly this table).
/// Liquid.Registry / Liquid.LiquidSet order (Global.Awake); a liquid is its index here.
/// Each liquid's identity (the registry key) is the k-th ldstr of Global.Awake, which registers them in this order.
pub static LIQUIDS: std::sync::LazyLock<Vec<(&'static str, &'static str)>> = std::sync::LazyLock::new(|| {
    let table = crate::assets::il::get();
    liquid_classes().into_iter().enumerate().map(|(k, c)| (table.str("Global::Awake", k), c)).chain(std::iter::once(("CRAB BLOOD", "CrabBlood"))) // extras (not in the real game): the crab's blood, after the game's liquids.
        .collect()
});

/// LIQUID_CLASSES; 1.28.45's Global.Awake registers Nasumica, Amaraevelum and Dulcamarus right after Coolant.
fn liquid_classes() -> Vec<&'static str> {
    let mut classes = LIQUID_CLASSES.to_vec();
    if crate::assets::il::is_1_28_45() {
        let k = classes.iter().position(|&c| c == "Coolant").unwrap() + 1;
        classes.splice(k..k, ["Nasumica", "Amaraevelum", "Dulcamarus"]);
    }
    classes
}

/// The registry index of the liquid of class `class` (the class names do not move between versions; the ldstr indices do).
pub fn liquid_of_class(class: &str) -> usize {
    LIQUIDS.iter().position(|(_, c)| *c == class).unwrap_or_else(|| panic!("no liquid of class {class}"))
}

/// The Liquid class of each registry entry.
const LIQUID_CLASSES: &[&str] = &[
    "Blood",
    "GorseBlood",
    "Oil",
    "Nitroglycerine",
    "ExoticLiquid",
    "Tritium",
    "InertLiquid",
    "BeverageM04",
    "Coolant",
    "ZombiePoisonLiquid",
    "AdrenalineLiquid",
    "AcidLiquid",
    "BoneHurtingJuiceLiquid",
    "CoagulationLiquid",
    "InstantDeathPoisonLiquid",
    "FreezePoisonLiquid",
    "KnockoutPoisonLiquid",
    "LifeSerumLiquid",
    "UltraStrengthSerumLiquid",
    "MendingSerum",
    "WaterBreathingSerum",
    "OsteomorphosisAgent",
    "ImmortalitySerum",
    "PainKillerLiquid",
    "PinkDormantLiquid",
    "VestibularPoison",
    "MusclePoison",
    "NumbingPoison",
    "ReflectionPoison",
    "TransparencyPoison",
    "MassManipulationPoison",
    "ExplosionPoison",
    "CrushingPoison",
    "DurabilitySerum",
    "RegenerationSerum",
    "SizeManipulationPoison",
    "CirculationPoison",
    "CombustionAgent",
    "MuscleEnhancementSerum",
    "TissueDeconstructionAgent",
    "DebugDiscolorationLiquid",
];

/// Nasumica.GetDisplayName, Amaraevelum.GetDisplayName, Dulcamarus.GetDisplayName, ZombiePoisonLiquid.GetDisplayName, TissueDeconstructionAgent.GetDisplayName, OsteomorphosisAgent.GetDisplayName, Coolant.GetDisplayName, CombustionAgent.GetDisplayName, WaterBreathingSerum.GetDisplayName, VestibularPoison.GetDisplayName, AcidLiquid.GetDisplayName, AdrenalineLiquid.GetDisplayName, Blood.GetDisplayName, BoneHurtingJuiceLiquid.GetDisplayName, BeverageM04.GetDisplayName, CirculationPoison.GetDisplayName, CoagulationLiquid.GetDisplayName, CrushingPoison.GetDisplayName, DebugDiscolorationLiquid.GetDisplayName, DurabilitySerum.GetDisplayName, ExoticLiquid.GetDisplayName, ExplosionPoison.GetDisplayName, FreezePoisonLiquid.GetDisplayName, GorseBlood.GetDisplayName, ImmortalitySerum.GetDisplayName, InertLiquid.GetDisplayName, InstantDeathPoisonLiquid.GetDisplayName, KnockoutPoisonLiquid.GetDisplayName, LifeSerumLiquid.GetDisplayName, MassManipulationPoison.GetDisplayName, MendingSerum.GetDisplayName, MuscleEnhancementSerum.GetDisplayName, MusclePoison.GetDisplayName, Nitroglycerine.GetDisplayName, NumbingPoison.GetDisplayName, Oil.GetDisplayName, PainKillerLiquid.GetDisplayName, PinkDormantLiquid.GetDisplayName, ReflectionPoison.GetDisplayName, RegenerationSerum.GetDisplayName, SizeManipulationPoison.GetDisplayName, TransparencyPoison.GetDisplayName, Tritium.GetDisplayName, UltraStrengthSerumLiquid.GetDisplayName, Liquid.GetDisplayName: a liquid's name as the Liquidentifier lists it (the base returns GetIdentity(this), the registry key).
pub fn liquid_display_name(l: usize) -> &'static str {
    match LIQUIDS[l].1 {
        "Blood" => crate::il!(str "Blood::GetDisplayName" 0),
        "GorseBlood" => crate::il!(str "GorseBlood::GetDisplayName" 0),
        "CrabBlood" => "Crab blood",
        "Oil" => "Oil",
        "Nitroglycerine" => "Nitroglycerine",
        "ExoticLiquid" => crate::il!(str "Chemistry/ExoticLiquid::GetDisplayName" 0),
        "Tritium" => "Tritium",
        "InertLiquid" => crate::il!(str "Chemistry/InertLiquid::GetDisplayName" 0),
        "BeverageM04" => crate::il!(str "Chemistry/BeverageM04::GetDisplayName" 0),
        "Coolant" => "Coolant",
        "Nasumica" => crate::il45!(str "Chemistry/Nasumica::GetDisplayName" 0),
        "Amaraevelum" => crate::il45!(str "Chemistry/Amaraevelum::GetDisplayName" 0),
        "Dulcamarus" => crate::il45!(str "Chemistry/Dulcamarus::GetDisplayName" 0),
        "ZombiePoisonLiquid" => crate::il!(str "ZombieSyringe/ZombiePoisonLiquid::GetDisplayName" 0),
        "AdrenalineLiquid" => "Adrenaline",
        "AcidLiquid" => "Acid",
        "BoneHurtingJuiceLiquid" => crate::il!(str "BoneEatingPoisonSyringe/BoneHurtingJuiceLiquid::GetDisplayName" 0),
        "CoagulationLiquid" => crate::il!(str "CoagulationSyringe/CoagulationLiquid::GetDisplayName" 0),
        "InstantDeathPoisonLiquid" => crate::il!(str "DeathSyringe/InstantDeathPoisonLiquid::GetDisplayName" 0),
        "FreezePoisonLiquid" => crate::il!(str "FreezeSyringe/FreezePoisonLiquid::GetDisplayName" 0),
        "KnockoutPoisonLiquid" => crate::il!(str "KnockoutSyringe/KnockoutPoisonLiquid::GetDisplayName" 0),
        "LifeSerumLiquid" => crate::il!(str "LifeSyringe/LifeSerumLiquid::GetDisplayName" 0),
        "UltraStrengthSerumLiquid" => crate::il!(str "UltraStrengthSyringe/UltraStrengthSerumLiquid::GetDisplayName" 0),
        "MendingSerum" => crate::il!(str "MendingSyringe/MendingSerum::GetDisplayName" 0),
        "WaterBreathingSerum" => crate::il!(str "WaterBreathingSyringe/WaterBreathingSerum::GetDisplayName" 0),
        "OsteomorphosisAgent" => crate::il!(str "OsteomorphosisAgent::GetDisplayName" 0),
        "ImmortalitySerum" => crate::il!(str "ImmortalitySerum::GetDisplayName" 0),
        "PainKillerLiquid" => "Painkiller",
        "PinkDormantLiquid" => crate::il!(str "PinkSyringe/PinkDormantLiquid::GetDisplayName" 0),
        "VestibularPoison" => crate::il!(str "PinkSyringe/VestibularPoison::GetDisplayName" 0),
        "MusclePoison" => crate::il!(str "PinkSyringe/MusclePoison::GetDisplayName" 0),
        "NumbingPoison" => crate::il!(str "PinkSyringe/NumbingPoison::GetDisplayName" 0),
        "ReflectionPoison" => crate::il!(str "PinkSyringe/ReflectionPoison::GetDisplayName" 0),
        "TransparencyPoison" => crate::il!(str "PinkSyringe/TransparencyPoison::GetDisplayName" 0),
        "MassManipulationPoison" => crate::il!(str "PinkSyringe/MassManipulationPoison::GetDisplayName" 0),
        "ExplosionPoison" => crate::il!(str "PinkSyringe/ExplosionPoison::GetDisplayName" 0),
        "CrushingPoison" => crate::il!(str "PinkSyringe/CrushingPoison::GetDisplayName" 0),
        "DurabilitySerum" => crate::il!(str "PinkSyringe/DurabilitySerum::GetDisplayName" 0),
        "RegenerationSerum" => crate::il!(str "PinkSyringe/RegenerationSerum::GetDisplayName" 0),
        "SizeManipulationPoison" => crate::il!(str "PinkSyringe/SizeManipulationPoison::GetDisplayName" 0),
        "CirculationPoison" => crate::il!(str "PinkSyringe/CirculationPoison::GetDisplayName" 0),
        "CombustionAgent" => crate::il!(str "PinkSyringe/CombustionAgent::GetDisplayName" 0),
        "MuscleEnhancementSerum" => crate::il!(str "PinkSyringe/MuscleEnhancementSerum::GetDisplayName" 0),
        "TissueDeconstructionAgent" => crate::il!(str "PinkSyringe/TissueDeconstructionAgent::GetDisplayName" 0),
        "DebugDiscolorationLiquid" => crate::il!(str "Chemistry/DebugDiscolorationLiquid::GetDisplayName" 0),
        c => unreachable!("{c} overrides GetDisplayName"),
    }
}

/// PinkSyringe.PinkSyringeColor (static).
pub const PINK_SYRINGE_COLOR: [f32; 4] = [1.0, 0.4196079, 0.7944902, 1.0];

/// Liquid.Color per LIQUIDS entry, as each Liquid constructor builds it.
pub fn liquid_color(l: usize) -> [f32; 4] {
    match LIQUIDS[l].1 {
        "Blood" => [0.65, 0.0, 0.0, 1.0],
        "GorseBlood" => [0.03, 0.57, 0.0, 1.0],
        "CrabBlood" => ppg_extras::CRAB_BLOOD_COLOUR,
        "Oil" => [0.0, 0.04689861, 0.1803922, 1.0],
        "Nitroglycerine" => [245.0 / 255.0, 152.0 / 255.0, 66.0 / 255.0, 1.0],
        "ExoticLiquid" => [0.0, 0.0, 1.0, 1.0],
        "Tritium" => [0.08 * 2.0, 0.62 * 2.0, 0.28 * 2.0, 2.0],
        "InertLiquid" => [1.0, 1.0, 1.0, 0.1],
        "BeverageM04" => [0.91, 0.25, 0.0, 0.5],
        "Coolant" => [0.0, 0.22, 0.6, 0.74],
        "Nasumica" => [0.64, 0.84, 0.74, 0.1],
        "Amaraevelum" => [0.58, 0.18, 0.16, 0.2],
        // new Color(0.8f, 0.9f, 1f, 1f) * 2.5f: every component, alpha too.
        "Dulcamarus" => [0.8 * 2.5, 0.9 * 2.5, 1.0 * 2.5, 1.0 * 2.5],
        "ZombiePoisonLiquid" => [0.8835177, 1.0, 0.7122642, 1.0],
        "AdrenalineLiquid" => [0.45, 0.27, 0.77, 1.0],
        "AcidLiquid" => [0.49, 0.44, 0.52, 1.0],
        "BoneHurtingJuiceLiquid" => [1.0, 0.55, 0.22, 1.0],
        "CoagulationLiquid" => [1.0, 1.0, 0.0, 1.0],
        "InstantDeathPoisonLiquid" => [1.0, 0.65, 0.63, 1.0],
        "FreezePoisonLiquid" => [0.8, 1.0, 1.0, 1.0],
        "KnockoutPoisonLiquid" => [0.68, 0.62, 0.541, 1.0],
        "LifeSerumLiquid" => [0.6745283, 1.0, 0.7830077, 1.0],
        "UltraStrengthSerumLiquid" => [0.7604228, 0.7137255, 1.0, 1.0],
        "MendingSerum" => [0.0, 0.67, 0.5, 1.0],
        "WaterBreathingSerum" => [0.21, 0.57, 0.96, 1.0],
        "OsteomorphosisAgent" => [1.0, 0.84, 0.56, 1.0],
        "ImmortalitySerum" => [0.87, 1.0, 0.0, 1.0],
        "PainKillerLiquid" => [0.71, 0.75, 0.65, 0.1],
        "DebugDiscolorationLiquid" => [1.0, 0.0, 1.0, 1.0],
        // PinkSyringe.PinkSyringeColor
        _ => PINK_SYRINGE_COLOR,
    }
}

/// Utils.ChangeRedToOrange
pub fn change_red_to_orange(v: [f32; 4]) -> [f32; 4] {
    let h = rgb_to_hsv(v)[0];
    if (h - 0.0).abs() < 0.03 || (h - 1.0).abs() < 0.03 {
        [0.87, 0.6, 0.18, 1.0]
    } else {
        v
    }
}

pub fn rgb_to_hsv(c: [f32; 4]) -> [f32; 3] {
    let (offset, dominant, one, two) = if c[2] > c[1] && c[2] > c[0] {
        (4.0, c[2], c[0], c[1])
    } else if c[1] > c[0] {
        (2.0, c[1], c[2], c[0])
    } else {
        (0.0, c[0], c[1], c[2])
    };
    let v = dominant;
    if v == 0.0 {
        return [0.0, 0.0, v];
    }
    let num = if one > two { two } else { one };
    let num2 = v - num;
    let (mut h, s) = if num2 != 0.0 {
        ((d(offset) + (d(one) - two) / num2).f(), num2 / v)
    } else {
        ((d(offset) + (d(one) - two)).f(), 0.0)
    };
    h /= 6.0;
    if h < 0.0 {
        h += 1.0;
    }
    [h, s, v]
}

pub fn liquid(id: &str) -> usize {
    LIQUIDS.iter().position(|(n, _)| *n == id).unwrap_or_else(|| panic!("no liquid {id}"))
}

/// LiquidMixInstructions: SourceLiquids and TargetLiquid as registry indices (LIQUIDS); ContainerFilter is either null or
/// LiquidMixingController's `c => !(c is CirculationBehaviour)` (only_outside_circulations).
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidMixInstructions {
    pub SourceLiquids: Vec<usize>,
    pub TargetLiquid: usize,
    pub RatePerSecond: f32,
    pub only_outside_circulations: bool,
}

impl Default for LiquidMixInstructions {
    /// The field initialisers alone (RatePerSecond = 0.05f).
    fn default() -> Self {
        Self { SourceLiquids: Vec::new(), TargetLiquid: 0, RatePerSecond: 0.05, only_outside_circulations: false }
    }
}

impl LiquidMixInstructions {
    /// LiquidMixInstructions..ctor(Liquid[] / sourceA, sourceB[, sourceC], targetLiquid, ratePerSecond = 0.05f): every overload
    /// sets SourceLiquids, TargetLiquid and RatePerSecond; ContainerFilter stays null.
    pub fn new(sources: Vec<usize>, target: usize, rate: f32) -> Self {
        Self { SourceLiquids: sources, TargetLiquid: target, RatePerSecond: rate, only_outside_circulations: false }
    }
}

/// LiquidMixingController..cctor: LiquidMixingController.MixInstructions as Global.Awake fills it.
pub fn mix_instructions() -> Vec<LiquidMixInstructions> {
    let l = liquid;
    // Liquid.GetLiquid(id) of a liquid registered by Global.Awake, by its class (the ldstr index moves in 1.28.45).
    let c = liquid_of_class;
    let outside = |m: LiquidMixInstructions| LiquidMixInstructions { only_outside_circulations: true, ..m };
    let mut v = vec![
        outside(LiquidMixInstructions::new(vec![l("TRITIUM"), c("MendingSerum")], c("ImmortalitySerum"), 0.01)),
        outside(LiquidMixInstructions::new(vec![l("ACID"), c("MendingSerum")], c("OsteomorphosisAgent"), 0.05)),
        LiquidMixInstructions::new(vec![c("WaterBreathingSerum"), l("TRITIUM")], c("ExoticLiquid"), 0.05),
        LiquidMixInstructions::new(vec![l("ADRENALINE"), c("MendingSerum"), c("InertLiquid")], c("BeverageM04"), 0.05),
    ];
    if crate::assets::il::is_1_28_45() {
        // 1.28.45: (CIRCULATION POISON, REANIMATION AGENT, FREEZE POISON, INSTANT DEATH POISON) -> NASUMICA with
        // ContainerFilter c => !(c is CirculationBehaviour); (COOLANT, EXOTIC LIQUID, MENDING SERUM) -> AMARAEVELUM;
        // (NASUMICA, AMARAEVELUM) -> DULCAMARUS; each at the default 0.05.
        v.push(outside(LiquidMixInstructions::new(vec![c("CirculationPoison"), c("ZombiePoisonLiquid"), c("FreezePoisonLiquid"), c("InstantDeathPoisonLiquid")], c("Nasumica"), 0.05)));
        v.push(LiquidMixInstructions::new(vec![c("Coolant"), c("ExoticLiquid"), c("MendingSerum")], c("Amaraevelum"), 0.05));
        v.push(LiquidMixInstructions::new(vec![c("Nasumica"), c("Amaraevelum")], c("Dulcamarus"), 0.05));
    }
    v
}

/// BloodContainer.RefFloat
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct RefFloat {
    pub Previous: f32,
    pub raw: f32,
}

impl RefFloat {
    /// RefFloat..ctor(v): Raw = v.
    pub fn new(v: f32) -> Self {
        let mut r = Self { Previous: 0.0, raw: 0.0 };
        r.set_raw(v);
        r
    }
    /// RefFloat.set_Raw: raw = Clamp(value, 0, float.MaxValue).
    pub fn set_raw(&mut self, v: f32) {
        self.raw = mathf::clamp(v, 0.0, f32::MAX);
    }
    /// RefFloat.HasDecreased
    pub fn has_decreased(&self, threshold: f32) -> bool {
        self.Previous - self.raw > threshold
    }
    /// RefFloat.HasChanged
    pub fn has_changed(&self) -> bool {
        (self.Previous - self.raw).abs() > 0.0001
    }
}

/// Instance ids of one BleedingParticle prefab clone: the root (GameObject, Transform, ParticleSystem,
/// ParticleSystemRenderer, BleedingParticleBehaviour), Trail (5), extraDecals (GameObject, Transform,
/// FancyBloodSplatController, Optout, ActivateIfFancyEffects, DrippingBloodController) and water (GameObject,
/// Transform, ParticleSystemRenderer, ParticleSystem, ActivateIfFancyEffects, Optout).
pub const BLEEDING_PARTICLE_OBJECTS: usize = 22;

script! {
    Circulation {
        IsPump: bool = false,
        BloodLossRateMultiplier: f32 = 1.0,
        BleedingRate: f32 = 0.0,
        IsDisconnected: bool = false,
        WasInitiallyPumping: bool = false,
        BloodFlow: f32 = 0.0,
        BloodRegenerationPerSecond: f32 = 0.0025,
        BleedingPointCount: u8 = 0,
        StabWoundCount: u16 = 0,
        GunshotWoundCount: u16 = 0,
        InternalBleedingIntensity: f32 = 0.0,
        ImmuneToDamage: bool = false,
        diffuseOscillator: bool = false,
        canUnlockGlowingHumanAchievement: bool = true,
        NewlySpawned: bool = true,
        ArtificialHeartbeat: f32 = 0.0,
        cachedHeartRate: Option<f32> = None,
    }
    base { base: BloodContainer }
    links {
        /// A recording's CirculationBehaviour instance id.
        recorded_id: Option<i32> = None,
        /// Source / PushesTo as limb indices.
        Source: Option<usize> = None,
        PushesTo: Vec<usize> = Vec::new(),
        /// Bounds (center xyz, extents xyz) in limb space.
        LocalArteryRects: Option<Vec<[f32; 6]>> = None,
        /// Bleeding particle indices (Game::bleeding).
        BleedingParticles: Vec<usize> = Vec::new(),
        enabled: bool = true,
    }
}

impl Game {
    pub fn blood_type(&self, l: usize) -> usize {
        liquid(&self.limbs[l].BloodLiquidType)
    }

    /// CirculationBehaviour.GetAmountOfBlood
    pub fn amount_of_blood(&self, l: usize) -> f32 {
        let c = &self.circulations[l];
        c.base.LiquidDistribution.get(self.blood_type(l)).map_or(0.0, |r| r.raw)
    }

    /// CirculationBehaviour.HasCirculation
    pub fn has_circulation(&self, l: usize) -> bool {
        self.circulations[l].BloodFlow > 0.01 && self.amount_of_blood(l) > 0.01
    }

    /// CirculationBehaviour.get_HasBloodFlow: BloodFlow > 0.05.
    #[allow(dead_code)]
    pub fn has_blood_flow(&self, l: usize) -> bool {
        self.circulations[l].BloodFlow > 0.05
    }

    /// CirculationBehaviour.get_IsConnectedToMainBody (obsolete): the limb's node IsConnectedToRoot (every limb has a node).
    #[allow(dead_code)]
    pub fn is_connected_to_main_body(&self, l: usize) -> bool {
        self.nodes[l].IsConnectedToRoot
    }

    /// CirculationBehaviour.OnImpactCreated(gm): gm's BloodImpactBehaviour (`blood_puffs` index; None when gm has none)
    /// gets the computed colour when the limb holds more than 0.05 scaled blood, else the original blood colour. The
    /// SetColor result.
    pub fn circulation_on_impact_created(&mut self, l: usize, gm: Option<usize>) -> Option<[f32; 4]> {
        let b = gm?;
        let original = self.original_blood_color(l);
        let color = if self.scaled_liquid_amount(super::container::Owner::Circulation(l)) > 0.05 {
            self.get_computed_color(super::container::Owner::Circulation(l), original)
        } else {
            original
        };
        Some(self.blood_impact_set_color(b, color))
    }

    /// CirculationBehaviour.GetHeartRate
    pub fn heart_rate(&mut self, l: usize) -> f32 {
        if !self.circulations[l].IsPump || !self.limb_is_considered_alive(l) {
            return self.circulations[l].ArtificialHeartbeat;
        }
        if let Some(v) = self.circulations[l].cachedHeartRate {
            return v;
        }
        let connected = self.nodes[l].IsConnectedToRoot;
        let c = &self.circulations[l];
        let p = &self.people[self.limbs[l].Person];
        let num = mathf::clamp01(c.BloodFlow);
        let t = mathf::clamp01(c.base.TotalLiquidAmount);
        let pick = |v: f32| if connected { mathf::clamp01(v) } else { 0.0 };
        let (t2, num2, num3, num4, num5) =
            (pick(p.OxygenLevel), pick(p.Consciousness), pick(p.PainLevel), pick(p.AdrenalineLevel), pick(p.ShockLevel));
        // Mono evaluates each float expression in double, rounding at stores and call arguments.
        let mut num6 = mathf::lerp(0.0, 95.0, (d(num4) + num5 + num3).f());
        num6 += if connected { mathf::clamp(p.SeizureTime * 0.5, 0.0, 30.0) } else { 0.0 };
        num6 = (d(num6) + d(mathf::perlin(self.time.time / 7.0, self.limbs[l].randomOffset)) * 10.0f32).f();
        let v = mathf::max(
            c.ArtificialHeartbeat,
            ((d(mathf::lerp(0.0, (70.0f32 - (d(1.0f32) - num2) * 22.0f32).f(), t)) + num6) * mathf::lerp(0.5, 1.0, t2) * num).f(),
        );
        self.circulations[l].cachedHeartRate = Some(v);
        v
    }

    // ---- CirculationBehaviour ----

    /// CirculationBehaviour.Disintegrate
    pub fn circulation_disintegrate(&mut self, l: usize) {
        self.limbs[l].BreakingThreshold = 0.0;
        self.circulations[l].BleedingRate += 20.0;
        for i in 0..self.circulations[l].PushesTo.len() {
            let other = self.circulations[l].PushesTo[i];
            if self.node_is_connected_to(l, other) && !self.physicals[self.limbs[other].PhysicalBehaviour].isDisintegrated {
                self.circulations[other].BleedingRate += 20.0;
            }
        }
        if let Some(source) = self.circulations[l].Source {
            if self.node_is_connected_to(l, source) {
                self.circulations[source].BleedingRate += 20.0;
            }
        }
        self.circulations[l].IsDisconnected = true;
    }

    /// CirculationBehaviour.FixedUpdate
    pub fn circulation_fixed_update(&mut self, l: usize) {
        self.circulations[l].cachedHeartRate = None;
        let fdt = self.time.fixedDeltaTime;
        if self.limbs[l].InternalTemperature <= self.limbs[l].BodyTemperature - 9.0 && self.random.value() > 0.999 {
            self.circulations[l].IsPump = false;
        }
        let person = self.limbs[l].Person;
        if self.nodes[l].IsConnectedToRoot && self.people[person].SeizureTime > 0.0 && self.random.value() > 0.999 {
            self.circulations[l].IsPump = false;
        }
        self.pump_behaviour(l, fdt);
        self.handle_bleeding(l, fdt);
        self.handle_penetration_bleeding(l);
        self.handle_damage_edge_cases(l);
        let c = &mut self.circulations[l];
        c.BloodFlow = mathf::clamp01(c.BloodFlow);
        // Mathf.Max's argument: the subtraction is evaluated in double and rounded once.
        c.InternalBleedingIntensity = mathf::max(0.0, (d(c.InternalBleedingIntensity) - d(fdt) * 0.01f32).f());
        self.diffuse_step(l);
        self.flow_step(l);
        let c = &self.circulations[l];
        if !c.IsDisconnected
            && c.BleedingRate < 1.0
            && c.BloodRegenerationPerSecond >= mathf::EPSILON
            && self.limbs[l].Health > 0.1
            && self.amount_of_blood(l) < 1.0
        {
            let amount = c.BloodRegenerationPerSecond * fdt;
            let blood = self.blood_type(l);
            self.add_liquid(Owner::Circulation(l), blood, amount);
        }
        if self.circulations[l].canUnlockGlowingHumanAchievement && self.get_amount(Owner::Circulation(l), liquid("TRITIUM")) > mathf::EPSILON {
            super::stats::unlock_achievement("GLOWING_HUMAN");
        }
    }

    /// CirculationBehaviour.PumpBehaviour
    pub(crate) fn pump_behaviour(&mut self, l: usize, dt: f32) {
        let charge = self.physicals[self.limbs[l].PhysicalBehaviour].Charge();
        if self.circulations[l].IsPump {
            if charge > 0.5 && self.random.value() as f64 > 0.995 - (charge / 500.0) as f64 {
                self.circulations[l].IsPump = false;
            }
        } else if self.circulations[l].WasInitiallyPumping
            && self.random.value() as f64 > 0.999
            && charge > 0.001
            && self.circulations[l].BloodFlow < 0.1
        {
            self.circulations[l].IsPump = true;
        }
        let c = &mut self.circulations[l];
        if c.IsPump {
            c.BloodFlow = c.base.TotalLiquidAmount;
        } else if c.WasInitiallyPumping {
            c.BloodFlow = (d(c.BloodFlow) - d(dt) * 0.2f32).f();
        } else {
            c.BloodFlow = (d(c.BloodFlow) - d(dt) * 0.05f32).f();
        }
        c.BloodFlow = mathf::clamp(c.BloodFlow, 0.0, c.base.TotalLiquidAmount);
    }

    /// CirculationBehaviour.HandleDamageEdgeCases
    pub(crate) fn handle_damage_edge_cases(&mut self, l: usize) {
        let ph = &self.physicals[self.limbs[l].PhysicalBehaviour];
        let num = mathf::max(ph.BurnProgress, self.skins[l].acidProgress);
        if num > 0.8 && ph.Temperature > 0.0 {
            self.drain(Owner::Circulation(l), mathf::lerp(0.02, 0.1, mathf::map_range(0.8, 1.0, 0.0, 1.0, num)));
        }
    }

    /// CirculationBehaviour.HandleBleeding
    pub(crate) fn handle_bleeding(&mut self, l: usize, dt: f32) {
        let temperature = self.physicals[self.limbs[l].PhysicalBehaviour].Temperature;
        let c = &mut self.circulations[l];
        if c.BleedingRate > 0.05 && temperature > 0.0 {
            if c.BleedingRate < 1.0 {
                c.BleedingRate = (d(c.BleedingRate) - d(self.time.deltaTime) * 0.05f32).f();
            }
            let amount = (d(dt) / mathf::lerp(60.0, 20.0, c.BloodFlow) * c.BleedingRate * 0.2f32).f();
            self.drain(Owner::Circulation(l), amount);
        }
    }

    /// CirculationBehaviour.FlowStep
    pub(crate) fn flow_step(&mut self, l: usize) {
        for i in 0..self.circulations[l].PushesTo.len() {
            let other = self.circulations[l].PushesTo[i];
            let flow = self.circulations[l].BloodFlow;
            if !self.physicals[self.limbs[other].PhysicalBehaviour].isDisintegrated
                && self.node_is_connected_to(l, other)
                && self.circulations[other].BloodFlow < flow
                && !self.circulations[other].IsDisconnected
            {
                let c = &mut self.circulations[other];
                c.BloodFlow = mathf::lerp(c.BloodFlow, flow, 0.9);
            }
        }
    }

    /// CirculationBehaviour.DiffuseStep
    pub(crate) fn diffuse_step(&mut self, l: usize) {
        let rate = self.time.fixedDeltaTime * 1.5;
        let n = self.limbs[l].ConnectedLimbs.len();
        if self.circulations[l].diffuseOscillator {
            for i in 0..n {
                self.diffuse_to_limb(l, self.limbs[l].ConnectedLimbs[i], rate);
            }
        } else {
            for i in (0..n).rev() {
                self.diffuse_to_limb(l, self.limbs[l].ConnectedLimbs[i], rate);
            }
        }
        let c = &mut self.circulations[l];
        c.diffuseOscillator = !c.diffuseOscillator;
    }

    /// CirculationBehaviour.DiffuseToLimb
    pub(crate) fn diffuse_to_limb(&mut self, l: usize, other: usize, rate: f32) {
        if !self.physicals[self.limbs[other].PhysicalBehaviour].isDisintegrated && other != l && self.node_is_connected_to(l, other) {
            if !(self.circulations[other].base.TotalLiquidAmount > self.circulations[l].base.TotalLiquidAmount) {
                self.transfer_to(Owner::Circulation(l), rate, Owner::Circulation(other));
            }
        }
    }

    /// LimbBehaviour.GetOriginalBloodType().Color
    pub fn original_blood_color(&self, l: usize) -> [f32; 4] {
        liquid_color(self.blood_type(l))
    }

    /// CirculationBehaviour.HandlePenetrationBleeding
    pub(crate) fn handle_penetration_bleeding(&mut self, l: usize) {
        let p = self.limbs[l].PhysicalBehaviour;
        for i in 0..self.physicals[p].victimPenetrations.len() {
            let pen = self.physicals[p].victimPenetrations[i];
            if self.penetration_arena[pen].Active && self.physicals[self.penetration_arena[pen].Stabber].StabCausesWound {
                let depth = (d(self.penetration_current_depth(pen)) * 4.0f32).f();
                self.circulations[l].BleedingRate = mathf::max(self.circulations[l].BleedingRate, depth);
            }
        }
    }

    /// CirculationBehaviour.Stabbed
    pub fn circulation_stabbed(&mut self, l: usize, s: super::stab::Stabbing) {
        if !self.circulations[l].ImmuneToDamage && !self.limbs[l].IsZombie && self.physicals[s.stabber].StabCausesWound {
            let c = &mut self.circulations[l];
            c.StabWoundCount = c.StabWoundCount.wrapping_add(1);
            c.InternalBleedingIntensity = (d(c.InternalBleedingIntensity) + 0.1f32).f();
            if self.circulations[l].IsPump && self.random.value() < 0.6 && self.is_world_point_in_vital_part(l, s.point) {
                self.circulations[l].IsPump = false;
            }
        }
    }

    /// CirculationBehaviour.Unstabbed
    pub fn circulation_unstabbed(&mut self, l: usize, s: super::stab::Stabbing) {
        if !self.circulations[l].ImmuneToDamage && self.physicals[s.stabber].StabCausesWound {
            let flag = self.is_world_point_in_artery_rect(l, s.point);
            self.circulations[l].BleedingRate += if flag { 4.0 } else { 1.0 };
            self.create_bleeding_particle(l, s.point, s.normal, if flag { 1.0 } else { 0.0 });
        }
    }

    /// CirculationBehaviour.IsWorldPointInArteryRect (mindistance = 4f / 35f)
    pub fn is_world_point_in_artery_rect(&self, l: usize, world_point: [f32; 2]) -> bool {
        let Some(rects) = &self.circulations[l].LocalArteryRects else { return false };
        let mindistance = 4.0f32 / 35.0;
        let num = mindistance * mindistance;
        let vector = self.inverse_transform_point(self.limbs[l].PhysicalBehaviour, [world_point[0], world_point[1], 0.0]);
        for r in rects {
            // Bounds.ClosestPoint: the point clamped to center -+ extents.
            let mut closest = [0.0f32; 3];
            for i in 0..3 {
                let (min, max) = (r[i] - r[i + 3], r[i] + r[i + 3]);
                closest[i] = if vector[i] < min { min } else if vector[i] > max { max } else { vector[i] };
            }
            let dv = [closest[0] - vector[0], closest[1] - vector[1], closest[2] - vector[2]];
            if (d(dv[0]) * dv[0] + d(dv[1]) * dv[1] + d(dv[2]) * dv[2]).f() <= num {
                return true;
            }
        }
        false
    }

    /// Instantiate(Person.BleedingParticlePrefab, position, Quaternion.identity, parent limb's transform) and
    /// `transform.up = up`: the 22 objects of the clone take instance ids, then the clone's Awakes run (the two
    /// ActivateIfFancyEffects each draw `Chance(1) > Random.value` before reading FancyEffects). `at` is the world
    /// position, `up` the direction of the new transform's up axis. Returns the BleedingParticle's index; the caller
    /// sets the fields CreateBleedingParticle / CreateDismembermentBloodParticle set after the Instantiate.
    fn instantiate_bleeding_particle(&mut self, p: usize, person: usize, parent: Option<usize>, at: [f32; 3], up: [f32; 3]) -> usize {
        use crate::unity::transform::{from_to_rotation_up, set_world, Trs};
        for _ in 0..BLEEDING_PARTICLE_OBJECTS {
            self.allocate_instance_id();
        }
        // The world pose under the parent (the prefab's scale is 1), then transform.up = up sets the world rotation.
        let chain: Vec<Trs> = self.local_chain(p).into_iter().rev().collect();
        let ((t, _), _) = set_world(&chain, at, [0.0, 0.0, 0.0, 1.0]);
        let ((_, q), _) = set_world(&chain, at, from_to_rotation_up(up));
        let local = Trs { t: [t[0], t[1], t[2], 0.0], q, s: [1.0, 1.0, 1.0, 0.0] };
        let extra_decals = 1.0 > self.random.value() && self.prefs.FancyEffects;
        let water_puff = 1.0 > self.random.value() && self.prefs.FancyEffects;
        // The extraDecals child is the FancyBloodSplatController (Start is delayed: started = false).
        let splat = if extra_decals {
            self.splats.push(FancyBloodSplat {
                UnparentAtStart: false,
                DirectionMode: 0,
                Continuous: true,
                Direction: [0.0, 1.0],
                SpawningChance: 0.0,
                AmountRange: [1.0, 4.0],
                AngleRandomisation: 9.0,
                LifetimeInSeconds: [1.0, 2.0],
                EjectionForce: [0.0, 1.0],
                DecalColourMultiplier: [1.0, 0.0, 0.0, 1.0],
                Size: 0.1,
                DestroyWhenNoParticles: true,
                TriggerOnCollide: false,
                on_collide: None,
                on_hole: None,
                Decal: "Sparse_blood".into(),
                particles: Default::default(),
                parent_position: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                started: false,
                destroyed: false,
                CollisionLayers: 10752,
                parent_gone: false,
                // The extraDecals child's local rotation is identity.
                attached: Some((p, local, [0.0, 0.0, 0.0, 1.0])),
                attached_node: None,
                parent_person: parent.map(|l| self.limbs[l].Person),
                id: 0,
            });
            self.behaviours.add(0, super::Behaviour::Splat(self.splats.len() - 1));
            let s = self.splats.len() - 1;
            self.update_splat_pose(s);
            self.pending_starts.push(super::misc::PendingStart::Splat(s));
            Some(s)
        } else {
            None
        };
        // BleedingParticleBehaviour.Start (WaterPuffSystem) draws nothing; Awake is `BleedingParticle::awake` below.
        let mut bleeding = BleedingParticle {
            IsOnlyForInternalBleeding: false,
            SpeedMultiplier: 0.6,
            // The BleedingParticle prefab's: 1.25 on 1.28.41, 0.8 on 1.28.52 (both games' recorded clones).
            RateMultiplier: if crate::assets::il::is_1_28_45() { 0.8 } else { 1.25 },
            Laminarity: 0.0,
            DripSoundRateThreshold: 1.0,
            ShouldBecomeSmokeInWater: false,
            PulseSpeed: 2.0,
            PulseIntensity: 1.3,
            fakeHeartClock: 0.0,
            CirculationBehaviour: parent,
            Parent: p,
            PushingTo: None,
            BloodSplatController: splat,
            dripSoundClock: FixedIntervalDistributor::with_rate(0.5),
            DripSounds: self.people[person].BleedingDripSounds,
            visible: false,
            prefab: self.people[person].BleedingParticlePrefab,
            water_puff,
            WaterPuffSystem: self.people[person].BleedingWaterPuff,
            loaded: false,
            psm: None,
            pss: Vec::new(),
            local_position: [t[0], t[1], t[2]],
            local_up: [2.0 * (q[0] * q[1] - q[3] * q[2]), 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]), 2.0 * (q[1] * q[2] + q[3] * q[0])],
            color: [0.0; 4],
            startSpeedMultiplier: 0.0,
            rateOverTimeMultiplier: 0.0,
            emission_enabled: false,
            destroyed: false,
            collision: false,
        };
        bleeding.awake(self.prefs.FancyEffects);
        self.bleeding.push(bleeding);
        self.behaviours.add(0, super::Behaviour::Bleeding(self.bleeding.len() - 1));
        self.bleeding.len() - 1
    }

    /// CirculationBehaviour.CreateBleedingParticle
    /// Every caller passes makeSound = false, so DripSounds is always nulled.
    pub fn create_bleeding_particle(&mut self, l: usize, world_position: [f32; 2], direction: [f32; 2], laminarity: f32) {
        if !self.limbs[l].IsAndroid && !self.limbs[l].IsZombie && self.circulations[l].BleedingPointCount < 8 {
            let p = self.limbs[l].PhysicalBehaviour;
            let (c, e) = self.sprite_world_bounds(p);
            let (closest, _) = crate::unity::transform::bounds_closest_point([world_position[0], world_position[1], 0.0], c, e);
            let at = [closest[0] - direction[0] * 0.1, closest[1] - direction[1] * 0.1, 0.0];
            let b = self.instantiate_bleeding_particle(p, self.limbs[l].Person, Some(l), at, [direction[0], direction[1], 0.0]);
            self.bleeding[b].ShouldBecomeSmokeInWater = self.circulations[l].BleedingPointCount == 0;
            self.bleeding[b].CirculationBehaviour = Some(l);
            self.bleeding[b].Laminarity = laminarity;
            self.bleeding[b].DripSounds = None;
            self.circulations[l].BleedingParticles.push(b);
            self.circulations[l].BleedingPointCount += 1;
        }
    }

    /// CirculationBehaviour.CreateDismembermentBloodParticle: a particle under the connected body's Transform at the
    /// joint's connected anchor, pointing from it to this limb. It is on neither BleedingParticles nor
    /// BleedingPointCount, and keeps the prefab's DripSounds. `transform.GetComponent<CirculationBehaviour>()` is null
    /// when that body is not a limb: the particle's Update then throws on every frame.
    pub fn create_dismemberment_blood_particle(&mut self, l: usize) {
        if !self.limbs[l].IsAndroid && self.limbs[l].HasJoint {
            let h = self.limbs[l].Joint.expect("MissingReferenceException: HasJoint without a Joint");
            let body = self.hinges[h].connected.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            let parent = self.limb_of_body(body);
            let pp = match parent {
                Some(parent) => self.limbs[parent].PhysicalBehaviour,
                None => self
                    .physicals
                    .iter()
                    .position(|p| !p.destroyed && p.rigidbody.body == body)
                    .unwrap_or_else(|| unimplemented!("mod-only: CirculationBehaviour.CreateDismembermentBloodParticle: a connectedBody on a GameObject without a PhysicalBehaviour")),
            };
            let anchor = self.hinges[h].connectedAnchor;
            let position = self.transform_point3(pp, [anchor[0], anchor[1], 0.0]);
            let (a, b) = (self.physicals[self.limbs[l].PhysicalBehaviour].transform.position, self.physicals[pp].transform.position);
            let up = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
            let person = self.limbs[l].Person;
            let particle = self.instantiate_bleeding_particle(pp, person, parent, position, up);
            // obj.AddComponent<Optout>()
            self.allocate_instance_id();
            self.bleeding[particle].PushingTo = Some(l);
            self.bleeding[particle].ShouldBecomeSmokeInWater = true;
            self.bleeding[particle].Laminarity = self.random.value();
        }
    }

    /// CirculationBehaviour.Awake
    pub fn circulation_awake(&mut self, l: usize) {
        self.circulations[l].NewlySpawned = true;
        self.circulations[l].WasInitiallyPumping = self.circulations[l].IsPump;
        self.circulations[l].BloodFlow = 1.0;
        for i in 0..self.circulations[l].PushesTo.len() {
            let to = self.circulations[l].PushesTo[i];
            self.circulations[to].Source = Some(l);
        }
    }

    /// CirculationBehaviour.Start. The Person prefab's SerialisableDistributions are empty on all 14 limbs, so the
    /// TRITIUM check of the glowing-human achievement flag only looks at the species.
    pub fn circulation_start(&mut self, l: usize) {
        if self.limbs[l].SpeciesIdentity != "Human" {
            self.circulations[l].canUnlockGlowingHumanAchievement = false;
        }
        let liquid = self.blood_type(l);
        if self.circulations[l].NewlySpawned {
            self.add_liquid(Owner::Circulation(l), liquid, 1.0);
            self.circulations[l].NewlySpawned = false;
        }
        if !self.physicals[self.limbs[l].PhysicalBehaviour].isDisintegrated {
            self.create_dismemberment_blood_particle(l);
        }
    }

    /// CirculationBehaviour.Cut
    pub fn cut(&mut self, l: usize, point: [f32; 2], direction: [f32; 2]) {
        if !self.circulations[l].ImmuneToDamage {
            let flag = self.is_world_point_in_artery_rect(l, point);
            self.circulations[l].BleedingRate += if flag { 1.0 } else { 0.25 };
            self.create_bleeding_particle(l, point, direction, if flag { 1.0 } else { 0.0 });
        }
    }

    /// CirculationBehaviour.HealBleeding
    pub fn heal_bleeding(&mut self, l: usize) {
        for i in std::mem::take(&mut self.circulations[l].BleedingParticles) {
            self.bleeding[i].destroyed = true;
            // The extraDecals splat is a child of the particle.
            if let Some(s) = self.bleeding[i].BloodSplatController {
                self.splats[s].destroyed = true;
                self.splats_destroying.push(s);
            }
        }
        let c = &mut self.circulations[l];
        c.BleedingRate = 0.0;
        c.BleedingPointCount = 0;
        c.InternalBleedingIntensity = 0.0;
    }

    /// CirculationBehaviour.ActOnJointBreak2D(joint): only `joint == Limb.Joint` (a hinge index; None for a joint that is no
    /// limb's hinge) bleeds.
    pub fn circulation_on_joint_break(&mut self, l: usize, joint: Option<usize>) {
        if joint.is_none() || joint != self.limbs[l].Joint {
            return;
        }
        self.circulations[l].BleedingRate += 20.0;
        if let Some(s) = self.circulations[l].Source {
            if self.node_is_connected_to(l, s) {
                self.circulations[s].BleedingRate += 20.0;
            }
        }
        self.circulations[l].IsDisconnected = true;
    }
}


/// Liquid.HasID: a non-null id found in Liquid.Registry (an empty string is not null, it is just not registered).
pub fn liquid_has_id(id: &str) -> bool {
    LIQUIDS.iter().any(|(n, _)| *n == id)
}

/// Liquid.GetLiquid: the registered liquid, or None where the Registry gives null.
pub fn liquid_get(id: &str) -> Option<usize> {
    LIQUIDS.iter().position(|(n, _)| *n == id)
}

/// Liquid.GetIdentity: the key a liquid was registered under (Global.Awake's Liquid.Register order).
pub fn liquid_identity(l: usize) -> &'static str {
    LIQUIDS[l].0
}
