// More hand-written twins checked against the game's DLL (tools/dllcheck, DllCheck2.cs): classes M to Z. Rows are
// DllCheck's fnpure lines, objects mode: args[0] is the object as recorded, ret is the return value then `set(...)`.

/// A linked component's record (`@link#<id>=Type(...)` on the self record), for the reference field `name`.
fn dll2_link<'a>(c: &'a Case, name: &str) -> Result<String, String> {
    let id = reference(&dll_self_field(c, name)?).ok_or_else(|| format!("{name} is no reference"))?;
    dll_self_field(c, &format!("@link#{id}"))
}
fn dll2_shard(rec: &str) -> Result<super::super::shatter::Shard, String> {
    let r = dll_rec(rec)?;
    let n = |k: &str| -> Result<i64, String> { dll_get(&r, k)?.parse::<i64>().map_err(|_| format!("{k} not an int")) };
    Ok(super::super::shatter::Shard { min_range: n("MinRange")? as u8, size: n("Size")? as u8, texture_asset_id: n("TextureAssetId")? as i32 })
}
fn dll2_opt_string(c: &Case, name: &str) -> Result<Option<String>, String> {
    let v = dll_self_field(c, name)?;
    Ok(if v == "null" { None } else { Some(v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).ok_or_else(|| format!("{name} not a string: {v}"))?.replace('_', " ")) })
}
/// gapC r25: a seeded ResizeHandles.MultiTarget (its three backing fields).
fn dll2_multi(c: &Case) -> Result<super::super::resize_handles::MultiTarget, String> {
    Ok(super::super::resize_handles::MultiTarget {
        flipped: dll_i32(c, "<Flipped>k__BackingField")? != 0,
        scale: dll_floats(c, "<Scale>k__BackingField")?,
        position: dll_floats(c, "<Position>k__BackingField")?,
    })
}
/// A PowerGaugeBehaviour on its recorded PhysicalBehaviour (physicals[0]).
fn dll2_power_gauge(c: &Case) -> Result<super::super::Game, String> {
    let mut g = dll_game();
    let r = dll2_link(c, "PhysicalBehaviour")?;
    let r = dll_rec(&r)?;
    let mut p = super::super::Physical::default();
    p.charge = dll_bits(dll_get(&r, "charge")?)?;
    p.ForceNoCharge = dll_get(&r, "ForceNoCharge")? == "1";
    g.physicals.push(p);
    let mut s = super::super::gauge::PowerGauge::default();
    s.MaxChargeDisplay = dll_f32(c, "MaxChargeDisplay")?;
    s.PhysicalBehaviour = Some(0);
    g.power_gauges.push(s);
    Ok(g)
}

/// gapA r17: a seeded `Cell(ReadTemperature=.. WriteTemperature=.. CanCreateNeighbours=.. CanTransferTemperature=..)`.
fn dll2_cell(v: &str) -> Result<super::super::temperature::Cell, String> {
    let r = dll_rec(v)?;
    Ok(super::super::temperature::Cell {
        ReadTemperature: dll_bits(dll_get(&r, "ReadTemperature")?)?,
        WriteTemperature: dll_bits(dll_get(&r, "WriteTemperature")?)?,
        CanCreateNeighbours: dll_get(&r, "CanCreateNeighbours")? == "1",
        CanTransferTemperature: dll_get(&r, "CanTransferTemperature")? == "1",
    })
}
fn dll2_cell_fields(s: &super::super::temperature::Cell) -> Vec<(&'static str, String)> {
    let b = |x: bool| (x as u8).to_string();
    vec![("ReadTemperature", hex(s.ReadTemperature)), ("WriteTemperature", hex(s.WriteTemperature)), ("CanCreateNeighbours", b(s.CanCreateNeighbours)), ("CanTransferTemperature", b(s.CanTransferTemperature))]
}

const PURE_DLL2: &[(&str, PureCall)] = &[
    // gapA r17: plain classes on DllCheck's seeded rows (`DllCheck seeded`, DLLCHECK_DEFAULTS=1): AmbientTemperatureGridBehaviour
    // .Cell and BloodContainer.RefFloat, held by no Component the probe can make a case on.
    ("Cell.ForceSet", |c| { let w = c.with(2)?; let mut s = dll2_cell(c.args[0])?; let t = w.p_f32(1)?; let b = dll2_cell_fields(&s); s.ForceSet(t); Ok(dll_seeded_row("void".into(), &b, &dll2_cell_fields(&s), &[hex(t)])) }),
    ("Cell.Sync", |c| { c.with(1)?; let mut s = dll2_cell(c.args[0])?; let b = dll2_cell_fields(&s); s.Sync(); Ok(dll_seeded_row("void".into(), &b, &dll2_cell_fields(&s), &[])) }),
    ("Cell.Get", |c| { c.with(1)?; let s = dll2_cell(c.args[0])?; let b = dll2_cell_fields(&s); let r = s.Get(); Ok(dll_seeded_row(r.enc(), &b, &dll2_cell_fields(&s), &[])) }),
    ("ExplosionParameters.set_LargeExplosionParticles", |c| {
        let w = c.with(2)?;
        use super::super::explosion::{EffectSize, ExplosionParameters};
        let EffectSize = match dll_i32(c, "EffectSize")? { 0 => EffectSize::Medium, 1 => EffectSize::Small, 2 => EffectSize::Large, e => return Err(format!("EffectSize {e}")) };
        let code = |e: EffectSize| match e { EffectSize::Medium => "0", EffectSize::Small => "1", EffectSize::Large => "2" }.to_string();
        let mut p = ExplosionParameters { EffectSize, ..Default::default() };
        let v = w.p_bool(1)?;
        let b = [("EffectSize", code(p.EffectSize))];
        p.set_large_explosion_particles(v);
        Ok(dll_seeded_row("void".into(), &b, &[("EffectSize", code(p.EffectSize))], &[(v as u8).to_string()]))
    }),
    ("RefFloat.HasDecreased", |c| {
        let w = c.with(2)?;
        let rec = dll_rec(c.args[0])?;
        let r = super::super::circulation::RefFloat { Previous: dll_bits(dll_get(&rec, "Previous")?)?, raw: dll_bits(dll_get(&rec, "raw")?)? };
        let th = w.p_f32(1)?;
        Ok(dll_seeded_row(r.has_decreased(th).enc(), &[], &[], &[hex(th)]))
    }),
    ("RefFloat.get_Raw", |c| { c.with(1)?; Ok(dll_ret(super::super::circulation::RefFloat { Previous: dll_f32(c, "Previous")?, raw: dll_f32(c, "raw")? }.raw)) }),
    ("RefFloat.set_Raw", |c| {
        let w = c.with(2)?;
        let mut r = super::super::circulation::RefFloat { Previous: dll_f32(c, "Previous")?, raw: dll_f32(c, "raw")? };
        let before = [("Previous", hex(r.Previous)), ("raw", hex(r.raw))];
        r.set_raw(w.p_f32(1)?);
        Ok(format!("void {}", dll_changes(&before, &[("Previous", hex(r.Previous)), ("raw", hex(r.raw))])))
    }),
    ("RequiredMod.get_IsFromWorkshop", |c| {
        c.with(1)?;
        let m = super::super::contraption::RequiredMod { Name: dll2_opt_string(c, "Name")?, Identifier: dll2_opt_string(c, "Identifier")?, WorkshopId: dll2_opt_string(c, "WorkshopId")? };
        Ok(dll_ret(m.IsFromWorkshop()))
    }),
    ("Shard.GetHashCode", |c| { c.with(1)?; Ok(dll_ret(dll2_shard(c.args[0])?.get_hash_code())) }),
    // Shard.Equals(Shard) and Shard.Equals(object): an argument that is not a Shard (null, a boxed int) is false.
    ("Shard.Equals", |c| { c.with(2)?; Ok(dll_ret(c.args[1].starts_with("Shard(") && dll2_shard(c.args[0])? == dll2_shard(c.args[1])?)) }),
    ("Shard.op_Equality", |c| { c.with(2)?; Ok(Enc::enc(dll2_shard(c.args[0])? == dll2_shard(c.args[1])?)) }),
    ("Shard.op_Inequality", |c| { c.with(2)?; Ok(Enc::enc(dll2_shard(c.args[0])? != dll2_shard(c.args[1])?)) }),
    ("RotatedRectangle.Equals", |c| { c.with(2)?; Ok(dll_ret(c.args[1].starts_with("RotatedRectangle(") && dll2_rect(c.args[0])? == dll2_rect(c.args[1])?)) }),
    ("RotatedRectangle.op_Equality", |c| { c.with(2)?; Ok(Enc::enc(dll2_rect(c.args[0])? == dll2_rect(c.args[1])?)) }),
    ("RotatedRectangle.op_Inequality", |c| { c.with(2)?; Ok(Enc::enc(dll2_rect(c.args[0])? != dll2_rect(c.args[1])?)) }),
    ("MendingSerum.get_RemovalChancePerSecond", |c| { c.with(1)?; Ok(dll_ret(super::super::syringe::mending_serum_removal_chance_per_second())) }),
    ("OsteomorphosisAgent.get_RemovalChancePerSecond", |c| { c.with(1)?; Ok(dll_ret(super::super::syringe::osteomorphosis_agent_removal_chance_per_second())) }),
    ("PowerGaugeBehaviour.GetProgress", |c| { c.with(1)?; Ok(dll_ret(dll2_power_gauge(c)?.power_gauge_get_progress(0, 0))) }),
    // Awake on the recorded display: only the clocks' RateHz change (their accumulators stay). The clocks are private
    // fields DllCheck lists (`FixedIntervalDistributor(..)`); the script's field list leaves them out.
    ("ReactorA5DisplayBehaviour.Awake", |c| {
        c.with(1)?;
        use super::super::misc::FixedIntervalDistributor as F;
        let load = |n: &str| -> Result<F, String> {
            let v = dll_self_field(c, n)?;
            let r = dll_rec(&v)?;
            Ok(F {
                timeStepAccumulator: dll_bits(dll_get(&r, "timeStepAccumulator")?)?,
                MaxCycles: dll_get(&r, "<MaxCycles>k__BackingField")?.parse().map_err(|_| "MaxCycles")?,
                InterpolationTime: dll_bits(dll_get(&r, "<InterpolationTime>k__BackingField")?)?,
                IntervalSeconds: dll_bits(dll_get(&r, "<IntervalSeconds>k__BackingField")?)?,
            })
        };
        let show = |f: &F| format!("FixedIntervalDistributor(<MaxCycles>k__BackingField={} <InterpolationTime>k__BackingField={} timeStepAccumulator={} <IntervalSeconds>k__BackingField={})",
            f.MaxCycles, hex(f.InterpolationTime), hex(f.timeStepAccumulator), hex(f.IntervalSeconds));
        let mut s = super::super::reactor::ReactorA5Display::default();
        s.alertBeepClock = load("alertBeepClock")?;
        s.updateDisplayClock = load("updateDisplayClock")?;
        let before = [("alertBeepClock", show(&s.alertBeepClock)), ("updateDisplayClock", show(&s.updateDisplayClock))];
        s.awake(None);
        let after = [("alertBeepClock", show(&s.alertBeepClock)), ("updateDisplayClock", show(&s.updateDisplayClock))];
        Ok(format!("void {}", dll_changes(&before, &after)))
    }),
    // Stabbed on the recorded PhysicalBehaviour: its Properties' StabSound (each clip named by its `@name#id`), the audioSource
    // (an AudioSource on an active object, disabled before), isDisintegrated and the Random state. The effects as DllCheck lists
    // them: `!enabled(@source)=1` for the enable, `!PlayOneShot(@source,@clip,volume)` for the one-shot.
    ("PhysicalBehaviour.Stabbed", |c| {
        c.with(2)?;
        use super::super::audio::{AudioSource, Clip};
        let mut g = dll_game();
        // DllCheck records no `@rng` for a row that draws nothing (an empty StabSound).
        if let Ok(r) = dll_random(c) { g.random = r }
        let mut p = super::super::Physical::default();
        p.isDisintegrated = dll_i32(c, "isDisintegrated")? == 1;
        let source = dll_self_field(c, "audioSource")?;
        let sid = if source == "null" { None } else { Some(reference(&source).ok_or("audioSource")?) };
        p.audioSource = sid.is_some();
        p.ids.audioSource = sid.unwrap_or(0);
        if let Some(id) = sid {
            g.audio.add(id, AudioSource { active: true, enabled: false, ..Default::default() });
        }
        let props = dll2_link(c, "Properties")?;
        let stab = dll_get(&dll_rec(&props)?, "StabSound")?.to_string();
        let mut pp = super::super::physical::PhysicalProperties::default();
        if stab != "null" {
            let items = dll_items(&stab)?;
            pp.StabSound = Some(items.len());
            pp.clips.StabSound = items.iter().map(|t| (*t != "null").then(|| std::sync::Arc::new(Clip {
                asset: Default::default(),
                clip: crate::assets::classes::AudioClip { name: t.to_string(), load_type: 0, preload_audio_data: true, load_in_background: false, channels: 1, frequency: 44100, length: 1.0, resource: Default::default(), compression_format: 0 },
                pcm: None,
                stream: None,
            }))).collect();
        }
        g.properties.push(pp);
        p.Properties = 0;
        g.physicals.push(p);
        g.physical_stabbed(0);
        let mut fx = Vec::new();
        if let Some(s) = sid.and_then(|id| g.audio.sources.get(&id)) {
            if s.enabled { fx.push(format!("!enabled({source})=1")) }
            for v in &s.one_shots { fx.push(format!("!PlayOneShot({source},{},{})", v.clip.clip.name, hex(v.volume_scale))) }
        }
        Ok(format!("void set({})", fx.join(" ")))
    }),
    ("PowerGaugeBehaviour.GetDisplayValueFor", |c| { let w = c.with(2)?; Ok(dll_ret(dll2_power_gauge(c)?.power_gauge_display_value_for(0, w.p_f32(1)?))) }),
    ("PinkSyringe.GetFingerprintValue", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let mut s = super::super::syringe::Syringe::default();
        s.Fingerprint = dll_items(&dll_self_field(c, "Fingerprint")?)?.into_iter().map(dll_bits).collect::<Result<_, _>>()?;
        s.fingerprintReadPos = dll_i32(c, "fingerprintReadPos")?;
        let before = s.fingerprintReadPos;
        g.syringes.push(s);
        let ret = g.fingerprint_value(0);
        let after = g.syringes[0].fingerprintReadPos;
        Ok(format!("{} set({})", hex(ret), if after != before { format!("fingerprintReadPos={after}") } else { String::new() }))
    }),
    ("SortingLayerChild.OffsetIsApplicable", |c| { c.with(1)?; Ok(dll_ret(super::super::visual::sorting_layer_child_offset_is_applicable(dll_i32(c, "Behaviour")?))) }),
    ("SnapToVanillaPixelPosition.get_snap", |c| { c.with(1)?; Ok(dll_ret(super::super::visual::snap_to_vanilla_snap(dll_f32(c, "PixelSize")?, dll_i32(c, "Half")? == 1))) }),
    ("SyringeBehaviour.get_Pressure", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let mut s = super::super::syringe::Syringe::default();
        s.PressureMode = dll_i32(c, "PressureMode")?;
        g.syringes.push(s);
        Ok(dll_ret(g.container_pressure(Owner::Syringe(0))))
    }),
    ("Utils.PreferenceToCelsius", |c| { let w = c.with(1)?; Ok(Enc::enc(super::super::Prefs::default().preference_to_celsius(w.p_f32(0)?))) }),
    ("ModLoadStep.get_DisplayName", |c| { c.with(0)?; Ok(Enc::enc(crate::game::background_loader::LaunchStep::display_name(&crate::ui::data::ModLoadStep::default()))) }),
    ("ModLoadStep.get_Thumbnail", |c| { c.with(0)?; Ok(if crate::game::background_loader::LaunchStep::workshop_thumbnail(&crate::ui::data::ModLoadStep::default()) { "Sprite".into() } else { "null".into() }) }),
    ("SteamContraptionLoadStep.get_DisplayName", |c| { c.with(0)?; Ok(Enc::enc(crate::game::background_loader::LaunchStep::display_name(&crate::ui::data::SteamContraptionLoadStep::default()))) }),
    // Only statuses past Catastrophic come back (the others read Time.time, an engine call outside Unity): the time is not read there.
    ("ReactorA5DisplayBehaviour.GetColorForStatus", |c| { let w = c.with(1)?; Ok(Enc::enc(super::super::reactor::color_for_status(w.p_int(0)?, 0.0))) }),
    // gapA r20: Muted, then each Source's `!enabled(@source)=!v` as DllCheck lists the set_enabled calls. A source starts at
    // enabled = v, so only the rewrite's set to !v lists it.
    ("MuteBehaviour.SetMute", |c| {
        let w = c.with(2)?;
        let v = w.p_bool(1)?;
        let mut g = dll_game();
        let field = dll_self_field(c, "Sources")?;
        let names = dll_items(&field)?;
        let sources = names.iter().map(|n| reference(n)).collect::<Vec<_>>();
        for s in sources.iter().flatten() {
            g.audio.sources.insert(*s, super::super::audio::AudioSource { active: true, enabled: v, ..Default::default() });
        }
        g.audio.source_fields.insert((1, "Sources".into()), sources.clone());
        let before = dll_i32(c, "Muted")? == 1;
        g.mutes.push(before);
        g.mute_ids.push(1);
        g.mute_set(0, v);
        let mut out = Vec::new();
        if g.mutes[0] != before { out.push(format!("Muted={}", bit(g.mutes[0]))) }
        for (n, s) in names.iter().zip(&sources) {
            if let Some(e) = s.and_then(|id| g.audio.sources.get(&id)).map(|s| s.enabled).filter(|&e| e != v) { out.push(format!("!enabled({n})={}", bit(e))) }
        }
        Ok(format!("void set({})", out.join(" ")))
    }),
    ("SirenBehaviour.Update", |c| {
        c.with(1)?;
        let mut g = dll_on_physical(c)?;
        g.time.deltaTime = dll_f32(c, "@dt")?;
        let mut s = super::super::small_machines::SirenBehaviour::default();
        dll_load!(c, s);
        s.this_physical = Some(0);
        if let Ok(m) = dll2_link(c, "DamagableMachineryBehaviour") {
            let mut x = super::super::machinery::Machinery::default();
            x.Destroyed = dll_get(&dll_rec(&m)?, "Destroyed")? == "1";
            g.machinery.push(x);
            s.DamagableMachineryBehaviour = Some(0);
        }
        let before = dll_show!(s);
        g.tr.siren_behaviour.push(s);
        g.siren_behaviour_update(0);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.tr.siren_behaviour[0]))))
    }),
    ("ResistorBehaviour.Start", |c| dll2_resistor(c, 1, |g| g.resistor_behaviour_start(0))),
    ("ResistorBehaviour.OnFlip", |c| dll2_resistor(c, 2, |g| g.resistor_behaviour_on_flip(0))),
    ("ResistorBehaviour.OnDisable", |c| dll2_resistor(c, 1, |g| g.resistor_behaviour_on_disable(0))),
    // The Shot's normal and damage are railgun_shot_normal and railgun_shot_damage; its point the pos; CanCrush false,
    // no cartridge and triggersExplosiveOverride true are the send_shot_flags arguments railgun_fire passes with them.
    ("RailgunBehaviour.GenerateShot", |c| {
        let w = c.with(4)?;
        let mut g = dll_on_physical(c)?;
        let mut s = super::super::blaster::Railgun::default();
        s.barrelDirection = dll_floats(c, "barrelDirection")?;
        s.StartingEnergy = dll_f32(c, "StartingEnergy")?;
        s.own = super::super::script_object::ObjectXf::Physical(0);
        g.railguns.push(s);
        let (normal, pos, energy) = (w.p_arr::<2>(1)?, w.p_arr::<2>(2)?, w.p_f32(3)?);
        let n = g.railgun_shot_normal(0, normal);
        Ok(format!("Shot(normal={} point={} damage={} cartridge=null CanCrush=0 triggersExplosiveOverride=1) set()", vec(n), vec(pos), hex(g.railgun_shot_damage(0, energy))))
    }),
    ("WireBehaviour.Slice", |c| {
        c.with(1)?;
        let class = c.args[0].split('(').next().unwrap_or("");
        let kind = match class {
            "CopperWireBehaviour" => WireKind::Copper,
            _ => return Err(format!("no wire kind for {class}")),
        };
        let mut g = dll_game();
        let mut w = super::super::blood_wire::BloodWire::default();
        w.kind = kind;
        w.physical = 0;
        w.breakForceBuffer = dll_f32(c, "breakForceBuffer")?;
        let before = w.breakForceBuffer;
        g.blood_wires.push(w);
        g.blood_wire_slice(0);
        let after = g.blood_wires[0].breakForceBuffer;
        Ok(format!("void set({})", if after.to_bits() != before.to_bits() { format!("breakForceBuffer={}", hex(after)) } else { String::new() }))
    }),
    // A case whose draw would spawn needs the game's data (not loaded here): it panics, a FAIL.
    ("RandomPrefabSpawnerBehaviour.Update", |c| {
        c.with(1)?;
        let mut g = dll_game();
        g.time.deltaTime = dll_f32(c, "@dt")?;
        let mut s = super::super::scenery::Spawner::default();
        dll_load!(c, s);
        // Without the recorded generator a case that draws is not checked (the draw decides whether it spawns).
        match dll_random(c) {
            Ok(r) => g.random = r,
            Err(e) if s.t + g.time.deltaTime > s.PeriodInSeconds => return Err(e),
            Err(_) => {}
        }
        let before = dll_show!(s);
        g.spawners.push(s);
        g.spawner_update(0);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.spawners[0]))))
    }),
    ("PinkSyringe.CreateNewFingerprint", |c| {
        c.with(1)?;
        let mut g = dll_game();
        g.random = dll_random(c)?;
        let mut s = super::super::syringe::Syringe::default();
        s.Fingerprint = dll_items(&dll_self_field(c, "Fingerprint")?)?.into_iter().map(dll_bits).collect::<Result<_, _>>()?;
        s.fingerprintReadPos = dll_i32(c, "fingerprintReadPos")?;
        let show = |s: &super::super::syringe::Syringe| [("Fingerprint", format!("[{}]", s.Fingerprint.iter().map(|v| hex(*v)).collect::<Vec<_>>().join(";"))), ("fingerprintReadPos", s.fingerprintReadPos.to_string())];
        let before = show(&s);
        g.syringes.push(s);
        g.pink_syringe_create_new_fingerprint(0);
        Ok(format!("void {}", dll_changes(&before, &show(&g.syringes[0]))))
    }),
    ("ReturnToPoolAfterTime.get_Progress", |c| {
        c.with(1)?;
        Ok(dll_ret(super::super::pool::ReturnToPoolAfterTime { Life: dll_f32(c, "Life")?, time: dll_f32(c, "time")?, ..Default::default() }.progress()))
    }),
    ("WaterBehaviour.ShouldUpdate", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let mut w = super::super::water::Water::default();
        w.active = dll_i32(c, "@active")? == 1;
        w.enabled = dll_i32(c, "@enabled")? == 1;
        g.waters.push(w);
        Ok(dll_ret(g.water_should_update(0)))
    }),
    ("SelectionBoxBehaviour.Snap", |c| { let w = c.with(1)?; Ok(Enc::enc(super::super::tool_wheel::selection_box_snap(w.p_f32(0)?))) }),
    ("PhaseLinkBehaviour.GetWireColor", |c| { c.with(0)?; Ok(Enc::enc(super::super::link_device::phase_link_wire_color())) }),
    ("PhaseLinkBehaviour.GetWireWidth", |c| { c.with(0)?; Ok(Enc::enc(super::super::link_device::phase_link_wire_width())) }),
    ("PoisonSpreadBehaviour.get_AllowMultipleActivations", |c| { c.with(0)?; Ok(Enc::enc(super::super::poison::poison_allow_multiple_activations())) }),
    ("VisibilityExtensions.Opposite", |c| { let w = c.with(1)?; Ok(Enc::enc(super::super::tool_wheel::visibility_opposite(w.p_int::<i32>(0)?))) }),
    ("RefInt.Increment", |c| { c.with(1)?; dll2_ref_int(c, super::super::water::ref_int_increment) }),
    ("RefInt.Decrement", |c| { c.with(1)?; dll2_ref_int(c, super::super::water::ref_int_decrement) }),
    ("TranslationAnimationBehaviour.Awake", |c| {
        c.with(1)?;
        let before = dll_floats::<3>(c, "startPos")?;
        let after = super::super::link::translation_animation_awake(dll_floats::<3>(c, "@lpos")?);
        Ok(format!("void {}", dll_changes(&[("startPos", vec(before))], &[("startPos", vec(after))])))
    }),
    ("SkinMaterialHandler.get_SmoothedBloodColour", |c| {
        c.with(1)?;
        Ok(dll_ret(crate::limb_material::BloodColour(dll_floats::<4>(c, "bloodColor")?).smoothed_blood_colour()))
    }),
    ("SteamWorkshopController.Report", |c| {
        let w = c.with(2)?;
        let mut s = super::super::mods::SteamWorkshopController { currentProgress: dll_f32(c, "currentProgress")?, ..Default::default() };
        let before = [("currentProgress", hex(s.currentProgress))];
        s.Report(w.p_f32(1)?);
        Ok(format!("void {}", dll_changes(&before, &[("currentProgress", hex(s.currentProgress))])))
    }),
    ("ResizeHandles.StartResizeDrag", |c| {
        let w = c.with(2)?;
        let mut g = dll_game();
        g.resize.is_dragging = dll_i32(c, "isDragging")? == 1;
        g.resize.current_drag_axis = super::super::resize_handles::Axis(dll_i32(c, "currentDragAxis")?);
        let show = |g: &super::super::Game| [("currentDragAxis", g.resize.current_drag_axis.0.to_string()), ("isDragging", bit(g.resize.is_dragging))];
        let before = show(&g);
        g.resize_start_drag(super::super::resize_handles::Axis(w.p_int::<i32>(1)?));
        Ok(format!("void {}", dll_changes(&before, &show(&g))))
    }),
    ("ResizeHandles.get_HasTargets", |c| { c.with(1)?; Ok(dll_ret(dll2_resize(c)?.resize_has_targets())) }),
    ("ResizeHandles.GetSelectionRect", |c| {
        c.with(1)?;
        let (center, extents) = dll2_resize(c)?.resize_selection_rect();
        Ok(format!("Bounds(m_Center={} m_Extents={}) set()", vec(center), vec(extents)))
    }),
    // multiplyingScale and initialTargets before and after.
    // gapC r25: ResizeHandles.MultiTarget's auto-properties on DllCheck's seeded objects.
    ("MultiTarget.get_Flipped", |c| { c.with(1)?; Ok(dll_seeded_row(bit(dll2_multi(c)?.flipped), &[], &[], &[])) }),
    ("MultiTarget.get_Scale", |c| { c.with(1)?; Ok(dll_seeded_row(vec(dll2_multi(c)?.scale), &[], &[], &[])) }),
    ("MultiTarget.get_Position", |c| { c.with(1)?; Ok(dll_seeded_row(vec(dll2_multi(c)?.position), &[], &[], &[])) }),
    ("MultiTarget.set_Flipped", |c| { let w = c.with(2)?; let mut t = dll2_multi(c)?; let b = bit(t.flipped); let v = w.p_bool(1)?; t.flipped = v; Ok(dll_prop_set("<Flipped>k__BackingField", b, bit(t.flipped), bit(v))) }),
    ("MultiTarget.set_Scale", |c| { let w = c.with(2)?; let mut t = dll2_multi(c)?; let b = vec(t.scale); let v = w.p_arr::<2>(1)?; t.scale = v; Ok(dll_prop_set("<Scale>k__BackingField", b, vec(t.scale), vec(v))) }),
    ("MultiTarget.set_Position", |c| { let w = c.with(2)?; let mut t = dll2_multi(c)?; let b = vec(t.position); let v = w.p_arr::<2>(1)?; t.position = v; Ok(dll_prop_set("<Position>k__BackingField", b, vec(t.position), vec(v))) }),
    ("ResizeHandles.ResetHandles", |c| {
        c.with(1)?;
        let mut g = dll2_resize(c)?;
        g.resize.multiplying_scale = dll_f32(c, "multiplyingScale")?;
        if dll_self_field(c, "initialTargets")? != "[]" { return Err("initialTargets not empty".into()) }
        let state = |g: &super::super::Game| [("multiplyingScale", hex(g.resize.multiplying_scale)), ("initialTargets", format!("[{}]", g.resize.initial_targets.iter()
            .map(|t| format!("MultiTarget(<Flipped>k__BackingField={} <Scale>k__BackingField={} <Position>k__BackingField={})", bit(t.flipped), vec(t.scale), vec(t.position))).collect::<Vec<_>>().join(";")))];
        let before = state(&g);
        g.resize_reset_handles();
        Ok(format!("void {}", dll_changes(&before, &state(&g))))
    }),
    // An empty selection (the recorded rawSelection=[]): nothing flips, and the selection stays as it is.
    ("SelectionController.FlipSelection", |c| {
        c.with(1)?;
        if dll_self_field(c, "rawSelection")? != "[]" { return Err("a selection: not loaded here".into()) }
        let mut g = dll_game();
        g.tool = Some(Default::default());
        g.flip_selection();
        let after = g.tool.as_ref().unwrap().rawSelection.len().to_string();
        Ok(format!("void {}", dll_changes(&[("rawSelection", "0".to_string())], &[("rawSelection", after)])))
    }),
    ("TraversalSetPool.CreateFresh", |c| { c.with(0)?; Ok(format!("[{}]", super::super::signal::traversal_set_create_fresh().iter().map(|x| x.to_string()).collect::<Vec<_>>().join(";"))) }),
    // gapC r28: DllCheck's seeded HashSet<Object> (args[1], its `@set<i>#<id>` stand-ins by id): the set after Clear.
    ("TraversalSetPool.ResetObjectForNextUse", |c| {
        c.with(2)?;
        let mut set = dll_items(c.args[1])?.iter().map(|x| x.rsplit('#').next().and_then(|i| i.parse::<i32>().ok()).ok_or_else(|| format!("not a reference: {x}"))).collect::<Result<Vec<i32>, _>>()?;
        super::super::signal::traversal_set_reset_for_next_use(&mut set);
        Ok(dll_seeded_row("void".into(), &[], &[], &[dll_list(set)]))
    }),
    ("TraversalSetPool.GetOverCapacityFallback", |c| { c.with(0)?; Ok(format!("[{}]", super::super::signal::traversal_set_over_capacity_fallback().iter().map(|x| x.to_string()).collect::<Vec<_>>().join(";"))) }),
    // SpawnSpawnParticles off (every row): no SpawnParticles clone, so no instance ids taken.
    ("PhysicalBehaviour.SetupSpawnPrefab", |c| {
        c.with(1)?;
        let mut g = dll_on_physical(c)?;
        g.physicals[0].SpawnSpawnParticles = dll_i32(c, "SpawnSpawnParticles")? == 1;
        if g.physicals[0].SpawnSpawnParticles { return Err("SpawnSpawnParticles: the clone is not recorded here".into()) }
        let before = [("@instances", g.next_instance_id.to_string())];
        g.physical_setup_spawn_prefab(0);
        Ok(format!("void {}", dll_changes(&before, &[("@instances", g.next_instance_id.to_string())])))
    }),
    // GenerateOnStart off (every row): nothing generated, so no objects made.
    ("ShatteredObjectGenerator.Start", |c| {
        c.with(1)?;
        let mut sog = super::super::shatter::ShatteredObjectGenerator::default();
        sog.GenerateOnStart = dll_i32(c, "GenerateOnStart")? == 1;
        if sog.GenerateOnStart { return Err("GenerateOnStart: the shards are not recorded here".into()) }
        let (mut g, mut st) = (dll_game(), super::super::shatter::ShatterState::default());
        let own = super::super::shatter::ToReplace { position: dll_floats(c, "@pos")?, rotation: dll_floats(c, "@rot")?, lossy_scale: dll_floats(c, "@lscale")? };
        let before = [("@instances", g.next_instance_id.to_string()), ("@physicals", g.physicals.len().to_string())];
        g.shattered_object_generator_start(&mut st, &sog, own);
        Ok(format!("void {}", dll_changes(&before, &[("@instances", g.next_instance_id.to_string()), ("@physicals", g.physicals.len().to_string())])))
    }),
    // Not selecting (every row's isSelecting=0): nothing happens.
    ("ToolControllerBehaviour.StopSelectionDrag", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let mut t = super::super::tool::Tool::default();
        t.isSelecting = dll_i32(c, "isSelecting")? == 1;
        if t.isSelecting { return Err("a selection drag: not loaded here".into()) }
        g.tool = Some(t);
        g.stop_selection_drag(false);
        let t = g.tool.as_ref().unwrap();
        Ok(format!("void {}", dll_changes(&[("isSelecting", bit(false)), ("rawSelection", "0".into())], &[("isSelecting", bit(t.isSelecting)), ("rawSelection", t.rawSelection.len().to_string())])))
    }),
    // An empty selection (rawSelection=[]): nothing deleted.
    ("SelectionController.UserDeleteSelection", |c| {
        c.with(1)?;
        if dll_self_field(c, "rawSelection")? != "[]" { return Err("a selection: not loaded here".into()) }
        let mut g = dll_game();
        g.tool = Some(Default::default());
        g.user_delete_selection();
        Ok(format!("void {}", dll_changes(&[("rawSelection", "0".into())], &[("rawSelection", g.tool.as_ref().unwrap().rawSelection.len().to_string())])))
    }),
    // Shocked(zap): Use(null) on an idle bed; Use flips Activated only while enabled (its UpdateActivated plays the effect, not
    // loaded here, so an enabled idle bed is an error).
    ("ThrusterbedBehaviour.Shocked", |c| {
        c.with(2)?;
        let mut g = dll_game();
        let mut t = super::super::thruster::Thrusterbed::default();
        t.Activated = dll_i32(c, "Activated")? == 1;
        t.enabled = dll_i32(c, "@enabled")? == 1;
        if !t.Activated && t.enabled { return Err("an enabled idle bed: UpdateActivated not loaded here".into()) }
        g.thrusterbeds.push(t);
        let before = [("Activated", bit(g.thrusterbeds[0].Activated))];
        g.thrusterbed_shocked(0);
        Ok(format!("void {}", dll_changes(&before, &[("Activated", bit(g.thrusterbeds[0].Activated))])))
    }),
    // OnEMPHit is Break(default): a broken or indestructible set stays as it is (a working one plays its particles and a clip,
    // not loaded here).
    ("TelevisionBehaviour.OnEMPHit", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let mut t = super::super::television::Television::default();
        t.Broken = dll_i32(c, "Broken")? == 1;
        t.Indestructible = dll_i32(c, "Indestructible")? == 1;
        if !t.Broken && !t.Indestructible { return Err("a working set: the break's particles and clip are not loaded here".into()) }
        g.televisions.push(t);
        let before = [("Broken", bit(g.televisions[0].Broken))];
        g.television_on_emp_hit(0);
        Ok(format!("void {}", dll_changes(&before, &[("Broken", bit(g.televisions[0].Broken))])))
    }),
    // No children and no rectangles (every row): nothing to place, nothing made (a child is an Instantiate, not loaded here).
    ("SpawnableOutlineBehaviour.SetOutline", |c| {
        c.with(2)?;
        if dll_self_field(c, "children")? != "[]" || dll_record_field(c.args[1], "Rectangles")? != "[]" { return Err("children or rectangles: not loaded here".into()) }
        let mut o = super::super::spawnable_outline::SpawnableOutline::new();
        o.SetOutline(&[]);
        Ok(format!("void {}", dll_changes(&[("children", "0".into())], &[("children", o.children.len().to_string())])))
    }),
    // Awake on the recorded (or seeded) helper: `initialised` is the stored scale's presence.
    ("PoolableSizeHelper.Awake", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let (init, at) = (dll_i32(c, "initialised")? == 1, dll_floats::<3>(c, "localScaleAtStart")?);
        let show = |h: &(i32, Option<[f32; 3]>)| [("initialised", bit(h.1.is_some())), ("localScaleAtStart", vec(h.1.unwrap_or(at)))];
        g.poolable_size_helpers.push((1, init.then_some(at)));
        let before = show(&g.poolable_size_helpers[0]);
        g.poolable_size_helper_awake(1, dll_floats(c, "@lscale")?);
        Ok(format!("void {}", dll_changes(&before, &show(&g.poolable_size_helpers[0]))))
    }),
    ("UISoundBehaviour.Blip", |c| dll2_ui_sound(c, super::super::audio::UiSoundKind::Blip)),
    ("UISoundBehaviour.Warning", |c| dll2_ui_sound(c, super::super::audio::UiSoundKind::Warning)),
    ("UISoundBehaviour.Error", |c| dll2_ui_sound(c, super::super::audio::UiSoundKind::Error)),
    ("UISoundBehaviour.Up", |c| dll2_ui_sound(c, super::super::audio::UiSoundKind::Up)),
    ("UISoundBehaviour.Down", |c| dll2_ui_sound(c, super::super::audio::UiSoundKind::Down)),
    ("ResizeHandles.get_ForceUniformScaling", |c| { c.with(1)?; Ok(dll_ret(dll2_resize(c)?.resize_force_uniform_scaling())) }),
    ("ResizeHandles.get_SingleTarget", |c| { c.with(1)?; Ok(dll_ret(dll2_resize(c)?.resize_single_target())) }),
    ("PhysicalBehaviour.GetCustomLimitState", |c| {
        c.with(2)?;
        let pen = c.args[1];
        if dll_record_field(pen, "Joint")? != "null" {
            return Err("GetCustomLimitState with a Joint: not loaded here".into());
        }
        let mut g = dll_game();
        g.penetration_arena.push(super::super::stab::Penetration { Active: dll_record_field(pen, "Active")? == "1", ..Default::default() });
        Ok(dll_ret(g.get_custom_limit_state(0, 0) as i32))
    }),
    ("ModLoadStep.OnModDiscovered", |c| {
        c.with(1)?;
        let mut s = crate::ui::data::ModLoadStep::default();
        s.requesting = dll_i32(c, "<RequestingLoad>k__BackingField")? == 1;
        let before = [("<RequestingLoad>k__BackingField", bit(s.requesting))];
        s.OnModDiscovered();
        Ok(format!("void {}", dll_changes(&before, &[("<RequestingLoad>k__BackingField", bit(s.requesting))])))
    }),
    ("PollingDirectoryWatcher.Stop", |c| { c.with(1)?; dll2_watcher(c, |w| w.Stop()) }),
    ("PollingDirectoryWatcher.Dispose", |c| { c.with(1)?; dll2_watcher(c, |w| w.Stop()) }),
    // TyreDeflationBehaviour.Break(impactVelocity) is PopTyre(); Unstabbed(stabbing) is PopTyre(stabbing.point).
    ("TyreDeflationBehaviour.Break", |c| { let w = c.with(2)?; w.p_arr::<2>(1)?; dll2_tyre(c, None) }),
    ("TyreDeflationBehaviour.Unstabbed", |c| {
        c.with(2)?;
        let p = dll_record_field(c.args[1], "point")?;
        let v: Vec<f32> = p.split(',').map(|h| u32::from_str_radix(h, 16).map(f32::from_bits).map_err(|_| format!("point {p}"))).collect::<Result<_, _>>()?;
        dll2_tyre(c, Some([v[0], v[1]]))
    }),
    ("MagnetBehaviour.FixedUpdate", |c| {
        c.with(1)?;
        let mut g = dll_on_physical(c)?;
        let mut s = super::super::magnet::MagnetBehaviour::default();
        dll_load!(c, s);
        if s.Activated {
            return Err("an activated magnet pulls the world's objects: not loaded here".into());
        }
        let before = dll_show!(s);
        g.tr.magnet_behaviour.push(s);
        g.magnet_behaviour_fixed_update(0);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.tr.magnet_behaviour[0]))))
    }),
    ("VibrationBehaviour.FixedUpdate", |c| {
        c.with(1)?;
        let mut g = dll_on_physical(c)?;
        g.time.time = dll_f32(c, "@time")?;
        let mut s = super::super::centrifuge::Vibration::default();
        dll_load!(c, s);
        if s.VibrateForce != 0.0 {
            return Err("a vibrating force goes to the physics world: not loaded here".into());
        }
        let before = dll_show!(s);
        g.vibrations.push(s);
        g.vibration_fixed_update(0);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.vibrations[0]))))
    }),
    // SyringeBehaviour.Break(velocity) is BreakSyringe().
    ("SyringeBehaviour.Break", |c| {
        let w = c.with(2)?;
        w.p_arr::<2>(1)?;
        let mut g = dll_game();
        g.syringes.push(super::super::syringe::Syringe::default());
        let total = dll_f32(c, "<TotalLiquidAmount>k__BackingField")?;
        if total > 0.0 {
            return Err("a syringe with liquid explodes: not loaded here".into());
        }
        g.bc_mut(Owner::Syringe(0)).TotalLiquidAmount = total;
        let show = |g: &super::super::Game| [("<TotalLiquidAmount>k__BackingField", hex(g.bc(Owner::Syringe(0)).TotalLiquidAmount))];
        let before = show(&g);
        g.break_syringe(0);
        Ok(format!("void {}", dll_changes(&before, &show(&g))))
    }),
    ("PhysicalBehaviour.OnBeforeSerialise", |c| {
        c.with(1)?;
        if dll_self_field(c, "penetrations")? != "[]" {
            return Err("penetrations: not loaded here".into());
        }
        let mut g = dll_on_physical(c)?;
        let names = ["serialisable_victims", "serialisable_durations", "serialisable_axes", "serialisable_anchors", "serialisable_active"];
        let before: Vec<(&str, String)> = names.iter().map(|n| Ok((*n, dll_self_field(c, n)?))).collect::<Result<_, String>>()?;
        let s = g.physical_on_before_serialise(0);
        let lens = [s.victims.len(), s.durations.len(), s.axes.len(), s.anchors.len(), s.active.len()];
        if lens.iter().any(|&n| n != 0) {
            return Err("stab arrays with entries: not formatted here".into());
        }
        let after: Vec<(&str, String)> = names.iter().map(|n| (*n, "[]".to_string())).collect();
        Ok(format!("void {}", dll_changes(&before, &after)))
    }),
    ("LineSegment.Equals", |c| { c.with(2)?; Ok(dll_ret(c.args[1].starts_with("LineSegment(") && dll2_seg(c.args[0])? == dll2_seg(c.args[1])?)) }),
    // ToolLibrary.Tool (the rewrite's ToolEntry): only Type is read; the rows' strings have no spaces or underscores.
    ("Tool.op_Equality", |c| { c.with(2)?; Ok(Enc::enc(super::super::tool::ToolEntry::op_Equality(&dll2_tool(c.args[0])?, &dll2_tool(c.args[1])?))) }),
    ("Tool.op_Inequality", |c| { c.with(2)?; Ok(Enc::enc(super::super::tool::ToolEntry::op_Inequality(&dll2_tool(c.args[0])?, &dll2_tool(c.args[1])?))) }),
    // gapA r20: Equals(object): a null or non-Tool argument (`null`, an int) is `obj is Tool` false.
    ("Tool.Equals", |c| { c.with(2)?; if !c.args[1].starts_with("Tool(") { return Ok(dll_ret(false)) } Ok(dll_ret(dll2_tool(c.args[0])?.Equals(&dll2_tool(c.args[1])?))) }),
    ("Tool.GetHashCode", |c| { c.with(1)?; Ok(dll_ret(dll2_tool(c.args[0])?.GetHashCode())) }),
    // NonSteamStatManager.StatCollection on the row's Stats (in insertion order): the return, then the Stats after.
    ("StatCollection.GetStat", |c| { c.with(3)?; let mut s = dll2_stats(c.args[0])?; let r = s.get_stat(&dll2_str(c.args[1])?, dll_bits(c.args[2])?); Ok(format!("{} after={}", r.enc(), dll2_stats_text(&s))) }),
    ("StatCollection.SetStat", |c| { c.with(3)?; let mut s = dll2_stats(c.args[0])?; s.set_stat(&dll2_str(c.args[1])?, dll_bits(c.args[2])?); Ok(format!("void after={}", dll2_stats_text(&s))) }),
    ("StatCollection.HasStat", |c| { c.with(2)?; let s = dll2_stats(c.args[0])?; let r = s.has_stat(&dll2_str(c.args[1])?); Ok(format!("{} after={}", r.enc(), dll2_stats_text(&s))) }),
    ("StatCollection.Increment", |c| { c.with(3)?; let mut s = dll2_stats(c.args[0])?; s.increment(&dll2_str(c.args[1])?, dll_bits(c.args[2])?); Ok(format!("void after={}", dll2_stats_text(&s))) }),
    // The grid controllers' coordinate helpers on a seeded GridSize / GridScale (args[0]).
    ("RadiationGridController.WorldToGridPos", |c| {
        c.with(2)?;
        let mut g = super::super::grid::RadiationGrid::new(0);
        g.GridSize = dll_bits(c.args[0])?;
        let w: Vec<f32> = c.args[1].split(',').map(dll_bits).collect::<Result<_, _>>()?;
        let (x, y) = g.world_to_grid_pos([w[0], w[1]]);
        Ok(format!("Vector2Int(m_X={x} m_Y={y})"))
    }),
    ("PressureGridController.GetWorldPosition", |c| {
        c.with(3)?;
        let mut g = super::super::grid::PressureGrid::new(0);
        g.GridScale = dll_bits(c.args[0])?;
        let n = |i: usize| c.args[i].parse::<i32>().map_err(|_| format!("arg {i} not an int"));
        Ok(vec(g.get_world_position(n(1)?, n(2)?)))
    }),
    ("LineSegment.op_Equality", |c| { c.with(2)?; Ok(Enc::enc(dll2_seg(c.args[0])? == dll2_seg(c.args[1])?)) }),
    ("LineSegment.op_Inequality", |c| { c.with(2)?; Ok(Enc::enc(dll2_seg(c.args[0])? != dll2_seg(c.args[1])?)) }),
    // Utils' contact helpers over a seeded ContactPoint2D[8]: only m_NormalImpulse is read; GetFirstValidContact's answer is
    // the record of the element it picks (the rewrite's pick found by tagging each element's collider with its index).
    ("Utils.GetFirstValidContact", |c| {
        let (items, buffer, count) = dll2_contacts(c)?;
        Ok(items[super::super::physical::get_first_valid_contact(&buffer, count).collider as usize].to_string())
    }),
    ("Utils.GetAverageImpulseRemoveOutliers", |c| {
        if c.args.get(2) != Some(&"3f800000") { return Err("an outlierThreshold other than 1".into()) }
        let (_, buffer, count) = dll2_contacts(c)?;
        Ok(Enc::enc(super::super::physical::get_average_impulse_remove_outliers(&buffer, count)))
    }),
    ("Utils.GetMinImpulse", |c| { let (_, buffer, count) = dll2_contacts(c)?; Ok(Enc::enc(super::super::physical::get_min_impulse(&buffer, count))) }),
    ("Utils.GetMaxImpulse", |c| { let (_, buffer, count) = dll2_contacts(c)?; Ok(Enc::enc(super::super::utils::get_max_impulse(&buffer, count))) }),
    ("Utils.GetAverageImpulse", |c| { let (_, buffer, count) = dll2_contacts(c)?; Ok(Enc::enc(super::super::utils::get_average_impulse(&buffer, count))) }),
    // A seeded Bounds (args[0], its m_Center and m_Extents) and a Vector2.
    ("Utils.IsPointInsideBounds", |c| {
        c.with(2)?;
        let v = |s: String| s.split(',').map(dll_bits).collect::<Result<Vec<f32>, _>>();
        let (center, extents) = (v(dll_record_field(c.args[0], "m_Center")?)?, v(dll_record_field(c.args[0], "m_Extents")?)?);
        let p: Vec<f32> = c.args[1].split(',').map(dll_bits).collect::<Result<_, _>>()?;
        Ok(Enc::enc(super::super::utils::is_point_inside_bounds([center[0], center[1]], [extents[0], extents[1]], [p[0], p[1]])))
    }),
    // HashSet<int> (args[0], sorted) and a range; the set after, sorted.
    ("Utils.AddRange", |c| {
        c.with(2)?;
        let mut set: std::collections::HashSet<i32> = dll_ints(c.args[0])?.into_iter().collect();
        super::super::utils::add_range(&mut set, dll_ints(c.args[1])?);
        let mut after: Vec<i32> = set.into_iter().collect();
        after.sort();
        Ok(format!("void after={}", dll_list(after)))
    }),
    ("Stat.Equals", |c| {
        c.with(2)?;
        let me = super::super::stats::Stat::new(&dll_string(c, "APIName")?);
        let other = c.args[1];
        Ok(dll_ret(if other.starts_with("Stat(") {
            me.Equals(&super::super::stats::Stat::new(dll_record_field(other, "APIName")?.trim_matches('"')))
        } else if other == "null" {
            me.Equals(&())
        } else {
            me.Equals(&other.parse::<i32>().map_err(|_| format!("Stat.Equals arg {other}"))?)
        }))
    }),
];

/// A ResistorBehaviour on its recorded object (physicals[0]); its PhysicalBehaviour field, when it is another
/// component than the object's own record, physicals[1] from its `@link` record. The row's changes: the script's fields,
/// then the linked PhysicalBehaviour's EnergyWireResistance as `@<id>.EnergyWireResistance`.
fn dll2_resistor(c: &Case, n: usize, call: fn(&mut super::super::Game)) -> Result<String, String> {
    c.with(n)?;
    let mut g = dll_on_physical(c)?;
    let mut s = super::super::small_machines::ResistorBehaviour::default();
    dll_load!(c, s);
    s.this_physical = Some(0);
    let mut link = None;
    if let (Ok(r), Some(id)) = (dll2_link(c, "PhysicalBehaviour"), dll_self_field(c, "PhysicalBehaviour").ok().as_deref().and_then(reference)) {
        let mut p = super::super::Physical::default();
        p.EnergyWireResistance = dll_bits(dll_get(&dll_rec(&r)?, "EnergyWireResistance")?)?;
        g.physicals.push(p);
        s.PhysicalBehaviour = Some(1);
        link = Some(id);
    }
    let wire = |g: &super::super::Game| link.map(|id| (id, hex(g.physicals[1].EnergyWireResistance)));
    let (before, wire_before) = (dll_show!(s), wire(&g));
    g.tr.resistor_behaviour.push(s);
    call(&mut g);
    let changes = dll_changes(&before, &dll_show!(g.tr.resistor_behaviour[0]));
    let mut list: Vec<String> = changes.trim_start_matches("set(").trim_end_matches(')').split(' ').filter(|x| !x.is_empty()).map(String::from).collect();
    if let (Some((id, a)), Some((_, b))) = (wire(&g), wire_before) {
        if a != b { list.push(format!("@{id}.EnergyWireResistance={a}")) }
    }
    list.sort();
    Ok(format!("void set({})", list.join(" ")))
}

/// RefInt(Value=v): the call on Value, `set(Value=..)` when it changed.
fn dll2_ref_int(c: &Case, call: fn(&mut i32)) -> Result<String, String> {
    let before = dll_i32(c, "Value")?;
    let mut v = before;
    call(&mut v);
    Ok(format!("void {}", dll_changes(&[("Value", before.to_string())], &[("Value", v.to_string())])))
}

/// A Game whose ResizeHandles holds the row's Targets: each a PhysicalBehaviour from its record (`@link#id`, a seeded row's)
/// with its Resizable, isDisintegrated, position and localScale.
fn dll2_resize(c: &Case) -> Result<super::super::Game, String> {
    let mut g = dll_game();
    for t in dll_items(&dll_self_field(c, "Targets")?)? {
        let id = reference(t).ok_or_else(|| format!("target {t}"))?;
        let mut p = super::super::Physical::default();
        // A target with no @link record is a destroyed PhysicalBehaviour (Unity null): IsValidTarget is false.
        let Ok(rec) = dll_self_field(c, &format!("@link#{id}")) else {
            p.destroyed = true;
            g.resize.Targets.push(g.physicals.len());
            g.physicals.push(p);
            continue;
        };
        let r = dll_rec(&rec)?;
        p.Resizable = dll_get(&r, "Resizable")? == "1";
        p.isDisintegrated = dll_get(&r, "isDisintegrated")? == "1";
        let f3 = |k: &str| -> Result<[f32; 3], String> {
            let v: Vec<f32> = dll_get(&r, k)?.split(',').map(dll_bits).collect::<Result<_, _>>()?;
            v.try_into().map_err(|_| format!("{k} not 3 floats"))
        };
        p.transform.position = f3("@pos")?;
        p.transform.localScale = f3("@lscale")?;
        g.resize.Targets.push(g.physicals.len());
        g.physicals.push(p);
    }
    Ok(g)
}

/// A UISoundBehaviour method on the recorded controller: its AudioSource (live) and clips named by their `@name#id`; the
/// one-shot as DllCheck lists it, `!PlayOneShot(@source,@clip,volume)`.
fn dll2_ui_sound(c: &Case, kind: super::super::audio::UiSoundKind) -> Result<String, String> {
    use super::super::audio::{AudioSource, Clip, UiSound};
    c.with(1)?;
    let source = dll_self_field(c, "AudioSource")?;
    let id = reference(&source).ok_or("AudioSource null")?;
    let clip = |n: &str| -> Result<Option<super::super::audio::ClipRef>, String> {
        let t = dll_self_field(c, n)?;
        Ok((t != "null").then(|| std::sync::Arc::new(Clip {
            asset: Default::default(),
            clip: crate::assets::classes::AudioClip { name: t, load_type: 0, preload_audio_data: true, load_in_background: false, channels: 1, frequency: 44100, length: 1.0, resource: Default::default(), compression_format: 0 },
            pcm: None,
            stream: None,
        })))
    };
    let mut s = UiSound {
        source: id,
        clips: [clip("DefaultClip")?, clip("WarningClip")?, clip("ErrorClip")?, clip("ScrollClip")?, clip("UpClip")?, clip("DownClip")?],
        timeOfLastScrollClip: dll_f32(c, "timeOfLastScrollClip")?,
    };
    let mut g = dll_game();
    g.audio.add(id, AudioSource { active: true, enabled: true, ..Default::default() });
    s.play(&mut g.audio, 0.0, kind);
    let fx: Vec<String> = g.audio.sources[&id].one_shots.iter().map(|v| format!("!PlayOneShot({source},{},{})", v.clip.clip.name, hex(v.volume_scale))).collect();
    Ok(format!("void set({})", fx.join(" ")))
}

/// A PollingDirectoryWatcher with the row's isWatching; `set(isWatching=..)` when the call changed it.
fn dll2_watcher(c: &Case, call: fn(&super::super::contraption::PollingDirectoryWatcher)) -> Result<String, String> {
    use std::sync::atomic::Ordering::SeqCst;
    let w = super::super::contraption::PollingDirectoryWatcher::default();
    w.isWatching.store(dll_i32(c, "isWatching")? == 1, SeqCst);
    let before = [("isWatching", bit(w.isWatching.load(SeqCst)))];
    call(&w);
    Ok(format!("void {}", dll_changes(&before, &[("isWatching", bit(w.isWatching.load(SeqCst)))])))
}

/// A tyre with the row's Deflated and Indestructible; PopTyre past its guard needs the tyre's physical, so only a row the
/// guard stops is run.
fn dll2_tyre(c: &Case, point: Option<[f32; 2]>) -> Result<String, String> {
    let mut g = dll_game();
    let mut t = super::super::tyre::TyreDeflationBehaviour::default();
    t.Deflated = dll_i32(c, "Deflated")? == 1;
    t.Indestructible = dll_i32(c, "Indestructible")? == 1;
    if !t.Deflated && !t.Indestructible {
        return Err("PopTyre past its guard: the tyre's physical is not loaded here".into());
    }
    let before = [("Deflated", bit(t.Deflated))];
    g.tyres.push(t);
    g.tyre_pop(0, point);
    Ok(format!("void {}", dll_changes(&before, &[("Deflated", bit(g.tyres[0].Deflated))])))
}

/// A `RotatedRectangle(Center=.. Size=.. Pivot=.. AngleDegrees=..)` record (the fields as is, not through the constructor).
fn dll2_rect(rec: &str) -> Result<super::super::value_types::RotatedRectangle, String> {
    let r = dll_rec(rec)?;
    let fl = |k: &str| -> Result<Vec<f32>, String> { dll_get(&r, k)?.split(',').map(|h| u32::from_str_radix(h, 16).map(f32::from_bits).map_err(|_| format!("{k} not floats"))).collect() };
    let v2 = |k: &str| -> Result<[f32; 2], String> { let v = fl(k)?; Ok([v[0], v[1]]) };
    Ok(super::super::value_types::RotatedRectangle { Center: v2("Center")?, Size: v2("Size")?, Pivot: v2("Pivot")?, AngleDegrees: fl("AngleDegrees")?[0] })
}

/// A `Tool(Name=.. Type=.. Parent=..)` record as the rewrite's ToolEntry (Type is never null; a null Name or Parent is empty).
fn dll2_tool(rec: &str) -> Result<super::super::tool::ToolEntry, String> {
    let r = dll_rec(rec)?;
    let s = |k: &str| -> Result<String, String> { let v = dll_get(&r, k)?; Ok(if v == "null" { String::new() } else { v.trim_matches('"').to_string() }) };
    if dll_get(&r, "Type")? == "null" { return Err("Type null".into()) }
    Ok(super::super::tool::ToolEntry { name: s("Name")?, ty: s("Type")?, parent: s("Parent")?, ..Default::default() })
}

/// A quoted string argument with no spaces (the rows' keys), as written.
fn dll2_str(v: &str) -> Result<String, String> {
    Ok(v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).ok_or_else(|| format!("not a string: {v}"))?.to_string())
}
/// A `{"key":bits;...}` Dictionary<string, float> as StatCollection.Stats.
fn dll2_stats(v: &str) -> Result<super::super::stats::StatCollection, String> {
    let s = v.strip_prefix('{').and_then(|s| s.strip_suffix('}')).ok_or_else(|| format!("not a dict: {v}"))?;
    let mut stats = Vec::new();
    for e in if s.is_empty() { Vec::new() } else { split_args(s) } {
        let (k, x) = e.rsplit_once(':').ok_or("not key:value")?;
        stats.push((dll2_str(k)?, dll_bits(x)?));
    }
    Ok(super::super::stats::StatCollection { Identifier: String::new(), Stats: stats })
}
fn dll2_stats_text(s: &super::super::stats::StatCollection) -> String {
    format!("{{{}}}", s.Stats.iter().map(|(k, x)| format!("\"{k}\":{}", hex(*x))).collect::<Vec<_>>().join(";"))
}

/// A `LineSegment(A=.. B=..)` record.
fn dll2_seg(rec: &str) -> Result<super::super::value_types::LineSegment, String> {
    let r = dll_rec(rec)?;
    let v2 = |k: &str| -> Result<[f32; 2], String> {
        let v: Vec<f32> = dll_get(&r, k)?.split(',').map(|h| u32::from_str_radix(h, 16).map(f32::from_bits).map_err(|_| format!("{k} not floats"))).collect::<Result<_, _>>()?;
        Ok([v[0], v[1]])
    };
    Ok(super::super::value_types::LineSegment { A: v2("A")?, B: v2("B")? })
}

/// A `[ContactPoint2D(..);..]` buffer of 8 and its count, each element's collider set to its index.
fn dll2_contacts<'a>(c: &Case<'a>) -> Result<(Vec<&'a str>, [crate::b2::contacts2d::ContactPoint2D; 8], usize), String> {
    let items = dll_items(c.args[0])?;
    if items.len() != 8 { return Err("not a buffer of 8".into()) }
    let mut buffer = [crate::b2::contacts2d::ContactPoint2D::default(); 8];
    for (i, it) in items.iter().enumerate() {
        let h = dll_record_field(it, "m_NormalImpulse")?;
        buffer[i].normal_impulse = f32::from_bits(u32::from_str_radix(&h, 16).map_err(|_| "m_NormalImpulse not a float")?);
        buffer[i].collider = i as i32;
    }
    let count = c.args[1].parse::<usize>().map_err(|_| "count not an int")?;
    Ok((items, buffer, count))
}
