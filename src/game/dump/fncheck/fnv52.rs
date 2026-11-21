//! --fn-check entries for the functions 1.28.45 (now 1.28.52) added (tools/progress/changed-1.28.45.tsv): the new classes'
//! (v45_new.rs) and the new liquids' (chemistry.rs). The recordings come from PPG_FNPROBE=gen on the 1.28.52 game
//! (tools/fnv45/list52b.txt). Looked up before the generated table.
use super::*;
use crate::game::v45_new::{Pole, SpeedUnit};

fn speedometer(g: &Game, c: &Case) -> Result<usize, String> {
    g.v45.speedometers.iter().position(|x| x.id == c.sid).ok_or_else(|| format!("no SpeedometerBehaviour #{}", c.sid))
}
fn buzzer(g: &Game, c: &Case) -> Result<usize, String> {
    g.v45.buzzers.iter().position(|x| x.id == c.sid).ok_or_else(|| format!("no BuzzerBehaviour #{}", c.sid))
}
fn crossbow(g: &Game, c: &Case) -> Result<usize, String> {
    g.v45.crossbows.iter().position(|x| x.id == c.sid).ok_or_else(|| format!("no CrossbowBehaviour #{}", c.sid))
}
fn drip(g: &Game, c: &Case) -> Result<usize, String> {
    g.v45.drips.iter().position(|x| x.id == c.sid).ok_or_else(|| format!("no DrippingBloodParticleController #{}", c.sid))
}
/// A `[b;b;...]` byte array argument.
fn bytes(a: &str) -> Result<Vec<u8>, String> {
    let a = a.trim_matches(['[', ']']);
    if a.is_empty() {
        return Ok(Vec::new());
    }
    a.split(';').map(|b| b.parse().map_err(|_| format!("not a byte: {b}"))).collect()
}
/// A PhysicalBehaviour return as the recorder writes it.
fn phys_ref(g: &Game, p: Option<usize>) -> String {
    p.map_or("null".to_string(), |p| format!("@{}#{}", g.physicals[p].name.replace(' ', "_"), g.physicals[p].id))
}
/// MagneticBehaviour.Pole as the recorder writes the enum: its value (North 0, South 1).
fn pole(a: &str) -> Result<Pole, String> {
    match a {
        "0" => Ok(Pole::North),
        "1" => Ok(Pole::South),
        _ => Err(format!("not a Pole: {a}")),
    }
}

/// fnv52 r2: the MagneticBehaviour the recorder added to a catalog item (V52Seed): no prefab has one, so its `magnets` entry is
/// made here from the case's ItemScript record, on the PhysicalBehaviour its Clone record lists beside it.
fn magnet(g: &mut Game, c: &Case) -> Result<usize, String> {
    if let Some(m) = g.v45.magnets.iter().position(|x| x.id == c.sid) {
        return Ok(m);
    }
    let me = format!("MagneticBehaviour:{}", c.sid);
    let clone = c.s.of("Clone").find(|r| r.get("comps").is_some_and(|v| v.trim_matches(['[', ']']).split(';').any(|x| x == me))).ok_or_else(|| format!("no Clone record holds MagneticBehaviour #{}", c.sid))?;
    let pid: i32 = clone.req("comps")?.trim_matches(['[', ']']).split(';').find_map(|x| x.strip_prefix("PhysicalBehaviour:")).ok_or("no PhysicalBehaviour beside the MagneticBehaviour")?.parse().map_err(|_| "PhysicalBehaviour id")?;
    let phys = phys_by_id(g, pid)?;
    g.v45.magnets.push(crate::game::v45_new::Magnetic { id: c.sid, phys, EffectiveDistance: 1.0, MagnetStrength: 1.0, ..Default::default() });
    let m = g.v45.magnets.len() - 1;
    let r = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or_else(|| format!("no ItemScript record of MagneticBehaviour #{}", c.sid))?;
    let x = &mut g.v45.magnets[m];
    for (k, f) in [("EffectiveDistance", &mut x.EffectiveDistance as &mut dyn Field), ("NorthPole", &mut x.NorthPole), ("SouthPole", &mut x.SouthPole), ("MagnetStrength", &mut x.MagnetStrength), ("globalNorthPole", &mut x.globalNorthPole), ("globalSouthPole", &mut x.globalSouthPole)] {
        if let Some(v) = r.get(k) {
            f.parse(v)?;
        }
    }
    Ok(m)
}
/// A GameObject argument's PhysicalBehaviour (the physical on that object).
fn go_physical(g: &mut Game, a: &str) -> Result<Option<usize>, String> {
    let Some(id) = reference(a) else { return Ok(None) };
    Ok((0..g.physicals.len()).find(|&p| !g.physicals[p].destroyed && g.physical_game_object(p) == id))
}

pub(super) const CALLS: &[(&str, Call)] = &[
    // The new liquids (Chemistry.cs): self is the registry liquid (`liquid=`).
    ("Nasumica.GetDisplayName", |_, c| Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?)))),
    ("Amaraevelum.GetDisplayName", |_, c| Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?)))),
    ("Dulcamarus.GetDisplayName", |_, c| Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?)))),
    ("Nasumica.OnEnterContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.chemistry_liquid_on_enter_container(a0, a1); VOID }),
    ("Amaraevelum.OnEnterContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.chemistry_liquid_on_enter_container(a0, a1); VOID }),
    ("Dulcamarus.OnEnterContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.chemistry_liquid_on_enter_container(a0, a1); VOID }),
    ("Nasumica.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.chemistry_liquid_on_exit_container(a0, a1); VOID }),
    ("Amaraevelum.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.chemistry_liquid_on_exit_container(a0, a1); VOID }),
    ("Dulcamarus.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.chemistry_liquid_on_exit_container(a0, a1); VOID }),
    ("Nasumica.OnEnterLimb", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_usize(g, 1)?; g.chemistry_liquid_on_enter_limb(a0, a1); VOID }),
    ("Amaraevelum.OnEnterLimb", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_usize(g, 1)?; g.chemistry_liquid_on_enter_limb(a0, a1); VOID }),
    ("Dulcamarus.OnEnterLimb", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_usize(g, 1)?; g.chemistry_liquid_on_enter_limb(a0, a1); VOID }),
    ("Nasumica.OnUpdateNew", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update_new(a0, a1); VOID }),
    ("Amaraevelum.OnUpdateNew", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update_new(a0, a1); VOID }),
    ("Liquid.OnUpdateNew", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update_new(a0, a1); VOID }),
    // SpeedometerBehaviour
    // The game's switch reads UserPreferenceManager.Current.SpeedUnit, not toUnit (a1).
    // fnv52 r2: the case's preference (V52Seed " su="), else ours.
    ("SpeedometerBehaviour.ConvertSpeed", |g, c| { let su = c.field("su").and_then(|v| v.parse().ok()).unwrap_or(g.prefs.SpeedUnit); let c = c.with(2)?; let a0 = c.p_f32(0)?; Ok(Enc::enc(crate::game::v45_new::convert_speed(a0, SpeedUnit::from_index(su)))) }),
    ("SpeedometerBehaviour.Start", |g, c| { let s = speedometer(g, c)?; g.speedometer_start(s); VOID }),
    ("SpeedometerBehaviour.Use", |g, c| { let s = speedometer(g, c)?; g.speedometer_use(s); VOID }),
    ("SpeedometerBehaviour.UpdateActivation", |g, c| { let s = speedometer(g, c)?; g.speedometer_update_activation(s); VOID }),
    ("SpeedometerBehaviour.OnEnable", |g, c| { let s = speedometer(g, c)?; g.speedometer_update_activation(s); VOID }),
    ("SpeedometerBehaviour.FixedUpdate", |g, c| { let s = speedometer(g, c)?; g.speedometer_fixed_update(s); VOID }),
    ("SpeedometerBehaviour.SetText", |g, c| { let s = speedometer(g, c)?; g.speedometer_set_text(s); VOID }),
    // BuzzerBehaviour
    ("BuzzerBehaviour.Start", |g, c| { let b = buzzer(g, c)?; g.buzzer_start(b); VOID }),
    ("BuzzerBehaviour.UpdateColour", |g, c| { let b = buzzer(g, c)?; g.buzzer_update_colour(b); VOID }),
    ("BuzzerBehaviour.UpdateLoudness", |g, c| { let b = buzzer(g, c)?; g.buzzer_update_loudness(b); VOID }),
    ("BuzzerBehaviour.Use", |g, c| { let b = buzzer(g, c)?; g.buzzer_use(b); VOID }),
    ("BuzzerBehaviour.OnBeforeSerialise", |g, c| { let b = buzzer(g, c)?; g.buzzer_on_before_serialise(b); VOID }),
    ("BuzzerBehaviour.GetId", |_, c| { c.with(1)?; Ok(Enc::enc(crate::game::contraption::xxhash(&bytes(c.args[0])?, 0))) }),
    // MagneticBehaviour's statics
    ("MagneticBehaviour.GetPoleSign", |_, c| { c.with(2)?; Ok(Enc::enc(crate::game::v45_new::magnetic_get_pole_sign(pole(c.args[0])?, pole(c.args[1])?))) }),
    // DrippingBloodParticleController
    ("DrippingBloodParticleController.Update", |g, c| { let d = drip(g, c)?; g.dripping_blood_update(d); VOID }),
    ("DrippingBloodParticleController.Set", |g, c| { let d = drip(g, c)?; c.with(2)?; let col = c.v::<4>(0); let amount = c.f(1); g.dripping_blood_set(d, col, amount); VOID }),
    // CrossbowBehaviour
    ("CrossbowBehaviour.Update", |g, c| { let x = crossbow(g, c)?; g.crossbow_update(x); VOID }),
    ("CrossbowBehaviour.DisconnectSyringe", |g, c| { let x = crossbow(g, c)?; g.crossbow_disconnect_syringe(x); VOID }),
    ("CrossbowBehaviour.OnAfterDeserialise", |g, c| { let x = crossbow(g, c)?; g.crossbow_on_after_deserialise(x); VOID }),
    ("CrossbowBehaviour.OnPreFlip", |g, c| { let x = crossbow(g, c)?; g.crossbow_on_pre_flip(x); VOID }),
    ("CrossbowBehaviour.OnPlayerDragStart", |g, c| { let x = crossbow(g, c)?; if let Some(s) = g.crossbow_attached(x) { let sp = g.syringes[s].Phys; g.crossbow_parent_syringe(x, sp); } VOID }),
    ("CrossbowBehaviour.OnPlayerDragEnd", |g, c| { let x = crossbow(g, c)?; if g.crossbow_attached(x).is_some() { g.crossbow_unparent(x, false); } VOID }),
    ("CrossbowBehaviour.OnJointBreak2D", |g, c| { let x = crossbow(g, c)?; c.with(1)?; if c.adopted_joint(g, 0).is_some() && c.adopted_joint(g, 0) == g.v45.crossbows[x].syringeJoint { g.crossbow_disconnect_syringe(x); } VOID }),
    ("CrossbowBehaviour.OnCollisionEnter2D", |g, c| { let x = crossbow(g, c)?; let col = c.with(1)?.p_collision(0)?; g.crossbow_on_collision_enter(x, &col); VOID }),
    // OnLaunch(sender, e): this crossbow's handler (the launcher's broadcast, on its own Launcher).
    ("CrossbowBehaviour.OnLaunch", |g, c| { let x = crossbow(g, c)?; c.with(2)?; let bolt = go_physical(g, c.args[0])?; let l = g.v45.crossbows[x].Launcher; g.crossbow_on_launch(l, bolt); VOID }),
    ("ProjectileLauncherBehaviour.get_PhysicalBehaviour", |g, c| { let l = g.launchers.iter().position(|x| x.id == c.sid).ok_or("no launchers entry")?; Ok(phys_ref(g, Some(g.launchers[l].physical))) }),
    // BuzzerBehaviour.ClipFromData(bytes, out id): the clip, then out= the bytes and the id.
    ("BuzzerBehaviour.ClipFromData", |g, c| { c.with(2)?; let b = bytes(c.args[0])?; let (clip, id) = g.buzzer_clip_from_data(b); Ok(format!("@{}#{} out={};{id}", clip.clip.name, clip.asset.path_id, c.args[0])) }),
    // MagneticBehaviour (fnv52 r2: on the catalog item V52Seed gave one)
    ("MagneticBehaviour.FixedUpdate", |g, c| { let m = magnet(g, c)?; g.magnetic_fixed_update(m); VOID }),
    ("MagneticBehaviour.GetGlobalPole", |g, c| { let m = magnet(g, c)?; c.with(1)?; Ok(Enc::enc(g.magnetic_global_pole(m, pole(c.args[0])?))) }),
    ("MagneticBehaviour.Attract", |g, c| { let m = magnet(g, c)?; let w = c.with(2)?; let o = w.p_usize(g, 0)?; let p = pole(c.args[1])?; Ok(Enc::enc(g.magnetic_attract(m, o, p))) }),
    ("MagneticBehaviour.GetForce", |g, c| { let m = magnet(g, c)?; let w = c.with(2)?; let (a, b) = (w.p_arr::<2>(0)?, w.p_arr::<2>(1)?); Ok(Enc::enc(crate::game::v45_new::magnetic_get_force(g.v45.magnets[m].MagnetStrength, a, b))) }),
    ("MagneticBehaviour.Awake", |g, c| { magnet(g, c)?; VOID }),
    ("CrossbowBehaviour.ConnectSyringe", |g, c| {
        let x = crossbow(g, c)?;
        c.with(1)?;
        let id = reference(c.args[0]).ok_or("null syringe")?;
        let s = g.syringes.iter().position(|s| s.id == id).ok_or_else(|| format!("no syringe #{id}"))?;
        g.crossbow_connect_syringe(x, s);
        VOID
    }),
    // Global.GetPhysicsObjectsNearPositionAccurateNonAlloc(position, radius, buffer): the count; out= the buffer with its first
    // `count` slots overwritten.
    ("Global.GetPhysicsObjectsNearPositionAccurateNonAlloc", |g, c| {
        let w = c.with(3)?;
        let (pos, radius) = (w.p_arr::<2>(0)?, w.p_f32(1)?);
        let mut buf: Vec<String> = c.args[2].trim_matches(['[', ']']).split(';').filter(|x| !x.is_empty()).map(str::to_string).collect();
        let found = g.physics_objects_near_position_accurate_non_alloc(pos, radius, buf.len());
        for (k, &p) in found.iter().enumerate() {
            buf[k] = format!("@{}#{}", g.physicals[p].name.replace(' ', "_"), g.physicals[p].id);
        }
        Ok(format!("{} out={};{};[{}]", found.len(), c.args[0], c.args[1], buf.join(";")))
    }),
    ("PhysicalBehaviour.GetRandomPointInHull", |g, c| { let p = c.with(1)?.p_usize(g, 0)?; Ok(Enc::enc(g.physical_random_point_in_hull(p))) }),
    ("PhysicalBehaviour.TransferPropertiesForPenetration", |g, c| { let w = c.with(2)?; let (p, o) = (w.p_usize(g, 0)?, w.p_usize(g, 1)?); g.transfer_properties_for_penetration(p, o); VOID }),
    ("PhysicalBehaviour.OnBeforeSerialise", |g, c| { let p = c.with(1)?.p_usize(g, 0)?; g.physical_on_before_serialise(p); VOID }),
    // ToolControllerBehaviour.SelectByMass/Size/Depth(mouse): the PhysicalBehaviour picked, or null.
    ("ToolControllerBehaviour.SelectByMass", |g, c| { let m = c.with(1)?.p_arr::<3>(0)?; let p = g.select_by_mass(m); Ok(phys_ref(g, p)) }),
    ("ToolControllerBehaviour.SelectBySize", |g, c| { let m = c.with(1)?.p_arr::<3>(0)?; let p = g.select_by_size(m); Ok(phys_ref(g, p)) }),
    ("ToolControllerBehaviour.SelectByDepth", |g, c| { let m = c.with(1)?.p_arr::<3>(0)?; let p = g.select_by_depth(m); Ok(phys_ref(g, p)) }),
    ("ExistInBeta.Start", |g, c| { g.exist_in_beta_start(c.sid); VOID }),
    // ThrusterbedBehaviour.Boost (1.28.45's property)
    ("ThrusterbedBehaviour.get_Boost", |g, c| { let i = g.thrusterbeds.iter().position(|x| x.id == c.sid).ok_or("no thrusterbeds entry")?; Ok(Enc::enc(g.thrusterbed_boost(i))) }),
];

/// The new classes' dumped fields for the item loader and compare (fnitems.rs item_fields): the ItemScript record's names.
pub(in super::super) fn v45_fields<'a>(g: &'a mut Game, class: &str, id: i32) -> Option<(Vec<(&'static str, &'a mut dyn Field)>, Vec<(&'static str, &'a mut dyn Field)>)> {
    let v = &mut g.v45;
    match class {
        "SpeedometerBehaviour" => v.speedometers.iter_mut().find(|x| x.id == id).map(|x| (vec![("enabled", &mut x.enabled as &mut dyn Field), ("Activated", &mut x.Activated), ("TopSpeedMs", &mut x.TopSpeedMs), ("speedMs", &mut x.speedMs)], Vec::new())),
        "BuzzerBehaviour" => v.buzzers.iter_mut().find(|x| x.id == id).map(|x| (vec![("enabled", &mut x.enabled as &mut dyn Field), ("CurrentDataId", &mut x.CurrentDataId), ("Loudness", &mut x.Loudness), ("lastPlayedTime", &mut x.lastPlayedTime)], Vec::new())),
        "CrossbowBehaviour" => v.crossbows.iter_mut().find(|x| x.id == id).map(|x| {
            (vec![("enabled", &mut x.enabled as &mut dyn Field), ("LocalSyringeAttachPoint", &mut x.LocalSyringeAttachPoint), ("SyringeEjectForce", &mut x.SyringeEjectForce), ("SyringeFluidUsage", &mut x.SyringeFluidUsage), ("lastDisconnectTime", &mut x.lastDisconnectTime), ("hasSyringe", &mut x.hasSyringe)],
             vec![("AttachedSyringe", &mut x.AttachedSyringe as &mut dyn Field), ("syringeJoint", &mut x.syringeJoint)])
        }),
        "MagneticBehaviour" => v.magnets.iter_mut().find(|x| x.id == id).map(|x| (vec![("EffectiveDistance", &mut x.EffectiveDistance as &mut dyn Field), ("NorthPole", &mut x.NorthPole), ("SouthPole", &mut x.SouthPole), ("MagnetStrength", &mut x.MagnetStrength), ("globalNorthPole", &mut x.globalNorthPole), ("globalSouthPole", &mut x.globalSouthPole)], Vec::new())),
        "DrippingBloodParticleController" => v.drips.iter_mut().find(|x| x.id == id).map(|x| (vec![("Colour", &mut x.Colour as &mut dyn Field), ("Amount", &mut x.Amount), ("DecaySpeed", &mut x.DecaySpeed)], Vec::new())),
        _ => None,
    }
}
