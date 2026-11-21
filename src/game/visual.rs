//! Sprite colours, sprite enables and render-only Transforms: TrivialAnimationBehaviour and its Translation, Rotation
//! and Color subclasses, TrivialAnimationActivationBehaviour, ColorWave, SpriteGradientAnimation and
//! VertexColourBasedOnTemperatureBehaviour. Their targets are a physical's own SpriteRenderer or an entry of
//! `Game::renderers`; the renderer side redraws whatever changed.
#![allow(non_snake_case)]

use super::coroutine::Yield;
use super::{d, mathf, Game};
use crate::assets::mono::Value;
use crate::game_assets::{Curve, CurveCache};
use crate::unity::transform::Trs;

/// A SpriteRenderer the game draws: a physical's own, or one in `Game::renderers`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpriteAt {
    Physical(usize),
    Renderer(usize),
}

/// UnityEngine.Gradient: up to eight colour and alpha keys, times in 1/65535.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gradient {
    keys: [[f32; 4]; 8],
    ctime: [u16; 8],
    atime: [u16; 8],
    fixed: bool,
    colors: u8,
    alphas: u8,
}

impl Gradient {
    /// A serialized Gradient's raw fields (a LineRenderer's colorGradient).
    pub fn from_raw(keys: [[f32; 4]; 8], ctime: [u16; 8], atime: [u16; 8], mode: i32, colors: u8, alphas: u8) -> Self {
        Self { keys, ctime, atime, fixed: mode == 1, colors, alphas }
    }

    pub fn read(v: &Value) -> Result<Self, String> {
        let mut g = Self::default();
        let int = |n: &str| v.get(n).and_then(Value::i64).ok_or_else(|| format!("Gradient.{n}"));
        for i in 0..8 {
            let k = v.get(&format!("key{i}")).ok_or("Gradient key")?;
            for (j, c) in ["r", "g", "b", "a"].iter().enumerate() {
                g.keys[i][j] = k.get(c).and_then(Value::f32).ok_or("Gradient key colour")?;
            }
            g.ctime[i] = int(&format!("ctime{i}"))? as u16;
            g.atime[i] = int(&format!("atime{i}"))? as u16;
        }
        g.fixed = int("m_Mode")? == 1;
        (g.colors, g.alphas) = (int("m_NumColorKeys")? as u8, int("m_NumAlphaKeys")? as u8);
        Ok(g)
    }

    /// ColorRGBA32(Color) (180230a10): each channel clamped, times 255 plus a half, truncated; read back as byte / 255.
    fn bytes(c: [f32; 4]) -> [f32; 4] {
        c.map(|x| ((x.max(0.0).min(1.0) * 255.0 + 0.5) as u8) as f32 / 255.0)
    }

    /// LineRenderer.startColor = c (LineRenderer_CUSTOM_set_startColor_Injected 1809302e0): colour key 0's rgb and alpha key 0's alpha.
    pub fn set_start_color(&mut self, c: [f32; 4]) {
        self.keys[0] = Self::bytes(c);
    }

    /// LineRenderer.endColor = c (LineParameters::SetEndColor 1809b6bd0): at least two colour and alpha keys (the second at 1.0 when
    /// added), then the last colour key's rgb and the last alpha key's alpha.
    pub fn set_end_color(&mut self, c: [f32; 4]) {
        if self.colors < 2 {
            (self.colors, self.ctime[1]) = (2, 0xffff);
        }
        if self.alphas < 2 {
            (self.alphas, self.atime[1]) = (2, 0xffff);
        }
        let b = Self::bytes(c);
        self.keys[self.colors as usize - 1][..3].copy_from_slice(&b[..3]);
        let k = if self.colors == self.alphas { self.colors } else { self.alphas } as usize - 1;
        self.keys[k][3] = b[3];
    }

    /// Gradient.Evaluate (Gradient::Evaluate 180864890): t clamped to [0, 1]. Blend (EvaluateHDR<0> 1801d0580) works in key
    /// time units: tt = t * 65535 clamped to the first and last key, the first key i >= 1 with time[i] >= tt, then
    /// k[i-1] + (k[i] - k[i-1]) * min((tt - time[i-1]) / max(time[i] - time[i-1], 1e-6), 1); one key or none is 1.
    /// Fixed takes the first key at or after it.
    pub fn evaluate(&self, t: f32) -> [f32; 4] {
        let t = if 0.0 > t { 0.0 } else { t.min(1.0) };
        let at = |n: u8, time: &[u16; 8], k: usize| -> f32 {
            if !self.fixed {
                let n = n as usize;
                // One key is that key (ResizeHandles.HandleDisplay's one-key line gradients, fnba15); none is 1.
                if n <= 1 {
                    return if n == 1 { self.keys[0][k] } else { 1.0 };
                }
                let tf = |i: usize| time[i] as f32;
                let tt = (t * 65535.0).max(tf(0)).min(tf(n - 1));
                let mut i = 1;
                while i < n - 1 && tf(i) < tt {
                    i += 1;
                }
                let s = ((tt - tf(i - 1)) / (tf(i) - tf(i - 1)).max(1e-6)).min(1.0);
                return self.keys[i - 1][k] + (self.keys[i][k] - self.keys[i - 1][k]) * s;
            }
            let n = n.max(1) as usize;
            let tk = |i: usize| time[i] as f32 / 65535.0;
            if t <= tk(0) {
                return self.keys[0][k];
            }
            for i in 1..n {
                if t <= tk(i) {
                    return self.keys[i][k];
                }
            }
            self.keys[n - 1][k]
        };
        [at(self.colors, &self.ctime, 0), at(self.colors, &self.ctime, 1), at(self.colors, &self.ctime, 2), at(self.alphas, &self.atime, 3)]
    }
}

/// Color * Color, Color * float and Color.Lerp, one rounding per component.
pub fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| a[i] * b[i])
}
pub fn scale(a: [f32; 4], s: f32) -> [f32; 4] {
    a.map(|x| x * s)
}
pub fn color_lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = mathf::clamp01(t);
    std::array::from_fn(|i| (d(a[i]) + (d(b[i]) - a[i]) * t).f())
}

/// What a TrivialAnimationBehaviour subclass animates in AnimationTick.
#[derive(Clone, Debug)]
pub enum Anim {
    /// TranslationAnimationBehaviour: localPosition = startPos + DistanceOverTime(p) * Multiplier on Axis.
    Translation { curve: Curve, cache: CurveCache, vertical: bool, Multiplier: f32, startPos: [f32; 3] },
    /// RotationAnimationBehaviour: localEulerAngles = Offset + Multiplier * DegreeOverTime(p) (z only in the game).
    Rotation { curve: Curve, cache: CurveCache, Offset: f32, Multiplier: f32 },
    /// ColorAnimationBehaviour: spriteRenderer.color = ColourOverTime(p).
    Color { gradient: Gradient, sprite: Option<SpriteAt> },
}

/// One visual script.
#[derive(Clone, Debug)]
pub enum Kind {
    /// TrivialAnimationBehaviour. `local` is its object's local Transform, written into `refs` (renderer, chain index).
    Trivial { Duration: f32, Loop: bool, PlayOnStart: bool, IsPlaying: bool, anim: Anim, local: Trs, refs: Vec<(usize, usize)> },
    /// TrivialAnimationActivationBehaviour: its physical and the animation's index.
    Activation { phys: usize, animation: usize, heat: i32, time: f32 },
    /// ColorWave; `own` is its own object's SpriteRenderer (OnWillRenderObject's culling).
    ColorWave { renderer: Option<SpriteAt>, own: SpriteAt, gradient: Gradient, Duration: f32, Offset: f32, UseSinWave: bool, OffsetVaration: f32, SpeedVaration: f32, Randomisation: f32, oV: f32, sV: f32 },
    /// SpriteGradientAnimation (ObjectGradientAnimation); `at` is where its Transform hangs.
    SpriteGradient { gradient: Gradient, Tint: [f32; 4], Speed: f32, Offset: f32, CycleType: i32, ReplayOnEnable: bool, PositionOffsetInfluence: f32, t: f32, sprite: Option<SpriteAt>, at: (super::BareAnchor, Vec<Trs>) },
    /// HologramJitter: TextMesh and Sprite, Colors, JitterIntensity and Awake's offset; `own` is its object's renderer
    /// (OnWillRenderObject's culling), `local` its object's local Transform, written into `refs` (renderer, chain index) and
    /// `text_refs` (world text, chain index). Multiply (1.28.41) scales the gradient's colour; DisplayBehaviour sets it.
    Hologram { text: Option<usize>, sprite: Option<SpriteAt>, own: HologramOwn, Colors: Gradient, Multiply: [f32; 4], JitterIntensity: f32, offset: f32, local: Trs, refs: Vec<(usize, usize)>, text_refs: Vec<(usize, usize)> },
    /// SortingLayerChild: Awake's spriteRenderer and lineRenderer (its own object's) and parentRenderer (the parent's
    /// SpriteRenderer, OnTransformParentChanged); Behaviour and Offset.
    /// `transform`: its object's Transform id.
    SortingChild { transform: i32, sprite: Option<SpriteAt>, line: Option<usize>, parent: Option<SpriteAt>, Behaviour: i32, Offset: i32 },
    /// VertexColourBasedOnTemperatureBehaviour
    VertexColour { gradient: Gradient, phys: usize, sprite: SpriteAt, TemperatureRange: [f32; 2] },
}

/// The renderer on a HologramJitter's own object.
#[derive(Clone, Copy, Debug)]
pub enum HologramOwn {
    Sprite(SpriteAt),
    Text(usize),
}

#[derive(Clone, Debug)]
pub struct Visual {
    /// The component's and its GameObject's instance ids.
    pub id: i32,
    pub go: i32,
    /// Behaviour.enabled
    pub enabled: bool,
    pub kind: Kind,
}

impl Game {
    pub fn sprite_color(&self, s: SpriteAt) -> [f32; 4] {
        match s {
            SpriteAt::Physical(p) => self.physicals[p].sprite.expect("a physical sprite").1,
            SpriteAt::Renderer(r) => self.renderers[r].sprite.1,
        }
    }

    pub fn set_sprite_color(&mut self, s: SpriteAt, c: [f32; 4]) {
        match s {
            SpriteAt::Physical(p) => self.physicals[p].sprite.as_mut().expect("a physical sprite").1 = c,
            SpriteAt::Renderer(r) => self.renderers[r].sprite.1 = c,
        }
    }

    pub fn sprite_enabled(&self, s: SpriteAt) -> bool {
        match s {
            SpriteAt::Physical(p) => self.physicals[p].sprite_enabled,
            SpriteAt::Renderer(r) => self.renderers[r].enabled,
        }
    }

    /// `SpriteRenderer.enabled = on` through a translated script's presentation link (`present_ids`).
    pub fn present_sprite_enabled(&mut self, this: i32, field: &'static str, on: bool) {
        if let Some(at) = self.present_ids.get(&(this, field)).and_then(|&id| self.item_sprite(id)) {
            self.set_sprite_enabled(at, on);
        }
    }

    /// The AudioSource behind a translated script's presentation link (`present_ids`), if the rewrite has it.
    pub fn present_audio(&self, this: i32, field: &'static str) -> Option<i32> {
        self.present_ids.get(&(this, field)).copied().filter(|id| self.audio.sources.contains_key(id))
    }

    /// AudioSource.Play() (`on`) or Stop() through a translated script's presentation link (`present_ids`).
    pub fn present_audio_play(&mut self, this: i32, field: &'static str, on: bool) {
        if let Some(id) = self.present_audio(this, field) {
            if on { self.audio.play(id) } else { self.audio.stop(id) }
        }
    }

    pub fn set_sprite_enabled(&mut self, s: SpriteAt, on: bool) {
        match s {
            SpriteAt::Physical(p) => {
                self.physicals[p].sprite_enabled = on;
                let (id, active) = (self.physicals[p].sprite_id, self.physicals[p].active);
                if id != 0 {
                    self.renderer_scene(id, on && active);
                }
            }
            SpriteAt::Renderer(r) => {
                self.renderers[r].enabled = on;
                self.renderer_sync(r);
            }
        }
    }

    /// Whether the camera draws the sprite this frame (Renderer.isVisible, and when OnWillRenderObject runs): enabled,
    /// active, and its bounds inside the view (headless: always).
    pub fn sprite_drawn(&self, s: SpriteAt) -> bool {
        let (on, (c, e)) = match s {
            SpriteAt::Physical(p) => {
                let x = &self.physicals[p];
                if !(x.sprite_enabled && x.active && !x.destroyed && !x.isDisintegrated) {
                    return false;
                }
                (true, self.sprite_world_bounds(p))
            }
            SpriteAt::Renderer(r) => {
                let x = &self.renderers[r];
                let up = match x.anchor {
                    super::BareAnchor::Physical(p) => self.physicals[p].active && !self.physicals[p].destroyed,
                    _ => true,
                };
                if !(x.enabled && x.visible && up) {
                    return false;
                }
                (true, crate::unity::transform::transform_aabb(x.bounds.0, x.bounds.1, &self.anchored_nodes(x.anchor, &x.chain)))
            }
        };
        on && self.input.view.is_none_or(|(min, max)| (0..2).all(|i| c[i] + e[i] >= min[i] && c[i] - e[i] <= max[i]))
    }

    /// Whether a will-render loop runs component `id`: Unity's dispatch (`dispatched`: enabled, alive, drawn), or under
    /// fn-check (gapA r5) only the target: the recorder calls OnWillRenderObject on it directly, whatever the camera drew.
    pub fn will_render_dispatch(&self, id: i32, dispatched: bool) -> bool {
        self.fn_render_only.map_or(dispatched, |t| t == id)
    }

    // ---- TrivialAnimationBehaviour ----

    /// TrivialAnimationBehaviour.Start: Play when PlayOnStart. ObjectGradientAnimation.Start: ApplyColour.
    pub fn visual_start(&mut self, v: usize) {
        match &self.visuals[v].kind {
            Kind::Trivial { PlayOnStart: true, .. } => self.trivial_animation_play(v),
            Kind::SpriteGradient { .. } => self.object_gradient_apply_colour(v),
            _ => {}
        }
    }

    /// TrivialAnimationBehaviour.Play: StopAllCoroutines, then StartCoroutine(Routine).
    pub fn trivial_animation_play(&mut self, v: usize) {
        let (id, go) = (self.visuals[v].id, self.visuals[v].go);
        self.stop_all_coroutines(id);
        let mut t = 0.0f32;
        let mut started = false;
        self.start_coroutine(
            id,
            go,
            Box::new(move |g: &mut Game| {
                // Routine: t = 0, IsPlaying = true, AnimationTick(0); then each frame AnimationTick(t / Duration), wait for the
                // frame's end, t += deltaTime; past Duration it loops (t = 0) or stops with AnimationTick(1).
                if !started {
                    started = true;
                    return g.trivial_animation_routine_first(v);
                } else {
                    t += g.time.deltaTime;
                    if t >= g.trivial_duration(v) {
                        if !g.trivial_is_playing(v) || !g.trivial_loops(v) {
                            return g.trivial_animation_finish(v);
                        }
                        t = 0.0;
                    }
                }
                if !g.trivial_is_playing(v) {
                    return g.trivial_animation_finish(v);
                }
                let p = t / g.trivial_duration(v);
                g.trivial_animation_tick(v, p);
                Yield::WaitForEndOfFrame
            }),
        );
    }

    /// Routine's first MoveNext: t = 0, IsPlaying = true, AnimationTick(0), then the loop's first AnimationTick(t / Duration)
    /// and `yield return new WaitForEndOfFrame()` (IsPlaying was just set).
    pub(crate) fn trivial_animation_routine_first(&mut self, v: usize) -> Yield {
        self.trivial_set_playing(v, true);
        self.trivial_animation_tick(v, 0.0);
        let p = 0.0 / self.trivial_duration(v);
        self.trivial_animation_tick(v, p);
        Yield::WaitForEndOfFrame
    }

    /// The end of Routine: if still IsPlaying, IsPlaying = false and AnimationTick(1).
    fn trivial_animation_finish(&mut self, v: usize) -> Yield {
        if self.trivial_is_playing(v) {
            self.trivial_set_playing(v, false);
            self.trivial_animation_tick(v, 1.0);
        }
        Yield::Done
    }

    /// TrivialAnimationBehaviour.get_IsPlaying
    pub fn trivial_is_playing(&self, v: usize) -> bool {
        matches!(self.visuals[v].kind, Kind::Trivial { IsPlaying: true, .. })
    }

    /// TrivialAnimationBehaviour.set_IsPlaying
    pub(crate) fn trivial_set_playing(&mut self, v: usize, on: bool) {
        if let Kind::Trivial { IsPlaying, .. } = &mut self.visuals[v].kind {
            *IsPlaying = on;
        }
    }

    fn trivial_duration(&self, v: usize) -> f32 {
        match self.visuals[v].kind {
            Kind::Trivial { Duration, .. } => Duration,
            _ => unreachable!(),
        }
    }

    pub fn trivial_set_duration(&mut self, v: usize, value: f32) {
        if let Kind::Trivial { Duration, .. } = &mut self.visuals[v].kind {
            *Duration = value;
        }
    }

    fn trivial_loops(&self, v: usize) -> bool {
        matches!(self.visuals[v].kind, Kind::Trivial { Loop: true, .. })
    }

    /// TrivialAnimationBehaviour.Stop
    pub fn trivial_animation_stop(&mut self, v: usize) {
        self.trivial_set_playing(v, false);
    }

    /// TrivialAnimationBehaviour.ForceSetProgress
    pub fn trivial_force_set_progress(&mut self, v: usize, progress: f32) {
        self.trivial_animation_tick(v, progress);
    }

    /// TranslationAnimationBehaviour.AnimationTick, RotationAnimationBehaviour.AnimationTick, ColorAnimationBehaviour.AnimationTick.
    pub(crate) fn trivial_animation_tick(&mut self, v: usize, progress: f32) {
        let Kind::Trivial { anim, local, refs, .. } = &mut self.visuals[v].kind else { unreachable!() };
        match anim {
            Anim::Translation { curve, cache, vertical, Multiplier, startPos } => {
                let num = curve.evaluate(cache, progress) * *Multiplier;
                let [x, y, z] = *startPos;
                local.t = if *vertical { [x, y + num, z, 0.0] } else { [x + num, y, z, 0.0] };
            }
            Anim::Rotation { curve, cache, Offset, Multiplier } => {
                let num = curve.evaluate(cache, progress);
                local.q = super::menu::local_euler_z_rotation(*Offset + *Multiplier * num);
            }
            Anim::Color { gradient, sprite } => {
                // `(bool)spriteRenderer && ColourOverTime != null`
                if let Some(s) = *sprite {
                    let c = gradient.evaluate(progress);
                    self.set_sprite_color(s, c);
                }
                return;
            }
        }
        let (local, refs) = (*local, refs.clone());
        for (r, k) in refs {
            self.renderers[r].chain[k] = local;
        }
        // gapB r13: base.transform's own localPosition/localRotation too (ours wrote only the renderers' copy).
        let id = self.visuals[v].id;
        if !self.unsynced.iter().chain(self.retained.iter()).any(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
            return;
        }
        self.with_component_instance(id, |_, inst, o| {
            if inst.objects[o].trs.t[..3] != local.t[..3] {
                inst.set_local_position(o, [local.t[0], local.t[1], local.t[2]]);
            }
            if inst.objects[o].trs.q != local.q {
                inst.store_local_rotation(o, local.q);
            }
        });
    }

    /// TrivialAnimationBehaviour.Play by the component's instance id (a UnityEvent's persistent call).
    pub fn trivial_animation_play_id(&mut self, id: i32) {
        let v = self.visual_ids[&id];
        self.trivial_animation_play(v);
    }

    // ---- per-frame scripts ----

    /// The Update of TrivialAnimationActivationBehaviour, SpriteGradientAnimation and
    /// VertexColourBasedOnTemperatureBehaviour.
    pub fn visual_update(&mut self, v: usize) {
        let dt = self.time.deltaTime;
        match self.visuals[v].kind {
            Kind::Activation { phys, animation, .. } => {
                // TrivialAnimationActivationBehaviour.Update
                let used = self.used_continuously(phys);
                let duration = self.trivial_duration(animation);
                let Kind::Activation { heat, time, .. } = &mut self.visuals[v].kind else { unreachable!() };
                if used {
                    *heat = 2;
                }
                *heat -= 1;
                *time = if *heat > 0 { (d(*time) + d(dt) / duration).f() } else { (d(*time) - d(dt) / duration).f() };
                *time = mathf::clamp01(*time);
                let time = *time;
                self.trivial_force_set_progress(animation, time);
            }
            Kind::SpriteGradient { .. } => {
                // ObjectGradientAnimation.Update
                let Kind::SpriteGradient { t, Speed, .. } = &mut self.visuals[v].kind else { unreachable!() };
                *t = (d(*t) + d(dt) * *Speed).f();
                self.object_gradient_apply_colour(v);
            }
            Kind::VertexColour { phys, sprite, TemperatureRange, .. } => {
                // VertexColourBasedOnTemperatureBehaviour.Update
                if self.sprite_drawn(sprite) {
                    let time = mathf::map_range(TemperatureRange[0], TemperatureRange[1], 0.0, 1.0, self.physicals[phys].Temperature);
                    let Kind::VertexColour { gradient, .. } = &self.visuals[v].kind else { unreachable!() };
                    let c = gradient.evaluate(time);
                    self.set_sprite_color(sprite, c);
                }
            }
            _ => {}
        }
    }

    /// ColorWave.OnEnable (two Random.value draws) and ObjectGradientAnimation.OnEnable.
    pub fn visual_on_enable(&mut self, v: usize) {
        match self.visuals[v].kind {
            Kind::ColorWave { .. } => {
                let (a, b) = (self.random.value(), self.random.value());
                let Kind::ColorWave { oV, sV, .. } = &mut self.visuals[v].kind else { unreachable!() };
                (*oV, *sV) = (a, b);
            }
            Kind::SpriteGradient { ReplayOnEnable: true, .. } => {
                let Kind::SpriteGradient { t, .. } = &mut self.visuals[v].kind else { unreachable!() };
                *t = 0.0;
                self.object_gradient_apply_colour(v);
            }
            _ => {}
        }
    }

    /// ObjectGradientAnimation's transform.position.
    pub(crate) fn object_gradient_position(&self, v: usize) -> [f32; 3] {
        let Kind::SpriteGradient { at, .. } = &self.visuals[v].kind else { unreachable!() };
        self.anchored_world(at.0, &at.1).0
    }

    /// The Transform chain of a SpriteGradientAnimation, object first.
    pub(crate) fn object_gradient_nodes(&self, v: usize) -> Vec<Trs> {
        let Kind::SpriteGradient { at, .. } = &self.visuals[v].kind else { unreachable!() };
        self.anchored_nodes(at.0, &at.1)
    }

    /// ObjectGradientAnimation.ApplyColour: the colour at t, then SetColor.
    pub(crate) fn object_gradient_apply_colour(&mut self, v: usize) {
        let [x, y, _] = self.object_gradient_position(v);
        let Kind::SpriteGradient { gradient, Tint, Offset, CycleType, PositionOffsetInfluence, t, .. } = &self.visuals[v].kind else { unreachable!() };
        let num = (d((d(*t) + *Offset).f()) + mathf::lerp(0.0, x, *PositionOffsetInfluence)).f();
        let mut num = (d(num) + mathf::lerp(0.0, y, *PositionOffsetInfluence)).f();
        match CycleType {
            0 => num = mathf::clamp01(num),
            1 => num = ((num as f64).sin() as f32 * 0.5 + 0.5) as f32,
            2 => num %= 1.0,
            _ => {}
        }
        let c = mul(gradient.evaluate(num), *Tint);
        self.sprite_gradient_set_color(v, c);
    }

    /// SpriteGradientAnimation.SetColor: `if ((bool)SpriteRenderer) SpriteRenderer.color = color`.
    pub(crate) fn sprite_gradient_set_color(&mut self, v: usize, color: [f32; 4]) {
        let Kind::SpriteGradient { sprite, .. } = &self.visuals[v].kind else { unreachable!() };
        if let Some(s) = *sprite {
            self.set_sprite_color(s, color);
        }
    }

    /// OnWillRenderObject of ColorWave for each drawn one (`view` as `Game::input.view`).
    pub fn visual_will_render(&mut self) {
        for v in 0..self.visuals.len() {
            if matches!(self.visuals[v].kind, Kind::Hologram { .. }) {
                self.hologram_will_render(v);
                continue;
            }
            if let Kind::SortingChild { sprite, line, .. } = self.visuals[v].kind {
                // Unity calls OnWillRenderObject only for a renderer that draws.
                if sprite.is_some_and(|s| self.sprite_drawn(s)) || line.is_some_and(|i| self.line_drawn(i)) {
                    self.sorting_layer_child_will_render(v);
                }
                continue;
            }
            let Kind::ColorWave { own, .. } = self.visuals[v].kind else { continue };
            if !self.will_render_dispatch(self.visuals[v].id, self.visuals[v].enabled && !self.destroyed_components.contains(&self.visuals[v].id) && self.sprite_drawn(own)) {
                continue;
            }
            self.color_wave_will_render(v);
        }
    }

    /// ColorWave.OnWillRenderObject of visual `v`.
    pub(crate) fn color_wave_will_render(&mut self, v: usize) {
        let Kind::ColorWave { renderer, ref gradient, Duration, Offset, UseSinWave, OffsetVaration, SpeedVaration, Randomisation, oV, sV, .. } = self.visuals[v].kind else { unreachable!("a ColorWave visual") };
        // `(bool)Renderer && Gradient != null && Duration > 0`
        let Some(r) = renderer else { return };
        if !(Duration > 0.0) {
            return;
        }
        let time = self.time.time;
        let num = (d((d(time) * (d(1.0) + d(sV) * SpeedVaration) / Duration).f()) + Offset + d(oV) * OffsetVaration).f();
        let mut num2 = if UseSinWave { ((num as f64).sin() as f32 * 0.5 + 0.5) as f32 } else { super::physical::utils_mod(num, 1.0) };
        if Randomisation != 0.0 {
            num2 += mathf::perlin2_mapped(time / Duration, oV * 423.0)[0];
        }
        let c = gradient.evaluate(num2);
        self.set_sprite_color(r, c);
    }
}

impl Game {
    /// A TextMeshPro's MeshRenderer is drawn: the text and renderer enabled, active, and its mesh's bounds in the view.
    fn text_drawn(&self, i: usize) -> bool {
        let t = &self.world_texts[i];
        if !(t.visible && t.enabled && t.renderer_enabled) {
            return false;
        }
        let up = match t.anchor {
            super::BareAnchor::Physical(p) => self.physicals[p].active && !self.physicals[p].destroyed,
            _ => true,
        };
        if !up {
            return false;
        }
        let Some((min, max)) = self.input.view else { return true };
        // Mesh.RecalculateBounds over the vertices (an empty mesh: a point at the origin).
        let (mut lo, mut hi) = ([0.0f32; 3], [0.0f32; 3]);
        for (n, p) in t.mesh.quads.iter().flat_map(|q| q.pos.iter()).enumerate() {
            for k in 0..3 {
                lo[k] = if n == 0 { p[k] } else { lo[k].min(p[k]) };
                hi[k] = if n == 0 { p[k] } else { hi[k].max(p[k]) };
            }
        }
        let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
        let extent = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5];
        let (c, e) = crate::unity::transform::transform_aabb(center, extent, &self.anchored_nodes(t.anchor, &t.chain));
        (0..2).all(|k| c[k] + e[k] >= min[k] && c[k] - e[k] <= max[k])
    }

    /// A LineRenderer of `Game::lines` is drawn: enabled, active, at least two points, its physical alive.
    fn line_drawn(&self, i: usize) -> bool {
        let l = &self.lines[i];
        let alive = match l.anchor {
            super::BareAnchor::Physical(p) => self.physicals[p].active && !self.physicals[p].isDisintegrated && !self.physicals[p].destroyed,
            _ => true,
        };
        l.visible && l.enabled && alive && l.positions.len() >= 2
    }

    /// A SpriteRenderer's sortingLayerID and sortingOrder.
    fn sprite_sorting(&self, s: SpriteAt) -> (i32, i16) {
        match s {
            SpriteAt::Physical(p) => (self.physicals[p].sprite_layer, self.physicals[p].sprite.expect("a physical sprite").2),
            SpriteAt::Renderer(r) => (self.renderers[r].layer, self.renderers[r].sprite.2),
        }
    }

    /// SortingLayerChild.OnWillRenderObject, when a renderer of its object is drawn: the sprite's and the line's sorting
    /// layer from the parent's SpriteRenderer, the order the parent's, plus Offset (Above) or minus it (Behind).
    pub(crate) fn sorting_layer_child_will_render(&mut self, v: usize) {
        let Kind::SortingChild { sprite, line, parent, Behaviour, Offset, .. } = self.visuals[v].kind else { unreachable!() };
        // `if (!parentRenderer) return;`
        let Some(parent) = parent else { return };
        if !self.renderer_alive(parent) {
            return;
        }
        let (layer, order) = self.sprite_sorting(parent);
        // Renderer.sortingOrder keeps a 16-bit value.
        let order = match Behaviour {
            2 => (order as i32 + Offset).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            1 => order,
            0 => (order as i32 - Offset).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            _ => return,
        };
        match sprite {
            Some(SpriteAt::Physical(p)) => {
                self.physicals[p].sprite_layer = layer;
                if let Some(s) = &mut self.physicals[p].sprite {
                    s.2 = order;
                }
            }
            Some(SpriteAt::Renderer(r)) => {
                self.renderers[r].layer = layer;
                self.renderers[r].sprite.2 = order;
            }
            None => {}
        }
        if let Some(i) = line {
            self.lines[i].layer = layer;
            self.lines[i].order = order;
        }
    }

    /// SortingLayerChild.OnTransformParentChanged on object `o` of clone `inst` (its Transform's current parent):
    /// `parentRenderer = transform.parent ? transform.parent.GetComponent<SpriteRenderer>() : null`. A clone root
    /// attached under another clone's object has that object as its parent.
    pub(crate) fn sorting_layer_child_parent_changed(&mut self, inst: &super::spawn::Instance, o: usize) {
        let t = inst.objects[o].components[0];
        let Some(v) = self.visuals.iter().position(|x| matches!(x.kind, Kind::SortingChild { transform, .. } if transform == t)) else { return };
        let found = match inst.objects[o].parent {
            Some(q) => self.sprite_among(&inst.objects[q].components),
            None if o == 0 => match self.attached_parent_of(t) {
                Some((_, super::destroyable::AttachParent::Physical(p))) => self.sprite_among(&[self.physicals[p].sprite_id]),
                // The parent object's components (a body-less collider's or object's, in another clone).
                Some((_, super::destroyable::AttachParent::Bare(id) | super::destroyable::AttachParent::Object(id))) => {
                    let comps = self.unsynced.iter().chain(&self.retained).flat_map(|i| &i.objects).find(|x| x.components.contains(&id)).map(|x| x.components.clone());
                    comps.and_then(|c| self.sprite_among(&c))
                }
                None => None,
            },
            None => None,
        };
        if let Kind::SortingChild { parent, .. } = &mut self.visuals[v].kind {
            *parent = found;
        }
    }

    /// GetComponent<SpriteRenderer>() of the object holding components `comps`: a physical's or a bare renderer.
    fn sprite_among(&self, comps: &[i32]) -> Option<SpriteAt> {
        if let Some(p) = self.physicals.iter().position(|p| p.sprite_id != 0 && comps.contains(&p.sprite_id)) {
            return Some(SpriteAt::Physical(p));
        }
        self.renderers.iter().position(|r| comps.contains(&r.id)).map(SpriteAt::Renderer)
    }

    /// `(bool)renderer`: its object not destroyed.
    fn renderer_alive(&self, s: SpriteAt) -> bool {
        match s {
            SpriteAt::Physical(p) => !self.physicals[p].destroyed,
            SpriteAt::Renderer(r) => match self.renderers[r].anchor {
                super::BareAnchor::Physical(p) => !self.physicals[p].destroyed,
                _ => true,
            },
        }
    }

    /// `Jitter.Multiply = c` on HologramJitter visual `v`.
    pub fn hologram_set_multiply(&mut self, v: usize, c: [f32; 4]) {
        let Kind::Hologram { Multiply, .. } = &mut self.visuals[v].kind else { unreachable!("a HologramJitter's visual") };
        *Multiply = c;
    }

    /// HologramJitter.OnWillRenderObject, when its object's renderer is drawn.
    pub(crate) fn hologram_will_render(&mut self, v: usize) {
        let Kind::Hologram { text, sprite, own, ref Colors, Multiply, JitterIntensity, offset, mut local, ref refs, ref text_refs } = self.visuals[v].kind else { unreachable!() };
        let drawn = match own {
            HologramOwn::Sprite(s) => self.sprite_drawn(s),
            HologramOwn::Text(t) => self.text_drawn(t),
        };
        if !drawn || self.destroyed_components.contains(&self.visuals[v].id) {
            return;
        }
        let (Colors, refs, text_refs) = (Colors.clone(), refs.clone(), text_refs.clone());
        let x = (d(self.time.time) * 250.0f32 + offset).f();
        let time = mathf::perlin(x, -384.4597);
        let t = mathf::perlin(x, 908.354);
        // Color color = Multiply * Colors.Evaluate(time) (component-wise)
        let g = Colors.evaluate(time);
        let color = [Multiply[0] * g[0], Multiply[1] * g[1], Multiply[2] * g[2], Multiply[3] * g[3]];
        if let Some(i) = text {
            self.text_color_set(i, color);
        }
        if let Some(s) = sprite {
            self.set_sprite_color(s, color);
        }
        // base.transform.localPosition = new Vector3(0f, Mathf.Lerp(-JitterIntensity, JitterIntensity, t), 0f)
        let p = [0.0, mathf::lerp(-JitterIntensity, JitterIntensity, t), 0.0];
        local.t = [p[0], p[1], p[2], local.t[3]];
        for (r, k) in refs {
            self.renderers[r].chain[k] = local;
        }
        for (i, k) in text_refs {
            self.world_texts[i].chain[k] = local;
        }
        let id = self.visuals[v].id;
        self.with_component_instance(id, |_, inst, o| inst.set_local_position(o, p));
        let Kind::Hologram { local: l, .. } = &mut self.visuals[v].kind else { unreachable!() };
        *l = local;
    }
}

/// SnapToVanillaPixelPosition.get_snap (an edit-mode script's private property): PixelSize, halved when Half.
pub fn snap_to_vanilla_snap(pixel_size: f32, half: bool) -> f32 {
    if !half {
        return pixel_size;
    }
    (d(pixel_size) / 2.0f32).f()
}

/// SortingLayerChild.OffsetIsApplicable: Behaviour != SortBehaviour.Same (1).
pub fn sorting_layer_child_offset_is_applicable(behaviour: i32) -> bool {
    behaviour != 1
}
