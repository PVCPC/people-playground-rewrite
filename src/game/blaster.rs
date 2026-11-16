//! BlasterBehaviour (the Blaster, Blaster Rifle and Machine Blaster) and BlasterboltBehaviour (their bolts): a bare
//! Transform the script steps along its right vector with Utils.MaterialAwareRaycast until a ray finds something.
//! The muzzle flash plays (particles.rs) and the bolt's TrailRenderer is drawn by trail.rs. The TrailRenderer has
//! autodestruct set once it closed: the native renderer destroys the GameObject when its trail (`m_Time`) has faded.
#![allow(non_snake_case)]

use super::explosion::{ExplosionParameters, RayHit2D};
use super::translated::rt::Xf;
use super::coroutine::{Routine, Yield};
use super::{d, mathf, script, Game};
use crate::assets::db::ObjRef;
use crate::b2::math::Vec2 as V;

/// Global.main.EnergyLayers (Main.unity): layer 17.
const ENERGY_LAYERS: u32 = 1 << 17;
/// Utils.MaterialAwareRaycast's maxSteps default.
const MAX_STEPS: i32 = 8;

script! {
    Blaster {
        Interval: f32 = 0.05,
        Recoil: f32 = 1.0,
        InaccuracyMultiplier: f32 = 0.05,
        Automatic: bool = false,
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// GetComponent<PhysicalBehaviour>() (Awake).
        physical: usize = 0,
        enabled: bool = true,
        /// Bolt: instantiated by Shoot; its pose is the script's own (link checks it has no physics).
        Bolt: Option<ObjRef> = None,
        /// Clips.Length
        Clips: usize = 0,
        ScreenShakeMultiplier: f32 = 1.0,
        /// The private `t`: the seconds since the last shot.
        t: f32 = 0.0,
        /// FlipMuzzleFlash, and Muzzleflash's Transform id (None: null, an NRE in a flipped Start).
        FlipMuzzleFlash: bool = true,
        muzzle: Option<i32> = None,
    }
}

script! {
    Blasterbolt {
        Speed: f32 = 1.0,
        ImpactStrength: f32 = 0.0,
        damage: f32 = 1.0,
        createHitDecal: bool = true,
        SpawnExplosionEffect: bool = false,
        CreateExplosion: bool = false,
        DecalSizeMultiplier: f32 = 1.0,
        TemperatureTarget: f32 = 900.0,
        ExplosionDamage: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// layers (LayerMask bits)
        layers: u32 = 0,
        ImpactEffect: Option<ObjRef> = None,
        HitDecal: Option<ObjRef> = None,
        /// ImpactDecal: an arena index of `Game::decal_descriptors`; None for a null asset.
        ImpactDecal: Option<usize> = None,
        /// The Trail's m_Time: 0.025 in all three bolt prefabs (the simulation's copy; trail.rs draws the trail).
        trail_time: f32 = 0.025,
        /// The private `closing`, and the time it was set.
        closing: bool = false,
        closed_at: f64 = 0.0,
        /// Trail.autodestruct: Update sets it on every frame once closing (gapB r24; FnResult ` tad=`).
        trail_autodestruct: bool = false,
    }
}

script! {
    BeamCannon {
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
        OverchargeThreshold: f32 = 50.0,
        ScreenShake: f32 = 5.0,
        Recoil: f32 = 25.0,
    }
    links {
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// The component's instance id.
        id: i32 = 0,
        /// PhysicalBehaviour (a serialised reference).
        physical: usize = 0,
        /// LayerMask (bits)
        mask: u32 = 0,
        /// LineRenderer: its index in `Game::lines`.
        line: usize = 0,
        /// GlowSprite
        glow: Option<super::visual::SpriteAt> = None,
    }
}

script! {
    RayCannon {
        RayCastWidth: f32 = 0.0,
        RayDelay: f32 = 0.296,
        RayDuration: f32 = 1.058,
        ShakeIntensity: f32 = 2.0,
        AmbientTemperatureIncrease: f32 = 30.0,
        Recoil: f32 = 12.0,
        localBarrelPosition: [f32; 2] = [0.0; 2],
        localBarrelDirection: [f32; 2] = [0.0; 2],
    }
    links {
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference on the same object).
        physical: usize = 0,
        /// LayerMask (bits)
        mask: u32 = 0,
        ExplosionParams: Option<ExplosionParameters> = None,
        ExplosionPrefab: Option<ObjRef> = None,
        /// The private isBusy.
        isBusy: bool = false,
        /// RayRenderer: its index in `Game::lines`.
        line: usize = 0,
        /// GlowSprite
        glow: Option<super::visual::SpriteAt> = None,
    }
}

// RayCannonBehaviour.MoveNext (HeatUpRayArea's `line` iterator; its GetEnumerator, Current, Reset and Dispose are the iterator plumbing)
// RayCannonBehaviour.System.Collections.Generic.IEnumerable<UnityEngine.Vector2Int>.GetEnumerator: the `line` iterator's plumbing (HeatUpRayArea).
// RayCannonBehaviour.System.Collections.IEnumerator.get_Current: the `line` iterator's plumbing (HeatUpRayArea).
// RayCannonBehaviour.System.Collections.Generic.IEnumerator<UnityEngine.Vector2Int>.get_Current: the `line` iterator's plumbing (HeatUpRayArea).
// RayCannonBehaviour.System.Collections.IEnumerable.GetEnumerator: the `line` iterator's plumbing (HeatUpRayArea).
// RayCannonBehaviour.System.Collections.IEnumerator.Reset: the `line` iterator's plumbing (HeatUpRayArea).
// RayCannonBehaviour.System.IDisposable.Dispose: the `line` iterator's plumbing (HeatUpRayArea).
/// RayCannonBehaviour.HeatUpRayArea's local `line`: the grid points from (x, y) to (x2, y2), Bresenham style.
fn ray_cannon_line(mut x: i32, mut y: i32, x2: i32, y2: i32) -> Vec<(i32, i32)> {
    let (num, num2) = (x2 - x, y2 - y);
    let dx1 = num.signum();
    let dy1 = num2.signum();
    let mut dx2 = num.signum();
    let mut dy2 = 0;
    let mut longest = num.abs();
    let mut shortest = num2.abs();
    if longest <= shortest {
        longest = num2.abs();
        shortest = num.abs();
        dy2 = num2.signum();
        dx2 = 0;
    }
    let mut numerator = longest >> 1;
    let mut out = Vec::with_capacity(longest as usize + 1);
    for _ in 0..=longest {
        out.push((x, y));
        numerator += shortest;
        if numerator >= longest {
            numerator -= longest;
            x += dx1;
            y += dy1;
        } else {
            x += dx2;
            y += dy2;
        }
    }
    out
}

/// LayerMask.NameToLayer("Bounds") (TagManager.asset).
const BOUNDS_LAYER: i32 = 11;

impl Game {
    /// RayCannonBehaviour.BarrelPosition: rigidbody.GetRelativePoint(localBarrelPosition * transform.localScale).
    pub(crate) fn ray_cannon_barrel_position(&mut self, i: usize) -> [f32; 2] {
        let p = self.ray_cannons[i].physical;
        let s = self.xf_local_scale(&self.ray_cannons[i].own);
        let b = self.ray_cannons[i].localBarrelPosition;
        let v = [b[0] * s[0], b[1] * s[1]];
        self.sync_pending();
        let xf = self.world.body(self.physicals[p].rigidbody.body).xf;
        [xf.q.c * v[0] - xf.q.s * v[1] + xf.p.x, xf.q.s * v[0] + xf.q.c * v[1] + xf.p.y]
    }

    /// RayCannonBehaviour.BarrelDirection: rigidbody.GetRelativeVector(localBarrelDirection) * transform.localScale.x.
    pub(crate) fn ray_cannon_barrel_direction(&mut self, i: usize) -> [f32; 2] {
        let p = self.ray_cannons[i].physical;
        let b = self.ray_cannons[i].localBarrelDirection;
        self.sync_pending();
        let xf = self.world.body(self.physicals[p].rigidbody.body).xf;
        let s = self.xf_local_scale(&self.ray_cannons[i].own)[0];
        [(xf.q.c * b[0] - xf.q.s * b[1]) * s, (xf.q.s * b[0] + xf.q.c * b[1]) * s]
    }

    /// RayCannonBehaviour.Shoot / RayCannonBehaviour.Use
    /// RayCannonBehaviour.Use / Shoot: while enabled, StartCoroutine(ShootRoutine).
    pub fn ray_cannon_shoot(&mut self, i: usize) {
        if self.ray_cannons[i].enabled {
            let id = self.ray_cannons[i].id;
            self.start_coroutine(id, id, Self::ray_cannon_routine(i));
        }
    }

    /// RayCannonBehaviour.OnDisable: StopAllCoroutines, AudioSource.Stop, the line and glow off, Impact/MuzzleParticles.Stop()
    /// and isBusy = false.
    pub fn ray_cannon_on_disable(&mut self, i: usize) {
        let id = self.ray_cannons[i].id;
        self.stop_all_coroutines(id);
        let src = self.audio_src(id, "AudioSource");
        self.audio.stop(src);
        self.ray_cannon_show(i, false);
        self.ps_field_call(id, "ImpactParticles", true, super::PsCall::Stop(self.time.timeAsDouble));
        self.ps_field_call(id, "MuzzleParticles", true, super::PsCall::Stop(self.time.timeAsDouble));
        self.ray_cannons[i].isBusy = false;
    }

    /// RayCannonBehaviour.Start: RayRenderer.enabled = false and GlowSprite.enabled = false, Muzzle/ImpactParticles.Stop().
    pub fn ray_cannon_start(&mut self, i: usize) {
        self.ray_cannon_show(i, false);
        let id = self.ray_cannons[i].id;
        self.ps_field_call(id, "MuzzleParticles", true, super::PsCall::Stop(self.time.timeAsDouble));
        self.ps_field_call(id, "ImpactParticles", true, super::PsCall::Stop(self.time.timeAsDouble));
    }

    /// RayRenderer.enabled and GlowSprite.enabled.
    fn ray_cannon_show(&mut self, i: usize, on: bool) {
        let (line, glow) = (self.ray_cannons[i].line, self.ray_cannons[i].glow.expect("RayCannonBehaviour.GlowSprite"));
        self.lines[line].enabled = on;
        self.set_sprite_enabled(glow, on);
    }

    /// RayCannonBehaviour.ShootRoutine. After RayDelay, each FixedUpdate for RayDuration seconds: the recoil, a
    /// CircleCast from the barrel, AffectObject on a hit in the world and, with ambient temperature transfer, the heat
    /// along the ray. Then a last CircleCast with the last barrel pose explodes at its end.
    pub(crate) fn ray_cannon_routine(i: usize) -> Routine {
        #[derive(Clone, Copy)]
        enum Stage {
            Begin,
            Fire,
            Step,
        }
        let mut stage = Stage::Begin;
        let (mut dir, mut pos, mut ambient, mut t) = ([0.0f32; 2], [0.0f32; 2], false, 0.0f32);
        Box::new(move |game| {
            match stage {
                Stage::Begin => {
                    if game.ray_cannons[i].isBusy {
                        return Yield::Done;
                    }
                    game.ray_cannons[i].isBusy = true;
                    let id = game.ray_cannons[i].id;
                    game.audio_one_shot(id, "AudioSource", "CannonClip");
                    game.ps_field_play_stop(id, "ChargeUpParticles", true);
                    stage = Stage::Fire;
                    return Yield::WaitForSeconds(game.ray_cannons[i].RayDelay);
                }
                Stage::Fire => {
                    dir = game.ray_cannon_barrel_direction(i);
                    pos = game.ray_cannon_barrel_position(i);
                    game.ray_cannon_show(i, true);
                    let id = game.ray_cannons[i].id;
                    game.ps_field_play_stop(id, "ImpactParticles", true);
                    game.ps_field_play_stop(id, "MuzzleParticles", true);
                    ambient = game.prefs.AmbientTemperatureTransfer;
                    t = 0.0;
                    stage = Stage::Step;
                }
                Stage::Step => t += game.time.fixedDeltaTime,
            }
            let s = &game.ray_cannons[i];
            let (p, width, duration, mask, shake, recoil) = (s.physical, s.RayCastWidth, s.RayDuration, s.mask, s.ShakeIntensity, s.Recoil);
            if t < duration {
                let d0 = game.ray_cannon_barrel_direction(i);
                let k = 0.0 - recoil;
                let rb = game.physicals[p].rigidbody.clone();
                rb.add_force(&mut game.world, [d0[0] * k, d0[1] * k]);
                dir = game.ray_cannon_barrel_direction(i);
                pos = game.ray_cannon_barrel_position(i);
                let hit = game.circle_cast(pos, width, dir, 10000.0, mask);
                let vector = match &hit {
                    Some(h) => [h.point.x, h.point.y],
                    None => [pos[0] + dir[0] * 10000.0, pos[1] + dir[1] * 10000.0],
                };
                let (line, glow) = (game.ray_cannons[i].line, game.ray_cannons[i].glow.expect("RayCannonBehaviour.GlowSprite"));
                let local = game.line_inverse_transform_point(line, [vector[0], vector[1], 0.0]);
                game.line_set_position(line, 1, local);
                let at = game.xf_position(&game.ray_cannons[i].own);
                game.camera_shake(shake, [at[0], at[1]]);
                // GlowSprite.transform.position = (vector + pos) * 0.5f (a Vector2) and localScale = Vector3.one * distance (0 without a hit).
                game.set_sprite_world_position(glow, [(vector[0] + pos[0]) * 0.5, (vector[1] + pos[1]) * 0.5, 0.0]);
                let distance = hit.as_ref().map_or(0.0, |h| h.distance);
                game.set_sprite_local_scale(glow, [distance, distance, distance]);
                if let Some(h) = hit {
                    let h = RayHit2D { collider: h.collider, game_object: h.game_object, body: h.body, point: h.point, normal: h.normal, distance: h.distance, fraction: h.fraction };
                    if let Some(v) = game.hit_world_physical(&h) {
                        game.ray_cannon_affect_object(v, [h.point.x, h.point.y], [h.normal.x, h.normal.y]);
                    }
                }
                if ambient {
                    game.ray_cannon_heat_up(i, pos, vector);
                }
                return Yield::WaitForFixedUpdate;
            }
            let vector = match game.circle_cast(pos, width, dir, 10000.0, mask) {
                Some(h) => [h.point.x, h.point.y],
                None => [pos[0] + dir[0] * 10000.0, pos[1] + dir[1] * 10000.0],
            };
            let s = &game.ray_cannons[i];
            let mut ex = s.ExplosionParams.clone().expect("RayCannonBehaviour.ExplosionParams");
            let prefab = s.ExplosionPrefab;
            ex.Position = [vector[0], vector[1], 0.0];
            let (force, range) = (ex.FragmentForce, ex.Range);
            let under_water = game.is_point_under_water(vector);
            game.create_explosion_with_water(under_water, ex);
            game.create_pulse_explosion_with([vector[0], vector[1], 0.0], force, range, false, true);
            let prefab = prefab.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
            game.spawn_prefab(prefab, [vector[0], vector[1], 0.0]);
            let id = game.ray_cannons[i].id;
            game.ps_field_play_stop(id, "MuzzleParticles", false);
            game.ps_field_play_stop(id, "ImpactParticles", false);
            game.ray_cannon_show(i, false);
            game.ray_cannons[i].isBusy = false;
            // yield return new WaitForSeconds(0.5f), and nothing after it.
            stage = Stage::Step;
            Yield::Done
        })
    }

    /// RayCannonBehaviour.HeatUpRayArea
    fn ray_cannon_heat_up(&mut self, i: usize, pos: [f32; 2], end: [f32; 2]) {
        let inc = self.ray_cannons[i].AmbientTemperatureIncrease;
        let a = self.WorldToGridPoint(pos[0], pos[1]);
        let b = self.WorldToGridPoint(end[0], end[1]);
        for (x, y) in ray_cannon_line(a.0, a.1, b.0, b.1) {
            self.EnsureExistence(x, y);
            self.ChangeTemperatureAt(x, y, inc);
        }
    }

    /// RayCannonBehaviour.AffectObject
    pub(crate) fn ray_cannon_affect_object(&mut self, v: usize, pos: [f32; 2], normal: [f32; 2]) {
        if self.physicals[v].isDisintegrated {
            return;
        }
        if self.physicals[v].SimulateTemperature {
            let r = self.random.range_int(40, 60);
            self.physicals[v].Temperature += r as f32;
        }
        if self.physicals[v].rigidbody.bodyType == 0 {
            let rb = self.physicals[v].rigidbody.clone();
            rb.add_force_at_position(&mut self.world, [normal[0] * -5.0, normal[1] * -5.0], pos);
        }
        if self.random.value() > 0.95 {
            self.send_slice(v);
        }
        if let Some(l) = self.limbs.iter().position(|l| l.PhysicalBehaviour == v) {
            self.limbs[l].InternalTemperature += 5.0;
            self.skin_add_damage_point(l, super::misc::damage_type::BURN, pos, 40.0);
            if self.random.value() > 0.95 {
                self.limb_crush(l);
            }
        }
    }

    /// BeamCannonBehaviour.Awake: LineRenderer.useWorldSpace = false and LineRenderer.enabled = false (run at link: `beam_cannon_awake`).
    pub fn beam_cannon_awake(&mut self, i: usize) {
        let line = self.beam_cannons[i].line;
        self.lines[line].use_world_space = false;
        self.lines[line].enabled = false;
    }

    /// BeamCannonBehaviour.BarrelPosition: transform.TransformPoint(barrelPosition), as a Vector2.
    pub(crate) fn beam_cannon_barrel_position(&self, i: usize) -> [f32; 2] {
        let b = self.beam_cannons[i].barrelPosition;
        let p = self.xf_transform_point(&self.beam_cannons[i].own, [b[0], b[1], 0.0]);
        [p[0], p[1]]
    }

    /// BeamCannonBehaviour.BarrelDirection: transform.TransformDirection(barrelDirection) * transform.localScale.x.
    pub(crate) fn beam_cannon_barrel_direction(&self, i: usize) -> [f32; 2] {
        let own = &self.beam_cannons[i].own;
        let b = self.beam_cannons[i].barrelDirection;
        let v = self.xf_transform_direction(own, [b[0], b[1], 0.0]);
        let s = self.xf_local_scale(own)[0];
        [v[0] * s, v[1] * s]
    }

    /// BeamCannonBehaviour.Shoot / BeamCannonBehaviour.Use
    /// BeamCannonBehaviour.Use / Shoot: the recoil impulse, then StartCoroutine of the fire routine (nothing runs on an inactive
    /// GameObject), which runs to BlinkBeam's wait at once.
    pub fn beam_cannon_shoot(&mut self, i: usize) {
        let p = self.beam_cannons[i].physical;
        let (dir, pos) = (self.beam_cannon_barrel_direction(i), self.beam_cannon_barrel_position(i));
        let k = 0.0 - self.beam_cannons[i].Recoil;
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_impulse_at_position(&mut self.world, [dir[0] * k, dir[1] * k], pos);
        let id = self.beam_cannons[i].id;
        let mut blink = false;
        self.start_coroutine(
            id,
            id,
            Box::new(move |game| {
                if blink {
                    // BlinkBeam after the wait.
                    game.beam_cannon_blink(i, false);
                    return Yield::Done;
                }
                blink = true;
                game.beam_cannon_fire(i);
                // yield return BlinkBeam(): enabled, then WaitForSeconds(0.05f).
                game.beam_cannon_blink(i, true);
                Yield::WaitForSeconds(0.05)
            }),
        );
    }

    /// BeamCannonBehaviour.BlinkBeam's LineRenderer.enabled and GlowSprite.enabled.
    pub(crate) fn beam_cannon_blink(&mut self, i: usize, on: bool) {
        let line = self.beam_cannons[i].line;
        self.lines[line].enabled = on;
        let glow = self.beam_cannons[i].glow.expect("BeamCannonBehaviour.GlowSprite");
        self.set_sprite_enabled(glow, on);
    }

    /// BeamCannonBehaviour.SetupGlowSprite: GlowSprite.transform.localPosition = (length / 2, 0) and localScale = 2 * (length, length).
    pub(crate) fn beam_cannon_setup_glow(&mut self, i: usize, length: f32) {
        let glow = self.beam_cannons[i].glow.expect("BeamCannonBehaviour.GlowSprite");
        self.set_sprite_local_position(glow, [length / 2.0, 0.0, 0.0]);
        // 2f * new Vector3(length, length): the z stays 0.
        self.set_sprite_local_scale(glow, [2.0 * length, 2.0 * length, 0.0]);
    }

    /// NormalFireRoutine and OverchargedFireRoutine up to BlinkBeam.
    fn beam_cannon_fire(&mut self, i: usize) {
        let p = self.beam_cannons[i].physical;
        if self.physicals[p].Charge() > self.beam_cannons[i].OverchargeThreshold {
            self.beam_cannon_overcharged_fire(i);
        } else {
            self.beam_cannon_normal_fire(i);
        }
    }

    fn beam_cannon_shake_at(&self, i: usize) -> [f32; 2] {
        let t = self.xf_position(&self.beam_cannons[i].own);
        [t[0], t[1]]
    }

    /// BeamCannonBehaviour.OverchargedFireRoutine up to BlinkBeam: LinecastNonAlloc(BarrelPosition, BarrelPosition + BarrelDirection * 100000f, buffer[32]).
    pub(crate) fn beam_cannon_overcharged_fire(&mut self, i: usize) {
        let (shake, line) = (self.beam_cannons[i].ScreenShake, self.beam_cannons[i].line);
        let shake_at = self.beam_cannon_shake_at(i);
        let (pos, dir) = (self.beam_cannon_barrel_position(i), self.beam_cannon_barrel_direction(i));
        let end = V::new(pos[0] + dir[0] * 100000.0, pos[1] + dir[1] * 100000.0);
        let hits = self.linecast_hits(V::new(pos[0], pos[1]), end, super::explosion::DEFAULT_RAYCAST_LAYERS, 32);
        self.line_set_position(line, 1, [100000.0, 0.0, 0.0]);
        self.beam_cannon_setup_glow(i, 100000.0);
        for hit in &hits {
            let v = self.hit_world_physical(hit);
            self.beam_cannon_on_hit(i, v, [hit.point.x, hit.point.y]);
        }
        let id = self.beam_cannons[i].id;
        self.audio_one_shot_own(id, "AudioSource");
        self.camera_shake(shake * 2.0, shake_at);
    }

    /// BeamCannonBehaviour.NormalFireRoutine up to BlinkBeam: Physics2D.Raycast(BarrelPosition, BarrelDirection, 100000f, LayerMask).
    pub(crate) fn beam_cannon_normal_fire(&mut self, i: usize) {
        let p = self.beam_cannons[i].physical;
        let (shake, mask, line) = (self.beam_cannons[i].ScreenShake, self.beam_cannons[i].mask, self.beam_cannons[i].line);
        let shake_at = self.beam_cannon_shake_at(i);
        let (pos, dir) = (self.beam_cannon_barrel_position(i), self.beam_cannon_barrel_direction(i));
        if let Some(hit) = self.raycast(V::new(pos[0], pos[1]), dir, 100000.0, mask, f32::NEG_INFINITY, f32::INFINITY) {
            let local = self.xf_inverse_transform_point(&self.beam_cannons[i].own, [hit.point.x, hit.point.y, 0.0]);
            self.line_set_position(line, 1, local);
            self.beam_cannon_setup_glow(i, hit.distance);
            let v = self.hit_world_physical(&hit);
            self.beam_cannon_on_hit(i, v, [hit.point.x, hit.point.y]);
        } else {
            self.line_set_position(line, 1, [100000.0, 0.0, 0.0]);
            self.beam_cannon_setup_glow(i, 100000.0);
        }
        let id = self.beam_cannons[i].id;
        self.audio_one_shot_own(id, "AudioSource");
        self.camera_shake(shake, shake_at);
        if self.physicals[p].SimulateTemperature {
            self.physicals[p].Temperature += 30.0;
        }
    }

    /// BeamCannonBehaviour.OnHit(other, pos): `v` is other's Global.PhysicalObjectsInWorldByTransform entry.
    pub(crate) fn beam_cannon_on_hit(&mut self, i: usize, v: Option<usize>, pos: [f32; 2]) {
        self.explosion_creator_explode(ExplosionParameters {
            FragmentationRayCount: 32,
            Position: [pos[0], pos[1], 0.0],
            Range: 2.0,
            FragmentForce: 2.0,
            CreateParticlesAndSound: true,
            EffectSize: super::explosion::EffectSize::Large,
            DismemberChance: 1.0,
            BallisticShrapnelCount: 0,
        });
        let Some(v) = v else { return };
        if self.physical_layer(v) == BOUNDS_LAYER {
            return;
        }
        if self.physicals[v].SimulateTemperature {
            self.physicals[v].Temperature += 300.0;
        }
        self.ignite_at(v, true, pos);
        let dir = self.beam_cannon_barrel_direction(i);
        let rb = self.physicals[v].rigidbody.clone();
        rb.add_impulse_at_position(&mut self.world, [dir[0] * 5.0, dir[1] * 5.0], pos);
        self.physicals[v].burnIntensity = 1.0;
        let dir = self.beam_cannon_barrel_direction(i);
        // value.SendMessage: send_break reads its collider only without a physical.
        self.send_break(0, Some(v), [dir[0] * 25.0, dir[1] * 25.0]);
        self.send_slice(v);
    }

    /// BlasterBehaviour.BarrelPosition
    /// BarrelPosition: transform.TransformPoint(barrelPosition), as a Vector2.
    pub(crate) fn blaster_barrel_position(&self, i: usize) -> [f32; 2] {
        let b = self.blasters[i].barrelPosition;
        let p = self.transform_point3(self.blasters[i].physical, [b[0], b[1], 0.0]);
        [p[0], p[1]]
    }

    /// BlasterBehaviour.BarrelDirection
    /// BarrelDirection: transform.TransformDirection(barrelDirection) * transform.localScale.x.
    pub(crate) fn blaster_barrel_direction(&self, i: usize) -> [f32; 2] {
        let p = self.blasters[i].physical;
        let b = self.blasters[i].barrelDirection;
        let v = self.transform_direction(p, [b[0], b[1], 0.0]);
        let s = self.physicals[p].transform.localScale[0];
        [v[0] * s, v[1] * s]
    }

    /// BlasterBehaviour.Use
    pub fn blaster_use(&mut self, i: usize) {
        self.blaster_shoot(i);
    }

    /// BlasterBehaviour.UseContinuous: ContinuousActivationBehaviour.AssertState() only checks.
    pub fn blaster_use_continuous(&mut self, i: usize) {
        let s = &self.blasters[i];
        if s.Automatic && !(s.t < s.Interval) && s.enabled {
            self.blaster_shoot(i);
        }
    }

    /// BlasterBehaviour.Update
    pub fn blaster_update(&mut self, i: usize) {
        self.blasters[i].t += self.time.deltaTime;
    }

    /// BlasterBehaviour.Start (and OnFlip): a mirrored blaster turns its muzzle flash to world eulerAngles (0, 0, 180)
    /// (ObjectState saves the flash's children's poses).
    pub fn blaster_start(&mut self, i: usize) {
        let s = &self.blasters[i];
        if self.physicals[s.physical].transform.localScale[0] < 0.0 && s.FlipMuzzleFlash {
            let Some(m) = s.muzzle else { super::menu::npe() };
            let q = super::menu::euler_z_rotation(180.0);
            self.with_component_instance(m, |_, inst, o| inst.set_world_rotation(o, q));
        }
    }

    /// Muzzleflash.transform.rotation (None: no muzzle flash).
    pub(crate) fn blaster_muzzle_rotation(&mut self, i: usize) -> Option<[f32; 4]> {
        let m = self.blasters[i].muzzle?;
        Some(self.with_component_instance(m, |_, inst, o| inst.rotation(o)))
    }

    /// BlasterBehaviour.Shoot
    pub fn blaster_shoot(&mut self, i: usize) {
        if !self.blasters[i].enabled {
            return;
        }
        let vector = self.blaster_barrel_position(i);
        let vector2 = self.blaster_barrel_direction(i);
        self.blasters[i].t = 0.0;
        let s = &self.blasters[i];
        let (p, recoil, inaccuracy, bolt, clips, shake) = (s.physical, s.Recoil, s.InaccuracyMultiplier, s.Bolt, s.Clips, s.ScreenShakeMultiplier);
        let k = 0.0 - recoil;
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_impulse_at_position(&mut self.world, [vector2[0] * k, vector2[1] * k], vector);
        // Object.Instantiate(Bolt, vector, identity).transform.right = vector2 + InaccuracyMultiplier * insideUnitCircle
        let bolt = bolt.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let id = self.spawn_prefab(bolt, [vector[0], vector[1], 0.0]);
        let r = self.random.inside_unit_circle();
        self.tr_set_right(Xf::Object(id), [vector2[0] + inaccuracy * r[0], vector2[1] + inaccuracy * r[1], 0.0]);
        // blasterSoundSource.PlayOneShot(Clips.Length != 0 ? Clips.PickRandom() : blasterSoundSource.clip)
        let id = self.blasters[i].id;
        let source = self.audio.source_field(id, "blasterSoundSource").unwrap_or_else(|| super::menu::throw("NullReferenceException: BlasterBehaviour.blasterSoundSource"));
        let clip = if clips != 0 { self.pick_clip(id, "Clips", clips) } else { self.audio.get(source).clip.clone() };
        self.audio.play_one_shot(source, clip.as_ref(), 1.0);
        self.camera_shake(0.5 * shake, vector);
        self.ps_field_play_stop(id, "Muzzleflash", true);
    }

    /// The physical a hit's transform is (Global.PhysicalObjectsInWorldByTransform), if it is in the world.
    pub(super) fn hit_world_physical(&mut self, hit: &RayHit2D) -> Option<usize> {
        self.hit_transform_physical(hit).or_else(|| self.physical_of_collider(hit.collider)).filter(|&p| self.physicals[p].in_world)
    }

    /// BlasterboltBehaviour.Update
    pub fn blasterbolt_update(&mut self, b: usize) {
        let id = self.blasterbolts[b].id;
        if self.blasterbolts[b].closing {
            // Trail.autodestruct = true: the renderer destroys the object once its trail has faded (blasterbolt_trails_update).
            self.blasterbolts[b].trail_autodestruct = true;
            return;
        }
        let (position, rotation) = self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.rotation(o)));
        let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
        let s = &self.blasterbolts[b];
        let (speed, layers, strength, damage) = (s.Speed, s.layers, s.ImpactStrength, s.damage);
        let num = speed * self.time.deltaTime;
        let laser = self.material_aware_raycast([position[0], position[1]], [right[0], right[1]], num, layers, MAX_STEPS, 0, 0.0);
        let Some(hit) = laser.hit else {
            let to = [position[0] + right[0] * num, position[1] + right[1] * num, position[2] + right[2] * num];
            self.with_component_instance(id, |_, inst, o| inst.set_world_position(o, to));
            return;
        };
        let (point, normal) = ([hit.point.x, hit.point.y], [hit.normal.x, hit.normal.y]);
        let flag = super::physical::has_layer(ENERGY_LAYERS, self.world.layers[&hit.game_object]);
        match laser.physical {
            Some(p) if self.physicals[p].ReflectsLasers || flag => {
                let k = 0.0 - strength;
                let rb = self.physicals[p].rigidbody.clone();
                rb.add_impulse_at_position(&mut self.world, [normal[0] * k, normal[1] * k], point);
                // Vector2.Reflect(transform.right, normal)
                let dir = [right[0], right[1]];
                let num = (d(-2.0) * (d(normal[0]) * dir[0] + d(normal[1]) * dir[1]).f()).f();
                let reflected = [(d(num) * normal[0] + dir[0]).f(), (d(num) * normal[1] + dir[1]).f()];
                // transform.position = value.point (z becomes 0); transform.right = vector.
                self.with_component_instance(id, |_, inst, o| inst.set_world_position(o, [point[0], point[1], 0.0]));
                self.tr_set_right(Xf::Object(id), [reflected[0], reflected[1], 0.0]);
                if self.physicals[p].SimulateTemperature {
                    let t = self.physicals[p].Temperature;
                    self.physicals[p].Temperature = (d(t) + d(damage) * self.random.value() * 0.02f32).f();
                }
                if !flag {
                    let bp = self.physicals[p].BurnProgress;
                    self.physicals[p].BurnProgress = (d(bp) + d(0.002f32) * self.random.value()).f();
                }
            }
            Some(p) if self.physicals[p].AbsorbsLasers => {
                self.with_component_instance(id, |_, inst, o| inst.set_world_position(o, [point[0], point[1], 0.0]));
                // value.collider.attachedRigidbody.AddForceAtPosition(normal * -ImpactStrength, point) (ForceMode2D.Force)
                let rb = self.rigidbody_of_body(hit.body).unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
                let k = 0.0 - strength;
                rb.add_force_at_position(&mut self.world, [normal[0] * k, normal[1] * k], point);
                let bp = self.physicals[p].BurnProgress;
                self.physicals[p].BurnProgress = (d(bp) + d(0.2f32) * self.random.value()).f();
                self.blasterbolt_impact(b, &hit);
            }
            _ => self.blasterbolt_impact(b, &hit),
        }
    }

    /// PostLateUpdate's renderer update: an autodestructing bolt's TrailRenderer destroys its GameObject once the
    /// trail has faded, after the frame's Updates (gapB r23: not inside BlasterboltBehaviour.Update).
    pub fn blasterbolt_trails_update(&mut self) {
        for b in 0..self.blasterbolts.len() {
            let s = &self.blasterbolts[b];
            if s.trail_autodestruct && self.time.timeAsDouble - s.closed_at >= s.trail_time as f64 {
                let id = s.id;
                self.destroy_object(id);
            }
        }
    }

    /// BlasterboltBehaviour.Impact
    pub(crate) fn blasterbolt_impact(&mut self, b: usize, hit: &RayHit2D) {
        let (point, normal) = ([hit.point.x, hit.point.y], [hit.normal.x, hit.normal.y]);
        let at = [point[0], point[1], 0.0];
        let s = &self.blasterbolts[b];
        let (strength, damage, target, effect, decal, create_decal, spawn_fx, explode, decal_multiplier, explosion_damage, impact_decal) =
            (s.ImpactStrength, s.damage, s.TemperatureTarget, s.ImpactEffect, s.HitDecal, s.createHitDecal, s.SpawnExplosionEffect, s.CreateExplosion, s.DecalSizeMultiplier, s.ExplosionDamage, s.ImpactDecal);
        let under_water = self.is_point_under_water(point);
        if under_water {
            // A new ExplosionParameters: only its Position is set.
            self.create_underwater_explosion_effect(at, 0.0, false);
        } else {
            // Instantiate(ImpactEffect, hit.point, Euler(0, 0, Atan2(normal.y, normal.x))).transform.up = normal. The effect
            // has no physics and nothing reads its pose before this, so only FromToRotation(up, normal) remains.
            let effect_id = self.spawn_prefab(effect.expect("BlasterboltBehaviour: Instantiate(null ImpactEffect) (ArgumentException)"), at);
            self.tr_set_up(Xf::Object(effect_id), [normal[0], normal[1], 0.0]);
            if create_decal {
                // Instantiate(HitDecal, hit.point, identity, hit.transform): it follows hit.transform's GameObject
                // (`update_attached_clones`) and goes with it.
                let decal_id = self.spawn_prefab(decal.expect("BlasterboltBehaviour: Instantiate(null HitDecal) (ArgumentException)"), at);
                let parent = match self.hit_transform_physical(hit) {
                    Some(p) => super::destroyable::AttachParent::Physical(p),
                    None => super::destroyable::AttachParent::Bare(hit.collider),
                };
                self.attach_clone(decal_id, parent, at);
            }
        }
        self.create_pulse_explosion_with(at, strength, 1.0, !under_water && spawn_fx, false);
        if explode {
            let under_water = self.is_point_under_water(point);
            let ex = ExplosionParameters::new(8, at, 4.0, explosion_damage, false, super::explosion::EffectSize::Medium, 0.2, 0);
            self.create_explosion_with_water_splash(under_water, ex, false);
        }
        // PhysicalObjectsInWorldByTransform.TryGetValue(hit.transform): Temperature = Lerp(Temperature, target, ...).
        let physical = self.hit_transform_physical(hit);
        if let Some(p) = self.hit_world_physical(hit).filter(|&p| self.physicals[p].SimulateTemperature) {
            let t = self.physicals[p].Temperature;
            self.physicals[p].Temperature = mathf::lerp(t, target, 0.015 / self.heat_capacity(p));
        }
        // hit.transform.CompareTag("Limb") && TryGetComponent<SkinMaterialHandler>: the limbs.
        if let Some(l) = self.physical_of_collider(hit.collider).and_then(|p| self.limbs.iter().position(|l| l.PhysicalBehaviour == p)).filter(|&l| self.limb_tagged(l)) {
            self.skin_add_damage_point(l, super::misc::damage_type::BURN, point, target * 0.01);
        }
        self.send_shot(hit.collider, physical, normal, point, damage, false, None);
        if let Some(ty) = impact_decal {
            // calculateDecalSize(damage): Max(Pow(damage, 3.35) / Pow(10, 8), 0.15) * DecalSizeMultiplier
            let size = mathf::max(mathf::pow(damage, 3.35) / mathf::pow(10.0, 8.0), 0.15) * decal_multiplier;
            // SendMessage("Decal", DecalInstruction(ImpactDecal, point, size)) to hit.transform.
            match physical.or_else(|| self.physical_of_collider(hit.collider)) {
                Some(p) => self.physical_decal(p, ty, point, size, [1.0; 4]),
                None if self.bare_colliders[&hit.collider].receives(&["DecalControllerBehaviour", "DecalRenderer", "DecalRendererController"]) => {
                    unimplemented!("mod-only: SendMessage Decal to the scripts of a collider without a PhysicalBehaviour")
                }
                None => {}
            }
        }
        let s = &mut self.blasterbolts[b];
        s.closing = true;
        s.closed_at = self.time.timeAsDouble;
    }
}

script! {
    Railgun {
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
        ChargeDuration: f32 = 0.85,
        ScreenShake: f32 = 5.0,
        StartingEnergy: f32 = 1000.0,
        EnergyLossPerUnit: f32 = 0.1,
    }
    links {
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference on the same object).
        physical: usize = 0,
        /// LayerMask (bits)
        mask: u32 = 0,
        /// Lights.Length and ShootSounds.Length.
        Lights: usize = 0,
        /// Lights: the TrivialAnimationBehaviours' component ids.
        light_ids: Vec<i32> = Vec::new(),
        ShootSounds: usize = 0,
        ImpactPoolable: Option<ObjRef> = None,
        /// The private previousVictim and isFiring.
        previousVictim: Option<usize> = None,
        isFiring: bool = false,
        /// TrailRenderer: its index in `Game::lines`; TrailColor; the private trailGlowHeat.
        line: usize = 0,
        TrailColor: [f32; 4] = [0.04, 0.03, 0.16, 1.0],
        trailGlowHeat: f32 = 0.0,
    }
}

// RailgunBehaviour.Awake: waterLayer is WATER_LAYER below; the Lights' Duration = 0.3 at link (link.rs).
/// LayerMask.GetMask("Water")
const WATER_LAYER: u32 = 1 << 4;

/// Vector2.normalized: the magnitude is (float)Math.Sqrt(x * x + y * y), in double; below 1E-05 the zero vector.
fn v2_normalized(v: [f32; 2]) -> [f32; 2] {
    let m = (d(v[0]) * v[0] + d(v[1]) * v[1]).0.sqrt() as f32;
    if m > 1e-5 { [v[0] / m, v[1] / m] } else { [0.0, 0.0] }
}

impl Game {
    /// RailgunBehaviour.OnEnable: HumAudioSource.Play when set, then each light's Stop and ForceSetProgress(0).
    pub fn railgun_on_enable(&mut self, i: usize) {
        if let Some(h) = self.audio.source_field(self.railguns[i].id, "HumAudioSource") {
            self.audio.play(h);
        }
        self.railgun_reset_lights(i);
    }

    fn railgun_reset_lights(&mut self, i: usize) {
        for id in self.railguns[i].light_ids.clone() {
            let v = self.visual_ids[&id];
            self.trivial_animation_stop(v);
            self.trivial_force_set_progress(v, 0.0);
        }
    }

    /// RailgunBehaviour.BarrelPosition: transform.TransformPoint(barrelPosition).
    pub(crate) fn railgun_barrel_position(&self, i: usize) -> [f32; 2] {
        let b = self.railguns[i].barrelPosition;
        let p = self.xf_transform_point(&self.railguns[i].own, [b[0], b[1], 0.0]);
        [p[0], p[1]]
    }

    /// RailgunBehaviour.BarrelDirection: transform.TransformDirection(barrelDirection) * transform.localScale.x.
    pub(crate) fn railgun_barrel_direction(&self, i: usize) -> [f32; 2] {
        let own = &self.railguns[i].own;
        let b = self.railguns[i].barrelDirection;
        let v = self.xf_transform_direction(own, [b[0], b[1], 0.0]);
        let s = self.xf_local_scale(own)[0];
        [v[0] * s, v[1] * s]
    }

    /// RailgunBehaviour.Shoot / RailgunBehaviour.Use: while enabled and not firing, StartCoroutine(FireCoroutine).
    pub fn railgun_shoot(&mut self, i: usize) {
        if self.railguns[i].enabled && !self.railguns[i].isFiring {
            let id = self.railguns[i].id;
            self.start_coroutine(id, id, Self::railgun_routine(i));
        }
    }

    /// RailgunBehaviour.OnDisable: AudioSource.Stop, HumAudioSource.Stop, StopCoroutine("FireCoroutine"), isFiring = false and TrailRenderer.enabled = false.
    pub fn railgun_on_disable(&mut self, i: usize) {
        let id = self.railguns[i].id;
        let (a, h) = (self.audio_src(id, "AudioSource"), self.audio_src(id, "HumAudioSource"));
        self.audio.stop(a);
        self.audio.stop(h);
        self.stop_all_coroutines(id);
        self.railguns[i].isFiring = false;
        let line = self.railguns[i].line;
        self.lines[line].enabled = false;
    }

    /// RailgunBehaviour.Update: while the trail is enabled its colours and width fade with trailGlowHeat; at 0 it is disabled.
    pub fn railgun_update(&mut self, i: usize) {
        let (line, color, heat) = (self.railguns[i].line, self.railguns[i].TrailColor, self.railguns[i].trailGlowHeat);
        if !self.lines[line].enabled {
            return;
        }
        // Color * float: one rounding per component.
        let times = |k: f32| color.map(|c| c * k);
        self.lines[line].gradient.set_start_color(times((d(heat) * heat).f()));
        self.lines[line].gradient.set_end_color(times(heat));
        self.lines[line].width = mathf::lerp(1.0, 0.15, heat);
        let heat = (d(heat) - self.time.deltaTime).f();
        self.railguns[i].trailGlowHeat = heat;
        if heat <= 0.0 {
            self.lines[line].enabled = false;
        }
    }

    /// RailgunBehaviour.FireCoroutine: the sound's draw, the Lights' waits and Plays, the 0.3 s
    /// charge, then the shot (`railgun_fire`) and a second's wait before isFiring clears.
    pub(crate) fn railgun_routine(i: usize) -> Routine {
        #[derive(Clone, Copy)]
        enum Stage {
            Begin,
            Light(usize),
            Charge,
            Fire,
            End,
        }
        let mut stage = Stage::Begin;
        Box::new(move |game| match stage {
            Stage::Begin => {
                if game.railguns[i].isFiring {
                    return Yield::Done;
                }
                game.railguns[i].isFiring = true;
                // AudioSource.PlayOneShot(ShootSounds.PickRandom())
                let (clips, id) = (game.railguns[i].ShootSounds, game.railguns[i].id);
                let clip = game.pick_clip(id, "ShootSounds", clips);
                let source = game.audio.source_field(id, "AudioSource").unwrap_or_else(|| super::menu::throw("NullReferenceException: RailgunBehaviour.AudioSource"));
                game.audio.play_one_shot(source, clip.as_ref(), 1.0);
                let n = game.railguns[i].Lights;
                if n == 0 {
                    stage = Stage::Charge;
                    return Yield::WaitForSeconds(0.3);
                }
                stage = Stage::Light(0);
                let interval = ((d(game.railguns[i].ChargeDuration) - 0.3f32) / (n as f32)).f();
                Yield::WaitForSeconds(interval)
            }
            Stage::Light(k) => {
                let light = game.visual_ids[&game.railguns[i].light_ids[k]];
                game.trivial_animation_play(light);
                let n = game.railguns[i].Lights;
                if k + 1 < n {
                    stage = Stage::Light(k + 1);
                    let interval = ((d(game.railguns[i].ChargeDuration) - 0.3f32) / (n as f32)).f();
                    Yield::WaitForSeconds(interval)
                } else {
                    stage = Stage::Charge;
                    Yield::WaitForSeconds(0.3)
                }
            }
            Stage::Charge => {
                if !game.railguns[i].isFiring {
                    return Yield::Done;
                }
                game.railgun_reset_lights(i);
                game.railgun_fire(i);
                stage = Stage::End;
                Yield::WaitForSeconds(1.0)
            }
            Stage::Fire | Stage::End => {
                game.railguns[i].isFiring = false;
                Yield::Done
            }
        })
    }

    /// RailgunBehaviour.GenerateShot's normal: the barrel direction for a zero normal, else the normalised one. The
    /// Shot's damage is energy / StartingEnergy * 500.
    pub(crate) fn railgun_shot_normal(&self, i: usize, normal: [f32; 2]) -> [f32; 2] {
        if normal[0] * normal[0] + normal[1] * normal[1] <= 0.0 {
            self.railgun_barrel_direction(i)
        } else {
            v2_normalized(normal)
        }
    }

    pub(crate) fn railgun_shot_damage(&self, i: usize, energy: f32) -> f32 {
        (d(energy) / self.railguns[i].StartingEnergy * 500.0f32).f()
    }

    /// RailgunBehaviour.FireCoroutine after the charge: the muzzle flash, the recoil, the trace and the trail.
    fn railgun_fire(&mut self, i: usize) {
        let s = self.railguns[i].clone();
        let p = s.physical;
        let at = self.xf_position(&s.own);
        self.camera_shake(s.ScreenShake * 2.0, [at[0], at[1]]);
        self.ps_field_play_stop(s.id, "Muzzleflash", true);
        let (bd, bp) = (self.railgun_barrel_direction(i), self.railgun_barrel_position(i));
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_impulse_at_position(&mut self.world, [bd[0] * -30.0, bd[1] * -30.0], bp);
        let mut energy = s.StartingEnergy;
        let mut vector = self.railgun_barrel_position(i);
        let mut dir = v2_normalized(self.railgun_barrel_direction(i));
        let mut rhs = dir;
        let mut list = vec![[vector[0], vector[1], 0.0f32]];
        let num = energy / s.EnergyLossPerUnit;
        let mut last_water: Option<i32> = None;
        self.railguns[i].previousVictim = None;
        loop {
            if energy <= 0.0 {
                list.push([vector[0], vector[1], 0.0]);
                break;
            }
            while energy > 0.0 {
                let hit = self.overlap_point_depth(vector, s.mask, -0.1, 0.1);
                if let Some((physical, collider)) = hit {
                    let go = self.world.collider_game_object(collider).expect("a collider's fixture");
                    if super::physical::has_layer(WATER_LAYER, self.world.layers[&go]) {
                        // collider.transform.parent.TryGetComponent<WaterBehaviour>: the Drag, else 0.05.
                        let drag = match self.waters.iter().position(|w| w.trigger == collider) {
                            Some(w) => self.waters[w].Drag,
                            None => {
                                // collider.transform.parent (null throws) .TryGetComponent<WaterBehaviour>: 0.05 without one.
                                let found = self.with_component_instance(collider, |g, inst, o| {
                                    inst.objects[o].parent.map(|p| g.waters.iter().position(|w| inst.objects[p].components.contains(&w.id)))
                                });
                                match found {
                                    None => unimplemented!("mod-only: RailgunBehaviour: a Water-layer collider without a parent Transform"),
                                    Some(Some(w)) => self.waters[w].Drag,
                                    Some(None) => 0.05,
                                }
                            }
                        };
                        let r = self.random.inside_unit_circle();
                        dir = v2_normalized([dir[0] + r[0] * drag, dir[1] + r[1] * drag]);
                        energy = (d(energy) - d(s.EnergyLossPerUnit) * drag).f();
                        vector = [vector[0] + dir[0] * 0.1, vector[1] + dir[1] * 0.1];
                        list.push([vector[0], vector[1], 0.0]);
                        continue;
                    }
                    if let Some(v) = physical.filter(|&v| self.physicals[v].in_world && v != p) {
                        vector = [vector[0] + dir[0] * 0.1, vector[1] + dir[1] * 0.1];
                        energy = (d(energy) - d(s.EnergyLossPerUnit) * 0.1f32).f();
                        list.push([vector[0], vector[1], 0.0]);
                        self.railgun_handle_impact(i, v, [-dir[0], -dir[1]], vector, &mut dir, &mut energy);
                        continue;
                    }
                }
                list.push([vector[0], vector[1], 0.0]);
                if let Some(prev) = self.railguns[i].previousVictim {
                    let normal = self.railgun_shot_normal(i, [-dir[0], -dir[1]]);
                    let damage = self.railgun_shot_damage(i, energy);
                    let c = self.physicals[prev].all_colliders.first().copied().unwrap_or(0);
                    self.send_shot_flags(c, Some(prev), normal, vector, damage, true, None, false, true);
                }
                self.railguns[i].previousVictim = None;
                break;
            }
            let Some(h) = self.raycast(V::new(vector[0], vector[1]), dir, num, s.mask, f32::NEG_INFINITY, f32::INFINITY) else {
                // list.Add(vector + num * dir): Vector2 * float, then Vector2 + Vector2.
                list.push([(d((d(num) * dir[0]).f()) + vector[0]).f(), (d((d(num) * dir[1]).f()) + vector[1]).f(), 0.0]);
                break;
            };
            energy = (d(energy) - d(s.EnergyLossPerUnit) * h.distance).f();
            vector = [h.point.x + dir[0] * 0.01, h.point.y + dir[1] * 0.01];
            let normal = [h.normal.x, h.normal.y];
            let mut corner = None;
            if let Some(v) = self.hit_world_physical(&h) {
                if self.railguns[i].previousVictim != Some(v) {
                    corner = Some(vector);
                    let softness = self.properties[self.physicals[v].Properties].Softness;
                    // Vector2.Dot and Vector2.Reflect in Mono's double arithmetic, rounded at each float result.
                    let dot = (d(normal[0]) * dir[0] + d(normal[1]) * dir[1]).f();
                    if softness < 0.5 && dot.abs() < 0.5 {
                        let k = (d(-2.0) * dot).f();
                        dir = [(d(k) * normal[0] + dir[0]).f(), (d(k) * normal[1] + dir[1]).f()];
                    }
                }
                self.railgun_handle_impact(i, v, normal, vector, &mut dir, &mut energy);
            }
            if super::physical::has_layer(WATER_LAYER, self.world.layers[&h.game_object]) {
                if last_water != Some(h.game_object) {
                    corner = Some(vector);
                    let n = self.railgun_shot_normal(i, normal);
                    let damage = self.railgun_shot_damage(i, energy);
                    self.send_shot_flags(h.collider, None, n, vector, damage, false, None, false, true);
                }
                last_water = Some(h.game_object);
            }
            if ((d(dir[0]) * rhs[0] + d(dir[1]) * rhs[1]).f()) < 0.98 {
                corner = Some(vector);
            }
            if let Some(c) = corner {
                list.push([c[0], c[1], 0.0]);
            }
            rhs = dir;
        }
        let line = s.line;
        self.lines[line].width = 0.15;
        self.line_set_position_count(line, list.len());
        self.line_set_positions(line, &list);
        let color = s.TrailColor;
        self.lines[line].gradient.set_start_color(color.map(|c| c * 2.0));
        self.lines[line].gradient.set_end_color(color);
        self.lines[line].enabled = true;
        self.railguns[i].trailGlowHeat = 1.0;
    }

    /// RailgunBehaviour.HandleImpact
    fn railgun_handle_impact(&mut self, i: usize, phys: usize, normal: [f32; 2], pos: [f32; 2], dir: &mut [f32; 2], energy: &mut f32) {
        let s = self.railguns[i].clone();
        let props = &self.properties[self.physicals[phys].Properties];
        let (softness, absorption) = (props.Softness, props.BulletSpeedAbsorptionPower);
        let num = mathf::clamp01((d(1.0f32) - softness).f());
        let r = self.random.inside_unit_circle();
        let k = (d(0.05f32) * num).f();
        *dir = v2_normalized([dir[0] + r[0] * k, dir[1] + r[1] * k]);
        *energy = (d(*energy) - d(num) * 10.0f32).f();
        *energy = (d(*energy) - absorption).f();
        let num2 = (d(mathf::max(0.0, *energy)) / s.StartingEnergy).f();
        let force = (d(num2) * 50.0f32 * (d(num) + 0.1f32)).f();
        let rb = self.physicals[phys].rigidbody.clone();
        rb.add_force_at_position(&mut self.world, [dir[0] * force, dir[1] * force], pos);
        if s.previousVictim != Some(phys) {
            let poolable = s.ImpactPoolable.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            self.pool_request_prefab(poolable, pos);
            if self.physicals[phys].SimulateTemperature {
                let t = self.physicals[phys].Temperature;
                self.physicals[phys].Temperature = (d(t) + d(num2) * 200.0f32).f();
            }
            let n = self.railgun_shot_normal(i, normal);
            let damage = self.railgun_shot_damage(i, *energy);
            let c = self.physicals[phys].all_colliders.first().copied().unwrap_or(0);
            self.send_shot_flags(c, Some(phys), n, pos, damage, false, None, false, true);
            self.send_break(c, Some(phys), [dir[0] * num2 * 50.0, dir[1] * num2 * 50.0]);
        }
        self.railguns[i].previousVictim = Some(phys);
    }
}

script! {
    BeamRepeater {
        barrelPosition: [f32; 2] = [0.0; 2],
        barrelDirection: [f32; 2] = [0.0; 2],
        ShootRate: i32 = 25,
        AimRandomness: f32 = 0.1,
        ScreenShake: f32 = 5.0,
        Knockback: f32 = 0.5,
        LimbDamage: f32 = 0.5,
    }
    links {
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference on the same object).
        physical: usize = 0,
        /// LayerMask (bits)
        mask: u32 = 0,
        /// ShootSounds.Length (the clips are presentation).
        ShootSounds: usize = 0,
        ImpactPoolable: Option<ObjRef> = None,
        /// LaserRenderer: its index in `Game::lines` (None: null), and when FireCoroutine's WaitForSeconds(0.02f)
        /// turns it off (rendering only).
        line: Option<usize> = None,
        line_off_at: Option<f64> = None,
        // BeamRepeaterBehaviour.Awake: dist = new FixedIntervalDistributor { RateHz = ShootRate }
        /// The private dist (Awake: RateHz = ShootRate).
        dist: super::misc::FixedIntervalDistributor = super::misc::FixedIntervalDistributor::with_rate(25.0),
    }
}

script! {
    AndroidLaser {
        cooldownTime: i32 = 0,
        recoil: f32 = 0.0,
        hitKnockback: f32 = 0.0,
    }
    links {
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// trigger (a serialised reference on the same object).
        trigger: usize = 0,
        /// HitLayer (bits)
        HitLayer: u32 = 0,
        /// The private cooldown.
        cooldown: f32 = 0.0,
    }
}

impl Game {
    /// BeamRepeaterBehaviour.BarrelPosition: transform.TransformPoint(barrelPosition), as a Vector2.
    pub(crate) fn beam_repeater_barrel_position(&self, i: usize) -> [f32; 2] {
        let b = self.beam_repeaters[i].barrelPosition;
        let p = self.xf_transform_point(&self.beam_repeaters[i].own, [b[0], b[1], 0.0]);
        [p[0], p[1]]
    }

    /// BeamRepeaterBehaviour.BarrelDirection: transform.TransformDirection(barrelDirection) * transform.localScale.x.
    pub(crate) fn beam_repeater_barrel_direction(&self, i: usize) -> [f32; 2] {
        let own = &self.beam_repeaters[i].own;
        let b = self.beam_repeaters[i].barrelDirection;
        let v = self.xf_transform_direction(own, [b[0], b[1], 0.0]);
        let s = self.xf_local_scale(own)[0];
        [v[0] * s, v[1] * s]
    }

    /// BeamRepeaterBehaviour.Use: timer = 0 (never read), dist.ResetAccumulator(), Shoot().
    pub fn beam_repeater_use(&mut self, i: usize) {
        self.beam_repeaters[i].dist.reset_accumulator(0.0);
        self.beam_repeater_shoot(i);
    }

    /// BeamRepeaterBehaviour.UseContinuous: empty.
    pub fn beam_repeater_use_continuous(&mut self, _i: usize) {}

    /// BeamRepeaterBehaviour.Shoot
    pub fn beam_repeater_shoot(&mut self, i: usize) {
        if self.beam_repeater_enabled(i) {
            self.beam_repeater_fire(i);
        }
    }

    fn beam_repeater_enabled(&self, i: usize) -> bool {
        self.beam_repeaters[i].enabled && !self.disabled_ids.contains(&self.beam_repeaters[i].id)
    }

    /// BeamRepeaterBehaviour.FireCoroutine: it runs to the WaitForSeconds(0.02f) at once; after it the line goes off
    /// (beam_repeater_update, rendering only).
    pub(crate) fn beam_repeater_fire(&mut self, i: usize) {
        let (clips, shake, aim, mask) = {
            let s = &self.beam_repeaters[i];
            (s.ShootSounds, s.ScreenShake, s.AimRandomness, s.mask)
        };
        let id = self.beam_repeaters[i].id;
        self.ps_field_play_stop(id, "Muzzleflash", true);
        self.audio_one_shot_pick(id, "AudioSource", "ShootSounds", clips);
        let at = self.xf_position(&self.beam_repeaters[i].own);
        self.camera_shake(shake * 2.0, [at[0], at[1]]);
        let (pos, dir) = (self.beam_repeater_barrel_position(i), self.beam_repeater_barrel_direction(i));
        let r = self.random.inside_unit_circle();
        let normalized = mathf::normalized2([dir[0] + r[0] * aim, dir[1] + r[1] * aim]);
        // LaserRenderer.SetPosition(0, BarrelPosition), SetPosition(1, the hit or BarrelPosition + BarrelDirection * 1000f).
        let hit = self.raycast(V::new(pos[0], pos[1]), normalized, 100000.0, mask, f32::NEG_INFINITY, f32::INFINITY);
        if let Some(l) = self.beam_repeaters[i].line {
            let end = hit.as_ref().map_or([pos[0] + dir[0] * 1000.0, pos[1] + dir[1] * 1000.0], |h| [h.point.x, h.point.y]);
            self.line_set_position_count(l, self.lines[l].positions.len().max(2));
            self.line_set_position(l, 0, [pos[0], pos[1], 0.0]);
            self.line_set_position(l, 1, [end[0], end[1], 0.0]);
        }
        if let Some(hit) = hit {
            let (world, own) = (self.hit_world_physical(&hit), self.hit_transform_physical(&hit));
            self.beam_repeater_on_hit(i, world, own, [hit.point.x, hit.point.y]);
        }
        if let Some(l) = self.beam_repeaters[i].line {
            self.lines[l].enabled = true;
            self.beam_repeaters[i].line_off_at = Some(self.time.timeAsDouble + 0.02);
        }
    }

    /// BeamRepeaterBehaviour.OnHit(other, pos): `world` is other's Global.PhysicalObjectsInWorldByTransform entry, `own` the
    /// PhysicalBehaviour on other itself (the GameObject TryGetComponent<LimbBehaviour> looks on).
    pub(crate) fn beam_repeater_on_hit(&mut self, i: usize, world: Option<usize>, own: Option<usize>, pos: [f32; 2]) {
        let poolable = self.beam_repeaters[i].ImpactPoolable.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.pool_request_prefab(poolable, pos);
        if let Some(v) = world {
            if self.physicals[v].isStatic || self.physical_layer(v) == BOUNDS_LAYER {
                return;
            }
            if self.physicals[v].SimulateTemperature {
                let r = self.random.value();
                self.physicals[v].Temperature += 300.0 * r;
            }
            if self.random.value() > 0.9 {
                self.ignite_at(v, true, pos);
            }
            let (dir, k) = (self.beam_repeater_barrel_direction(i), self.beam_repeaters[i].Knockback);
            let rb = self.physicals[v].rigidbody.clone();
            rb.add_impulse_at_position(&mut self.world, [dir[0] * k, dir[1] * k], pos);
            self.physicals[v].burnIntensity += 0.1;
            if self.random.value() > 0.9 {
                let dir = self.beam_repeater_barrel_direction(i);
                self.send_break(0, Some(v), [dir[0] * 25.0, dir[1] * 25.0]);
            }
            if self.random.value() > 0.9 {
                self.send_slice(v);
            }
        }
        let Some(p) = own else { return };
        let Some(l) = self.limbs.iter().position(|l| l.PhysicalBehaviour == p) else { return };
        if !self.limbs[l].IsAndroid {
            if self.nodes[l].IsConnectedToRoot {
                let person = self.limbs[l].Person;
                self.add_pain(person, 0.4);
            }
            let r = self.random.range_int(30, 100);
            self.skin_add_damage_point(l, super::misc::damage_type::BURN, pos, r as f32);
        }
        let damage = self.beam_repeaters[i].LimbDamage;
        self.limb_damage(l, damage);
    }

    /// BeamRepeaterBehaviour.Update (first FireCoroutine's LaserRenderer.enabled = false after its wait, rendering only)
    pub fn beam_repeater_update(&mut self, i: usize) {
        if let (Some(l), Some(at)) = (self.beam_repeaters[i].line, self.beam_repeaters[i].line_off_at)
            && self.time.timeAsDouble >= at
        {
            self.lines[l].enabled = false;
            self.beam_repeaters[i].line_off_at = None;
        }
        if !self.beam_repeater_enabled(i) {
            return;
        }
        let p = self.beam_repeaters[i].physical;
        if self.used_continuously(p) {
            let dt = self.time.deltaTime;
            // The loop condition calls CalculateCycleCount(Time.deltaTime) on every pass.
            let mut k = 0;
            while k < self.beam_repeaters[i].dist.calculate_cycle_count(dt) {
                self.beam_repeater_fire(i);
                k += 1;
            }
        }
    }

    /// AndroidLaserBehaviour.Update
    pub fn android_laser_update(&mut self, i: usize) {
        if self.android_laser_off(i) {
            return;
        }
        if self.android_lasers[i].cooldown > 0.0 {
            self.android_lasers[i].cooldown -= self.time.deltaTime;
        }
    }

    /// AndroidLaserBehaviour.FixedUpdate
    pub fn android_laser_fixed_update(&mut self, i: usize) {
        if self.android_laser_off(i) {
            return;
        }
        if self.physicals[self.android_lasers[i].trigger].Charge() > 10.0 {
            self.android_laser_fire(i);
        }
    }

    /// AndroidLaserBehaviour.Use
    pub fn android_laser_use(&mut self, i: usize) {
        self.android_laser_fire(i);
    }

    fn android_laser_off(&self, i: usize) -> bool {
        !self.android_lasers[i].enabled || self.disabled_ids.contains(&self.android_lasers[i].id)
    }

    /// AndroidLaserBehaviour.Fire
    pub(crate) fn android_laser_fire(&mut self, i: usize) {
        if self.android_lasers[i].cooldown > 0.0 {
            return;
        }
        let (trigger, recoil, mask) = (self.android_lasers[i].trigger, self.android_lasers[i].recoil, self.android_lasers[i].HitLayer);
        self.android_lasers[i].cooldown = self.android_lasers[i].cooldownTime as f32;
        let id = self.android_lasers[i].id;
        self.audio_one_shot_own(id, "audioSource");
        self.ps_call(|e| e.owner == id && matches!(e.drive, super::EmbeddedDrive::AndroidLaser(_)), super::PsCall::Play);
        let own = &self.android_lasers[i].own;
        let (right, at) = (self.xf_right(own), self.xf_position(own));
        let k = 0.0 - recoil;
        let rb = self.physicals[trigger].rigidbody.clone();
        rb.add_impulse_at_position(&mut self.world, [right[0] * k, right[1] * k], [at[0], at[1]]);
        if let Some(hit) = self.raycast(V::new(at[0], at[1]), [right[0], right[1]], 1000.0, mask, f32::NEG_INFINITY, f32::INFINITY) {
            self.android_laser_handle_hit(i, &hit);
        }
    }

    /// AndroidLaserBehaviour.HandleHit
    fn android_laser_handle_hit(&mut self, i: usize, hit: &RayHit2D) {
        let knock = self.android_lasers[i].hitKnockback;
        self.create_pulse_explosion_with([hit.point.x, hit.point.y, 0.0], knock, 10.0, true, true);
        let Some(p) = self.hit_transform_physical(hit) else { return };
        let right = self.xf_right(&self.android_lasers[i].own);
        let (rx, ry) = (right[0] * knock, right[1] * knock);
        let rb = self.physicals[p].rigidbody.clone();
        let mass = rb.mass(&self.world);
        rb.add_impulse_at_position(&mut self.world, [rx * mass, ry * mass], [hit.point.x, hit.point.y]);
        let r = self.random.value();
        self.physicals[p].BurnProgress += r;
        if d(self.random.value()) > d(0.95) {
            self.ignite(p, false);
        }
    }
}
