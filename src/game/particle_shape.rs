//! The native ShapeModule (UnityPlayer 2020.3, read in the disassembly): ShapeModule::Start's templates, its
//! EmitterStoreData and ShapeModule::Update's timers. Every template runs batches of four lanes on the module's own
//! RandN (m_Random, ResetSeed 0x180dfe1d0), the lanes past the count drawing too.
#![cfg_attr(not(test), allow(dead_code))]
use crate::assets::particles::ParticleSystem;
use crate::game::particle_rand::RandN;
use crate::game::particle_sim::{evaluate, inverse, maxps, minps};

const INV_2PI: u32 = 0x3e22f983;

/// The module code's inline sincos polynomial (every site in ShapeModule): x = angle / 2pi, a quarter turn folded
/// with cvtps2dq, then p(s) * s.
fn poly(s: f32) -> f32 {
    let (c1, c2, c3, c4, c5) = [0xc2992322, 0x42a33422, 0xc2255ddc, 0x40c90fda, 0x421ea0cd].map(f32::from_bits).into();
    let x2 = s * s;
    let x4 = x2 * x2;
    (((c1 * x2 + c2) * x4 + (c3 * x2 + c4)) + (x4 * x4) * c5) * s
}

/// (sin, cos) of `angle`.
pub fn sincos(angle: f32) -> (f32, f32) {
    let x = angle * f32::from_bits(INV_2PI);
    let c = 0.25 - (x - x.round_ties_even()).abs();
    let y = x + -0.25;
    let s = 0.25 - (y - y.round_ties_even()).abs();
    (poly(s), poly(c))
}

/// The module code's normalize: rsqrtps and one Newton step; `default` at |v|^2 <= 1e-30.
pub fn normalize(v: [f32; 3], default: [f32; 3]) -> [f32; 3] {
    let sq = (v[1] * v[1] + v[2] * v[2]) + v[0] * v[0];
    let r = minps(crate::unity::sse::rsqrt(sq) * f32::from_bits(0x3f7ffffd), f32::MAX);
    let n = (f32::from_bits(0x40400001) - (r * sq) * r) * (0.5 * r);
    if f32::from_bits(0x0da24260) < sq { v.map(|c| c * n) } else { default }
}

/// rcpps and two Newton steps, the estimate itself when they give NaN.
fn inverse_nan(a: f32) -> f32 {
    let r0 = crate::unity::sse::rcp(a);
    let r1 = (f32::from_bits(0x40000002) - r0 * a) * r0;
    let r2 = r1 * (2.0 - r1 * a);
    if r2 == r2 { r2 } else { r0 }
}

fn floor(q: f32) -> f32 {
    let f = q.round_ties_even();
    if q < f { f - 1.0 } else { f }
}

fn ceil(n: f32) -> f32 {
    let c = n.round_ties_even();
    if c < n { c + 1.0 } else { c }
}

/// cvttps2dq then back.
fn trunc(x: f32) -> f32 {
    if x.is_nan() || x.abs() >= 2147483648.0 { -2147483648.0 } else { x.trunc() }
}

/// RandomUnitVector (0x180dfdb70): two draws, the angle and z.
fn random_unit_vector(rand: &mut RandN) -> [[f32; 3]; 4] {
    let (u1, u2) = (rand.get01(), rand.get01());
    std::array::from_fn(|i| {
        let z = u2[i] * 2.0 - 1.0;
        let s = (1.0 - z * z).sqrt();
        let (sin, cos) = sincos(u1[i] * f32::from_bits(0x40c90fdb));
        [cos * s, sin * s, z]
    })
}

/// a0 * x + (a1 * y + a2 * z) (+ t inside, as EmitterStoreData adds it).
fn mul3(a: &[[f32; 3]; 3], v: [f32; 3], t: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[0][k] * v[0] + (a[1][k] * v[1] + (a[2][k] * v[2] + t[k])))
}

fn mul3_no_t(a: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[0][k] * v[0] + (a[1][k] * v[1] + a[2][k] * v[2]))
}

/// The emission matrix's columns (localToWorldEmission in world space, identity in local) and its translation.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub m: [[f32; 3]; 3],
    pub t: [f32; 3],
}

/// What a shape writes for one particle: the position (the frame's translation plus the shape's) and the velocity
/// direction StartVelocity multiplies by the start speed.
pub type Emitted = ([f32; 3], [f32; 3]);

/// The shape's own state: m_Random and the radius and arc timers ShapeModule::Update advances.
#[derive(Clone, Debug)]
pub struct State {
    pub rand: RandN,
    pub radius_timer: [f32; 2],
    pub arc_timer: [f32; 2],
    /// The last `start`'s texture colours, one per particle (empty without a textured Rectangle): ApplyTexture
    /// multiplies each new particle's colour by its own.
    pub tint: Vec<u32>,
    /// A SpriteRenderer shape's cached sprite mesh (None: no renderer or sprite, so Start writes nothing).
    pub mesh: Option<std::sync::Arc<Mesh>>,
}

impl State {
    /// ShapeModule::ResetSeed: RandN::SetSeed([randomSeed; 4]), the timers and their previous values zeroed.
    pub fn new(random_seed: u32) -> State {
        State { rand: RandN::new([random_seed; 4]), radius_timer: [0.0; 2], arc_timer: [0.0; 2], tint: Vec::new(), mesh: None }
    }

    /// ShapeModule::Update (0x180dffe80) at the system's normalized time `t` for a step of `dt`: each timer's previous
    /// value, then speed(t, 1) * dt added.
    pub fn update(&mut self, ps: &ParticleSystem, t: f32, dt: f32) {
        self.radius_timer = [evaluate(&ps.shape_radius.speed, t, 1.0) * dt + self.radius_timer[0], self.radius_timer[0]];
        self.arc_timer = [evaluate(&ps.shape_arc.speed, t, 1.0) * dt + self.arc_timer[0], self.arc_timer[0]];
    }
}

/// m_Rotation (degrees) as the module code's Euler quaternion (kRotationOrderLUT's ZXY rows), not normalized: the
/// lanes (sx cz, sx sz, cx sz, cx cz) times sy (-1, 1, 1, 1) plus the swapped cy (1, -1, 1, 1) of the half angles.
pub fn euler_quat(rotation: [f32; 3]) -> [f32; 4] {
    let h = rotation.map(|d| d * f32::from_bits(0x3c8efa35) * 0.5);
    let [(sx, cx), (sy, cy), (sz, cz)] = h.map(sincos);
    let a = [sx * cz, sx * sz, cx * sz, cx * cz];
    let p = [-sy * a[0], sy * a[1], sy * a[2], sy * a[3]];
    let q = [cy * a[0], -cy * a[1], cy * a[2], cy * a[3]];
    [p[2] + q[0], p[3] + q[1], p[0] + q[2], p[1] + q[3]]
}

/// ShapeModule::Start's shape matrix (0x180dfe220): m_Rotation (degrees) as the Euler quaternion, its columns times
/// m_Scale, m_Position, all times emitterScale.
fn shape_matrix(ps: &ParticleSystem, emitter_scale: [f32; 3]) -> ([[f32; 3]; 3], [f32; 3]) {
    let [x, y, z, w] = euler_quat(ps.shape_rotation);
    let col0 = [1.0 + ((-2.0 * y) * y + (-2.0 * z) * z), 0.0 + ((2.0 * y) * x + (2.0 * z) * w), 0.0 + ((-2.0 * y) * w + (2.0 * z) * x)];
    let col1 = [0.0 + ((-2.0 * z) * w + (2.0 * x) * y), 1.0 + ((-2.0 * z) * z + (-2.0 * x) * x), 0.0 + ((2.0 * z) * y + (2.0 * x) * w)];
    let col2 = [0.0 + ((2.0 * x) * z + (2.0 * y) * w), 0.0 + ((-2.0 * x) * w + (2.0 * y) * z), 1.0 + ((-2.0 * x) * x + (-2.0 * y) * y)];
    let s = ps.shape_scale;
    let cols = [col0.map(|c| s[0] * c), col1.map(|c| s[1] * c), col2.map(|c| s[2] * c)];
    // diag(emitterScale) times each column: e_x * c.x + (e_y-row * c.y + e_z-row * c.z), the zero lanes included.
    let e = emitter_scale;
    let diag = |c: [f32; 3]| -> [f32; 3] {
        [e[0] * c[0] + (0.0 * c[1] + 0.0 * c[2]), 0.0 * c[0] + (e[1] * c[1] + 0.0 * c[2]), 0.0 * c[0] + (0.0 * c[1] + e[2] * c[2])]
    };
    let t = ps.shape_position;
    let tt = [e[0] * t[0] + (0.0 * t[2] + 0.0 * t[1]), 0.0 * t[0] + (0.0 * t[2] + e[1] * t[1]), 0.0 * t[0] + (e[2] * t[2] + 0.0 * t[1])];
    (cols.map(diag), tt)
}

/// EmitterStoreData (0x180dfc710) for the lanes of one batch: the direction normalized, randomDirection, then
/// sphericalDirection and randomPosition (each a RandomUnitVector draw when on), the shape matrix, the emission
/// matrix. alignToDirection and the axis of rotation are not rewritten (no game system turns them on).
fn store(ps: &ParticleSystem, frame: &Frame, a: &([[f32; 3]; 3], [f32; 3]), rand: &mut RandN, pos: [[f32; 3]; 4], dir: [[f32; 3]; 4], out: &mut Vec<Emitted>, lanes: usize) {
    if ps.shape_align_to_direction {
        unimplemented!("mod-only: ShapeModule alignToDirection (EmitterStoreData's 3D rotation branch): no game system uses it");
    }
    let mut dir = dir.map(|d| normalize(d, [0.0, 0.0, 1.0]));
    let mut pos = pos;
    if ps.shape_random_direction > 0.0 {
        let u = random_unit_vector(rand);
        let k = ps.shape_random_direction;
        dir = std::array::from_fn(|i| std::array::from_fn(|c| (u[i][c] - dir[i][c]) * k + dir[i][c]));
    }
    if ps.shape_spherical_direction > 0.0 {
        let k = ps.shape_spherical_direction;
        dir = std::array::from_fn(|i| {
            let n = normalize(pos[i], [0.0, 0.0, 1.0]);
            std::array::from_fn(|c| (n[c] - dir[i][c]) * k + dir[i][c])
        });
    }
    if ps.shape_random_position > 0.0 {
        let u = random_unit_vector(rand);
        let k = ps.shape_random_position;
        pos = std::array::from_fn(|i| std::array::from_fn(|c| u[i][c] * k + pos[i][c]));
    }
    for i in 0..lanes {
        let d = normalize(mul3_no_t(&a.0, dir[i]), [0.0, 0.0, 1.0]);
        let p = mul3_no_t(&frame.m, mul3(&a.0, pos[i], a.1));
        out.push(([0, 1, 2].map(|c| frame.t[c] + p[c]), mul3_no_t(&frame.m, d)));
    }
}

/// ShapeModule::Start for `n` new particles: the shape's position and direction of each. `radius` and `arc` are
/// the module's values after the scripts' writes; `emission` is the emission state's (toEmit, spacing) the PingPong
/// modes spread the batch with. Some(..) only when the module is on.
pub fn start(ps: &ParticleSystem, st: &mut State, frame: &Frame, emitter_scale: [f32; 3], radius: f32, arc: f32, emission: (f32, f32), n: usize) -> Vec<Emitted> {
    let a = shape_matrix(ps, emitter_scale);
    let mut out = Vec::with_capacity(n);
    st.tint.clear();
    let thick = ps.shape_radius_thickness;
    let arc_rad = arc * f32::from_bits(0x3c8efa35);
    // The PingPong lanes' toEmit + lane, 4 added per batch.
    let mut idx = [0.0, 1.0, 2.0, 3.0].map(|i| emission.0 + i);
    for b in (0..n).step_by(4) {
        let lanes = (n - b).min(4);
        let f = idx.map(|i| minps(1.0, maxps(0.0, emission.1 * i)));
        idx = idx.map(|i| i + 4.0);
        let (pos, dir): ([[f32; 3]; 4], [[f32; 3]; 4]) = match ps.shape_type {
            // Sphere<0> (0x180df5ce0): angle, z, then the radius from cbrt((1 - u) + (1 - thickness)^3 u).
            0 if ps.shape_arc.mode == 0 => {
                // expf(logf(1 - thickness) * 3) (0x18159bb90, 0x181599e70): exact at the two ends the game uses.
                let k3 = match thick {
                    1.0 => 0.0,
                    0.0 => 1.0,
                    _ => unimplemented!("mod-only: Sphere radiusThickness {thick}: the CRT logf/expf (0x181599e70, 0x18159bb90) are not rewritten"),
                };
                let step = ps.shape_arc.spread * arc_rad;
                let angle = snapped(&mut st.rand, arc_rad, step);
                let z = st.rand.get01().map(|u| u * 2.0 - 1.0);
                let r = st.rand.get01();
                let v: [[f32; 3]; 4] = std::array::from_fn(|i| {
                    let s = (1.0 - z[i] * z[i]).sqrt();
                    let (sin, cos) = sincos(angle[i]);
                    [cos * s, sin * s, z[i]]
                });
                let rad: [f32; 4] = std::array::from_fn(|i| cube_root((1.0 - r[i]) * 1.0 + k3 * r[i]) * radius);
                (std::array::from_fn(|i| v[i].map(|c| rad[i] * c)), v)
            }
            // Box (inline in ShapeModule::Start): three draws, each - 0.5; the direction +z.
            5 => {
                let (x, y, z) = (st.rand.get01(), st.rand.get01(), st.rand.get01());
                (std::array::from_fn(|i| [x[i] - 0.5, y[i] - 0.5, z[i] - 0.5]), [[0.0, 0.0, 1.0]; 4])
            }
            // Circle<0> (0x180de8ca0) and Circle<2> (PingPong, 0x180de7de0).
            10 if matches!(ps.shape_arc.mode, 0 | 2) => {
                let step = ps.shape_arc.spread * arc_rad;
                let t2 = (1.0 - thick) * (1.0 - thick);
                let angle = if ps.shape_arc.mode == 0 {
                    snapped(&mut st.rand, arc_rad, step)
                } else {
                    let (cur, prev) = (2.0 * (st.arc_timer[0] * std::f32::consts::PI), 2.0 * (st.arc_timer[1] * std::f32::consts::PI));
                    let inv = inverse_nan(arc_rad);
                    std::array::from_fn(|i| ping_pong(f[i], cur, prev, step, inv) * arc_rad)
                };
                let u = st.rand.get01();
                let v: [([f32; 3], [f32; 3]); 4] = std::array::from_fn(|i| {
                    let rad = (u[i] * (1.0 - t2) + t2).sqrt() * radius;
                    let (sin, cos) = sincos(angle[i]);
                    ([cos * rad, sin * rad, 0.0], [cos, sin, 0.0])
                });
                (v.map(|v| v.0), v.map(|v| v.1))
            }
            // SingleSidedEdge<0> (0x180df3cd0) and <2> (PingPong, 0x180df34b0, no draw).
            12 if matches!(ps.shape_radius.mode, 0 | 2) => {
                let step = radius * ps.shape_radius.spread;
                let x: [f32; 4] = if ps.shape_radius.mode == 0 {
                    snapped(&mut st.rand, radius, step).map(|v| 2.0 * v + -radius)
                } else {
                    let inv = inverse(radius);
                    std::array::from_fn(|i| 2.0 * (ping_pong(f[i], st.radius_timer[0], st.radius_timer[1], step, inv) * radius) + -radius)
                };
                (x.map(|x| [x, 0.0, 0.0]), [[0.0, 1.0, 0.0]; 4])
            }
            // Rectangle<0> (0x180df2e70); <1> samples the texture.
            18 if !ps.shape_texture => {
                let (x, y) = (st.rand.get01(), st.rand.get01());
                (std::array::from_fn(|i| [x[i] - 0.5, y[i] - 0.5, 0.0]), [[0.0, 0.0, 1.0]; 4])
            }
            // Rectangle<1> (0x180df2b70): Rectangle<0>'s draws, then ApplyTexture (0x180dfaae0) samples at them.
            18 => {
                let px = ps.shape_pixels.as_deref().expect("a textured Rectangle's pixels (the loader sets them)");
                let (x, y) = (st.rand.get01(), st.rand.get01());
                st.tint.extend((0..lanes).map(|i| texel(&ps.shape_texture_params, px, x[i], y[i])));
                (std::array::from_fn(|i| [x[i] - 0.5, y[i] - 0.5, 0.0]), [[0.0, 0.0, 1.0]; 4])
            }
            // EmitterStoreMeshData (0x180de0fa0, either colour template) for Triangle and Edge with mode Random: one
            // draw, then GetPositionMesh. A sprite's mesh has no normals or colours, so the normal is +z and the
            // colour the default material colour (white), which leaves the particle's colour as it is.
            20 => {
                let m = st.mesh.as_deref().expect("a SpriteRenderer shape's mesh (the caller takes the InitialModule path without)");
                if ps.shape_texture || ps.shape_mesh_spawn.0 != 0 || ps.shape_mesh_spawn.1 > 0.0 || !matches!(ps.shape_placement, 1 | 2) {
                    unimplemented!("mod-only: SpriteRenderer shape with texture {}, spawn mode {} spread {}, placement {}: no game system uses it", ps.shape_texture, ps.shape_mesh_spawn.0, ps.shape_mesh_spawn.1, ps.shape_placement);
                }
                let draw = st.rand.get01();
                mesh_point(m, &mut st.rand, ps.shape_placement == 1, draw, ps.shape_normal_offset)
            }
            t => unimplemented!("mod-only: ShapeModule::Start of shape type {t} (radius mode {}, arc mode {})", ps.shape_radius.mode, ps.shape_arc.mode),
        };
        store(ps, frame, &a, &mut st.rand, pos, dir, &mut out, lanes);
    }
    out
}

/// ShapeModule::UpdateCachedMesh (0x180dfffa0) of a sprite: each triangle's TriangleArea3D (0x180647130) and corners,
/// the total, and the lookup of min(triangles, 50) entries, entry i holding the area before the first triangle whose
/// running sum passes i * (total / entries).
#[derive(Debug)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<(f32, [u32; 3])>,
    pub lut: Vec<(f32, u32)>,
    pub total: f32,
}

impl Mesh {
    /// None without a triangle (m_CachedTriangleData empty: ShapeModule::Start writes nothing).
    pub fn new(positions: Vec<[f32; 3]>, indices: &[u32]) -> Option<Mesh> {
        let area = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| {
            let (e1, e2) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let (x, y, z) = (e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]);
            ((y * y + x * x) + z * z).sqrt() * 0.5
        };
        let p = |i: u32| positions[i as usize];
        let triangles: Vec<(f32, [u32; 3])> = indices.chunks_exact(3).map(|t| (area(p(t[0]), p(t[1]), p(t[2])), [t[0], t[1], t[2]])).collect();
        if triangles.is_empty() {
            return None;
        }
        let total = 0.0 + triangles.iter().fold(0.0, |a, t| a + t.0);
        let n = triangles.len().min(50);
        let step = total / n as f32;
        let (mut tri, mut acc) = (0, 0.0);
        let lut = (0..n)
            .map(|i| {
                let threshold = i as f32 * step;
                while tri < triangles.len() && acc + triangles[tri].0 <= threshold {
                    acc += triangles[tri].0;
                    tri += 1;
                }
                (acc, tri as u32)
            })
            .collect();
        Some(Mesh { positions, triangles, lut, total })
    }
}

/// GetPositionMesh<2,0> (Triangle, 0x180de58c0) and <2,1> (Edge, 0x180de3a20) of a mesh without normals or
/// colours: each lane's triangle from the lookup entry nearest draw * entries, walked to draw * total (Edge keeps
/// the fraction inside it); Triangle then draws two barycentrics (folded when their sum passes 1), Edge spreads
/// the fraction * 3 over the three edges. The normal (+z through the same weights) is the direction; the position
/// moves along it by normalOffset.
pub fn mesh_point(m: &Mesh, rand: &mut RandN, edge: bool, draw: [f32; 4], offset: f32) -> ([[f32; 3]; 4], [[f32; 3]; 4]) {
    let n = m.lut.len() as f32;
    let pick = |d: f32| -> (usize, f32) {
        let target = d * m.total;
        let entry = trunc(minps((d * n).round_ties_even(), n - 1.0)) as usize;
        let (mut acc, t) = m.lut[entry];
        if target >= acc {
            for t in t as usize..m.triangles.len() {
                acc += m.triangles[t].0;
                if acc >= target {
                    return (t, (target - (acc - m.triangles[t].0)) / m.triangles[t].0);
                }
            }
        } else {
            for r in (0..t as usize).rev() {
                acc -= m.triangles[r].0;
                if target > acc {
                    return (r, (target - (m.triangles[r].0 + acc)) / m.triangles[r].0 + 1.0);
                }
            }
        }
        (0, 0.0)
    };
    let picked = draw.map(pick);
    let weights: [[f32; 3]; 4] = if edge {
        let clamp = |x: f32| minps(1.0, maxps(0.0, x));
        picked.map(|(_, f)| {
            let t = f * 3.0;
            let (c0, c1, c2) = (clamp(t), clamp(t - 1.0), clamp(t - 2.0));
            [clamp(1.0 - t) + c2, c0 - c1, c1 - c2]
        })
    } else {
        let (r1, r2) = (rand.get01(), rand.get01());
        std::array::from_fn(|i| {
            let (a, b) = if 1.0 < r2[i] + r1[i] { (1.0 - r1[i], 1.0 - r2[i]) } else { (r1[i], r2[i]) };
            [a, b, (1.0 - a) - b]
        })
    };
    let mut pos = [[0.0; 3]; 4];
    let mut normal = [[0.0; 3]; 4];
    for i in 0..4 {
        let [w0, w1, w2] = weights[i];
        let [p0, p1, p2] = m.triangles[picked[i].0].1.map(|v| m.positions[v as usize]);
        let nz = [0.0, 0.0, 1.0];
        let p: [f32; 3] = std::array::from_fn(|c| w2 * p2[c] + (w1 * p1[c] + w0 * p0[c]));
        normal[i] = std::array::from_fn(|c| w2 * nz[c] + (w1 * nz[c] + w0 * nz[c]));
        pos[i] = std::array::from_fn(|c| p[c] + normal[i][c] * offset);
    }
    (pos, normal)
}

/// ApplyTexture's sample (0x180dfa3f0) at draws (u, v): the texel at min(u, 0.9999) * width (bilinear: less 0.5,
/// at least 0, the four texels blended by the fraction * 255 in two 0xff00ff halves), then the clip (alpha 0 where
/// the clip channel is under threshold * 256), RGB 255 unless colorAffectsParticles, alpha 255 unless
/// alphaAffectsParticles. ApplyTexture's clip also sets those particles' aliveTimePercent to 100.00001, but
/// StartModules (0x1801c35f0) writes it again after ShapeModule::Start, so that write never shows.
pub fn texel(t: &crate::assets::particles::ShapeTexture, (data, w, h): &(Vec<u32>, i32, i32), u: f32, v: f32) -> u32 {
    let (w, h) = (*w, *h);
    let (mut fu, mut fv) = (w as f32 * minps(u, f32::from_bits(0x3f7ff972)), h as f32 * minps(v, f32::from_bits(0x3f7ff972)));
    if t.bilinear {
        (fu, fv) = (maxps(fu - 0.5, 0.0), maxps(fv - 0.5, 0.0));
    }
    let (ix, iy) = (fu as i32, fv as i32);
    let at = |x: i32, y: i32| data[y.wrapping_mul(w).wrapping_add(x) as usize];
    let c = if t.bilinear {
        let (ix1, iy1) = ((ix + 1).min(w - 1), (iy + 1).min(h - 1));
        // floor by cvtps2dq: the rounded value, less 1 where it is above.
        let fl = |f: f32| {
            let r = f.round_ties_even();
            r - if f < r { 1.0 } else { 0.0 }
        };
        let (wx, wy) = (((fu - fl(fu)) * 255.0) as i32 as u32, ((fv - fl(fv)) * 255.0) as i32 as u32);
        let lerp = |a: u32, b: u32, k: u32| {
            let lo = (((b & 0xff00ff).wrapping_sub(a & 0xff00ff).wrapping_mul(k) >> 8).wrapping_add(a & 0xff00ff)) & 0xff00ff;
            let hi = (((b >> 8) & 0xff00ff).wrapping_sub((a & 0xff00ff00) >> 8).wrapping_mul(k).wrapping_add(a & 0xff00ff00)) & 0xff00ff00;
            hi | lo
        };
        lerp(lerp(at(ix, iy), at(ix1, iy), wx), lerp(at(ix, iy1), at(ix1, iy1), wx), wy)
    } else {
        at(ix, iy)
    };
    let mut b = c.to_le_bytes();
    let threshold = (t.clip_threshold * 256.0) as i32;
    if (b[t.clip_channel as usize] as i32) < threshold {
        b[3] = 0;
    } else {
        if !t.color_affects {
            b[..3].fill(255);
        }
        if !t.alpha_affects {
            b[3] = 255;
        }
    }
    u32::from_le_bytes(b)
}

/// ApplyTexture's colour multiply: per byte (c * p + 0x80 + ((c * p + 0x80) >> 8)) >> 8.
pub fn tint(color: u32, pixel: u32) -> u32 {
    let (c, p) = (color.to_le_bytes(), pixel.to_le_bytes());
    u32::from_le_bytes(std::array::from_fn(|i| {
        let t = c[i] as u32 * p[i] as u32 + 0x80;
        ((t + (t >> 8)) >> 8) as u8
    }))
}

/// One draw, spread in steps of `step` when it is above 0: floor((r * (step * ceil(range / step))) / step) * step.
fn snapped(rand: &mut RandN, range: f32, step: f32) -> [f32; 4] {
    let r = rand.get01();
    if step > 0.0 {
        let c = ceil(range / step);
        r.map(|r| floor((r * (c * step)) / step) * step)
    } else {
        r.map(|r| r * range)
    }
}

/// The PingPong fraction 0..1 between the timer's previous and current values.
fn ping_pong(f: f32, cur: f32, prev: f32, step: f32, inv: f32) -> f32 {
    let mut v = f * (prev - cur) + cur;
    if step > 0.0 {
        v = floor(v / step) * step;
    }
    let u = v * inv;
    let m = (u - trunc(u / 2.0) * 2.0).abs();
    let w = if 1.0 <= m { (2.0 - m) - f32::from_bits(0x358637bd) } else { m };
    w - floor(w)
}

/// Sphere<0>'s inline cube root: log2 of the mantissa and exponent as a cubic, / 3, then 2^x as a quadratic.
fn cube_root(t: f32) -> f32 {
    let f = |b: u32| f32::from_bits(b);
    let bits = t.to_bits();
    let m = f32::from_bits((bits & 0x807fffff) | 0x3f800000) - 1.0;
    let e = ((bits as i32) >> 23) as f32 - 127.0;
    let l = (m * m) * (f(0x3e470bd9) * m + f(0xbf21dda4)) + (e + f(0x3fb80d57) * m);
    let y = maxps(l * f(0x3eaaaaab), f(0xc2fe0000));
    let fl = floor(y);
    let frac = y - fl;
    let pow = f32::from_bits(((trunc(fl) as i32).wrapping_add(0x7f) as u32) << 23);
    ((frac * frac) * f(0x3ea2ad7f) + (f(0x3f2ea941) * frac + 1.0)) * pow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_maths_match_the_module_code() {
        // The polynomial is the module's sin/cos to about 1e-6.
        for a in [0.0f32, 0.3, 1.0, 2.5, -1.7, 6.0] {
            let (s, c) = sincos(a);
            assert!((s - (a as f64).sin() as f32).abs() < 2e-6 && (c - (a as f64).cos() as f32).abs() < 2e-6, "{a}");
        }
        assert_eq!(normalize([0.0; 3], [0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]);
        let n = normalize([3.0, 4.0, 0.0], [0.0; 3]);
        assert!((n[0] - 0.6).abs() < 1e-6 && (n[1] - 0.8).abs() < 1e-6);
        for t in [1.0f32, 0.5, 0.125, 0.9] {
            assert!((cube_root(t) - (t as f64).cbrt() as f32).abs() < 5e-3, "{t}");
        }
        // The cube root is the module's rough one (about 4e-3 off at 0.5).
        // PingPong runs 0 → 1 → 0 over two units of the timer.
        assert!((ping_pong(0.0, 0.25, 0.0, 0.0, 1.0) - 0.25).abs() < 1e-6);
        assert!((ping_pong(0.0, 1.5, 0.0, 0.0, 1.0) - 0.5).abs() < 1e-5);
    }

    /// The cached sprite mesh and GetPositionMesh against UnityPlayer.dll's (particle_mesh_dll.rs).
    #[test]
    fn mesh_dll() {
        type C = (bool, &'static [[u32; 3]], &'static [u32], [u32; 16], [u32; 4], u32, [[u32; 3]; 4], [[u32; 3]; 4], [u32; 16]);
        let cases: &[C] = &include!("particle_mesh_dll.rs");
        let lanes = |s: &[u32; 16]| RandN([0, 1, 2, 3].map(|l| crate::unity::Random { state: [s[l], s[4 + l], s[8 + l], s[12 + l]] }));
        for (n, (edge, positions, indices, state, draw, offset, pos, normal, after)) in cases.iter().enumerate() {
            let m = Mesh::new(positions.iter().map(|p| p.map(f32::from_bits)).collect(), indices).unwrap();
            let mut rand = lanes(state);
            let (p, d) = mesh_point(&m, &mut rand, *edge, draw.map(f32::from_bits), f32::from_bits(*offset));
            assert_eq!(p.map(|v| v.map(f32::to_bits)), *pos, "case {n} positions");
            assert_eq!(d.map(|v| v.map(f32::to_bits)), *normal, "case {n} normals");
            assert_eq!(rand.0.map(|r| r.state), lanes(after).0.map(|r| r.state), "case {n} state");
        }
    }

    /// ApplyTexture's sample and multiply against UnityPlayer.dll's (particle_texture_dll.rs).
    #[test]
    fn texture_dll() {
        type C = (i32, i32, &'static [u32], (i32, u32, bool, bool, bool), [u32; 4], [u32; 4], [u32; 4], [u32; 4]);
        let cases: &[C] = &include!("particle_texture_dll.rs");
        for (n, (w, h, px, (clip_channel, thr, color_affects, alpha_affects, bilinear), u, v, cin, cout)) in cases.iter().enumerate() {
            let t = crate::assets::particles::ShapeTexture { clip_channel: *clip_channel, clip_threshold: f32::from_bits(*thr), color_affects: *color_affects, alpha_affects: *alpha_affects, bilinear: *bilinear };
            let data = (px.to_vec(), *w, *h);
            for l in 0..4 {
                let got = tint(cin[l], texel(&t, &data, f32::from_bits(u[l]), f32::from_bits(v[l])));
                assert_eq!(got, cout[l], "case {n} lane {l}");
            }
        }
    }

}
