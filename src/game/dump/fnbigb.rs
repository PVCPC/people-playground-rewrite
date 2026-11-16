//! big-b's fn-check entries (tools/progress/big-b.tsv), looked up after the CALLS table. Coroutines the recorder runs to
//! their end (PPG_FNPROBE_DRAIN, tools/recorder/BigProbeB.cs: ret `steps:<yield>,...,end`, every segment between the PI
//! and PO snapshots): the rewrite runs its segments of the same coroutine in order, each yield skipped.
use super::fncheck::Call;

fn steps(s: &[String]) -> String {
    format!("steps:{}end", s.iter().map(|x| format!("{x},")).collect::<String>())
}
fn secs(v: f32) -> String {
    format!("WaitForSeconds({:08x})", v.to_bits())
}

/// A case the recorder marked `drain=1` (its coroutine run to its end): the run-to-end entry, whatever the CALLS table
/// holds for the first-yield cases of the same method. Unmarked cases of fnbb3..5 (recorded before the marker) found the
/// plain-named entries of BIGB_CALLS, the only ones for Explode and LaunchRoutine.
pub(super) fn drained(r: &super::Record) -> Option<&'static (&'static str, Call)> {
    if r.get("drain") != Some("1") { return None }
    BIGB_CALLS.iter().chain(DRAIN_CALLS).find(|(n, _)| *n == r.name)
}

/// Run-to-end entries for methods whose CALLS entry checks the first yield.
const DRAIN_CALLS: &[(&str, Call)] = &[
    // batch1 r9: BeamCannonBehaviour's fire routines: the body, then `yield return BlinkBeam()` (a nested Routine: beam and
    // glow on, WaitForSeconds(0.05f), both off).
    ("BeamCannonBehaviour.NormalFireRoutine", |g, c| {
        let i = g.beam_cannons.iter().position(|x| x.id == c.id).ok_or("no beam_cannons entry")?;
        g.beam_cannon_normal_fire(i);
        g.beam_cannon_blink(i, true);
        g.beam_cannon_blink(i, false);
        Ok(steps(&["Routine".into(), secs(0.05)]))
    }),
    ("BeamCannonBehaviour.OverchargedFireRoutine", |g, c| {
        let i = g.beam_cannons.iter().position(|x| x.id == c.id).ok_or("no beam_cannons entry")?;
        g.beam_cannon_overcharged_fire(i);
        g.beam_cannon_blink(i, true);
        g.beam_cannon_blink(i, false);
        Ok(steps(&["Routine".into(), secs(0.05)]))
    }),
    // DelayedSplash(d) (batch1): WaitForSeconds(d), then Splash.Play(withChildren) and SplashAudio.Play (underwater_splash).
    ("UnderwaterExplosionBehaviour.DelayedSplash", |g, c| {
        let u = g.underwater_explosions.iter().position(|x| x.id == c.id).ok_or("no underwater_explosions entry")?;
        let d: f32 = c.arg(0)?;
        g.underwater_splash(u);
        Ok(steps(&[secs(d)]))
    }),
    // EmitRoutine(position, amount) (batch2 r9): WaitForEndOfFrame, WaitForFixedUpdate, then amount Emits of the shrapnel
    // emitter (its state from em=, BigProbeB.Em). EmitShrapnel only starts it.
    ("GlobalShrapnelEmitter.EmitRoutine", |g, c| {
        if let Some(v) = c.field("em") {
            em_load(&mut g.shrapnel.as_mut().ok_or("no GlobalShrapnelEmitter")?.emitter, v)?;
        }
        g.global_shrapnel_emit_shrapnel(c.arg(0)?, c.arg(1)?);
        let seq = g.coroutine_seq;
        let k = g.end_of_frame.iter().position(|x| x.seq == seq).ok_or("no EmitRoutine")?;
        let co = g.end_of_frame.remove(k);
        g.resume_coroutine(co);
        let k = g.fixed_update_waits.iter().position(|x| x.seq > seq).ok_or("no EmitRoutine after its WaitForEndOfFrame")?;
        let co = g.fixed_update_waits.remove(k);
        g.resume_coroutine(co);
        Ok(steps(&["WaitForEndOfFrame".into(), "WaitForFixedUpdate".into()]))
    }),
    // BallisticIteration(origin, direction, speed, iteration, totalDistance, previousCollider, tracer null, shouldDoSplash,
    // canExplode, charge): bullet_iterate up to its yield, then its continuation (WaitForEndOfFrame or WaitForSeconds(0.01))
    // when the yield was this coroutine's own. The iterations it starts (StartCoroutine) run to their first yield and are
    // not drained, as in the game.
    ("BallisticsEmitter.BallisticIteration", |g, c| {
        let f = shooter(g, c)?;
        if c.arg::<String>(6)? != "null" { return Err("a tracer arg".into()) }
        let previous = if c.arg::<String>(5)? == "null" { None } else { Some(c.arg::<i32>(5)?) };
        let b = bl::Bullet { shooter: f, origin: c.arg(0)?, direction: c.arg(1)?, speed: c.arg(2)?, iteration: c.arg(3)?, total: c.arg(4)?, previous, tracer: false, line: None, splash: c.arg(7)?, can_explode: c.arg(8)?, charge: c.arg(9)? };
        let seq = g.coroutine_seq;
        let mut s = Vec::new();
        if g.bullet_iterate(b) {
            let bullet = |x: &crate::game::destroyable::Coroutine| x.seq > seq && matches!(x.step, crate::game::destroyable::Step::Bullet(_));
            if let Some(k) = g.end_of_frame.iter().position(bullet) {
                let co = g.end_of_frame.remove(k);
                s.push("WaitForEndOfFrame".to_string());
                g.resume_coroutine(co);
            } else if let Some(k) = g.coroutines.iter().position(bullet) {
                let co = g.coroutines.remove(k);
                s.push(secs(0.01));
                g.resume_coroutine(co);
            }
        }
        Ok(em(g, c, f, steps(&s)))
    }),
    // FireBolt: the bolt (lightning_fire_bolt), then each flash and gap its start_wait steps scheduled on the entity,
    // resumed in order.
    ("LightningToolEntityBehaviour.FireBolt", |g, c| {
        let i = g.lightning_entities.iter().position(|x| x.id == c.id).ok_or("no lightning_entities entry")?;
        let seq = g.coroutine_seq;
        g.lightning_fire_bolt(i);
        let mut s = Vec::new();
        while let Some(k) = g.coroutines.iter().position(|x| x.seq > seq && x.owner == c.id) {
            let co = g.coroutines.remove(k);
            s.push(secs((co.at - g.time.dynamicTimeAsDouble) as f32));
            g.resume_coroutine(co);
        }
        Ok(steps(&s))
    }),
];

/// BallisticsEmitter (a plain class: the recorder's self is the FirearmBehaviour's BallisticsEmitter field, the FnCase
/// id the FirearmBehaviour's, BigProbeB.Host): the rewrite's Shooter::Firearm of that firearm, with the emitter's
/// recorded state (BigProbeB.Em's `em=`, when the case has one) loaded into it.
fn shooter(g: &mut crate::game::Game, c: &Case) -> Result<bl::Shooter, String> {
    let i = g.firearms.iter().position(|x| x.id == c.id).ok_or("no firearms entry")?;
    if let Some(v) = c.field("em") {
        em_load(g.firearms[i].emitter.as_mut().ok_or("no emitter")?, v)?;
    }
    Ok(bl::Shooter::Firearm(i))
}
/// BigProbeB.EmState's 21 values loaded into an emitter (tracers -1: disposed, None).
pub(super) fn em_load(e: &mut bl::Emitter, v: &str) -> Result<(), String> {
    {
        let v: Vec<&str> = v.split(',').collect();
        if v.len() != 21 {
            return Err(format!("em= has {} values", v.len()));
        }
        let f = |k: usize| u32::from_str_radix(v[k], 16).map(f32::from_bits).map_err(|_| format!("em {k}"));
        let n = |k: usize| v[k].parse::<i64>().map_err(|_| format!("em {k}"));
        e.MaxBallisticsIterations = n(0)? as u32;
        e.MaxTotalDistance = f(1)?;
        e.ThicknessStepSize = f(2)?;
        e.RicochetChanceMultiplier = f(3)?;
        e.MaxRange = f(4)?;
        e.MaterialHardnessMultiplier = f(5)?;
        e.ImpactForceMultiplier = f(6)?;
        e.LayersToHit = n(7)? as u32;
        e.WaterLayer = n(8)? as u32;
        e.DoTraversal = v[9] == "1";
        e.TracerWidth = f(10)?;
        e.RenderTracersIfTraversal = v[11] == "1";
        e.BulletDropMultiplier = f(12)?;
        e.Charge = f(13)?;
        e.BulletCrackSpeedThreshold = f(14)?;
        e.BulletCrackChance = f(15)?;
        e.BulletCrackCooldownTime = f(16)?;
        e.ExplosiveRound = v[17] == "1";
        e.ExplodeAtHitPoint = v[18] == "1";
        e.lastCrackPlayTime = f(19)?;
        e.tracers = usize::try_from(n(20)?).ok();
    }
    Ok(())
}
/// batch2 r4: BigProbeB.EmState's 21 values then XpState's 10 (ExplosiveRoundParams): ExplosiveAttachmentBehaviour's em=.
pub(super) fn em_xp_load(e: &mut bl::Emitter, v: &str) -> Result<(), String> {
    let w: Vec<&str> = v.split(',').collect();
    if w.len() != 31 {
        return Err(format!("em= has {} values", w.len()));
    }
    em_load(e, &w[..21].join(","))?;
    let w = &w[21..];
    let f = |k: usize| u32::from_str_radix(w[k], 16).map(f32::from_bits).map_err(|_| format!("xp {k}"));
    let n = |k: usize| w[k].parse::<i64>().map_err(|_| format!("xp {k}"));
    use crate::game::explosion::EffectSize as S;
    e.ExplosiveRoundParams = crate::game::explosion::ExplosionParameters {
        FragmentationRayCount: n(0)? as u32,
        Position: [f(1)?, f(2)?, f(3)?],
        Range: f(4)?,
        FragmentForce: f(5)?,
        CreateParticlesAndSound: w[6] == "1",
        EffectSize: match n(7)? { 0 => S::Medium, 1 => S::Small, 2 => S::Large, x => return Err(format!("EffectSize {x}")) },
        DismemberChance: f(8)?,
        BallisticShrapnelCount: n(9)? as i32,
    };
    Ok(())
}
/// em_text plus ExplosiveRoundParams as BigProbeB.XpState writes it.
pub(super) fn em_xp_text(e: &bl::Emitter) -> String {
    use crate::game::explosion::EffectSize as S;
    let p = &e.ExplosiveRoundParams;
    let es = match p.EffectSize { S::Medium => 0, S::Small => 1, S::Large => 2 };
    format!("{},{},{},{},{},{},{},{},{},{},{}", em_text(e), p.FragmentationRayCount, hx(p.Position[0]), hx(p.Position[1]), hx(p.Position[2]),
        hx(p.Range), hx(p.FragmentForce), bt(p.CreateParticlesAndSound), es, hx(p.DismemberChance), p.BallisticShrapnelCount)
}
/// A RaycastHit2D arg (BigProbeB.Fmt: "ray:<collider id>,<point>,<normal>") as a BulletHit, its physical the phys arg
/// (else the collider's body's); None: no collider.
fn ray(g: &crate::game::Game, c: &Case, i: usize, phys: Option<usize>) -> Result<Option<bl::BulletHit>, String> {
    let a = c.arg::<String>(i)?;
    let v: Vec<&str> = a.strip_prefix("ray:").ok_or("not a ray arg")?.split(',').collect();
    let id: i32 = v[0].parse().map_err(|_| "ray collider id")?;
    if id == 0 {
        return Ok(None);
    }
    let h = |i: usize| u32::from_str_radix(v[i], 16).map(f32::from_bits).map_err(|_| "ray float".to_string());
    let fx = g.world.fixtures.iter().flatten().find(|f| f.collider.id == id).ok_or(format!("no collider #{id}"))?;
    let physical = match phys {
        Some(k) => {
            let pid: i32 = c.arg(k)?;
            g.physicals.iter().position(|x| x.id == pid).ok_or("no physical arg")?
        }
        None => g.physicals.iter().position(|p| !p.destroyed && p.rigidbody.body == fx.body).unwrap_or(0),
    };
    Ok(Some(bl::BulletHit { physical, hardness: 0.0, damage: 0.0, collider: id, point: [h(1)?, h(2)?], normal: [h(3)?, h(4)?], distance: if v.len() > 5 { h(5)? } else { 0.0 } }))
}
/// The emitter's state after the call, as BigProbeB.After appends it to the game's ret.
fn em(g: &crate::game::Game, c: &Case, f: bl::Shooter, ret: String) -> String {
    if c.field("em").is_none() {
        return ret;
    }
    let v = em_text(g.emitter(f));
    if ret.is_empty() { format!("em={v}") } else { format!("{ret} em={v}") }
}
/// An emitter's state as BigProbeB.EmState writes it.
pub(super) fn em_text(e: &bl::Emitter) -> String {
    [
        e.MaxBallisticsIterations.to_string(), hx(e.MaxTotalDistance), hx(e.ThicknessStepSize), hx(e.RicochetChanceMultiplier), hx(e.MaxRange),
        hx(e.MaterialHardnessMultiplier), hx(e.ImpactForceMultiplier), (e.LayersToHit as i32).to_string(), (e.WaterLayer as i32).to_string(),
        bt(e.DoTraversal), hx(e.TracerWidth), bt(e.RenderTracersIfTraversal), hx(e.BulletDropMultiplier), hx(e.Charge),
        hx(e.BulletCrackSpeedThreshold), hx(e.BulletCrackChance), hx(e.BulletCrackCooldownTime), bt(e.ExplosiveRound), bt(e.ExplodeAtHitPoint),
        hx(e.lastCrackPlayTime), e.tracers.map_or("-1".into(), |n| n.to_string()),
    ]
    .join(",")
}
/// An AutomaticSentryController (BigProbeB.Host: the FnCase id the SentryTurretBehaviour's) with its state from the FnCase's
/// em= (BigProbeB.ScState), taken out of the turret: (turret, controller, recorded previousPersonID).
fn sentry(g: &mut crate::game::Game, c: &Case) -> Result<(usize, crate::game::automatic_sentry::AutomaticSentry, i32), String> {
    let i = g.sentry_turrets.iter().position(|x| x.id == c.id).ok_or("no sentry_turrets entry")?;
    let v: Vec<String> = c.field("em").ok_or("no em=")?.split(',').map(String::from).collect();
    if v.len() != 8 {
        return Err(format!("em= has {} values", v.len()));
    }
    let f = |k: usize| u32::from_str_radix(&v[k], 16).map(f32::from_bits).map_err(|_| format!("em {k}"));
    let limb = |g: &crate::game::Game, s: &str| -> Result<usize, String> {
        let id: i32 = s.parse().map_err(|_| format!("limb id {s}"))?;
        g.limbs.iter().position(|x| x.id == id).ok_or(format!("no limb #{id}"))
    };
    let mut s = std::mem::take(&mut g.sentry_turrets[i].controller);
    s.t = f(0)?;
    s.currentTarget = if v[1] == "0" { None } else { Some(limb(g, &v[1])?) };
    s.targetStack = if v[2] == "-" { Vec::new() } else { v[2].split(':').map(|x| limb(g, x)).collect::<Result<_, _>>()? };
    let prev: i32 = v[3].parse().map_err(|_| "em 3")?;
    // A PersonBehaviour no longer in the scene: an index no limb's Person has.
    s.previousPerson = if prev == 0 { None } else { Some(g.people.iter().position(|x| x.id == prev).unwrap_or(usize::MAX)) };
    s.SightRange = f(4)?;
    s.LayerMask = v[5].parse::<i32>().map_err(|_| "em 5")? as u32;
    s.AimFuzziness = f(6)?;
    s.MaxAngle = f(7)?;
    Ok((i, s, prev))
}
/// The controller's state after the call as BigProbeB.After appends it ("em=..."), the controller put back.
fn sentry_after(g: &mut crate::game::Game, i: usize, s: crate::game::automatic_sentry::AutomaticSentry, prev: i32) -> String {
    let id = |l: usize| g.limbs[l].id.to_string();
    let after = [
        hx(s.t),
        s.currentTarget.map_or("0".into(), id),
        if s.targetStack.is_empty() { "-".into() } else { s.targetStack.iter().map(|&l| id(l)).collect::<Vec<_>>().join(":") },
        match s.previousPerson { None => "0".into(), Some(usize::MAX) => prev.to_string(), Some(k) => g.people[k].id.to_string() },
        hx(s.SightRange), (s.LayerMask as i32).to_string(), hx(s.AimFuzziness), hx(s.MaxAngle),
    ]
    .join(",");
    g.sentry_turrets[i].controller = s;
    format!("em={after}")
}
fn hx(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}
fn bt(b: bool) -> String {
    (if b { "1" } else { "0" }).into()
}
use super::fncheck::Case;
use crate::game::ballistics as bl;

/// A BallisticsEmitter getter: the field as the recorder formats it.
macro_rules! get {
    ($n:literal, $e:ident => $v:expr) => {
        ($n, |g, c| {
            let f = shooter(g, c)?;
            let $e = g.emitter(f);
            let r = $v;
            Ok(em(g, c, f, r))
        })
    };
}
/// A BallisticsEmitter setter: arg 0 into the field.
macro_rules! set {
    ($n:literal, $t:ty, $e:ident, $x:ident => $v:expr) => {
        ($n, |g, c| {
            let f = shooter(g, c)?;
            let $x: $t = c.arg(0)?;
            let bl::Shooter::Firearm(i) = f else { unreachable!() };
            let $e = g.firearms[i].emitter.as_mut().unwrap();
            $v;
            Ok(em(g, c, f, String::new()))
        })
    };
}

pub(super) const BIGB_CALLS: &[(&str, Call)] = &[
    get!("BallisticsEmitter.get_MaxBallisticsIterations", e => e.MaxBallisticsIterations.to_string()),
    get!("BallisticsEmitter.get_MaxTotalDistance", e => hx(e.MaxTotalDistance)),
    get!("BallisticsEmitter.get_RicochetChanceMultiplier", e => hx(e.RicochetChanceMultiplier)),
    get!("BallisticsEmitter.get_MaxRange", e => hx(e.MaxRange)),
    get!("BallisticsEmitter.get_LayersToHit", e => (e.LayersToHit as i32).to_string()),
    get!("BallisticsEmitter.get_WaterLayer", e => (e.WaterLayer as i32).to_string()),
    get!("BallisticsEmitter.get_DoTraversal", e => bt(e.DoTraversal)),
    get!("BallisticsEmitter.get_RenderTracersIfTraversal", e => bt(e.RenderTracersIfTraversal)),
    get!("BallisticsEmitter.get_Charge", e => hx(e.Charge)),
    get!("BallisticsEmitter.get_ExplosiveRound", e => bt(e.ExplosiveRound)),
    get!("BallisticsEmitter.get_ExplodeAtHitPoint", e => bt(e.ExplodeAtHitPoint)),
    get!("BallisticsEmitter.get_ThicknessStepSize", e => hx(e.ThicknessStepSize)),
    get!("BallisticsEmitter.get_MaterialHardnessMultiplier", e => hx(e.MaterialHardnessMultiplier)),
    get!("BallisticsEmitter.get_ImpactForceMultiplier", e => hx(e.ImpactForceMultiplier)),
    get!("BallisticsEmitter.get_BulletDropMultiplier", e => hx(e.BulletDropMultiplier)),
    get!("BallisticsEmitter.get_BulletCrackSpeedThreshold", e => hx(e.BulletCrackSpeedThreshold)),
    get!("BallisticsEmitter.get_BulletCrackChance", e => hx(e.BulletCrackChance)),
    get!("BallisticsEmitter.get_BulletCrackCooldownTime", e => hx(e.BulletCrackCooldownTime)),
    get!("BallisticsEmitter.get_TracerWidth", e => hx(e.TracerWidth)),
    set!("BallisticsEmitter.set_MaxBallisticsIterations", u32, e, x => e.MaxBallisticsIterations = x),
    set!("BallisticsEmitter.set_MaxTotalDistance", f32, e, x => e.MaxTotalDistance = x),
    set!("BallisticsEmitter.set_RicochetChanceMultiplier", f32, e, x => e.RicochetChanceMultiplier = x),
    set!("BallisticsEmitter.set_MaxRange", f32, e, x => e.MaxRange = x),
    set!("BallisticsEmitter.set_LayersToHit", i32, e, x => e.LayersToHit = x as u32),
    set!("BallisticsEmitter.set_WaterLayer", i32, e, x => e.WaterLayer = x as u32),
    set!("BallisticsEmitter.set_DoTraversal", bool, e, x => e.DoTraversal = x),
    set!("BallisticsEmitter.set_RenderTracersIfTraversal", bool, e, x => e.RenderTracersIfTraversal = x),
    set!("BallisticsEmitter.set_Charge", f32, e, x => e.Charge = x),
    set!("BallisticsEmitter.set_ExplosiveRound", bool, e, x => e.ExplosiveRound = x),
    set!("BallisticsEmitter.set_ExplodeAtHitPoint", bool, e, x => e.ExplodeAtHitPoint = x),
    set!("BallisticsEmitter.set_ThicknessStepSize", f32, e, x => e.ThicknessStepSize = x),
    set!("BallisticsEmitter.set_MaterialHardnessMultiplier", f32, e, x => e.MaterialHardnessMultiplier = x),
    set!("BallisticsEmitter.set_ImpactForceMultiplier", f32, e, x => e.ImpactForceMultiplier = x),
    set!("BallisticsEmitter.set_BulletDropMultiplier", f32, e, x => e.BulletDropMultiplier = x),
    set!("BallisticsEmitter.set_BulletCrackSpeedThreshold", f32, e, x => e.BulletCrackSpeedThreshold = x),
    set!("BallisticsEmitter.set_BulletCrackChance", f32, e, x => e.BulletCrackChance = x),
    set!("BallisticsEmitter.set_BulletCrackCooldownTime", f32, e, x => e.BulletCrackCooldownTime = x),
    set!("BallisticsEmitter.set_TracerWidth", f32, e, x => e.TracerWidth = x),
    ("BallisticsEmitter.GetDamageForDistance", |g, c| { let f = shooter(g, c)?; let r = hx(g.damage_for_distance(f, c.arg::<f32>(0)?)); Ok(em(g, c, f, r)) }),
    ("BallisticsEmitter.SpeedMultiplierByDistance", |g, c| { let f = shooter(g, c)?; Ok(em(g, c, f, hx(bl::speed_multiplier_by_distance(c.arg::<f32>(0)?)))) }),
    ("BallisticsEmitter.DamageMultiplierByDistance", |g, c| { let f = shooter(g, c)?; Ok(em(g, c, f, hx(bl::damage_multiplier_by_distance(c.arg::<f32>(0)?)))) }),
    ("BallisticsEmitter.CalculateHardness", |g, c| {
        let f = shooter(g, c)?;
        let id: i32 = c.arg(0)?;
        let p = g.physicals.iter().position(|x| x.id == id).ok_or("no physical arg")?;
        let r = hx(g.hardness(f, p));
        Ok(em(g, c, f, r))
    }),
    ("BallisticsEmitter.CanPlayCracks", |g, c| { let f = shooter(g, c)?; let r = bt(g.can_play_cracks(f, c.arg::<[f32; 2]>(0)?, c.arg::<f32>(1)?, c.arg::<f32>(2)?)); Ok(em(g, c, f, r)) }),
    ("BallisticsEmitter.TestForWaterHit", |g, c| { let f = shooter(g, c)?; let r = bt(g.test_for_water_hit(f, c.arg::<[f32; 2]>(0)?, c.arg::<[f32; 2]>(1)?, c.arg::<f32>(2)?, c.arg::<bool>(3)?)); Ok(em(g, c, f, r)) }),
    ("BallisticsEmitter.get_RandomisedAngle", |g, c| { let f = shooter(g, c)?; let a = g.bullet_random_angle(f); Ok(em(g, c, f, format!("{},{}", hx(a[0]), hx(a[1])))) }),
    ("BallisticsEmitter.Dispose", |g, c| { let f = shooter(g, c)?; g.emitter_dispose(f); Ok(em(g, c, f, String::new())) }),
    ("BallisticsEmitter.Emit", |g, c| { let f = shooter(g, c)?; g.emitter_emit(f, c.arg::<[f32; 2]>(0)?, c.arg::<[f32; 2]>(1)?); Ok(em(g, c, f, String::new())) }),
    // ShouldRicochet(incoming, hit, phys): the hit (BigProbeB.MakeArg's ray) with phys as its physical; no collider:
    // `!hit.transform`, false.
    ("BallisticsEmitter.ShouldRicochet", |g, c| {
        let f = shooter(g, c)?;
        let incoming: [f32; 2] = c.arg(0)?;
        let Some(h) = ray(g, c, 1, Some(2))? else { return Ok(em(g, c, f, "0".into())) };
        let r = bt(g.should_ricochet(f, incoming, &h));
        Ok(em(g, c, f, r))
    }),
    // RicochetBullet(direction, speed, iteration, totalDistance, charge, tracer null, hit): the callback, then its
    // BallisticIteration up to its first yield.
    ("BallisticsEmitter.RicochetBullet", |g, c| {
        let f = shooter(g, c)?;
        if c.arg::<String>(5)? != "null" {
            return Err("a tracer arg".into());
        }
        let h = ray(g, c, 6, None)?.unwrap_or(bl::BulletHit { physical: 0, hardness: 0.0, damage: 0.0, collider: 0, point: [0.0; 2], normal: [0.0; 2], distance: 0.0 });
        let b = bl::Bullet { shooter: f, origin: [0.0; 2], direction: c.arg(0)?, speed: c.arg(1)?, iteration: c.arg(2)?, total: c.arg(3)?, previous: None, tracer: false, line: None, splash: true, can_explode: false, charge: c.arg(4)? };
        g.ricochet_bullet(b, &h);
        Ok(em(g, c, f, String::new()))
    }),
    // GetTargetMotorSpeed(transform, barrelPos, barrelDir, out motorSpeed) on a SentryTurretBehaviour's Controller (BigProbeB.Host;
    // transform the turret's own): the controller's state from the FnCase's em= (BigProbeB.ScState), the same after the call
    // and the out args.
    ("AutomaticSentryController.GetTargetMotorSpeed", |g, c| {
        let (i, mut s, prev) = sentry(g, c)?;
        let p = g.sentry_turrets[i].phys;
        let delta = g.time.deltaTime;
        let (motor, _) = g.sentry_get_target_motor_speed(&mut s, p, c.arg(1)?, c.arg(2)?, delta);
        let after = sentry_after(g, i, s, prev);
        let args: Vec<&str> = c.field("args").ok_or("no args=")?.split(';').take(3).collect();
        // The void method's ret is BigProbeB.After's "em=<state>"; fn-check appends the game's " out=".
        Ok(format!("{after} out={};{}", args.join(";"), hx(motor)))
    }),
    // batch1 r7: GetMotorSpeed(transform, worldspaceDir) on a SentryTurretBehaviour's Controller (FnTiny seeds currentTarget and
    // previousPersonID; transform the turret's own): the clamped angle and the state after (a SentryTurret's controller has no
    // OnSight/OnShoot listeners).
    ("AutomaticSentryController.GetMotorSpeed", |g, c| {
        let (i, mut s, prev) = sentry(g, c)?;
        let p = g.sentry_turrets[i].phys;
        let cur = s.currentTarget.ok_or("currentTarget null (NullReferenceException)")?;
        let speed = g.sentry_motor_speed(&mut s, cur, p, c.arg(1)?, &mut Vec::new());
        Ok(format!("{} {}", hx(speed), sentry_after(g, i, s, prev)))
    }),
    // batch1 r8: HasLineOfSight(t, transform, pos, dir) on a SentryTurretBehaviour's Controller (FnTiny: t a living limb, pos and dir
    // towards it in some cases; transform the turret's own): the bool and the state after.
    ("AutomaticSentryController.HasLineOfSight", |g, c| {
        let (i, s, prev) = sentry(g, c)?;
        let p = g.sentry_turrets[i].phys;
        let id: i32 = c.arg(0)?;
        let l = g.limbs.iter().position(|x| x.id == id).ok_or(format!("no limb #{id}"))?;
        let r = bt(g.sentry_has_line_of_sight(&s, l, p, c.arg(2)?, c.arg(3)?));
        Ok(format!("{r} {}", sentry_after(g, i, s, prev)))
    }),
    // get_HasTarget: `(bool)currentTarget`; get_Target: currentTarget as the recorder formats a LimbBehaviour.
    ("AutomaticSentryController.get_HasTarget", |g, c| {
        let (i, s, prev) = sentry(g, c)?;
        let r = bt(g.sentry_has_target(&s));
        Ok(format!("{r} {}", sentry_after(g, i, s, prev)))
    }),
    ("AutomaticSentryController.get_Target", |g, c| {
        let (i, s, prev) = sentry(g, c)?;
        let r = match s.currentTarget {
            None => "null".to_string(),
            Some(l) if g.physicals[g.limbs[l].PhysicalBehaviour].destroyed => "destroyed".into(),
            Some(l) => format!("@{}#{}", g.physicals[g.limbs[l].PhysicalBehaviour].name.replace(' ', "_"), g.limbs[l].id),
        };
        Ok(format!("{r} {}", sentry_after(g, i, s, prev)))
    }),
    ("ExplosiveBehaviour.get_Armed", |g, c| {
        let i = g.explosives.iter().position(|x| x.id == c.id).ok_or("no explosives entry")?;
        Ok(bt(g.explosives[i].Armed))
    }),
    // DecalControllerBehaviour.Decal (BigProbeB: em=<its PhysicalBehaviour's id>,<index in GetComponents order>, the arg
    // "dec:<descriptor>,<globalPosition>,<size>,<colourMultiplier>", ret "dc=<dirty>:<localDecalPositions>").
    ("DecalControllerBehaviour.Decal", |g, c| {
        let em = c.field("em").ok_or("no em=")?;
        let (pid, k) = em.split_once(',').ok_or("bad em=")?;
        let pid: i32 = pid.parse().map_err(|_| "em physical")?;
        let k: usize = k.parse().map_err(|_| "em index")?;
        let p = g.physicals.iter().position(|x| x.id == pid).ok_or("no physical of the controller")?;
        if k >= g.physicals[p].decal_controllers.len() {
            return Err(format!("no decal controller {k}"));
        }
        let a = c.arg::<String>(0)?;
        let v: Vec<&str> = a.strip_prefix("dec:").ok_or("not a dec arg")?.split(',').collect();
        let [name, rest @ ..] = &v[..] else { return Err("dec arg".into()) };
        let f: Vec<f32> = rest.iter().map(|w| u32::from_str_radix(w, 16).map(f32::from_bits).map_err(|_| "dec float".to_string())).collect::<Result<_, _>>()?;
        let [px, py, size, r, gg, b, al] = f[..] else { return Err("dec arg length".into()) };
        let ty = if *name == "null" { crate::game::physical::NULL_DECAL } else { g.decal_descriptor(name) };
        g.decal_controller_decal(p, k, ty, [px, py], size, [r, gg, b, al]);
        let d = &g.physicals[p].decal_controllers[k];
        let at: Vec<String> = d.decals.iter().map(|x| format!("{},{}", hx(x.position[0]), hx(x.position[1]))).collect();
        Ok(format!("dc={}:{}", bt(d.holder.is_some()), at.join("/")))
    }),
    // gapC r23: DecalControllerBehaviour.Start (em= as Decal's; a seed nulls the descriptor on even cases), ret as Decal's.
    ("DecalControllerBehaviour.Start", |g, c| {
        let em = c.field("em").ok_or("no em=")?;
        let (pid, k) = em.split_once(',').ok_or("bad em=")?;
        let pid: i32 = pid.parse().map_err(|_| "em physical")?;
        let k: usize = k.parse().map_err(|_| "em index")?;
        let p = g.physicals.iter().position(|x| x.id == pid).ok_or("no physical of the controller")?;
        if k >= g.physicals[p].decal_controllers.len() {
            return Err(format!("no decal controller {k}"));
        }
        g.decal_controller_start(p, k, c.id);
        let d = &g.physicals[p].decal_controllers[k];
        let at: Vec<String> = d.decals.iter().map(|x| format!("{},{}", hx(x.position[0]), hx(x.position[1]))).collect();
        Ok(format!("dc={}:{}", bt(d.holder.is_some()), at.join("/")))
    }),
    // GetCurrentBladePos: the (Vector2, Vector2) tuple as the recorder formats a struct.
    ("EnergySwordMotionBlur.GetCurrentBladePos", |g, c| {
        let i = g.energy_blurs.iter().position(|x| x.id == c.id).ok_or("no energy_blurs entry")?;
        let (a, b) = g.energy_blur_current_blade_pos(i);
        Ok(format!("ValueTuple`2(Item1={},{} Item2={},{})", hx(a[0]), hx(a[1]), hx(b[0]), hx(b[1])))
    }),
    // WinchBehaviour: TurnWheel is the whole of Update (winch_update), OnFlip(center) is UpdateWinchLimits, Start adopts
    // the SliderJoint2D first (winch_slider).
    ("WinchBehaviour.TurnWheel", |g, c| { let i = g.winches.iter().position(|x| x.id == c.id).ok_or("no winches entry")?; g.winch_update(i); Ok(String::new()) }),
    ("WinchBehaviour.OnFlip", |g, c| { let i = g.winches.iter().position(|x| x.id == c.id).ok_or("no winches entry")?; g.winch_update_limits(i); Ok(String::new()) }),
    ("WinchBehaviour.Start", |g, c| {
        let i = g.winches.iter().position(|x| x.id == c.id).ok_or("no winches entry")?;
        let f = g.winch_slider(i);
        g.winch_start(i, f);
        Ok(String::new())
    }),
    // TeslaCoilLightningBoltBehaviour.Awake of the bolt spawned on its own (a coil's clones: TeslaCoilBehaviour.Awake).
    ("TeslaCoilLightningBoltBehaviour.Awake", |g, c| {
        let k = g.tesla_bolts.iter().position(|x| x.id == c.id).ok_or("no tesla_bolts entry")?;
        g.tesla_bolt_awake(k);
        Ok(String::new())
    }),
    // LaserBehaviour.UpdateLaser: `return finalHit.physicalBehaviour` (its Unity bool) as 1/0.
    ("LaserBehaviour.UpdateLaser", |g, c| {
        let l = g.lasers.iter().position(|x| x.id == c.id).ok_or("no lasers entry")?;
        Ok(if g.laser_update(l).is_some() { "1" } else { "0" }.into())
    }),
    ("PhysicalBehaviour.SetParticleEmission", |g, c| {
        let p = g.physicals.iter().position(|x| x.id == c.id).ok_or("no physicals entry")?;
        g.set_particle_emission(p);
        Ok(String::new())
    }),
    // The lazy IEnumerable the recorder enumerates in the call (BigProbeB.Ret): "[@name#id;...]".
    ("Global.GetPhysicsObjectsNearPositionAccurate", |g, c| {
        let (position, radius, accuracy) = (c.arg::<[f32; 2]>(0)?, c.arg::<f32>(1)?, c.arg::<f32>(2)?);
        let mut out = Vec::new();
        g.physics_objects_near_position_accurate_at(position, radius, accuracy, |g, o| out.push(format!("@{}#{}", g.physicals[o].name.replace(' ', "_"), g.physicals[o].id)));
        Ok(format!("[{}]", out.join(";")))
    }),
    // The RaycastHit2D argument (BigProbeB.Fmt): "ray:<collider id>,<point>,<normal>", the collider's fixture giving its
    // body and GameObject.
    ("BlasterboltBehaviour.Impact", |g, c| {
        let b = g.blasterbolts.iter().position(|x| x.id == c.id).ok_or("no blasterbolts entry")?;
        let a = c.arg::<String>(0)?;
        let v: Vec<&str> = a.strip_prefix("ray:").ok_or("not a ray arg")?.split(',').collect();
        let id: i32 = v[0].parse().map_err(|_| "ray collider id")?;
        let h = |i: usize| u32::from_str_radix(v[i], 16).map(f32::from_bits).map_err(|_| "ray float".to_string());
        let f = g.world.fixtures.iter().flatten().find(|f| f.collider.id == id).ok_or(format!("no collider #{id}"))?;
        let hit = crate::game::explosion::RayHit2D { collider: id, game_object: f.collider.game_object, body: f.body, point: crate::b2::math::Vec2::new(h(1)?, h(2)?), normal: crate::b2::math::Vec2::new(h(3)?, h(4)?), distance: 0.0, fraction: 0.0 };
        g.blasterbolt_impact(b, &hit);
        Ok(String::new())
    }),
    // Explode(delay, forceDoubleCheck): its first segment (explosive_start_explode, the arming), then after
    // WaitForSeconds(delay) and WaitForEndOfFrame the explosion (explosive_explode).
    ("ExplosiveBehaviour.Explode", |g, c| {
        let e = g.explosives.iter().position(|x| x.id == c.id).ok_or("no explosives entry")?;
        let (delay, fdc): (f32, bool) = (c.arg(0)?, c.arg(1)?);
        let seq = g.coroutine_seq;
        g.explosive_explode_first(e, delay, fdc);
        let Some(k) = g.explosives[e].coroutines.iter().position(|x| x.seq > seq) else { return Ok(steps(&[])) };
        g.explosives[e].coroutines.remove(k);
        g.explosive_explode(e);
        Ok(steps(&[secs(delay), "WaitForEndOfFrame".into()]))
    }),
    // weapons r20 BeamformerBehaviour.BeamRenderRoutine: both segments (beamformer_beam_render_routine), each yield skipped.
    ("BeamformerBehaviour.BeamRenderRoutine", |g, c| {
        let i = g.beamformers.iter().position(|x| x.id == c.id).ok_or("no beamformers entry")?;
        let mut r = crate::game::Game::beamformer_beam_render_routine(i);
        let mut s = Vec::new();
        loop {
            match r(g) {
                crate::game::coroutine::Yield::Done => break,
                crate::game::coroutine::Yield::WaitForSeconds(v) => s.push(secs(v)),
                _ => return Err("BeamRenderRoutine: an unexpected yield".into()),
            }
        }
        Ok(steps(&s))
    }),
    // LaunchRoutine: WaitForFixedUpdate, then the launch (launcher_launch_routine).
    ("ProjectileLauncherBehaviour.LaunchRoutine", |g, c| {
        let l = g.launchers.iter().position(|x| x.id == c.id).ok_or("no launchers entry")?;
        g.launcher_launch_routine(l);
        Ok(steps(&["WaitForFixedUpdate".into()]))
    }),
];
