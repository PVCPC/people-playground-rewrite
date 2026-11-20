//! 1.28.45's changes to save_members: the classes whose stored members differ (the whole list replaces 1.28.41's), the
//! member types and immutable members 1.28.45 adds. From tools/gen_members.py over 1.28.45's decompiled C#; AppliedBandageBehaviour
//! by hand (SoakAmount became Obsolete, SoakColour is new).

pub const PUBLIC_MEMBERS: &[(&str, &[&str])] = &[
    ("PhysicalBehaviour", &["SendUserPropagation", "ReflectsLasers", "AbsorbsLasers", "PlaySliderSound", "Resizable", "Selectable", "ChargeBurns", "BuoyancyModifier", "SimulateTemperature", "AmbientTemperatureTransfer", "isSliding", "EnergyWireResistance", "ActivationPropagationDelay", "SpawnSpawnParticles", "SelectionPriority", "BulletDeflectChanceOverride", "DisplayBloodDecals", "HasOutline", "SoundVolumeBoost", "IsWeightless", "charge", "ConductOverride", "ForceNoCharge", "ForceNoChargeParticles", "Deletable", "MaximumStabVictims", "Disintegratable", "StabCausesWound", "LooseEndSize", "StabReleaseSpeedModifier", "underwaterMarkings", "burnIntensity", "stabWoundCount", "Temperature", "beingHeldByGripper", "Wetness", "GridRes", "InitialBounds", "ForceContinuous", "TemperatureWhenBurningLerpMultiplier", "BurningProgressionMultiplier", "Circumference", "TrueInitialMass", "InitialMass", "InitialGravityScale", "isDisintegrated", "serialisable_victims", "serialisable_durations", "serialisable_axes", "serialisable_anchors", "serialisable_active", "serialisable_slash_victims", "serialisable_slash_strength", "BurnProgress*", "Charge*", "ShowOutline*", "BurnIntensity*"]),
    ("ThrusterbedBehaviour", &["StartupDuration", "Activated", "ThrustingForce", "RelativeDirection", "DoBurnOthers", "TemperatureTarget", "BurnRayCount", "BurnRayWidth", "BurnRayLength", "BurnRayOffset", "BurnRayPerpendicular", "LayersToBurn", "ChargeBoostModifier"]),
    ("AcidSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("AdrenalineSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("BoneEatingPoisonSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("CoagulationSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("DeathSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("EmptySyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("FreezeSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("KnockoutSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("LifeSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("MendingSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("PainKillerSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("PinkSyringe", &["Fingerprint", "Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("UltraStrengthSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("WaterBreathingSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("ZombieSyringe", &["Finite", "CanToggleInfinite", "PressureMode", "NewlySpawned", "IsAttachedToLauncher", "MeasuredPressure", "SerialisableDistributions"]),
    ("AppliedBandageBehaviour", &["SoakColour", "localRenderEndpoint", "targetDistance", "SpringStrength", "WireWidth", "WireColor", "WireMaterialName", "usedToHaveConnectedBody", "currentConnectedAnchor", "currentAnchor", "currentBreakingForce", "Invisible"]),
];

pub const MEMBER_TYPES: &[(&str, &str, &str)] = &[
    ("PhysicalBehaviour", "SelectionPriority", "System.Boolean"),
    ("PhysicalBehaviour", "serialisable_slash_victims", "System.Guid[]"),
    ("PhysicalBehaviour", "serialisable_slash_strength", "System.Single[]"),
    ("ThrusterbedBehaviour", "ChargeBoostModifier", "System.Single"),
    ("AcidSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("AdrenalineSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("BoneEatingPoisonSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("CoagulationSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("DeathSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("EmptySyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("FreezeSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("KnockoutSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("LifeSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("MendingSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("PainKillerSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("PinkSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("UltraStrengthSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("WaterBreathingSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("ZombieSyringe", "IsAttachedToLauncher", "System.Boolean"),
    ("AppliedBandageBehaviour", "SoakColour", "UnityEngine.Color"),
];

pub const IMMUTABLE_MEMBERS: &[(&str, &str)] = &[
    ("PhysicalBehaviour", "SelectionPriority"),
    ("ThrusterbedBehaviour", "ChargeBoostModifier"),
];
