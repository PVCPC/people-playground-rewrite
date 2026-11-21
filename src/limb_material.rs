//! Jaap/BodyShader and Jaap/LegacyBodyShader: skin, flesh and bone with the wounds, burns and blood over them.
//! SkinMaterialHandler.OnWillRenderObject and Sync, for the material of each limb.
use crate::{game::container::Owner, play::{LimbSprite, Play}};
use bevy::{asset::{embedded_asset, RenderAssetUsages}, prelude::*, reflect::TypePath, render::{render_resource::{AsBindGroup, ShaderType}, storage::{ShaderBuffer, ShaderBufferData}}, shader::ShaderRef, sprite_render::{AlphaMode2d, Material2d, Material2dPlugin}};

pub const BODY_SHADER: i64 = 898;
pub const LEGACY_BODY_SHADER: i64 = 849;

/// The vertex colour and the (scale, offset) from uv to the vertex position the shader was given.
#[derive(ShaderType, Clone, PartialEq)]
pub struct Extra {
    pub tint: Vec4,
    pub uv_to_local: Vec4,
}

/// The textures in the shared binding order.
pub struct Textures {
    pub main: Handle<Image>,
    pub flesh: Handle<Image>,
    pub bone: Handle<Image>,
    pub damage: Handle<Image>,
}

macro_rules! limb_material {
    ($name:ident, $path:literal) => {
        #[derive(Asset, TypePath, AsBindGroup, Clone)]
        pub struct $name {
            /// The $Globals constant buffer as Unity lays it out, then `_Time`: a buffer of its own, so the per-frame
            /// writes go to it in place and the material (its bind group) stays.
            #[storage(0, read_only)]
            pub cb0: Handle<ShaderBuffer>,
            #[uniform(1)]
            pub extra: Extra,
            #[texture(2)]
            #[sampler(3)]
            main: Handle<Image>,
            #[texture(4)]
            #[sampler(5)]
            flesh: Handle<Image>,
            #[texture(6)]
            #[sampler(7)]
            bone: Handle<Image>,
            #[texture(8)]
            #[sampler(9)]
            damage: Handle<Image>,
        }

        impl $name {
            /// The sprite's texture, and the flesh and bone textures a SetBodyTextures gave.
            pub fn set_textures(&mut self, main: Handle<Image>, flesh: Option<Handle<Image>>, bone: Option<Handle<Image>>) {
                self.main = main;
                if let Some(f) = flesh {
                    self.flesh = f;
                }
                if let Some(b) = bone {
                    self.bone = b;
                }
            }

            fn new(cb0: Handle<ShaderBuffer>, extra: Extra, t: Textures) -> Self {
                Self { cb0, extra, main: t.main, flesh: t.flesh, bone: t.bone, damage: t.damage }
            }
        }

        impl Material2d for $name {
            fn fragment_shader() -> ShaderRef {
                $path.into()
            }

            fn alpha_mode(&self) -> AlphaMode2d {
                AlphaMode2d::Blend
            }
        }
    };
}
limb_material!(Body, "embedded://people_playground_bevy/body.wesl");
limb_material!(LegacyBody, "embedded://people_playground_bevy/legacy_body.wesl");

pub fn add(app: &mut App) {
    embedded_asset!(app, "body.wesl");
    embedded_asset!(app, "legacy_body.wesl");
    app.add_plugins((Material2dPlugin::<Body>::default(), Material2dPlugin::<LegacyBody>::default()));
}

/// Byte offsets into $Globals, from the shaders' parameter tables. The first ten rows are the same in both.
struct Layout {
    stamps: Option<usize>,
    count: usize,
    burn: usize,
    acid: usize,
    temperature: usize,
    influence: usize,
    electrocution: usize,
    rotten: usize,
    multiplier: Option<usize>,
}
const BODY: Layout = Layout { stamps: Some(2208), count: 4244, burn: 4248, acid: 4256, temperature: 4260, influence: 4264, electrocution: 4268, rotten: 4272, multiplier: None };
const LEGACY: Layout = Layout { stamps: None, count: 4256, burn: 4260, acid: 4268, temperature: 4272, influence: 4276, electrocution: 4280, rotten: 4284, multiplier: Some(4288) };
const BLOOD_COLOR: usize = 144;
const DAMAGE_POINTS: usize = 160;

fn put(cb: &mut [UVec4; 269], at: usize, bits: u32) {
    cb[at / 16][at % 16 / 4] = bits;
}

fn put_f32(cb: &mut [UVec4; 269], at: usize, v: f32) {
    put(cb, at, v.to_bits());
}

fn put_vec(cb: &mut [UVec4; 269], at: usize, v: [f32; 4]) {
    for (i, x) in v.into_iter().enumerate() {
        put_f32(cb, at + i * 4, x);
    }
}

/// Unity turns a non-HDR colour property to linear when the project's colour space is.
fn to_linear(c: [f32; 4]) -> [f32; 4] {
    let l = Color::srgba(c[0], c[1], c[2], c[3]).to_linear();
    [l.red, l.green, l.blue, l.alpha]
}

/// One limb's material handle.
#[derive(Component)]
pub enum LimbMaterial {
    Body(Handle<Body>),
    Legacy(Handle<LegacyBody>),
}

/// SkinMaterialHandler.bloodColor: smoothed on the way to the renderer, so the simulation never reads it.
#[derive(Component)]
pub struct BloodColour(pub [f32; 4]);

impl BloodColour {
    /// SkinMaterialHandler.get_SmoothedBloodColour: bloodColor.
    pub fn smoothed_blood_colour(&self) -> [f32; 4] {
        self.0
    }
}

/// The limb's material, from the shared Material's properties.
pub fn new_material(m: &crate::assets::components::Material, legacy: bool, extra: Extra, t: Textures, bodies: &mut Assets<Body>, legacies: &mut Assets<LegacyBody>, buffers: &mut Assets<ShaderBuffer>) -> LimbMaterial {
    let mut cb = [UVec4::ZERO; 269];
    // The properties' defaults in the shader, for a material without them.
    let colours: [(&str, [f32; 4], bool); 8] = [
        ("_BruiseColor", [1.0, 0.0, 1.0, 1.0], false),
        ("_SecondBruiseColor", [1.0, 0.0, 1.0, 1.0], false),
        ("_ThirdBruiseColor", [1.0, 0.0, 1.0, 1.0], false),
        ("_Zombie", [1.0, 0.0, 0.0, 1.0], false),
        ("_GlowColour", [1.0, 0.0, 1.0, 1.0], true),
        ("_FreezeColour", [1.0, 1.0, 1.0, 1.0], false),
        ("_ElectroBoneColour", [1.0, 1.0, 1.0, 1.0], true),
        ("_BloodColor", [1.0, 0.0, 0.0, 1.0], false),
    ];
    for (i, (name, default, hdr)) in colours.into_iter().enumerate() {
        let c = m.color(name).unwrap_or(default);
        put_vec(&mut cb, 32 + i * 16, if hdr { c } else { to_linear(c) });
    }
    let layout = if legacy { &LEGACY } else { &BODY };
    put_f32(&mut cb, layout.influence, m.float("_BurnProgressInfluence").unwrap_or(1.0));
    if let Some(at) = layout.multiplier {
        put_f32(&mut cb, at, m.float("_DamageMultiplier").unwrap_or(1.0));
    }
    let cb = buffers.add(ShaderBuffer::new([cb.as_slice(), &[UVec4::ZERO]].concat(), RenderAssetUsages::default()));
    if legacy {
        LimbMaterial::Legacy(legacies.add(LegacyBody::new(cb, extra, t)))
    } else {
        LimbMaterial::Body(bodies.add(Body::new(cb, extra, t)))
    }
}

/// `cb` and `time` as they should be now; true if the shader will read the time.
fn sync(g: &mut crate::game::Game, l: usize, blood: &mut [f32; 4], layout: &Layout, cb: &mut [UVec4; 269]) -> bool {
    let p = g.limbs[l].PhysicalBehaviour;
    // OnWillRenderObject.
    let original = g.original_blood_color(l);
    let computed = g.get_computed_color(Owner::Circulation(l), original);
    let t = 1.0 - crate::game::mathf::pow(0.5, g.time.deltaTime);
    for i in 0..3 {
        blood[i] = (blood[i] + (computed[i] - blood[i]) * t.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    }
    blood[3] = 1.0;
    g.skins[l].colours[4] = Some(*blood);
    let ph = &g.physicals[p];
    let k = &g.skins[l];
    let charge = ph.Charge() / 25.0;
    put_f32(cb, layout.electrocution, charge);
    put_vec(cb, BLOOD_COLOR, to_linear(*blood));
    // PersonBehaviour.SetShaderColour: _BruiseColor, _SecondBruiseColor, _ThirdBruiseColor and _Zombie.
    for (i, c) in g.skins[l].colours[..4].iter().enumerate() {
        if let Some(c) = c {
            put_vec(cb, 32 + i * 16, to_linear(*c));
        }
    }
    let temperature = if ph.Temperature > 0.0 {
        if k.ShouldGlowWhenHot {
            let n = (ph.Temperature / k.MaxGlowTemperature as f32).clamp(0.0, 1.0);
            n * n
        } else {
            0.0
        }
    } else if k.ShouldRecolourWhenCold {
        crate::game::mathf::map_range(k.MinFreezeTemperature as f32, 0.0, -1.0, 0.0, ph.Temperature).clamp(-1.0, 0.0)
    } else {
        0.0
    };
    put_f32(cb, layout.temperature, temperature);
    put_f32(cb, layout.burn, ph.BurnProgress);
    // The setters' last writes to the material.
    put_f32(cb, layout.rotten, k.oldRottenProgress);
    put_f32(cb, layout.acid, k.materialAcid);
    // Sync.
    for (i, v) in k.damagePoints.iter().enumerate() {
        put_vec(cb, DAMAGE_POINTS + i * 16, *v);
    }
    put(cb, layout.count, k.currentDamagePointCount as u32);
    if let Some(at) = layout.stamps {
        for (i, v) in k.damagePointTimeStamps.iter().enumerate() {
            put_f32(cb, at + i * 16, *v);
        }
    }
    k.currentDamagePointCount > 0 || charge > 0.0
}

/// The render data of every limb; a material is written only when its data differ, the time included only while read.
pub fn limb_materials(mut play: ResMut<Play>, bodies: Res<Assets<Body>>, legacies: Res<Assets<LegacyBody>>, mut buffers: ResMut<Assets<ShaderBuffer>>, mut limbs: Query<(&LimbSprite, &LimbMaterial, &mut BloodColour)>) {
    let g = &mut play.game;
    let t = g.time.time;
    let time = Vec4::new(t / 20.0, t, t * 2.0, t * 3.0).to_array().map(f32::to_bits);
    for (&LimbSprite(l), material, mut blood) in &mut limbs {
        if !g.physicals[g.limbs[l].PhysicalBehaviour].active {
            continue;
        }
        let (cb0, layout) = match material {
            LimbMaterial::Body(h) => (bodies.get(h).map(|m| m.cb0.id()), &BODY),
            LimbMaterial::Legacy(h) => (legacies.get(h).map(|m| m.cb0.id()), &LEGACY),
        };
        let Some(Some(ShaderBufferData::Initialized { data, .. })) = cb0.map(|h| buffers.get(h).map(|b| &b.data)) else { continue };
        let mut cb = [UVec4::ZERO; 270];
        bytemuck::cast_slice_mut::<UVec4, u8>(&mut cb).copy_from_slice(data);
        let old = cb;
        let (globals, now) = cb.split_at_mut(269);
        if sync(g, l, &mut blood.0, layout, globals.try_into().unwrap()) {
            now[0] = UVec4::from_array(time);
        }
        if cb != old
            && let Some(mut b) = buffers.get_mut(cb0.unwrap())
            && let ShaderBufferData::Initialized { data, .. } = &mut b.data
        {
            data.copy_from_slice(bytemuck::cast_slice(&cb));
        }
    }
}
