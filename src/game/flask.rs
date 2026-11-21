//! FlaskBehaviour and GasTankBehaviour (a BloodContainer outside a limb), and the LiquidContainerController of a flask.
//! The flasks' PointLiquidTransferBehaviour acts only while the object is being used
//! continuously (FixedUpdate).
#![allow(non_snake_case)]

use super::circulation::LIQUIDS;
use super::container::{BloodContainer, Owner};
use super::{d, mathf, script, Game};

/// A SerialisableDistribution (FlaskBehaviour.StartLiquid): LiquidID and Amount, as a recording writes it,
/// `SerialisableDistribution(LiquidID="<id>" Amount=<float bits>)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Distribution {
    pub LiquidID: String,
    pub Amount: f32,
}

impl super::Field for Distribution {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        let bad = || format!("bad SerialisableDistribution {text}");
        let inner = text.strip_prefix("SerialisableDistribution(").and_then(|x| x.strip_suffix(')')).ok_or_else(bad)?;
        let (id, rest) = inner.strip_prefix("LiquidID=\"").and_then(|x| x.split_once("\" Amount=")).ok_or_else(bad)?;
        self.Amount = u32::from_str_radix(rest, 16).map(f32::from_bits).map_err(|_| bad())?;
        self.LiquidID = id.to_string();
        Ok(())
    }
    fn show(&self) -> String {
        format!("SerialisableDistribution(LiquidID=\"{}\" Amount={:08x})", self.LiquidID, self.Amount.to_bits())
    }
    fn set(&mut self, _v: &crate::assets::mono::Value) -> Result<(), String> {
        Err("SerialisableDistribution from game data: link.rs reads its LiquidID and Amount".into())
    }
}

#[test]
fn distribution_round_trip() {
    use super::Field;
    let mut d = Distribution::default();
    d.parse("SerialisableDistribution(LiquidID=\"TRITIUM\" Amount=3f000000)").unwrap();
    assert_eq!((d.LiquidID.as_str(), d.Amount), ("TRITIUM", 0.5));
    assert_eq!(d.show(), "SerialisableDistribution(LiquidID=\"TRITIUM\" Amount=3f000000)");
}

script! {
    Flask {
        Capacity: f32 = 2.8,
        NewlySpawned: bool = true,
    }
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Limits.y: GasTankBehaviour's 20 or FlaskBehaviour's Capacity; AllowsOverflow is false.
        limit: f32 = 20.0,
        /// StartLiquid
        StartLiquid: Distribution = Distribution::default(),
        /// LiquidContainer, the LiquidContainerController whose fill Update sets.
        LiquidContainer: Option<usize> = None,
        /// GetComponent<PhysicalBehaviour>()
        Phys: Option<usize> = None,
    }
}

script! {
    PointLiquidTransfer {
        Point: [f32; 2] = [0.0, 0.0],
        /// UnityEngine.Space: World 0, Self 1.
        Space: i32 = 0,
        UnitsPerSecond: f32 = 0.56,
    }
    links {
        /// GetComponent<PhysicalBehaviour>()
        Phys: usize = 0,
        /// GetComponent<BloodContainer>(): the `Game::flasks` index (the prefabs put it on a flask).
        Flask: usize = 0,
        /// Layers (LayerMask bits)
        Layers: u32 = 0,
    }
}

/// BloodContainer's subclasses.
pub(super) const BLOOD_CONTAINERS: &[&str] = &[
    "BloodTankBehaviour", "CentrifugeBehaviour", "CirculationBehaviour", "FlaskBehaviour", "GasTankBehaviour", "InspectorLiquidContainer",
    "LiquidCanisterBehaviour", "LiquidDuplicatorBehaviour", "LiquidFilterBehaviour", "LiquidOutletBehaviour", "LiquidPumpBehaviour",
    "LiquidentifierBehaviour", "ReactorConduitBehaviour", "SyringeBehaviour", "ValveBehaviour",
];

/// One of the 16 Particles: Position and Velocity.
#[derive(Clone, Copy, Debug, Default)]
pub struct Particle {
    pub Position: [f32; 2],
    pub Velocity: [f32; 2],
}

/// LiquidContainerController: the particles, preVelocity and the Random draws, the sloshing volume, and the shader inputs
/// (particlePositions, the colour and the MaterialPropertyBlock its renderer is drawn with, custom_material.rs).
#[derive(Clone, Debug)]
pub struct LiquidContainer {
    pub id: i32,
    pub FillPercentage: f32,
    /// Container.min and max
    pub min: [f32; 2],
    pub max: [f32; 2],
    /// Container.center and extents
    pub center: [f32; 3],
    pub extents: [f32; 3],
    /// UserPreferenceManager.Current.FancyEffects at Awake
    pub fancy: bool,
    /// The Rigidbody2D component's instance id (null: None).
    pub Rigidbody: Option<i32>,
    /// base.transform: where it hangs and its Transforms below that (object first).
    pub anchor: super::BareAnchor,
    pub chain: Vec<crate::unity::transform::Trs>,
    pub particles: Vec<Particle>,
    pub preVelocity: [f32; 2],
    pub SloshingIntensity: f32,
    pub ParticleRadius: f32,
    /// GetComponent<Renderer>(): the `Game.renderers` entry of the object's SpriteRenderer (none: null).
    pub renderer: Option<usize>,
    /// LowFidelityLiquid (a Material of the build).
    pub LowFidelityLiquid: Option<crate::assets::db::ObjRef>,
    /// The private color behind Color.
    pub color: [f32; 4],
    /// particlePositions (null without FancyEffects).
    pub particlePositions: Option<Vec<[f32; 4]>>,
    /// properties (null until OnWillRenderObject).
    pub properties: Option<super::props::PropBlock>,
    /// renderComponent is set (OnWillRenderObject).
    pub renderComponent: bool,
    /// renderComponent.isVisible: the renderer was drawn by the last render.
    pub isVisible: bool,
}

impl Default for LiquidContainer {
    fn default() -> Self {
        Self {
            id: Default::default(),
            FillPercentage: Default::default(),
            min: Default::default(),
            max: Default::default(),
            center: Default::default(),
            extents: Default::default(),
            fancy: Default::default(),
            Rigidbody: Default::default(),
            anchor: Default::default(),
            chain: Default::default(),
            particles: Default::default(),
            preVelocity: Default::default(),
            SloshingIntensity: Default::default(),
            ParticleRadius: 1.0,
            renderer: Default::default(),
            LowFidelityLiquid: Default::default(),
            color: Default::default(),
            particlePositions: Default::default(),
            properties: Default::default(),
            renderComponent: Default::default(),
            isVisible: Default::default(),
        }
    }
}

impl Game {
    // PointLiquidTransferBehaviour.Awake: GetComponent<BloodContainer>() and GetComponent<PhysicalBehaviour>(), the `Flask` and `Phys` links.
    /// FlaskBehaviour.Start
    pub fn flask_start(&mut self, f: usize) {
        let s = &self.flasks[f];
        if s.NewlySpawned && s.StartLiquid.Amount > mathf::EPSILON {
            let id = s.StartLiquid.LiquidID.clone();
            let amount = s.StartLiquid.Amount;
            match LIQUIDS.iter().position(|(n, _)| *n == id) {
                Some(liquid) => {
                    self.add_liquid(Owner::Flask(f), liquid, amount);
                }
                // Liquid.GetLiquid logs the error and returns null.
                None => {
                    self.add_null_liquid(Owner::Flask(f), amount);
                }
            }
        }
        self.flasks[f].NewlySpawned = false;
    }

    /// LiquidContainer.Color = GetComputedColor() of container `o` (the fallback is its cached colour).
    pub(super) fn liquid_container_color_from(&mut self, c: usize, o: Owner) {
        let fallback = self.bc(o).cachedComputedColour;
        let color = self.get_computed_color(o, fallback);
        self.liquid_container_set_color(c, color);
    }

    /// FlaskBehaviour.Update: BloodContainer.Update, then the LiquidContainer's fill and colour.
    pub fn flask_update(&mut self, f: usize) {
        self.blood_container_update(Owner::Flask(f));
        if let Some(c) = self.flasks[f].LiquidContainer {
            // FillPercentage = Mathf.Clamp01(ScaledLiquidAmount)
            self.liquid_containers[c].FillPercentage = mathf::clamp(self.scaled_liquid_amount(Owner::Flask(f)), 0.0, 1.0);
            self.liquid_container_color_from(c, Owner::Flask(f));
        }
    }

    /// PointLiquidTransferBehaviour.FixedUpdate
    pub fn point_liquid_fixed_update(&mut self, i: usize) {
        let phys = self.point_liquids[i].Phys;
        if !self.used_continuously(phys) {
            return;
        }
        self.point_liquid_transfer(i, self.time.fixedDeltaTime);
    }

    /// PointLiquidTransferBehaviour.GetPoint: Space.World is the point, anything else base.transform.TransformPoint(Point).
    pub(crate) fn point_liquid_get_point(&self, i: usize) -> [f32; 2] {
        let s = &self.point_liquids[i];
        if s.Space == 0 {
            return s.Point;
        }
        let w = self.transform_point3(s.Phys, [s.Point[0], s.Point[1], 0.0]);
        [w[0], w[1]]
    }

    /// PointLiquidTransferBehaviour.Transfer
    pub(crate) fn point_liquid_transfer(&mut self, i: usize, dt: f32) {
        let s = &self.point_liquids[i];
        let (flask, phys, mask) = (s.Flask, s.Phys, s.Layers);
        let amount = s.UnitsPerSecond * dt;
        let at = self.point_liquid_get_point(i);
        // Physics2D.OverlapPoint(point, Layers); the lowest-z collider, whose GameObject must not be this one and must
        // carry a BloodContainer (a limb's CirculationBehaviour or a flask).
        let Some((hit, collider)) = self.overlap_point_first(at, mask) else { return };
        let Some(q) = hit else {
            if self.bare_colliders[&collider].scripts.iter().any(|s| BLOOD_CONTAINERS.contains(&s.as_str())) {
                unimplemented!("mod-only: PointLiquidTransferBehaviour: a BloodContainer on a collider without a PhysicalBehaviour");
            }
            return;
        };
        if q == phys {
            return;
        }
        // TryGetComponent<BloodContainer>: any container class on the collider's GameObject.
        let Some(target) = self.physical_container(q) else { return };
        // !component.IsFull() || component.AllowsOverflow
        if self.is_full(target, 0.999) && !self.allows_overflow(target) {
            return;
        }
        self.transfer_to(Owner::Flask(flask), amount, target);
    }

    /// LiquidContainerController.Awake: with FancyEffects the 16 particles take a random position each, x then y.
    pub fn liquid_container_awake(&mut self, c: usize) {
        let fancy = self.prefs.FancyEffects;
        self.liquid_containers[c].fancy = fancy;
        if !fancy {
            // GetComponent<Renderer>().sharedMaterial = LowFidelityLiquid
            let Some(r) = self.liquid_containers[c].renderer else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
            self.renderers[r].material = self.liquid_containers[c].LowFidelityLiquid;
            return;
        }
        let (min, max) = (self.liquid_containers[c].min, self.liquid_containers[c].max);
        let mut particles = Vec::new();
        for _ in 0..16 {
            let x = self.random.range(min[0], max[0]);
            let y = self.random.range(min[1], max[1]);
            particles.push(Particle { Position: [x, y], Velocity: [0.0, 0.0] });
        }
        self.liquid_containers[c].particlePositions = Some(particles.iter().map(|p| [p.Position[0], p.Position[1], 0.0, 0.0]).collect());
        self.liquid_containers[c].particles = particles;
    }

    /// LiquidContainerController.SloshingSource
    fn sloshing_source(&self, c: usize) -> i32 {
        self.audio.source_field(self.liquid_containers[c].id, "SloshingSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: LiquidContainerController.SloshingSource"))
    }

    /// LiquidContainerController.TransformSoundVolume(input): (-4f * f * f + 4f * f)^3 * input, f = FillPercentage.
    pub(crate) fn liquid_transform_sound_volume(&self, c: usize, input: f32) -> f32 {
        let f = self.liquid_containers[c].FillPercentage;
        let num = (d(-4.0f32) * f * f + d(4.0f32) * f).f();
        (d(num) * num * num * input).f()
    }

    /// LiquidContainerController.Update: a NoNeedToSimulate container's SloshingSource.volume = 0; else particlePositions
    /// move toward the particles by 1 - 0.00025^deltaTime.
    pub fn liquid_container_update(&mut self, c: usize) {
        if self.liquid_no_need_to_simulate(c) {
            let s = self.sloshing_source(c);
            self.audio.get_mut(s).set_volume(0.0);
            return;
        }
        let t = (d(1.0f32) - mathf::pow(0.00025, self.time.deltaTime)).f();
        let s = &mut self.liquid_containers[c];
        let Some(positions) = s.particlePositions.as_mut() else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        // Vector2.Lerp(particlePositions[i], particles[i].Position, t): t clamped, then a + (b - a) * t per component (each
        // one expression); back to a Vector4.
        for i in 0..16 {
            let (a, b) = (positions[i], s.particles[i].Position);
            positions[i] = [mathf::lerp(a[0], b[0], t), mathf::lerp(a[1], b[1], t), 0.0, 0.0];
        }
    }

    /// LiquidContainerController.Start: SloshingSource.enabled = isFancy, then with FancyEffects SimulateLiquid and
    /// particlePositions = the particles' positions.
    pub fn liquid_container_start(&mut self, c: usize) {
        let s = self.sloshing_source(c);
        let fancy = self.liquid_containers[c].fancy;
        self.audio.set_enabled(s, fancy);
        if self.liquid_containers[c].fancy {
            self.liquid_simulate(c);
            let s = &mut self.liquid_containers[c];
            let positions = s.particlePositions.as_mut().expect("Awake made particlePositions");
            for i in 0..16 {
                positions[i] = [s.particles[i].Position[0], s.particles[i].Position[1], 0.0, 0.0];
            }
        }
    }

    /// LiquidContainerController.set_Color: color = value, and _LiquidColor on properties when there is one (the renderer
    /// takes it at the next SetPropertyBlock).
    pub fn liquid_container_set_color(&mut self, c: usize, value: [f32; 4]) {
        let s = &mut self.liquid_containers[c];
        s.color = value;
        if let Some(p) = &mut s.properties {
            p.set_color("_LiquidColor", value);
        }
    }

    /// LiquidContainerController.GetClipThreshold: (1 - FillPercentage) / Mathf.Max(0.01, ParticleRadius).
    pub(crate) fn liquid_clip_threshold(&self, c: usize) -> f32 {
        let s = &self.liquid_containers[c];
        (d((d(1.0f32) - s.FillPercentage).f()) / mathf::max(0.01, s.ParticleRadius)).f()
    }

    /// _UvBounds: (Container.min.x + 0.5, Container.min.y + 0.5, Container.max.x + 0.5, Container.max.y + 0.5).
    fn liquid_uv_bounds(&self, c: usize) -> [f32; 4] {
        let s = &self.liquid_containers[c];
        [(d(s.min[0]) + 0.5f32).f(), (d(s.min[1]) + 0.5f32).f(), (d(s.max[0]) + 0.5f32).f(), (d(s.max[1]) + 0.5f32).f()]
    }

    /// LiquidContainerController.SetShaderProperties (also OnBecameVisible): with a properties block, _Particles (when
    /// particlePositions is set), _ParticleCount 16, _ClipThreshold and _UvBounds, then renderComponent.SetPropertyBlock.
    pub(crate) fn liquid_set_shader_properties(&mut self, c: usize) {
        let Some(mut p) = self.liquid_containers[c].properties.take() else { return };
        if let Some(positions) = &self.liquid_containers[c].particlePositions {
            p.set_vector_array("_Particles", positions);
        }
        // SetInt: a float in the block.
        p.set_float("_ParticleCount", 16.0);
        p.set_float("_ClipThreshold", self.liquid_clip_threshold(c));
        p.set_vector("_UvBounds", self.liquid_uv_bounds(c));
        let Some(r) = self.liquid_containers[c].renderer.filter(|_| self.liquid_containers[c].renderComponent) else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        self.set_property_block(super::visual::SpriteAt::Renderer(r), &p);
        self.liquid_containers[c].properties = Some(p);
    }

    /// The render after LateUpdate: each container's renderer.isVisible (drawn by this render), OnBecameVisible when it
    /// turns visible, then LiquidContainerController.OnWillRenderObject while it is drawn.
    pub fn liquid_containers_will_render(&mut self) {
        for c in 0..self.liquid_containers.len() {
            let Some(r) = self.liquid_containers[c].renderer else { continue };
            let at = super::visual::SpriteAt::Renderer(r);
            let drawn = self.sprite_drawn(at);
            let was = std::mem::replace(&mut self.liquid_containers[c].isVisible, drawn);
            if !drawn || self.destroyed_components.contains(&self.liquid_containers[c].id) {
                continue;
            }
            if !was {
                // LiquidContainerController.OnBecameVisible
                self.liquid_set_shader_properties(c);
            }
            self.liquid_container_will_render(c);
        }
    }

    /// LiquidContainerController.OnWillRenderObject
    pub(crate) fn liquid_container_will_render(&mut self, c: usize) {
        {
            let Some(r) = self.liquid_containers[c].renderer else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
            let at = super::visual::SpriteAt::Renderer(r);
            if !self.liquid_containers[c].renderComponent || self.liquid_containers[c].properties.is_none() {
                let mut p = super::props::PropBlock::default();
                self.liquid_containers[c].renderComponent = true;
                p.set_color("_LiquidColor", self.liquid_containers[c].color);
                if self.liquid_containers[c].fancy {
                    if let Some(positions) = &self.liquid_containers[c].particlePositions {
                        p.set_vector_array("_Particles", positions);
                    }
                    p.set_float("_ParticleCount", 16.0);
                    p.set_float("_ClipThreshold", self.liquid_clip_threshold(c));
                    self.set_property_block(at, &p);
                }
                self.liquid_containers[c].properties = Some(p);
            }
            if !self.liquid_containers[c].fancy {
                let mut p = self.liquid_containers[c].properties.take().unwrap();
                p.set_color("_LiquidColor", self.liquid_containers[c].color);
                p.set_float("_Fill", self.liquid_containers[c].FillPercentage);
                p.set_vector("_UvBounds", self.liquid_uv_bounds(c));
                self.set_property_block(at, &p);
                self.liquid_containers[c].properties = Some(p);
            } else {
                self.liquid_set_shader_properties(c);
            }
        }
    }

    /// LiquidContainerController.FixedUpdate: `!NoNeedToSimulate && (!renderComponent || renderComponent.isVisible)`.
    pub fn liquid_container_fixed_update(&mut self, c: usize) {
        let s = &self.liquid_containers[c];
        if !self.liquid_no_need_to_simulate(c) && (!s.renderComponent || s.isVisible) {
            self.liquid_simulate(c);
        }
    }

    /// LiquidContainerController.NoNeedToSimulate: `isFancy && !Approximately(FillPercentage, 0) ? Approximately(FillPercentage, 1) : true`.
    pub fn liquid_no_need_to_simulate(&self, c: usize) -> bool {
        let s = &self.liquid_containers[c];
        if s.fancy && !mathf::approximately(s.FillPercentage, 0.0) {
            return mathf::approximately(s.FillPercentage, 1.0);
        }
        true
    }

    /// LiquidContainerController.OffsetRepulsionDistance => 0.01f + FillPercentage * 0.2f (double-evaluated).
    pub fn liquid_offset_repulsion_distance(&self, c: usize) -> f32 {
        (d(0.01f32) + d(self.liquid_containers[c].FillPercentage) * 0.2f32).f()
    }

    /// LiquidContainerController.SimulateLiquid. Every Vector2 operator is one float operation per component;
    /// the fused expressions (sqrMagnitude, OffsetRepulsionDistance, edgeRepulse) are double-evaluated.
    pub(crate) fn liquid_simulate(&mut self, c: usize) {
        let Some(rb) = self.liquid_containers[c].Rigidbody else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let (anchor, chain) = (self.liquid_containers[c].anchor, self.liquid_containers[c].chain.clone());
        let g = self.world.gravity;
        // base.transform.InverseTransformVector(Physics2D.gravity)
        let vector = self.anchored_inverse_vector(anchor, &chain, [g.x, g.y, 0.0]);
        let owner = self.rigidbody_owner(rb).expect("LiquidContainerController.Rigidbody");
        let r = self.owner_rigidbody(owner);
        let velocity = r.velocity(&self.world);
        let gravity_scale = r.gravity_scale(&self.world);
        let s = &self.liquid_containers[c];
        let acceleration = [s.preVelocity[0] - velocity[0], s.preVelocity[1] - velocity[1]];
        let vector2 = self.anchored_inverse_vector(anchor, &chain, [acceleration[0], acceleration[1], 0.0]);
        let s = &mut self.liquid_containers[c];
        s.preVelocity = velocity;
        let (fill, min, max, center, extents) = (s.FillPercentage, s.min, s.max, s.center, s.extents);
        let mut particles = std::mem::take(&mut s.particles);
        let mut num2 = 0.0f32;
        let offset = (d(0.01f32) + d(fill) * 0.2f32).f();
        for i in 0..16 {
            for j in 0..16 {
                if j == i {
                    continue;
                }
                let vector3 = [particles[j].Position[0] - particles[i].Position[0], particles[j].Position[1] - particles[i].Position[1]];
                let sqr_magnitude = mathf::sqr_magnitude(vector3);
                if sqr_magnitude == 0.0 {
                    let u = self.random.inside_unit_circle();
                    particles[i].Position[0] += u[0] * 0.01;
                    particles[i].Position[1] += u[1] * 0.01;
                    continue;
                }
                let num = mathf::sqrt(sqr_magnitude);
                if (num as f64) < offset as f64 * 1.5 {
                    let k = sqr_magnitude + 1.0;
                    for a in 0..2 {
                        particles[i].Velocity[a] += -(vector3[a] / num / k) * 0.07;
                    }
                }
            }
        }
        for p in particles.iter_mut() {
            // Container.Contains(Vector3): center -+ extents compared per axis (a NaN is inside).
            let at = [p.Position[0], p.Position[1], 0.0];
            let inside = (0..3).all(|a| !(center[a] - extents[a] > at[a] || at[a] > extents[a] + center[a]));
            if !inside {
                if p.Position[0] < min[0] || p.Position[0] > max[0] {
                    p.Velocity[0] = 0.0;
                }
                if p.Position[1] < min[1] || p.Position[1] > max[1] {
                    p.Velocity[1] = 0.0;
                }
                let closest = crate::unity::transform::bounds_closest_point(at, center, extents).0;
                p.Position = [closest[0], closest[1]];
            }
            for a in 0..2 {
                p.Position[a] += p.Velocity[a] * 0.1;
                p.Velocity[a] *= 0.95;
                p.Velocity[a] += vector2[a] * 0.25;
            }
            // 0.002f * Rigidbody.gravityScale * vector
            let scale = 0.002f32 * gravity_scale;
            p.Velocity[0] += scale * vector[0];
            p.Velocity[1] += scale * vector[1];
            num2 += mathf::sqr_magnitude(p.Velocity);
            let vector4 = [p.Position[0] - min[0], p.Position[1] - min[1]];
            let vector5 = [max[0] - p.Position[0], max[1] - p.Position[1]];
            // edgeRepulse: 0.5f / (num3 * num3) * 0.07f in one double expression.
            let edge_repulse = |delta: f32| {
                let num3 = 1.0 + delta.abs();
                (d(0.5f32) / (d(num3) * num3) * 0.07f32).f()
            };
            if vector4[0] < offset {
                p.Velocity[0] += edge_repulse(vector4[0]);
            }
            if vector4[1] < offset {
                p.Velocity[1] += edge_repulse(vector4[1]);
            }
            if vector5[0] < offset {
                p.Velocity[0] -= edge_repulse(vector5[0]);
            }
            if vector5[1] < offset {
                p.Velocity[1] -= edge_repulse(vector5[1]);
            }
        }
        self.liquid_containers[c].particles = particles;
        // SloshingSource.volume = Lerp(volume, TransformSoundVolume(SloshingIntensity * (num2 / 16f)), 0.6f)
        let target = self.liquid_transform_sound_volume(c, (d(self.liquid_containers[c].SloshingIntensity) * (d(num2) / 16.0f32)).f());
        let s = self.sloshing_source(c);
        let a = self.audio.get_mut(s);
        a.set_volume(mathf::lerp(a.volume, target, 0.6));
    }
}
