//! The wires' LineRenderers: the geometry of LineRendererGeometryJob (BuildLineSegment, BuildCap) and
//! Unlit/BloodWireShader, and Unlit/VertexColourCutout and Jaap/GlowingMetal for the other wires.
use crate::game::blood_wire::WireKind;
use crate::play::{load_texture, Play, Textures};
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    camera::visibility::NoFrustumCulling,
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline, Material2dPlugin},
};

const MITER_EPSILON: f32 = 1e-05;
const WIRE_ORDER: i16 = -2;

/// The corner's offset between two segments' normals: where their edges meet (LineRendererGeometryJob's miter), eased to `a`
/// as the normals near equal or opposite, and no longer than `max(1, 3 * t)`.
fn miter(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    let sqr = |x: f32, y: f32| x * x + y * y;
    let close = (sqr(a[0] - b[0], a[1] - b[1]) * 50.0).max(0.0);
    let opposite = (sqr(a[0] + b[0], a[1] + b[1]) * 50.0).max(0.0);
    let limit = (3.0 * t).max(1.0);
    let m1 = (a[0] / -a[1]).clamp(-100000.0, 100000.0);
    let c1 = a[1] - m1 * a[0];
    let m2 = (-b[0] / b[1]).clamp(-100000.0, 100000.0);
    let c2 = b[1] - m2 * b[0];
    let mut out = a;
    if (m1 - m2).abs() >= MITER_EPSILON {
        let x = (c2 - c1) / (m1 - m2);
        let y = m1 * x + c1;
        let w = close.min(1.0).min(opposite.min(1.0));
        out = [a[0] + (x - a[0]) * w, a[1] + (y - a[1]) * w];
    }
    let length = sqr(out[0], out[1]);
    if limit * limit < length {
        let k = limit / length.sqrt();
        out = [out[0] * k, out[1] * k];
    }
    out
}

/// LineRenderer.textureMode
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LineTexture {
    Tile,
    Stretch,
    RepeatPerSegment,
    DistributePerSegment,
}

/// A LineRenderer's widthCurve: an AnimationCurve of Hermite keys (time, value, inSlope, outSlope), clamped beyond its ends.
#[derive(Clone, Debug, PartialEq)]
pub struct WidthCurve(pub Vec<[f32; 4]>);

impl WidthCurve {
    pub fn constant(v: f32) -> Self {
        WidthCurve(vec![[0.0, v, 0.0, 0.0]])
    }

    /// AnimationCurve::Evaluate (UnityPlayer 180746eb0 with the cache of 180745ae0): one key is its value; before the first and
    /// after the last key the end value (the infinities are Clamp); between two keys the cubic ((dt*c0 + c1)*dt + c2)*dt + c3
    /// from the left key, infinite slopes stepping.
    pub fn evaluate(&self, t: f32) -> f32 {
        let k = &self.0;
        let n = k.len();
        if n == 1 || t >= k[n - 1][0] {
            return k[n - 1][1];
        }
        if t < k[0][0] {
            return k[0][1];
        }
        let i = (0..n - 1).rev().find(|&i| k[i][0] <= t).unwrap();
        let (a, b) = (k[i], k[i + 1]);
        let dx = (b[0] - a[0]).max(0.0001);
        let dy = b[1] - a[1];
        let (m1, m2) = (a[3] * dx, dx * b[2]);
        let l = 1.0 / (dx * dx);
        let (mut c0, mut c1, mut c2, mut c3) = (((m2 + m1 - dy - dy) * l) / dx, (dy + dy + dy - m1 - m1 - m2) * l, a[3], a[1]);
        if a[3] == f32::INFINITY || b[2] == f32::INFINITY {
            (c0, c1, c2, c3) = (0.0, 0.0, 0.0, a[1]);
        }
        let dt = t - a[0];
        ((dt * c0 + c1) * dt + c2) * dt + c3
    }
}

/// The vertex colour of a LineRenderer point: the gradient at t as ColorRGBA32 bytes, then back to floats.
fn quantized(g: &crate::game::visual::Gradient, t: f32) -> [f32; 4] {
    g.evaluate(t).map(|c| ((c.max(0.0).min(1.0) * 255.0 + 0.5) as u8) as f32 / 255.0)
}

/// LineRenderer with Local alignment, Tile texture mode and six cap vertices: StoreLineData, then a triangle strip of
/// BuildCap, BuildLineSegment for each point and BuildCap. u is the distance along the line.
pub fn line_mesh(points: &[[f32; 2]], width: f32, cap_vertices: usize, mode: LineTexture) -> Mesh {
    line_mesh_shaped(points, width, &WidthCurve::constant(1.0), None, cap_vertices, mode)
}

/// Build3DLine (UnityPlayer 18066ac00): the width of point i is widthCurve(t) * widthMultiplier and its colour the gradient at t,
/// t being the length so far over the whole (Stretch, Tile) or i / (n - 1) (DistributePerSegment, RepeatPerSegment). StoreLineData
/// sees only the widthMultiplier (its segment ratio and the shorter-than-wide test). Without a gradient the colour attribute is left out.
pub fn line_mesh_shaped(points: &[[f32; 2]], width: f32, curve: &WidthCurve, gradient: Option<&crate::game::visual::Gradient>, cap_vertices: usize, mode: LineTexture) -> Mesh {
    line_mesh_full(points, &[], width, curve, gradient, cap_vertices, 0, false, mode)
}

/// The engine's polynomial atan2 of BuildSmoothCorner<0> (1806692f0): atan of |y / x| from an odd polynomial on the smaller
/// of |q| and its refined reciprocal, folded back by pi/2, pi - for a negative x, and y's sign.
fn corner_atan2(y: f32, x: f32) -> f32 {
    let q = (y / x).abs();
    let big = 1.0 < q;
    let r = if big {
        let r = crate::particle_render::rcpps(q);
        let r = (f32::from_bits(0x40000004) - q * r) * r;
        r * (2.0 - r * q)
    } else {
        q
    };
    let s = r * r;
    let s3 = s * s * s;
    let s4 = s3 * s;
    let s5 = s4 * s;
    let p = r * (s * -0.3332646 + 0.99999946 + s * s * 0.19881524 + s3 * -0.13487266 + s4 * 0.08387136 + s5 * -0.037013143 + s * s5 * 0.007862508);
    let mut a = if big { 1.5707964 - p } else { p };
    if x.to_bits() >> 31 != 0 {
        a = 3.1415927 - a;
    }
    f32::from_bits(a.to_bits() ^ (y.to_bits() & 0x8000_0000))
}

/// The engine's polynomial cosine of an angle in turns (BuildSmoothCorner<0>, BuildCap<0>): cos 2 pi u.
fn turn_cos(u: f32) -> f32 {
    let c = 0.25 - (u - (u as i32) as f32).abs();
    let s = c * c;
    let s2 = s * s;
    ((s * -76.56862 + 81.60182) * s2 + s * -41.34166 + 6.283185 + s2 * s2 * 39.657032) * c
}

/// Build3DLine over a LineRenderer's positions with `corner_vertices` smooth corner vertices and `looped`: a loop is
/// stored (StoreLineData) as [p(k-1), p0 .. p(k-1), p0, p1] and the segments are built from the second to the one
/// before the last, every one a corner (Initialize passes positionCount + 1 and Build3DLine adds 2: BakeMesh).
/// `zs` are the points' z in the build space (BuildSmoothCorner and BuildCap flip their side for a positive one; empty =
/// 0, as in the View alignment's camera space, where z is negative). StoreLineData's View branch crosses the
/// segment with the clamped camera-space position, its other branch with (0, 0, -1): both give (d.y, -d.x) / |d| for a
/// point in front of a View camera, so the extrusion does not depend on z for Local.
pub fn line_mesh_full(points: &[[f32; 2]], zs: &[f32], width: f32, curve: &WidthCurve, gradient: Option<&crate::game::visual::Gradient>, cap_vertices: usize, corner_vertices: usize, looped: bool, mode: LineTexture) -> Mesh {
    let k = points.len();
    let (pts, zv): (Vec<[f32; 2]>, Vec<f32>) = if looped {
        let order: Vec<usize> = std::iter::once(k - 1).chain(0..k).chain([0, 1]).collect();
        (order.iter().map(|&i| points[i]).collect(), order.iter().map(|&i| zs.get(i).copied().unwrap_or(0.0)).collect())
    } else {
        (points.to_vec(), (0..k).map(|i| zs.get(i).copied().unwrap_or(0.0)).collect())
    };
    let (points, n) = (&pts[..], pts.len());
    let (first, last) = if looped { (1, n - 1) } else { (0, n) };
    let sub = |a: [f32; 2], b: [f32; 2]| [a[0] - b[0], a[1] - b[1]];
    let sqr = |v: [f32; 2]| v[0] * v[0] + v[1] * v[1];
    let normalize = |v: [f32; 2]| {
        let l = sqr(v).sqrt();
        [v[0] / l, v[1] / l]
    };
    let (mut u, mut ratio, mut normals) = (vec![0.0f32; n], vec![0.0f32; n], vec![[0.0f32; 2]; n]);
    let mut valid = None;
    for i in 0..n {
        let j = i.max(1);
        let mut d = sub(points[j - 1], points[j]);
        let length = sqr(d).sqrt();
        // currentLineLengths: a loop counts only the segments from the third vertex to the one before the last.
        if if looped { i >= 2 && i < n - 1 } else { i > 0 } {
            u[i] = u[i - 1] + length;
        } else if i > 0 {
            u[i] = u[i - 1];
        }
        ratio[i] = length / (2.0 * width);
        if sqr(d) < 1e-6 {
            if j < n - 1 {
                d = sub(points[j], points[j + 1]);
            }
            if sqr(d) < 1e-6 {
                d = valid.unwrap_or([1.0, 0.0]);
            } else {
                valid = Some(d);
            }
        } else {
            valid = Some(d);
        }
        normals[i] = normalize([d[1], -d[0]]);
        // A segment shorter than the line is wide: its ends' normals ease to their average.
        if length < 2.0 * width && i > 0 && (!looped || i < n - 2) {
            let (before, here) = (normals[i - 1], normals[i]);
            let average = [(before[0] + here[0]) * 0.5, (before[1] + here[1]) * 0.5];
            let t = (ratio[i] - 0.5).max(0.0) * 2.0;
            let ease = |v: [f32; 2]| {
                let e = [(v[0] - average[0]) * t + average[0], (v[1] - average[1]) * t + average[1]];
                if sqr(e) > 1e-30 { normalize(e) } else { v }
            };
            normals[i - 1] = ease(before);
            normals[i] = ease(here);
        }
    }
    let total = u[n - 1];
    let t: Vec<f32> = (0..n)
        .map(|i| {
            let raw = match mode {
                LineTexture::Tile | LineTexture::Stretch => u[i] * (1.0 / total),
                LineTexture::RepeatPerSegment | LineTexture::DistributePerSegment => i as f32 * (1.0 / (n - 1) as f32),
            };
            raw.max(0.0).min(1.0)
        })
        .collect();
    let half: Vec<f32> = t.iter().map(|&t| curve.evaluate(t) * width * 0.5).collect();
    let colors: Vec<[f32; 4]> = t.iter().map(|&t| gradient.map_or([1.0; 4], |g| quantized(g, t))).collect();
    match mode {
        LineTexture::Tile => {}
        LineTexture::Stretch => {
            let total = u[n - 1].max(1e-6);
            u.iter_mut().for_each(|x| *x /= total);
        }
        LineTexture::RepeatPerSegment => u.iter_mut().enumerate().for_each(|(i, x)| *x = i as f32),
        LineTexture::DistributePerSegment => u.iter_mut().enumerate().for_each(|(i, x)| *x = i as f32 * (1.0 / (n - 1) as f32)),
    }
    let (mut positions, mut uvs, mut vcolors): (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 4]>) = (Vec::new(), Vec::new(), Vec::new());
    let mut vertex = |i: usize, p: [f32; 2], off: [f32; 2], u: f32, v: f32| {
        positions.push([p[0] + off[0] * half[i], p[1] + off[1] * half[i], 0.0]);
        uvs.push([u, v]);
        vcolors.push(colors[i]);
    };
    let steps = cap_vertices + 1;
    for i in first..last {
        let p = points[i];
        if looped || (i > 0 && i < n - 1) {
            let m = miter([-normals[i][0], -normals[i][1]], [-normals[i + 1][0], -normals[i + 1][1]], ratio[i - 1].clamp(0.0, 1.0));
            if corner_vertices == 0 {
                vertex(i, p, m, u[i], 1.0);
                vertex(i, p, [-m[0], -m[1]], u[i], 0.0);
                continue;
            }
            // BuildSmoothCorner<0>: the inner side stays on the miter, the outer one sweeps an arc of the angle between the
            // two segments' extrusion directions, in corner_vertices + 1 steps (a loop's first vertex gives only the last).
            let (e0, e1) = (normals[i], normals[i + 1]);
            let mut diff = corner_atan2(e1[1], e1[0]) - corner_atan2(e0[1], e0[0]);
            if diff < -3.1415927 {
                diff += 6.2831855;
            }
            if diff > 3.1415927 {
                diff -= 6.2831855;
            }
            let cn = corner_vertices as f32;
            let r = crate::particle_render::rcpps(cn);
            let r = (f32::from_bits(0x40000004) - r * cn) * r;
            let step = diff * (r * (2.0 - r * cn));
            let mut sign = if diff.to_bits() >> 31 != 0 { 1.0f32 } else { -1.0 };
            if 0.0 < zv[i] {
                sign = -sign;
            }
            let h = half[i];
            let start = [p[0] + h * sign * m[0], p[1] + h * sign * m[1]];
            let reach = sign * h * 2.0;
            let (mut angle, mut it) = if looped && i == 1 { (diff, corner_vertices) } else { (0.0f32, 0) };
            while it <= corner_vertices {
                let (sin, cos) = (turn_cos(angle * 0.15915494 + -0.25), turn_cos(angle * 0.15915494 + 0.0));
                angle += step;
                let arc = [start[0] + (cos * e0[0] - sin * e0[1]) * reach, start[1] + (sin * e0[0] + cos * e0[1]) * sign * h * 2.0];
                let (near, far) = if reach < 0.0 { (arc, start) } else { (start, arc) };
                vertex(i, near, [0.0, 0.0], u[i], 1.0);
                vertex(i, far, [0.0, 0.0], u[i], 0.0);
                it += 1;
            }
            continue;
        }
        let normal = normals[(i + 1).min(n - 1)];
        let cap = |vertex: &mut dyn FnMut(usize, [f32; 2], [f32; 2], f32, f32), sign: f32| {
            for k in 0..=steps {
                let t = k as f32 / steps as f32;
                let angle = std::f32::consts::PI * t * sign;
                let (s, c) = (crate::unity::sinf(angle), crate::unity::cosf(angle));
                let rim = [normal[0] * c - normal[1] * s, normal[0] * s + normal[1] * c];
                // The end's cap starts at its centre.
                if sign > 0.0 {
                    vertex(i, p, rim, u[i], t);
                    vertex(i, p, [0.0, 0.0], u[i], 0.5);
                } else {
                    vertex(i, p, [0.0, 0.0], u[i], 0.5);
                    vertex(i, p, rim, u[i], t);
                }
            }
        };
        // BuildCap flips its side for a positive z, as BuildSmoothCorner does.
        let flip = if zv[i] > 0.0 { -1.0 } else { 1.0 };
        if i == 0 && cap_vertices > 0 {
            cap(&mut vertex, flip);
        }
        vertex(i, p, [-normal[0], -normal[1]], u[i], 1.0);
        vertex(i, p, normal, u[i], 0.0);
        if i == n - 1 && cap_vertices > 0 {
            cap(&mut vertex, -flip);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleStrip, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    if gradient.is_some() {
        let linear: Vec<[f32; 4]> = vcolors.iter().map(|c| Color::srgba(c[0], c[1], c[2], c[3]).to_linear().to_f32_array()).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, linear);
    }
    mesh
}

/// The vertex colours of a LineRenderer: startColor to endColor along the line, by u.
pub(crate) fn colored(mut mesh: Mesh, start: [f32; 4], end: [f32; 4]) -> Mesh {
    let bevy::mesh::VertexAttributeValues::Float32x2(uv) = mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap() else { unreachable!() };
    let length = uv.iter().map(|t| t[0]).fold(0.0, f32::max).max(1e-6);
    let colors: Vec<[f32; 4]> = uv
        .iter()
        .map(|t| {
            let k = t[0] / length;
            let c: [f32; 4] = std::array::from_fn(|i| start[i] + (end[i] - start[i]) * k);
            Color::srgba(c[0], c[1], c[2], c[3]).to_linear().to_f32_array()
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

/// Unlit/VertexColourCutout (`mode` 0), Jaap/GlowingMetal (`mode` 1) for the heat pipe, or Jaap/UseWire (`mode` 2) for
/// the logic wires (`time` is _Time.y).
#[derive(ShaderType, Clone, PartialEq)]
pub struct LineSettings {
    pub st: Vec4,
    pub glow: Vec4,
    pub cutoff: f32,
    pub progress: f32,
    pub mode: f32,
    pub time: f32,
    pub timestamp: f32,
    pub glow_time: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct WireLineMaterial {
    #[uniform(0)]
    pub settings: LineSettings,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
}

impl Material2d for WireLineMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/wire_line.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(_: &Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(ShaderType, Clone, PartialEq)]
pub struct WireSettings {
    /// _MainTex_ST.
    pub st: Vec4,
    pub blood: Vec4,
    pub flow: f32,
    pub bulge: f32,
    /// `_Time.w`.
    pub time: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct BloodWire {
    #[uniform(0)]
    pub settings: WireSettings,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Handle<Image>,
}

impl Material2d for BloodWire {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/blood_wire.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(_: &Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // Mesh2d culls back faces; a LineRenderer draws both.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub fn add(app: &mut App) {
    embedded_asset!(app, "blood_wire.wesl");
    embedded_asset!(app, "wire_line.wesl");
    crate::decals::add(app);
    crate::outline::add(app);
    crate::particles::add(app);
    app.add_plugins((Material2dPlugin::<BloodWire>::default(), Material2dPlugin::<WireLineMaterial>::default()));
}

/// A wire's line: `Game::blood_wires`' index, or a DistanceWireTool's own while dragging.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash)]
#[require(crate::play::MainScene)]
pub enum WireLine {
    Wire(usize),
    /// `Game::root_wires`' index.
    Root(usize),
    Tool(WireKind),
}

/// The line's last points and colours, so that the mesh is rebuilt only when they changed.
#[derive(Component)]
pub struct LastPoints(Vec<[f32; 2]>, [[f32; 4]; 2]);

/// A wire material's texture, _MainTex_ST and floats.
#[derive(Clone)]
pub struct WireStyle {
    queue: i16,
    texture: Handle<Image>,
    st: Vec4,
    bulge: f32,
    cutoff: f32,
    glow: Vec4,
    glow_time: f32,
    timestamp: f32,
}

fn style(g: &crate::game::Game, kind: WireKind, images: &mut Assets<Image>, textures: &mut Textures) -> WireStyle {
    let db = g.assets.as_deref().unwrap();
    let r = db.resource(kind.material(), 21).unwrap().unwrap();
    let m = crate::assets::components::Material::read(db.data(r)).unwrap();
    let (_, t, st) = m.textures.iter().find(|(n, ..)| n == "_MainTex").unwrap();
    let texture = load_texture(db, images, textures, db.resolve(r.file, *t).unwrap()).unwrap().0;
    WireStyle {
        queue: crate::play::render_queue(db, Some(r)),
        texture,
        st: Vec4::from_array(*st),
        bulge: m.float("_BulgeIntensity").unwrap_or(0.0),
        cutoff: m.float("_Cutoff").unwrap_or(0.5),
        glow: Vec4::from_array(m.color("_GlowColour").unwrap_or([0.0; 4])),
        glow_time: m.float("_GlowTime").unwrap_or(1.0),
        timestamp: m.float("_Timestamp").unwrap_or(0.0),
    }
}

/// TagManager's uniqueID of the "Background" sorting layer.
const BACKGROUND_LAYER: i32 = 1370879715;

/// What a line shows this frame.
struct Shown {
    kind: WireKind,
    points: Vec<[f32; 2]>,
    width: f32,
    caps: usize,
    texture: LineTexture,
    /// The bandage's sprite sortingLayerID and sortingOrder.
    sorting: Option<(i32, i16)>,
    flow: f32,
    blood: [f32; 4],
    colors: [[f32; 4]; 2],
    progress: f32,
    /// A local line's transform: origin, x axis and y axis (the line is meshed in its own space, LineAlignment.Local).
    local: Option<[[f32; 2]; 3]>,
    /// UseWireBehaviour's property block: _GlowColour (Start) and _Timestamp (SendGlow); else the material's.
    glow: Option<[f32; 3]>,
    timestamp: Option<f32>,
}

fn shown(g: &crate::game::Game, line: WireLine) -> Option<Shown> {
    match line {
        WireLine::Wire(w) => {
            let wire = g.blood_wires.get(w)?;
            // The ToolInvisibilityTracker's Renderer.enabled.
            if !wire.started || !wire.tracker.enabled || g.destroyed_components.contains(&wire.id) {
                return None;
            }
            // HeatPipeBehaviour.OnWillRenderObject: MapRange(0, 1000, 0, 1) of the temperature, clamped, squared.
            use crate::game::mathf;
            let hot = mathf::clamp01(mathf::map_range(0.0, 1000.0, 0.0, 1.0, wire.fakeCableTemperature));
            Some(Shown {
                kind: wire.kind,
                points: wire.vertices.iter().map(|v| [v[0], v[1]]).collect(),
                width: wire.WireWidth,
                caps: wire.kind.cap_vertices(),
                texture: if wire.kind.is_spring() || wire.kind.is_slider() { LineTexture::RepeatPerSegment } else { LineTexture::Tile },
                sorting: match wire.kind {
                    WireKind::Bandage => {
                        let p = &g.physicals[wire.physical];
                        let Some((_, _, order)) = p.sprite else { unimplemented!("mod-only: AppliedBandageBehaviour.Start: no SpriteRenderer") };
                        Some((p.sprite_layer, order))
                    }
                    // Created: sortingLayerName "Background"; Initialise's sortingOrder -2.
                    WireKind::SteelBinding | WireKind::WoodenBinding => Some((BACKGROUND_LAYER, -2)),
                    _ => None,
                },
                flow: wire.flow.clamp(-1.0, 1.0),
                blood: wire.color,
                colors: [wire.start_color, wire.end_color],
                progress: hot * hot,
                local: wire.kind.is_fixed().then(|| {
                    let at = |x, y| g.transform_point3(wire.physical, [x, y, 0.0]);
                    let (o, x, y) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0));
                    [[o[0], o[1]], [x[0] - o[0], x[1] - o[1]], [y[0] - o[0], y[1] - o[1]]]
                }),
                glow: matches!(wire.kind, WireKind::Use(..)).then_some([wire.Color[0], wire.Color[1], wire.Color[2]]),
                timestamp: wire.glow_timestamp,
            })
        }
        // A root wire's LineRenderer: enabled = !Invisible (no ToolInvisibilityTracker), world space, kept after the component
        // is destroyed until its object goes. Update set its positions before the first render.
        WireLine::Root(r) => {
            let wire = g.root_wires.get(r)?;
            if !wire.started || wire.gone || wire.Invisible || !wire.positionsCached {
                return None;
            }
            Some(Shown {
                kind: wire.kind,
                points: wire.line.iter().map(|v| [v[0], v[1]]).collect(),
                width: wire.WireWidth,
                // Initialise's numCapVertices 6 and Tile; the Created of the wood strut and the bindings sets caps 0. What Start
                // sets after Initialise (the spring's and slider's texture mode and caps) never runs. The blood wire's flow and
                // colour stay 0, the heat pipe's OnWillRenderObject throws before its progress, the use wire keeps the
                // material's glow.
                caps: if matches!(wire.kind, WireKind::WoodStrut | WireKind::SteelBinding | WireKind::WoodenBinding) { 0 } else { 6 },
                texture: LineTexture::Tile,
                sorting: matches!(wire.kind, WireKind::SteelBinding | WireKind::WoodenBinding).then_some((BACKGROUND_LAYER, -2)),
                flow: 0.0,
                blood: [0.0; 4],
                colors: [wire.start_color, wire.end_color],
                progress: 0.0,
                local: None,
                glow: None,
                timestamp: None,
            })
        }
        // OnToolChosen sets _Flow to 0; the material's own _BloodColor is never read then.
        WireLine::Tool(kind) => (g.wire_tool.kind == kind).then(|| g.wire_preview()).flatten().map(|points| Shown {
            kind,
            points: points.to_vec(),
            width: kind.width(),
            caps: kind.tool_line().0,
            texture: if kind.tool_line().1 { LineTexture::Stretch } else { LineTexture::Tile },
            sorting: None,
            flow: 0.0,
            blood: [0.0; 4],
            colors: [kind.color(); 2],
            progress: 0.0,
            local: None,
            // UseWireTool.OnToolChosen: the line's property block _GlowColour = GetWireColor() (its kin's colours too).
            // UseWireTool.OnToolUnchosen: a new, empty property block: the line is the material's again, shown only for the chosen tool.
            glow: if let WireKind::Use(channel, _) = kind { let c = crate::game::use_wire::wire_color(channel); Some([c[0], c[1], c[2]]) } else { None },
            timestamp: None,
        }),
    }
}

fn mesh_of(s: &Shown) -> Mesh {
    let mut mesh = colored(line_mesh(&s.points, s.width, s.caps, s.texture), s.colors[0], s.colors[1]);
    if let Some([o, x, y]) = s.local {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) else { unreachable!() };
        for p in positions {
            *p = [o[0] + x[0] * p[0] + y[0] * p[1], o[1] + x[1] * p[0] + y[1] * p[1], p[2]];
        }
    }
    mesh
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn wires(
    mut commands: Commands,
    play: Res<Play>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<BloodWire>>,
    mut line_materials: ResMut<Assets<WireLineMaterial>>,
    mut textures: ResMut<Textures>,
    mut styles: Local<std::collections::HashMap<WireKind, WireStyle>>,
    mut lines: Query<(Entity, &WireLine, &Mesh2d, Option<&MeshMaterial2d<BloodWire>>, Option<&MeshMaterial2d<WireLineMaterial>>, &mut LastPoints, &mut Visibility)>,
) {
    let g = &play.game;
    if g.assets.is_none() {
        return;
    }
    let time = g.time.time * 3.0;
    let mut have = std::collections::HashSet::new();
    for (entity, &line, mesh, material, line_material, mut last, mut visibility) in &mut lines {
        have.insert(line);
        let Some(s) = shown(g, line) else {
            *visibility = Visibility::Hidden;
            if matches!(line, WireLine::Wire(w) if w >= g.blood_wires.len()) || matches!(line, WireLine::Root(r) if r >= g.root_wires.len()) {
                commands.entity(entity).despawn();
            }
            continue;
        };
        *visibility = Visibility::Inherited;
        if s.local.is_some() || s.points != last.0 || s.colors != last.1 {
            let _ = meshes.insert(mesh.id(), mesh_of(&s));
            last.0 = s.points.clone();
            last.1 = s.colors;
        }
        if let Some(material) = material {
            let Some(m) = materials.get(material.id()) else { continue };
            let color = s.blood;
            let linear = Color::srgba(color[0], color[1], color[2], color[3]).to_linear().to_vec4();
            if m.settings.flow != s.flow || m.settings.blood != linear || (s.flow != 0.0 && m.settings.time != time) {
                let mut m = materials.get_mut(material.id()).unwrap();
                m.settings.flow = s.flow;
                m.settings.blood = linear;
                m.settings.time = time;
            }
        }
        if let Some(material) = line_material {
            let Some(m) = line_materials.get(material.id()) else { continue };
            let mut settings = m.settings.clone();
            settings.progress = s.progress;
            if settings.mode == 2.0 {
                let look = styles.get(&s.kind).expect("the use wire's style");
                settings.time = g.time.time;
                settings.timestamp = s.timestamp.unwrap_or(look.timestamp);
                settings.glow = s.glow.map_or(look.glow, |c| crate::custom_material::linear([c[0], c[1], c[2], 1.0]));
            }
            if settings != m.settings {
                line_materials.get_mut(material.id()).unwrap().settings = settings;
            }
        }
    }
    let mut spawn = |line: WireLine, index: usize, order: i16, commands: &mut Commands| {
        let Some(s) = shown(g, line) else { return };
        let look = styles.entry(s.kind).or_insert_with(|| style(g, s.kind, &mut images, &mut textures)).clone();
        let mut e = commands.spawn((
            line,
            LastPoints(s.points.clone(), s.colors),
            Mesh2d(meshes.add(mesh_of(&s))),
            Transform::default(),
            s.sorting.map_or_else(|| crate::play::line_key(order, look.queue, index), |(layer, order)| crate::play::sprite_line_key(layer, order, look.queue, index)),
        ));
        if s.kind == WireKind::Blood {
            let settings = WireSettings { st: look.st, blood: Vec4::ZERO, flow: 0.0, bulge: look.bulge, time };
            e.insert(MeshMaterial2d(materials.add(BloodWire { settings, texture: look.texture })));
        } else {
            let mode = match s.kind {
                WireKind::HeatPipe => 1.0,
                WireKind::Use(..) | WireKind::NewUse => 2.0,
                _ => 0.0,
            };
            let glow = match s.glow {
                Some(c) => crate::custom_material::linear([c[0], c[1], c[2], 1.0]),
                None if mode == 2.0 => crate::custom_material::linear(look.glow.to_array()),
                None => look.glow,
            };
            let timestamp = s.timestamp.unwrap_or(look.timestamp);
            let settings = LineSettings { st: look.st, glow, cutoff: look.cutoff, progress: s.progress, mode, time: g.time.time, timestamp, glow_time: look.glow_time };
            e.insert(MeshMaterial2d(line_materials.add(WireLineMaterial { settings, texture: look.texture })));
        }
    };
    for w in 0..g.blood_wires.len() {
        if !have.contains(&WireLine::Wire(w)) {
            spawn(WireLine::Wire(w), w, WIRE_ORDER, &mut commands);
        }
    }
    for r in 0..g.root_wires.len() {
        if !have.contains(&WireLine::Root(r)) {
            spawn(WireLine::Root(r), g.blood_wires.len() + r, WIRE_ORDER, &mut commands);
        }
    }
    let tool = WireLine::Tool(g.wire_tool.kind);
    if !have.contains(&tool) {
        spawn(tool, usize::MAX, g.wire_tool.kind.tool_line().2, &mut commands);
    }
}

/// Linefy's Hidden/Linefy/LinesTransparentWorldspaceXY vertex stage: a quad from A to B, each end its own width.
fn tissue_mesh(lines: &[&(i32, [f32; 2], [f32; 2], f32, f32, [f32; 4])]) -> Mesh {
    let (mut positions, mut colors, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    for &&(_, a, b, width_a, width_b, color) in lines {
        let d = [b[0] - a[0], b[1] - a[1]];
        let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if length == 0.0 {
            continue;
        }
        let side = [d[1] / length, -d[0] / length];
        let at = positions.len() as u32;
        for (p, sign, width) in [(a, -1.0, width_a), (a, 1.0, width_a), (b, 1.0, width_b), (b, -1.0, width_b)] {
            // Lines.widthMultiplier is 0.001; the shader offsets each side by half.
            let half = sign * width * 0.001 * 0.5;
            positions.push([p[0] + side[0] * half, p[1] + side[1] * half, 0.0]);
        }
        colors.extend([color; 4]);
        indices.extend([at, at + 1, at + 3, at + 3, at + 1, at + 2]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; positions.len()]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// The gore strings' Lines: Graphics.DrawMesh on the Default sorting layer in queue 3500 + renderOrder. Every string
/// has its own material in the game; only strings overlapping in one queue can tell these apart.
pub fn tissue(
    mut commands: Commands,
    play: Res<Play>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut line_materials: ResMut<Assets<WireLineMaterial>>,
    mut drawn: Local<std::collections::HashMap<i32, (Handle<Mesh>, Entity)>>,
) {
    if play.is_added() {
        drawn.clear();
    }
    let lines = play.game.gore_lines();
    for (&order, (mesh, entity)) in drawn.iter() {
        let own: Vec<_> = lines.iter().filter(|l| l.0 == order).collect();
        commands.entity(*entity).insert(if own.is_empty() { Visibility::Hidden } else { Visibility::Inherited });
        if !own.is_empty() {
            let _ = meshes.insert(mesh.id(), tissue_mesh(&own));
        }
    }
    for line in &lines {
        if !drawn.contains_key(&line.0) {
            let own: Vec<_> = lines.iter().filter(|l| l.0 == line.0).collect();
            let mesh = meshes.add(tissue_mesh(&own));
            let settings = LineSettings { st: Vec4::new(1.0, 1.0, 0.0, 0.0), glow: Vec4::ZERO, cutoff: 0.0, progress: 0.0, mode: 0.0, time: 0.0, timestamp: 0.0, glow_time: 1.0 };
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(line_materials.add(WireLineMaterial { settings, texture: Handle::default() })),
                    Transform::default(),
                    NoFrustumCulling,
                    crate::play::sprite_line_key(0, 0, (3500 + line.0) as i16, usize::MAX),
                    crate::play::MainScene,
                ))
                .id();
            drawn.insert(line.0, (mesh, entity));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_curve_clamps_and_blends() {
        let c = WidthCurve(vec![[0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
        assert_eq!((c.evaluate(-1.0), c.evaluate(0.0), c.evaluate(1.0), c.evaluate(2.0)), (1.0, 1.0, 0.0, 0.0));
        assert!((c.evaluate(0.5) - 0.5).abs() < 1e-6);
        assert_eq!(WidthCurve::constant(2.0).evaluate(0.3), 2.0);
    }

    fn uvs(mesh: &Mesh) -> &[[f32; 2]] {
        let bevy::mesh::VertexAttributeValues::Float32x2(uv) = mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap() else { panic!() };
        uv
    }

    fn positions(mesh: &Mesh) -> &[[f32; 3]] {
        let bevy::mesh::VertexAttributeValues::Float32x3(p) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() else { panic!() };
        p
    }

    #[test]
    fn corner_vertices_sweep_an_arc_and_a_loop_closes() {
        let c = WidthCurve::constant(1.0);
        let corner = line_mesh_full(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], &[], 0.1, &c, None, 0, 2, false, LineTexture::Tile);
        // Two end vertices, and the corner's 2 + 1 steps in between.
        assert_eq!(positions(&corner).len(), 2 + 3 * 2 + 2);
        // Turned off the axes: an extrusion direction with x = 0 gives the engine's polynomial atan2 a NaN (1 / 0 refined).
        let turn = |p: [f32; 2]| [p[0] * 0.8 - p[1] * 0.6, p[0] * 0.6 + p[1] * 0.8];
        let square = [turn([0.0, 0.0]), turn([1.0, 0.0]), turn([1.0, 1.0]), turn([0.0, 1.0])];
        let looped = line_mesh_full(&square, &[], 0.1, &c, None, 0, 2, true, LineTexture::Tile);
        let p = positions(&looped);
        // The first vertex gives only the end of its sweep, the other four the 3 steps.
        assert_eq!(p.len(), 2 * (1 + 4 * 3));
        assert_eq!((p[0], p[1]), (p[p.len() - 2], p[p.len() - 1]));
        // Every vertex is within half a width of the square's outline plus the arc's reach.
        assert!(p.iter().all(|v| v[0] > -0.7 && v[0] < 0.9 && v[1] > -0.1 && v[1] < 1.5 && v[0].is_finite() && v[1].is_finite()), "{p:?}");
        // The distance along the loop ends at its perimeter.
        assert!((uvs(&looped)[p.len() - 1][0] - 4.0).abs() < 1e-5);
    }

    #[test]
    fn a_loop_and_its_corners_have_bakemeshs_vertex_count_and_a_positive_z_flips_the_sides() {
        let c = WidthCurve::constant(1.0);
        let pts = [[0.0, 0.0], [1.0, 0.3], [1.5, 1.0], [0.2, 1.1], [-0.4, 0.6]];
        for corner in [0usize, 1, 3] {
            // BakeMesh: 2 * n + 2 * corner * (n - 2) for a line, 2 * (n + 1) + 2 * corner * n for a loop (n positions).
            let n = pts.len();
            let line = line_mesh_full(&pts, &[], 0.1, &c, None, 0, corner, false, LineTexture::Tile);
            assert_eq!(positions(&line).len(), 2 * n + 2 * corner * (n - 2), "line corner {corner}");
            let looped = line_mesh_full(&pts, &[], 0.1, &c, None, 0, corner, true, LineTexture::Tile);
            assert_eq!(positions(&looped).len(), 2 * (n + 1) + 2 * corner * n, "loop corner {corner}");
        }
        // Caps add 4 * cap + 8.
        let capped = line_mesh_full(&pts, &[], 0.1, &c, None, 3, 0, false, LineTexture::Tile);
        assert_eq!(positions(&capped).len(), 2 * pts.len() + 4 * 3 + 8);
        let (front, behind) = ([0.0; 5], [1.0; 5]);
        let a = line_mesh_full(&pts, &front, 0.1, &c, None, 2, 2, false, LineTexture::Tile);
        let b = line_mesh_full(&pts, &behind, 0.1, &c, None, 2, 2, false, LineTexture::Tile);
        assert_ne!(positions(&a), positions(&b));
    }

    #[test]
    fn a_straight_line_is_two_caps_and_its_two_ends_and_tiles_by_distance() {
        let mesh = line_mesh(&[[0.0, 0.0], [2.0, 0.0]], 0.05, 6, LineTexture::Tile);
        let uv = uvs(&mesh);
        assert_eq!(uv.len(), 16 + 2 + 2 + 16);
        assert_eq!(uv.iter().map(|t| t[0]).fold(0.0, f32::max), 2.0);
        let p = positions(&mesh);
        // The caps bulge outwards, away from the line.
        assert!(p.iter().take(16).all(|v| v[0] <= 1e-6) && p.iter().skip(20).all(|v| v[0] >= 2.0 - 1e-6));
        assert!(p.iter().take(16).any(|v| v[0] < -0.02) && p.iter().skip(20).any(|v| v[0] > 2.02));
        assert_eq!((uv[16][1], uv[17][1]), (1.0, 0.0));
    }

    #[test]
    fn a_corner_meets_at_the_miter_and_a_straight_point_keeps_the_width() {
        let mesh = line_mesh(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], 0.05, 6, LineTexture::Tile);
        let p = positions(&mesh);
        let (a, b) = (p[18], p[19]);
        assert_eq!(uvs(&mesh)[18], [1.0, 1.0]);
        assert!(((a[0] - b[0]).hypot(a[1] - b[1]) - 0.05 * 2f32.sqrt()).abs() < 1e-3, "{a:?} {b:?}");
        let mesh = line_mesh(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], 0.05, 6, LineTexture::Tile);
        let p = positions(&mesh);
        assert!(((p[18][1] - p[19][1]).abs() - 0.05).abs() < 1e-6 && p[18][0] == 1.0);
    }
}
