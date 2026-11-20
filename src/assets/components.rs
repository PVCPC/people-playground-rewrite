//! Unity 2020.3 physics and rendering component layouts (stripped player data). Every reader rejects data it does
//! not consume exactly.

use super::classes::PPtr;
use super::serialized::Reader;

fn floats<const N: usize>(r: &mut Reader) -> Result<[f32; N], String> {
    let mut v = [0.0; N];
    for x in &mut v {
        *x = r.f32()?;
    }
    Ok(v)
}

fn vec2(r: &mut Reader) -> Result<[f32; 2], String> {
    floats(r)
}

fn vec<T>(r: &mut Reader, mut item: impl FnMut(&mut Reader) -> Result<T, String>) -> Result<Vec<T>, String> {
    let n = r.i32()?;
    (0..n).map(|_| item(r)).collect()
}

/// A bool followed by 4-byte alignment.
fn flag(r: &mut Reader) -> Result<bool, String> {
    let v = r.bool()?;
    r.align();
    Ok(v)
}

fn done<T>(r: &Reader, what: &str, v: T) -> Result<T, String> {
    if r.pos != r.data.len() {
        return Err(format!("{what}: read {} of {} bytes", r.pos, r.data.len()));
    }
    Ok(v)
}

/// PointEffector2D (class 250): Behaviour, Effector2D (useColliderMask, colliderMask), then the point fields.
/// AreaEffector2D (class 249) shares the struct: `area` is its (useGlobalAngle, forceAngle).
#[derive(Clone, Debug)]
pub struct PointEffector2D {
    pub game_object: PPtr,
    pub enabled: bool,
    pub use_collider_mask: bool,
    pub collider_mask: i32,
    pub force_magnitude: f32,
    pub force_variation: f32,
    pub distance_scale: f32,
    pub force_source: i32,
    pub force_target: i32,
    pub force_mode: i32,
    pub drag: f32,
    pub angular_drag: f32,
    pub area: Option<(bool, f32)>,
}

impl PointEffector2D {
    pub const CLASS: i32 = 250;
    pub const AREA_CLASS: i32 = 249;

    pub fn read_area(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let enabled = flag(r)?;
        let use_collider_mask = flag(r)?;
        let collider_mask = r.i32()?;
        let area = Some((flag(r)?, r.f32()?));
        let (force_magnitude, force_variation) = (r.f32()?, r.f32()?);
        let force_target = r.i32()?;
        let (drag, angular_drag) = (r.f32()?, r.f32()?);
        done(r, "AreaEffector2D", PointEffector2D { game_object, enabled, use_collider_mask, collider_mask, force_magnitude, force_variation, distance_scale: 0.0, force_source: 0, force_target, force_mode: 0, drag, angular_drag, area })
    }

    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let enabled = flag(r)?;
        let use_collider_mask = flag(r)?;
        let collider_mask = r.i32()?;
        let force_magnitude = r.f32()?;
        let force_variation = r.f32()?;
        let distance_scale = r.f32()?;
        let (force_source, force_target, force_mode) = (r.i32()?, r.i32()?, r.i32()?);
        let (drag, angular_drag) = (r.f32()?, r.f32()?);
        done(r, "PointEffector2D", PointEffector2D { game_object, enabled, use_collider_mask, collider_mask, force_magnitude, force_variation, distance_scale, force_source, force_target, force_mode, drag, angular_drag, area: None })
    }
}

/// Rigidbody2D (class 50).
#[derive(Clone, Debug)]
pub struct Rigidbody2D {
    pub game_object: PPtr,
    pub body_type: i32,
    pub simulated: bool,
    pub use_full_kinematic_contacts: bool,
    pub use_auto_mass: bool,
    pub mass: f32,
    pub linear_drag: f32,
    pub angular_drag: f32,
    pub gravity_scale: f32,
    pub material: PPtr,
    pub interpolate: i32,
    pub sleeping_mode: i32,
    pub collision_detection: i32,
    pub constraints: i32,
}

impl Rigidbody2D {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let body_type = r.i32()?;
        let (simulated, use_full_kinematic_contacts, use_auto_mass) = (r.bool()?, r.bool()?, r.bool()?);
        r.align();
        let v = Self {
            game_object,
            body_type,
            simulated,
            use_full_kinematic_contacts,
            use_auto_mass,
            mass: r.f32()?,
            linear_drag: r.f32()?,
            angular_drag: r.f32()?,
            gravity_scale: r.f32()?,
            material: PPtr::read(r)?,
            interpolate: r.i32()?,
            sleeping_mode: r.i32()?,
            collision_detection: r.i32()?,
            constraints: r.i32()?,
        };
        done(r, "Rigidbody2D", v)
    }
}

/// SpriteTilingProperty, inside Box/PolygonCollider2D.
#[derive(Clone, Debug)]
pub struct SpriteTiling {
    pub border: [f32; 4],
    pub pivot: [f32; 2],
    pub old_size: [f32; 2],
    pub new_size: [f32; 2],
    pub adaptive_tiling_threshold: f32,
    pub draw_mode: i32,
    pub adaptive_tiling: bool,
}

impl SpriteTiling {
    fn read(r: &mut Reader) -> Result<Self, String> {
        Ok(Self {
            border: floats(r)?,
            pivot: vec2(r)?,
            old_size: vec2(r)?,
            new_size: vec2(r)?,
            adaptive_tiling_threshold: r.f32()?,
            draw_mode: r.i32()?,
            adaptive_tiling: flag(r)?,
        })
    }
}

#[derive(Clone, Debug)]
pub enum ColliderShape {
    Box { tiling: SpriteTiling, auto_tiling: bool, size: [f32; 2], edge_radius: f32 },
    Circle { radius: f32 },
    Polygon { tiling: SpriteTiling, auto_tiling: bool, paths: Vec<Vec<[f32; 2]>> },
    Capsule { size: [f32; 2], direction: i32 },
    Edge { edge_radius: f32, points: Vec<[f32; 2]>, adjacent_start: Option<[f32; 2]>, adjacent_end: Option<[f32; 2]> },
}

/// Collider2D subclasses: CircleCollider2D (58), PolygonCollider2D (60), BoxCollider2D (61), EdgeCollider2D (68),
/// CapsuleCollider2D (70).
#[derive(Clone, Debug)]
pub struct Collider2D {
    pub game_object: PPtr,
    pub enabled: bool,
    pub density: f32,
    pub material: PPtr,
    pub is_trigger: bool,
    pub used_by_effector: bool,
    pub used_by_composite: bool,
    pub offset: [f32; 2],
    pub shape: ColliderShape,
}

impl Collider2D {
    pub const CLASSES: [i32; 5] = [58, 60, 61, 68, 70];

    pub fn read(class_id: i32, data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let enabled = flag(r)?;
        let density = r.f32()?;
        let material = PPtr::read(r)?;
        let (is_trigger, used_by_effector, used_by_composite) = (r.bool()?, r.bool()?, r.bool()?);
        r.align();
        let offset = vec2(r)?;
        let shape = match class_id {
            58 => ColliderShape::Circle { radius: r.f32()? },
            61 => {
                let tiling = SpriteTiling::read(r)?;
                let auto_tiling = flag(r)?;
                ColliderShape::Box { tiling, auto_tiling, size: vec2(r)?, edge_radius: r.f32()? }
            }
            60 => {
                let tiling = SpriteTiling::read(r)?;
                let auto_tiling = flag(r)?;
                ColliderShape::Polygon { tiling, auto_tiling, paths: vec(r, |r| vec(r, vec2))? }
            }
            70 => ColliderShape::Capsule { size: vec2(r)?, direction: r.i32()? },
            68 => {
                let edge_radius = r.f32()?;
                let points = vec(r, vec2)?;
                let (start, end) = (vec2(r)?, vec2(r)?);
                let (use_start, use_end) = (r.bool()?, r.bool()?);
                r.align();
                ColliderShape::Edge {
                    edge_radius,
                    points,
                    adjacent_start: use_start.then_some(start),
                    adjacent_end: use_end.then_some(end),
                }
            }
            _ => return Err(format!("class {class_id} is not a Collider2D")),
        };
        let v = Self { game_object, enabled, density, material, is_trigger, used_by_effector, used_by_composite, offset, shape };
        done(r, "Collider2D", v)
    }
}

#[derive(Clone, Debug)]
pub enum JointKind {
    Hinge { use_motor: bool, motor_speed: f32, max_motor_torque: f32, use_limits: bool, lower: f32, upper: f32 },
    Fixed { damping_ratio: f32, frequency: f32 },
    Spring { auto_configure_distance: bool, distance: f32, damping_ratio: f32, frequency: f32 },
    Distance { auto_configure_distance: bool, distance: f32, max_distance_only: bool },
    Slider {
        auto_configure_angle: bool,
        angle: f32,
        use_motor: bool,
        motor_speed: f32,
        max_motor_force: f32,
        use_limits: bool,
        lower: f32,
        upper: f32,
    },
    Wheel { damping_ratio: f32, frequency: f32, angle: f32, use_motor: bool, motor_speed: f32, max_motor_torque: f32 },
    Relative { max_force: f32, max_torque: f32, correction_scale: f32, auto_configure_offset: bool, linear_offset: [f32; 2], angular_offset: f32 },
}

/// AnchoredJoint2D subclasses: SpringJoint2D (231), DistanceJoint2D (232), HingeJoint2D (233), SliderJoint2D (234),
/// WheelJoint2D (235), FixedJoint2D (255), and RelativeJoint2D (254), a Joint2D without anchors.
#[derive(Clone, Debug)]
pub struct Joint2D {
    pub game_object: PPtr,
    pub enabled: bool,
    pub enable_collision: bool,
    pub connected_body: PPtr,
    pub break_force: f32,
    pub break_torque: f32,
    pub auto_configure_connected_anchor: bool,
    pub anchor: [f32; 2],
    pub connected_anchor: [f32; 2],
    pub kind: JointKind,
}

impl Joint2D {
    pub const CLASSES: [i32; 7] = [231, 232, 233, 234, 235, 254, 255];

    pub fn read(class_id: i32, data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let enabled = flag(r)?;
        let enable_collision = flag(r)?;
        let connected_body = PPtr::read(r)?;
        let (break_force, break_torque) = (r.f32()?, r.f32()?);
        if class_id == 254 {
            let (max_force, max_torque, correction_scale) = (r.f32()?, r.f32()?, r.f32()?);
            let kind = JointKind::Relative { max_force, max_torque, correction_scale, auto_configure_offset: flag(r)?, linear_offset: vec2(r)?, angular_offset: r.f32()? };
            let v = Self { game_object, enabled, enable_collision, connected_body, break_force, break_torque, auto_configure_connected_anchor: false, anchor: [0.0; 2], connected_anchor: [0.0; 2], kind };
            return done(r, "Joint2D", v);
        }
        let auto_configure_connected_anchor = flag(r)?;
        let (anchor, connected_anchor) = (vec2(r)?, vec2(r)?);
        let kind = match class_id {
            233 => {
                let use_motor = flag(r)?;
                let (motor_speed, max_motor_torque) = (r.f32()?, r.f32()?);
                JointKind::Hinge { use_motor, motor_speed, max_motor_torque, use_limits: flag(r)?, lower: r.f32()?, upper: r.f32()? }
            }
            255 => JointKind::Fixed { damping_ratio: r.f32()?, frequency: r.f32()? },
            231 => JointKind::Spring { auto_configure_distance: flag(r)?, distance: r.f32()?, damping_ratio: r.f32()?, frequency: r.f32()? },
            232 => JointKind::Distance { auto_configure_distance: flag(r)?, distance: r.f32()?, max_distance_only: flag(r)? },
            234 => {
                let (auto_configure_angle, angle) = (flag(r)?, r.f32()?);
                let use_motor = flag(r)?;
                let (motor_speed, max_motor_force) = (r.f32()?, r.f32()?);
                JointKind::Slider {
                    auto_configure_angle,
                    angle,
                    use_motor,
                    motor_speed,
                    max_motor_force,
                    use_limits: flag(r)?,
                    lower: r.f32()?,
                    upper: r.f32()?,
                }
            }
            235 => {
                let (damping_ratio, frequency, angle) = (r.f32()?, r.f32()?, r.f32()?);
                JointKind::Wheel { damping_ratio, frequency, angle, use_motor: flag(r)?, motor_speed: r.f32()?, max_motor_torque: r.f32()? }
            }
            _ => return Err(format!("class {class_id} is not an AnchoredJoint2D")),
        };
        let v = Self {
            game_object,
            enabled,
            enable_collision,
            connected_body,
            break_force,
            break_torque,
            auto_configure_connected_anchor,
            anchor,
            connected_anchor,
            kind,
        };
        done(r, "Joint2D", v)
    }
}

/// PhysicsMaterial2D (class 62).
#[derive(Clone, Debug)]
pub struct PhysicsMaterial2D {
    pub name: String,
    pub friction: f32,
    pub bounciness: f32,
}

impl PhysicsMaterial2D {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let v = Self { name: r.string()?, friction: r.f32()?, bounciness: r.f32()? };
        done(r, "PhysicsMaterial2D", v)
    }
}

/// Material (class 21): its shader and saved floats and colours.
#[derive(Clone, Debug)]
pub struct Material {
    pub shader: PPtr,
    /// m_CustomRenderQueue: -1 takes the shader's.
    pub render_queue: i32,
    /// Name, texture and its scale and offset (the _ST vector).
    pub textures: Vec<(String, PPtr, [f32; 4])>,
    pub floats: Vec<(String, f32)>,
    pub colors: Vec<(String, [f32; 4])>,
}

impl Material {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        Ok(Self::read_with_keywords(data)?.0)
    }

    /// The material and its m_ShaderKeywords (space separated).
    pub fn read_with_keywords(data: &[u8]) -> Result<(Self, String), String> {
        let r = &mut Reader::new(data);
        let _name = r.string()?;
        let shader = PPtr::read(r)?;
        let keywords = r.string()?;
        let _lightmap_flags = r.u32()?;
        let _instancing_and_double_sided = (r.bool()?, r.bool()?);
        r.align();
        let render_queue = r.i32()?;
        let _string_tags = vec(r, |r| Ok((r.string()?, r.string()?)))?;
        let _disabled_passes = vec(r, |r| r.string())?;
        let textures = vec(r, |r| {
            let name = r.string()?;
            let texture = PPtr::read(r)?;
            let scale_offset: [f32; 4] = floats(r)?;
            Ok((name, texture, scale_offset))
        })?;
        let saved_floats = vec(r, |r| Ok((r.string()?, r.f32()?)))?;
        let colors = vec(r, |r| Ok((r.string()?, floats(r)?)))?;
        Ok((Self { shader, render_queue, textures, floats: saved_floats, colors }, keywords))
    }

    pub fn float(&self, name: &str) -> Option<f32> {
        self.floats.iter().find(|(n, _)| n == name).map(|&(_, v)| v)
    }

    pub fn color(&self, name: &str) -> Option<[f32; 4]> {
        self.colors.iter().find(|(n, _)| n == name).map(|&(_, v)| v)
    }
}

/// MeshRenderer (class 23): the Renderer fields a TextMeshPro's mesh is drawn with.
#[derive(Clone, Debug)]
pub struct MeshRenderer {
    pub enabled: bool,
    pub materials: Vec<PPtr>,
    pub sorting_layer_id: i32,
    pub sorting_order: i16,
}

impl MeshRenderer {
    pub const CLASS: i32 = 23;

    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let _game_object = PPtr::read(r)?;
        let enabled = r.bool()?;
        r.bytes(8)?;
        r.align();
        let _rendering_layer_mask = r.u32()?;
        let _renderer_priority = r.i32()?;
        let _lightmap_index = (r.u16()?, r.u16()?);
        let _lightmap_tiling_offset: [[f32; 4]; 2] = [floats(r)?, floats(r)?];
        let materials = vec(r, PPtr::read)?;
        let _static_batch_info = (r.u16()?, r.u16()?);
        let _static_batch_root = PPtr::read(r)?;
        let _probe_anchor = PPtr::read(r)?;
        let _light_probe_volume_override = PPtr::read(r)?;
        let sorting_layer_id = r.i32()?;
        let _sorting_layer = r.i16()?;
        let sorting_order = r.i16()?;
        Ok(Self { enabled, materials, sorting_layer_id, sorting_order })
    }
}

/// SpriteRenderer (class 212), with the Renderer fields scripts and drawing use.
#[derive(Clone, Debug)]
pub struct SpriteRenderer {
    pub game_object: PPtr,
    pub enabled: bool,
    pub materials: Vec<PPtr>,
    pub sorting_layer_id: i32,
    pub sorting_order: i16,
    pub sprite: PPtr,
    pub color: [f32; 4],
    pub flip_x: bool,
    pub flip_y: bool,
    pub draw_mode: i32,
    pub adaptive_mode_threshold: f32,
    /// SpriteTileMode: Continuous 0, Adaptive 1.
    pub sprite_tile_mode: i32,
    pub size: [f32; 2],
    pub mask_interaction: i32,
    pub sprite_sort_point: i32,
}

impl SpriteRenderer {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let enabled = r.bool()?;
        // CastShadows, ReceiveShadows, DynamicOccludee, MotionVectors, LightProbeUsage, ReflectionProbeUsage,
        // RayTracingMode, RayTraceProcedural.
        r.bytes(8)?;
        r.align();
        let _rendering_layer_mask = r.u32()?;
        let _renderer_priority = r.i32()?;
        let _lightmap_index = (r.u16()?, r.u16()?);
        let _lightmap_tiling_offset: [[f32; 4]; 2] = [floats(r)?, floats(r)?];
        let materials = vec(r, PPtr::read)?;
        let _static_batch_info = (r.u16()?, r.u16()?);
        let _static_batch_root = PPtr::read(r)?;
        let _probe_anchor = PPtr::read(r)?;
        let _light_probe_volume_override = PPtr::read(r)?;
        let sorting_layer_id = r.i32()?;
        let _sorting_layer = r.i16()?;
        let sorting_order = r.i16()?;
        r.align();
        let sprite = PPtr::read(r)?;
        let color = floats(r)?;
        let (flip_x, flip_y) = (r.bool()?, r.bool()?);
        r.align();
        let draw_mode = r.i32()?;
        let size = vec2(r)?;
        let adaptive_mode_threshold = r.f32()?;
        let sprite_tile_mode = r.i32()?;
        let _was_sprite_assigned = flag(r)?;
        let v = Self {
            game_object,
            enabled,
            materials,
            sorting_layer_id,
            sorting_order,
            sprite,
            color,
            flip_x,
            flip_y,
            draw_mode,
            adaptive_mode_threshold,
            sprite_tile_mode,
            size,
            mask_interaction: r.i32()?,
            sprite_sort_point: r.i32()?,
        };
        done(r, "SpriteRenderer", v)
    }
}

/// AudioSource (class 82): Behaviour, then the source's serialized fields.
#[derive(Clone, Debug)]
pub struct AudioSource {
    pub game_object: PPtr,
    pub enabled: bool,
    /// OutputAudioMixerGroup
    pub output: PPtr,
    /// m_audioClip
    pub clip: PPtr,
    pub play_on_awake: bool,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    pub mute: bool,
    pub spatialize: bool,
    pub spatialize_post_effects: bool,
    pub priority: i32,
    pub doppler_level: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    /// Pan2D: AudioSource.panStereo.
    pub pan_2d: f32,
    /// 0 Logarithmic, 1 Linear, 2 Custom.
    pub rolloff_mode: i32,
    /// BypassEffects, BypassListenerEffects, BypassReverbZones.
    pub bypass: [bool; 3],
    pub rolloff_curve: crate::game_assets::Curve,
    /// panLevelCustomCurve: AudioSource.spatialBlend over distance / maxDistance.
    pub spatial_blend: crate::game_assets::Curve,
    /// spreadCustomCurve: AudioSource.spread / 360 over distance / maxDistance.
    pub spread: crate::game_assets::Curve,
    pub reverb_zone_mix: crate::game_assets::Curve,
}

/// A serialized AnimationCurve with unweighted keys.
fn anim_curve(r: &mut Reader) -> Result<crate::game_assets::Curve, String> {
    let keys = vec(r, |r| {
        let k = floats::<4>(r)?;
        let weighted = r.i32()?;
        let _weights = floats::<2>(r)?;
        if weighted != 0 {
            unimplemented!("mod-only: an AudioSource curve with a weighted key");
        }
        Ok(k)
    })?;
    let (pre, post, _rotation_order) = (r.i32()?, r.i32()?, r.i32()?);
    Ok(crate::game_assets::Curve { keys, pre, post })
}

impl AudioSource {
    pub const CLASS: i32 = 82;

    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let game_object = PPtr::read(r)?;
        let enabled = flag(r)?;
        let (output, clip) = (PPtr::read(r)?, PPtr::read(r)?);
        let play_on_awake = flag(r)?;
        let (volume, pitch) = (r.f32()?, r.f32()?);
        let (looping, mute, spatialize, spatialize_post_effects) = (r.bool()?, r.bool()?, r.bool()?, r.bool()?);
        r.align();
        let priority = r.i32()?;
        let [doppler_level, min_distance, max_distance, pan_2d] = floats(r)?;
        let rolloff_mode = r.i32()?;
        let bypass = [r.bool()?, r.bool()?, r.bool()?];
        r.align();
        let (rolloff_curve, spatial_blend, spread, reverb_zone_mix) = (anim_curve(r)?, anim_curve(r)?, anim_curve(r)?, anim_curve(r)?);
        let v = Self {
            game_object,
            enabled,
            output,
            clip,
            play_on_awake,
            volume,
            pitch,
            looping,
            mute,
            spatialize,
            spatialize_post_effects,
            priority,
            doppler_level,
            min_distance,
            max_distance,
            pan_2d,
            rolloff_mode,
            bypass,
            rolloff_curve,
            spatial_blend,
            spread,
            reverb_zone_mix,
        };
        done(r, "AudioSource", v)
    }
}

/// Texture2D (class 28). Pixel data is inline or streamed from a .resS file.
#[derive(Clone, Debug)]
pub struct Texture2D {
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub complete_image_size: u32,
    pub format: i32,
    pub mip_count: i32,
    pub filter_mode: i32,
    pub wrap: [i32; 3],
    pub color_space: i32,
    pub readable: bool,
    pub image_data: Vec<u8>,
    /// (offset, size, path) into a .resS file when `image_data` is empty.
    pub stream: (u64, u32, String),
}

impl Texture2D {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let name = r.string()?;
        let _forced_fallback_format = r.i32()?;
        let (_downscale_fallback, _is_alpha_channel_optional) = (r.bool()?, r.bool()?);
        r.align();
        let (width, height, complete_image_size) = (r.i32()?, r.i32()?, r.u32()?);
        let _mips_stripped = r.i32()?;
        let (format, mip_count) = (r.i32()?, r.i32()?);
        // IsReadable, IsPreProcessed, IgnoreMasterTextureLimit, StreamingMipmaps.
        let readable = r.bytes(4)?[0] != 0;
        let _streaming_mipmaps_priority = r.i32()?;
        let (_image_count, _texture_dimension) = (r.i32()?, r.i32()?);
        let filter_mode = r.i32()?;
        let (_aniso, _mip_bias) = (r.i32()?, r.f32()?);
        let wrap = [r.i32()?, r.i32()?, r.i32()?];
        let _lightmap_format = r.i32()?;
        let color_space = r.i32()?;
        let n = r.i32()? as usize;
        r.bytes(n)?;
        r.align();
        let n = r.i32()? as usize;
        let image_data = r.bytes(n)?.to_vec();
        r.align();
        let stream = (r.u64()?, r.u32()?, r.string()?);
        let v = Self { name, width, height, complete_image_size, format, mip_count, filter_mode, wrap, color_space, readable, image_data, stream };
        done(r, "Texture2D", v)
    }

    /// Mip 0 as RGBA8 rows, bottom row first (Unity's order). `data_dir` holds the .resS files.
    pub fn rgba(&self, data_dir: &std::path::Path) -> Result<Vec<u8>, String> {
        if self.width == 0 || self.height == 0 {
            // Font textures that are filled at runtime.
            return Ok(Vec::new());
        }
        let streamed;
        let bytes = if self.image_data.is_empty() {
            let (offset, size, path) = &self.stream;
            if path.is_empty() {
                return Err(format!("{}: no image data ({}x{}, {} bytes, format {})", self.name, self.width, self.height, self.complete_image_size, self.format));
            }
            let file = data_dir.join(path.rsplit('/').next().unwrap_or(path));
            // Only this texture's range of the .resS (it holds many).
            use std::io::{Read, Seek, SeekFrom};
            let mut f = std::fs::File::open(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let mut buf = vec![0u8; *size as usize];
            f.seek(SeekFrom::Start(*offset as u64)).and_then(|_| f.read_exact(&mut buf)).map_err(|_| "stream past the end".to_string())?;
            streamed = buf;
            &streamed
        } else {
            &self.image_data
        };
        let n = (self.width * self.height) as usize;
        let px = |bpp: usize| bytes.get(..n * bpp).ok_or_else(|| format!("{}: short image data", self.name));
        Ok(match self.format {
            1 => px(1)?.iter().flat_map(|&a| [0, 0, 0, a]).collect(),
            3 => px(3)?.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
            4 => px(4)?.to_vec(),
            5 => px(4)?.chunks(4).flat_map(|c| [c[1], c[2], c[3], c[0]]).collect(),
            7 => px(2)?
                .chunks(2)
                .flat_map(|c| {
                    let v = u16::from_le_bytes([c[0], c[1]]);
                    let (r, g, b) = ((v >> 11) as u8, (v >> 5 & 63) as u8, (v & 31) as u8);
                    [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2, 255]
                })
                .collect(),
            17 => px(8)?
                .chunks(2)
                .map(|c| (half(u16::from_le_bytes([c[0], c[1]])).clamp(0.0, 1.0) * 255.0) as u8)
                .collect(),
            63 => px(1)?.iter().flat_map(|&v| [v, 0, 0, 255]).collect(),
            10 | 12 | 25 | 28 => {
                let (w, h) = (self.width as usize, self.height as usize);
                let mut out = vec![0u32; n];
                let res = match self.format {
                    10 => texture2ddecoder::decode_bc1(bytes, w, h, &mut out),
                    12 => texture2ddecoder::decode_bc3(bytes, w, h, &mut out),
                    25 => texture2ddecoder::decode_bc7(bytes, w, h, &mut out),
                    _ => texture2ddecoder::decode_unity_crunch(bytes, w, h, &mut out),
                };
                res.map_err(|e| format!("{}: {e}", self.name))?;
                // The decoders write BGRA.
                out.iter().flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                }).collect()
            }
            f => return Err(format!("{}: texture format {f} not rewritten", self.name)),
        })
    }
}

/// An IEEE half-precision float.
fn half(h: u16) -> f32 {
    let (sign, exp, frac) = ((h >> 15) as u32, (h >> 10 & 31) as u32, (h & 1023) as u32);
    let v = match exp {
        0 => frac as f32 / 1024.0 / 16384.0,
        31 => if frac == 0 { f32::INFINITY } else { f32::NAN },
        _ => f32::from_bits((exp + 112) << 23 | frac << 13),
    };
    if sign == 1 { -v } else { v }
}

#[derive(Clone, Debug)]
pub struct SubMesh {
    pub first_byte: u32,
    pub index_count: u32,
    pub topology: i32,
    pub base_vertex: u32,
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub local_aabb: [f32; 6],
}

/// (stream, offset, format, dimension) of one vertex channel.
pub type Channel = [u8; 4];

/// A SpriteAtlas's m_RenderDataMap entry, a packed Sprite's texture data.
#[derive(Clone, Debug)]
pub struct SpriteAtlasData {
    pub texture: PPtr,
    pub alpha_texture: PPtr,
    pub texture_rect: [f32; 4],
    pub texture_rect_offset: [f32; 2],
    pub atlas_rect_offset: [f32; 2],
    pub uv_transform: [f32; 4],
    pub downscale_multiplier: f32,
    pub settings_raw: u32,
}

/// SpriteAtlas (class 687078895): m_RenderDataMap.
pub fn sprite_atlas_data(data: &[u8]) -> Result<Vec<(([u8; 16], i64), SpriteAtlasData)>, String> {
    let r = &mut Reader::new(data);
    r.string()?;
    vec(r, PPtr::read)?;
    vec(r, |r| r.string())?;
    vec(r, |r| {
        let key = (<[u8; 16]>::try_from(r.bytes(16)?).unwrap(), r.i64()?);
        let v = SpriteAtlasData {
            texture: PPtr::read(r)?,
            alpha_texture: PPtr::read(r)?,
            texture_rect: floats(r)?,
            texture_rect_offset: vec2(r)?,
            atlas_rect_offset: vec2(r)?,
            uv_transform: floats(r)?,
            downscale_multiplier: r.f32()?,
            settings_raw: r.u32()?,
        };
        vec(r, |r| Ok((PPtr::read(r)?, r.string()?)))?;
        Ok((key, v))
    })
}

/// Sprite (class 213).
#[derive(Clone, Debug)]
pub struct Sprite {
    pub name: String,
    pub rect: [f32; 4],
    pub offset: [f32; 2],
    pub border: [f32; 4],
    pub pixels_to_units: f32,
    pub pivot: [f32; 2],
    pub extrude: u32,
    pub is_polygon: bool,
    /// m_RenderDataKey: the sprite's entry in its SpriteAtlas.
    pub render_data_key: ([u8; 16], i64),
    pub atlas: PPtr,
    pub texture: PPtr,
    pub alpha_texture: PPtr,
    pub sub_meshes: Vec<SubMesh>,
    pub index_buffer: Vec<u8>,
    pub vertex_count: u32,
    pub channels: Vec<Channel>,
    pub vertex_data: Vec<u8>,
    pub texture_rect: [f32; 4],
    pub texture_rect_offset: [f32; 2],
    pub atlas_rect_offset: [f32; 2],
    pub settings_raw: u32,
    pub uv_transform: [f32; 4],
    pub downscale_multiplier: f32,
    pub physics_shape: Vec<Vec<[f32; 2]>>,
}

impl Sprite {
    /// The mesh a Simple SpriteRenderer draws: Position (float3) and TexCoord0 (channel 4, float2), each from its stream (the
    /// streams follow one another, each started on 16 bytes), and the u16 indices.
    pub fn mesh(&self) -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
        const SIZE: [usize; 12] = [4, 2, 1, 1, 2, 2, 1, 1, 2, 2, 4, 4];
        let (position, uv) = (self.channels[0], self.channels[4]);
        assert!(position[2] == 0 && uv[2] == 0 && uv[3] & 15 == 2, "a sprite mesh without float Position and TexCoord0: {:?}", self.channels);
        let n = self.vertex_count as usize;
        let stride = |stream: u8| self.channels.iter().filter(|c| c[0] == stream && c[3] & 15 != 0)
            .map(|c| c[1] as usize + (c[3] & 15) as usize * SIZE[c[2] as usize]).max().unwrap_or(0);
        let start = |stream: u8| (0..stream).fold(0, |at, k| (at + n * stride(k) + 15) & !15);
        let f = |at: usize| f32::from_le_bytes(self.vertex_data[at..at + 4].try_into().unwrap());
        let at = |c: [u8; 4], i: usize| start(c[0]) + c[1] as usize + i * stride(c[0]);
        let positions = (0..n).map(|i| { let a = at(position, i); [f(a), f(a + 4), f(a + 8)] }).collect();
        let uvs = (0..n).map(|i| { let a = at(uv, i); [f(a), f(a + 4)] }).collect();
        let index = self.index_buffer.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u32).collect();
        (positions, uvs, index)
    }

    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let name = r.string()?;
        let (rect, offset, border, pixels_to_units, pivot, extrude) =
            (floats(r)?, vec2(r)?, floats(r)?, r.f32()?, vec2(r)?, r.u32()?);
        let is_polygon = flag(r)?;
        let render_data_key = (<[u8; 16]>::try_from(r.bytes(16)?).unwrap(), r.i64()?);
        let _atlas_tags = vec(r, |r| r.string())?;
        let atlas = PPtr::read(r)?;
        let texture = PPtr::read(r)?;
        let alpha_texture = PPtr::read(r)?;
        let _secondary_textures = vec(r, |r| Ok((PPtr::read(r)?, r.string()?)))?;
        let sub_meshes = vec(r, |r| {
            Ok(SubMesh {
                first_byte: r.u32()?,
                index_count: r.u32()?,
                topology: r.i32()?,
                base_vertex: r.u32()?,
                first_vertex: r.u32()?,
                vertex_count: r.u32()?,
                local_aabb: floats(r)?,
            })
        })?;
        let n = r.i32()? as usize;
        let index_buffer = r.bytes(n)?.to_vec();
        r.align();
        let vertex_count = r.u32()?;
        let channels = vec(r, |r| Ok([r.u8()?, r.u8()?, r.u8()?, r.u8()?]))?;
        let n = r.i32()? as usize;
        let vertex_data = r.bytes(n)?.to_vec();
        r.align();
        let _bindpose = vec(r, floats::<16>)?;
        let v = Self {
            name,
            rect,
            offset,
            border,
            pixels_to_units,
            pivot,
            extrude,
            is_polygon,
            render_data_key,
            atlas,
            texture,
            alpha_texture,
            sub_meshes,
            index_buffer,
            vertex_count,
            channels,
            vertex_data,
            texture_rect: floats(r)?,
            texture_rect_offset: vec2(r)?,
            atlas_rect_offset: vec2(r)?,
            settings_raw: r.u32()?,
            uv_transform: floats(r)?,
            downscale_multiplier: r.f32()?,
            physics_shape: vec(r, |r| vec(r, vec2))?,
        };
        let _bones = vec(r, |r| {
            let name = r.string()?;
            let (_position, _rotation): ([f32; 3], [f32; 4]) = (floats(r)?, floats(r)?);
            let (_length, _parent) = (r.f32()?, r.i32()?);
            Ok(name)
        })?;
        done(r, "Sprite", v)
    }
}

/// VideoClip (class 329): the .webm lives in a .resS file.
#[derive(Clone, Debug)]
pub struct VideoClip {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub frame_count: u64,
    /// (path, offset, size) of the file's bytes.
    pub stream: (String, u64, u64),
}

impl VideoClip {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let name = r.string()?;
        let _original_path = r.string()?;
        let (_proxy_width, _proxy_height, width, height) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        let (_aspect_num, _aspect_den) = (r.u32()?, r.u32()?);
        let frame_rate = f64::from_bits(r.u64()?);
        let frame_count = r.u64()?;
        let _format = r.i32()?;
        vec(r, |r| r.u16())?;
        r.align();
        vec(r, |r| r.u32())?;
        vec(r, |r| r.string())?;
        // Four bytes the installed data holds (0 in every clip) before the StreamedResource.
        r.u32()?;
        let stream = (r.string()?, r.u64()?, r.u64()?);
        // m_HasSplitAlpha, m_sRGB
        r.bytes(2)?;
        done(r, "VideoClip", Self { name, width, height, frame_rate, frame_count, stream })
    }

    /// VideoClip.length / VideoPlayer.length: the frames over the frame rate.
    pub fn length(&self) -> f64 {
        self.frame_count as f64 / self.frame_rate
    }

    /// The .webm's bytes; `data_dir` holds the .resS files.
    pub fn webm(&self, data_dir: &std::path::Path) -> Result<Vec<u8>, String> {
        let (path, offset, size) = &self.stream;
        let file = data_dir.join(path.rsplit('/').next().unwrap_or(path));
        let all = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        all.get(*offset as usize..(*offset + *size) as usize).map(<[u8]>::to_vec).ok_or_else(|| format!("{}: stream past the end", self.name))
    }
}

#[cfg(test)]
mod tests {
    use super::super::serialized::SerializedFile;
    use super::*;
    use std::collections::BTreeMap;

    /// Decoded pixels equal AssetRipper's PNG export of the same texture, for each texture format in the game.
    #[test]
    fn texture_pixels_match_export() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let export = std::path::Path::new("unity-export/ExportedProject/Assets/Texture2D");
        if !export.is_dir() {
            return;
        }
        let data = game.join("People Playground_Data");
        let mut checked = std::collections::BTreeMap::new();
        for e in std::fs::read_dir(&data).unwrap() {
            let name = e.unwrap().file_name().into_string().unwrap();
            if !(name.ends_with(".assets") || name.starts_with("level")) {
                continue;
            }
            let f = SerializedFile::open(&data.join(&name)).unwrap();
            for o in f.objects.iter().filter(|o| o.class_id == 28) {
                let t = Texture2D::read(f.data(o)).unwrap();
                let png_path = export.join(format!("{}.png", t.name));
                if checked.contains_key(&t.format) || t.width == 0 || !png_path.is_file() {
                    continue;
                }
                let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png_path).unwrap()));
                let mut reader = decoder.read_info().unwrap();
                let mut buf = vec![0; reader.output_buffer_size().unwrap()];
                let info = reader.next_frame(&mut buf).unwrap();
                if (info.width, info.height) != (t.width as u32, t.height as u32) {
                    continue;
                }
                let buf: Vec<u8> = match info.color_type {
                    png::ColorType::Rgba => buf,
                    png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
                    _ => continue,
                };
                let ours = t.rgba(&data).unwrap();
                let row = t.width as usize * 4;
                // PNG rows run top-down; Unity's bottom-up.
                let max_diff = (0..t.height as usize)
                    .flat_map(|y| {
                        let a = &ours[y * row..][..row];
                        let b = &buf[(t.height as usize - 1 - y) * row..][..row];
                        a.iter().zip(b).map(|(x, y)| x.abs_diff(*y))
                    })
                    .max()
                    .unwrap();
                checked.insert(t.format, (t.name.clone(), max_diff));
            }
        }
        eprintln!("{checked:?}");
        // RGB565 (7) is left out: the export shifts without bit replication, while the GPU (the only reader here;
        // "shard id mask" is not script-readable) replicates.
        for f in [1, 3, 4, 10, 12, 17, 25, 28, 63] {
            let (name, diff) = checked.get(&f).unwrap_or_else(|| panic!("no exported texture of format {f}"));
            assert_eq!(*diff, 0, "format {f} ({name})");
        }
    }

    /// Every physics/render component in the shipped data reads to exactly its serialized size.
    #[test]
    fn all_components_consume_exactly() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let data = game.join("People Playground_Data");
        let mut counts: BTreeMap<i32, (usize, Vec<String>)> = BTreeMap::new();
        let mut found_hinge = false;
        for e in std::fs::read_dir(&data).unwrap() {
            let name = e.unwrap().file_name().into_string().unwrap();
            if !(name.ends_with(".assets") || name.starts_with("level") || name == "globalgamemanagers") {
                continue;
            }
            let f = SerializedFile::open(&data.join(&name)).unwrap();
            for o in &f.objects {
                let d = f.data(o);
                let c = o.class_id;
                if c == 233 {
                    // A Person.prefab hinge, as the AssetRipper export of the same data shows it.
                    let j = Joint2D::read(c, d).unwrap();
                    if let JointKind::Hinge { max_motor_torque, use_limits, lower, upper, .. } = j.kind {
                        found_hinge |= j.anchor == [0.0032851696, -0.18]
                            && j.connected_anchor == [0.003785193, 0.11700004]
                            && j.break_force == f32::INFINITY
                            && (max_motor_torque, use_limits, lower, upper) == (1.0, true, -70.0, 80.0);
                    }
                }
                let res = match c {
                    50 => Rigidbody2D::read(d).map(drop),
                    62 => PhysicsMaterial2D::read(d).map(drop),
                    212 => SpriteRenderer::read(d).map(drop),
                    28 => Texture2D::read(d).and_then(|t| {
                        let px = t.rgba(&data)?;
                        if px.len() != (t.width * t.height * 4) as usize {
                            return Err("pixel count".into());
                        }
                        Ok(())
                    }),
                    213 => Sprite::read(d).map(drop),
                    c if Collider2D::CLASSES.contains(&c) => Collider2D::read(c, d).map(drop),
                    c if Joint2D::CLASSES.contains(&c) => Joint2D::read(c, d).map(drop),
                    _ => continue,
                };
                let e = counts.entry(c).or_default();
                e.0 += 1;
                if let Err(err) = res {
                    if e.1.len() < 3 {
                        e.1.push(format!("{name}#{}: {err}", o.path_id));
                    }
                }
            }
        }
        let mut bad = false;
        for (c, (n, errs)) in &counts {
            eprintln!("class {c}: {n} objects");
            for e in errs {
                eprintln!("    {e}");
                bad = true;
            }
        }
        assert!(!bad);
        assert!(found_hinge, "Person.prefab hinge (anchor 0.0032851696,-0.18, limits -70..80) not found");
        for c in [50, 61, 233, 212] {
            assert!(counts.contains_key(&c), "no class {c} objects");
        }
    }
}

/// AudioReverbFilter (class 164): Behaviour.enabled, the 14 FMOD SFX reverb parameters in the order
/// AudioReverbFilter::Update (180a99500) sets them (dryLevel, room, roomHF, decayTime, decayHFRatio, reflectionsLevel,
/// reflectionsDelay, reverbLevel, reverbDelay, diffusion, density, hfReference, roomLF, lfReference) and the preset.
#[derive(Clone, Debug)]
pub struct AudioReverbFilter {
    pub enabled: bool,
    pub params: [f32; 14],
    pub preset: i32,
}

impl AudioReverbFilter {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        PPtr::read(r)?;
        let enabled = flag(r)?;
        // Serialized: m_DryLevel, m_Room, m_RoomHF, m_DecayTime, m_DecayHFRatio, m_ReflectionsLevel, m_ReverbLevel,
        // m_ReverbDelay, m_Diffusion, m_Density, m_HFReference, m_RoomLF, m_LFReference, m_ReflectionsDelay.
        let s = floats::<14>(r)?;
        let preset = r.i32()?;
        let params = [s[0], s[1], s[2], s[3], s[4], s[5], s[13], s[6], s[7], s[8], s[9], s[10], s[11], s[12]];
        done(r, "AudioReverbFilter", Self { enabled, params, preset })
    }
}

/// Camera (class 20): the projection the simulation reads.
#[derive(Clone, Debug)]
pub struct Camera {
    pub orthographic: bool,
    pub orthographic_size: f32,
    /// CameraClearFlags: Skybox 1, SolidColor 2, Depth 3, Nothing 4.
    pub clear_flags: i32,
    pub background: [f32; 4],
    pub culling_mask: u32,
}

impl Camera {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        PPtr::read(r)?;
        flag(r)?; // m_Enabled
        let clear_flags = r.i32()?;
        let background = floats::<4>(r)?;
        r.i32()?; // m_projectionMatrixMode
        r.i32()?; // m_GateFitMode (this build has no m_FOVAxisMode)
        floats::<5>(r)?; // m_SensorSize, m_LensShift, m_FocalLength
        floats::<7>(r)?; // m_NormalizedViewPortRect, near/far clip plane, field of view
        let orthographic = flag(r)?;
        let orthographic_size = r.f32()?;
        r.f32()?; // m_Depth
        let culling_mask = r.u32()?;
        r.i32()?; // m_RenderingPath
        PPtr::read(r)?; // m_TargetTexture
        r.i32()?; // m_TargetDisplay
        r.i32()?; // m_TargetEye
        for _ in 0..4 {
            r.bool()?; // m_HDR, m_AllowMSAA, m_AllowDynamicResolution, m_ForceIntoRT
        }
        flag(r)?; // m_OcclusionCulling
        floats::<2>(r)?; // m_StereoConvergence, m_StereoSeparation
        done(r, "Camera", Self { orthographic, orthographic_size, clear_flags, background, culling_mask })
    }
}

/// The head of a ParticleSystem (class 198): its main module's timing (the modules after are not read).
#[derive(Clone, Copy, Debug)]
pub struct ParticleSystemHead {
    pub length_in_sec: f32,
    pub simulation_speed: f32,
    /// ParticleSystemStopAction: 0 None, 1 Disable, 2 Destroy, 3 Callback.
    pub stop_action: i32,
    pub looping: bool,
}

impl ParticleSystemHead {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        PPtr::read(r)?;
        let (length_in_sec, simulation_speed, stop_action) = (r.f32()?, r.f32()?, r.i32()?);
        r.i32()?; // cullingMode
        r.i32()?; // ringBufferMode
        vec2(r)?; // ringBufferLoopRange
        let looping = r.bool()?;
        Ok(Self { length_in_sec, simulation_speed, stop_action, looping })
    }
}

/// What the simulation reads of a TrailRenderer (class 96): the Renderer head as a SpriteRenderer's, then m_Time; the
/// rest (m_Parameters: width, curve, gradient) is rendering, and m_MinVertexDistance, m_Autodestruct and m_Emitting
/// are the last 6 bytes (a float and two bools).
#[derive(Clone, Copy, Debug)]
pub struct TrailHead {
    pub enabled: bool,
    pub time: f32,
    pub min_vertex_distance: f32,
    /// m_Parameters.widthMultiplier (the first field after m_Time).
    pub width_multiplier: f32,
    pub autodestruct: bool,
    pub emitting: bool,
}

/// A Renderer's head: enabled, m_Materials, m_SortingLayerID, m_SortingOrder.
fn renderer_head(r: &mut Reader) -> Result<(bool, Vec<PPtr>, i32, i16), String> {
    PPtr::read(r)?;
    let enabled = r.bool()?;
    r.bytes(8)?;
    r.align();
    r.u32()?; // m_RenderingLayerMask
    r.i32()?; // m_RendererPriority
    (r.u16()?, r.u16()?); // m_LightmapIndex
    let _: [[f32; 4]; 2] = [floats(r)?, floats(r)?];
    let materials = vec(r, PPtr::read)?;
    (r.u16()?, r.u16()?); // m_StaticBatchInfo
    for _ in 0..3 {
        PPtr::read(r)?; // m_StaticBatchRoot, m_ProbeAnchor, m_LightProbeVolumeOverride
    }
    let layer = r.i32()?;
    r.i16()?; // m_SortingLayer
    let order = r.i16()?;
    r.align();
    Ok((enabled, materials, layer, order))
}

/// A LineRenderer's (class 120) Renderer head and m_Parameters.
pub struct LineRendererData {
    pub enabled: bool,
    pub materials: Vec<PPtr>,
    pub layer: i32,
    pub order: i16,
    pub positions: Vec<[f32; 3]>,
    pub width_multiplier: f32,
    /// widthCurve's keys: time, value, inSlope, outSlope.
    pub width_keys: Vec<[f32; 4]>,
    /// widthCurve's weightedMode of each key, its preInfinity and postInfinity (2 is Clamp).
    pub width_weighted: Vec<i32>,
    pub width_infinity: [i32; 2],
    /// colorGradient: eight colours, their colour and alpha times (1/65535), the mode and the two key counts.
    pub gradient: ([[f32; 4]; 8], [u16; 8], [u16; 8], i32, u8, u8),
    pub corner_vertices: i32,
    pub cap_vertices: i32,
    /// LineAlignment: View 0, TransformZ 1 (m_Parameters.alignment).
    pub alignment: i32,
    /// LineTextureMode: Stretch 0, Tile 1, DistributePerSegment 2, RepeatPerSegment 3.
    pub texture_mode: i32,
    pub use_world_space: bool,
    pub looped: bool,
}

impl LineRendererData {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let head = renderer_head(r)?;
        let positions = vec(r, |r| floats::<3>(r))?;
        let mut x = line_parameters(r, head, positions)?;
        x.use_world_space = r.bool()?;
        x.looped = r.bool()?;
        done(r, "LineRenderer", x)
    }
}

/// A TrailRenderer (class 96) as the LineRenderer it draws like (world space, no positions of its own): the Renderer
/// head, m_Time, m_Parameters (a LineRenderer's), m_MinVertexDistance, m_Autodestruct and m_Emitting.
pub struct TrailRendererData {
    pub line: LineRendererData,
    pub time: f32,
    pub min_vertex_distance: f32,
    pub emitting: bool,
}

impl TrailRendererData {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let head = renderer_head(r)?;
        let time = r.f32()?;
        let line = line_parameters(r, head, Vec::new())?;
        let min_vertex_distance = r.f32()?;
        r.bool()?; // m_Autodestruct
        let emitting = r.bool()?;
        done(r, "TrailRenderer", Self { line, time, min_vertex_distance, emitting })
    }
}

/// m_Parameters (LineParameters) after a line's positions: widthMultiplier, widthCurve, colorGradient, the vertex
/// counts, alignment, textureMode, shadowBias and generateLightingData.
fn line_parameters(r: &mut Reader, (enabled, materials, layer, order): (bool, Vec<PPtr>, i32, i16), positions: Vec<[f32; 3]>) -> Result<LineRendererData, String> {
    {
        let width_multiplier = r.f32()?;
        // widthCurve: the keyframes (time, value, inSlope, outSlope, weightedMode, inWeight, outWeight), the two infinities and the rotation order.
        let keys = r.i32()?;
        let mut width_keys = Vec::new();
        let mut width_weighted = Vec::new();
        for _ in 0..keys {
            let k = floats::<4>(r)?;
            width_weighted.push(r.i32()?);
            r.bytes(8)?;
            width_keys.push(k);
        }
        let width_infinity = [r.i32()?, r.i32()?];
        r.bytes(4)?;
        // colorGradient: eight colours, eight colour and alpha times, the mode and the two key counts.
        let mut colors = [[0.0f32; 4]; 8];
        for c in &mut colors {
            *c = floats(r)?;
        }
        let (mut ctime, mut atime) = ([0u16; 8], [0u16; 8]);
        for t in &mut ctime {
            *t = r.u16()?;
        }
        for t in &mut atime {
            *t = r.u16()?;
        }
        let mode = r.i32()?;
        let (nc, na) = (r.bytes(1)?[0], r.bytes(1)?[0]);
        r.align();
        // numCornerVertices, numCapVertices, alignment, textureMode, shadowBias, generateLightingData.
        let corner_vertices = r.i32()?;
        let cap_vertices = r.i32()?;
        let alignment = r.i32()?;
        let texture_mode = r.i32()?;
        r.bytes(4)?;
        r.bool()?;
        r.align();
        Ok(LineRendererData { enabled, materials, layer, order, positions, width_multiplier, width_keys, width_weighted, width_infinity, gradient: (colors, ctime, atime, mode, nc, na), corner_vertices, cap_vertices, alignment, texture_mode, use_world_space: true, looped: false })
    }
}

/// LineRenderer (class 120) m_Positions.size(): positionCount. The positions follow the Renderer head.
pub fn line_position_count(data: &[u8]) -> Result<i32, String> {
    let r = &mut Reader::new(data);
    renderer_head(r)?;
    r.i32()
}

/// A component's m_GameObject.
pub fn game_object_of(data: &[u8]) -> Result<PPtr, String> {
    PPtr::read(&mut Reader::new(data))
}

/// A ParticleSystemRenderer (class 199).
pub struct ParticleRenderer {
    pub enabled: bool,
    pub materials: Vec<PPtr>,
    pub layer: i32,
    pub order: i16,
    pub render_mode: u16,
    /// m_MinParticleSize and m_MaxParticleSize: fractions of the view's width.
    pub size_range: [f32; 2],
    /// ParticleSystemSortMode: 0 none, 1 distance, 2 oldest in front, 3 youngest in front, 4 by depth.
    pub sort_mode: u8,
    pub sorting_fudge: f32,
    /// ParticleSystemRenderSpace: View, World, Local, Facing, Velocity.
    pub alignment: i32,
    pub allow_roll: bool,
    pub pivot: [f32; 3],
    pub flip: [f32; 3],
    pub custom_streams: bool,
    pub apply_active_color_space: bool,
    pub vertex_streams: Vec<u8>,
    pub mask_interaction: i32,
    /// m_CameraVelocityScale, m_VelocityScale, m_LengthScale (Stretched Billboard).
    pub stretch: [f32; 3],
    /// m_FreeformStretching, m_RotateWithStretchDirection.
    pub freeform: bool,
    pub rotate_with_stretch: bool,
}

impl ParticleRenderer {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let (enabled, materials, layer, order) = renderer_head(r)?;
        let render_mode = r.u16()?;
        let sort_mode = r.bytes(2)?[1]; // m_MeshDistribution, m_SortMode
        // m_MinParticleSize, m_MaxParticleSize, then the stretching, fudge, normal direction, shadow bias.
        let sizes = floats::<8>(r)?;
        let alignment = r.i32()?;
        let (pivot, flip) = (floats(r)?, floats(r)?);
        let custom_streams = r.bool()?;
        r.bool()?; // m_EnableGPUInstancing
        let apply_active_color_space = r.bool()?;
        let flags = r.bytes(3)?; // m_AllowRoll, m_FreeformStretching, m_RotateWithStretchDirection
        let (allow_roll, freeform, rotate_with_stretch) = (flags[0] != 0, flags[1] != 0, flags[2] != 0);
        r.align();
        let n = r.i32()? as usize;
        let vertex_streams = r.bytes(n)?.to_vec();
        r.align();
        for _ in 0..4 {
            PPtr::read(r)?; // m_Mesh, m_Mesh1, m_Mesh2, m_Mesh3
        }
        let mask_interaction = r.i32()?;
        if r.pos != data.len() {
            return Err(format!("ParticleSystemRenderer: read {} of {} bytes", r.pos, data.len()));
        }
        Ok(Self { enabled, materials, layer, order, render_mode, size_range: [sizes[0], sizes[1]], sort_mode, sorting_fudge: sizes[5], alignment, allow_roll, pivot, flip, custom_streams, apply_active_color_space, vertex_streams, mask_interaction, stretch: [sizes[2], sizes[3], sizes[4]], freeform, rotate_with_stretch })
    }
}

impl TrailHead {
    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let (enabled, ..) = renderer_head(r)?;
        let time = r.f32()?;
        let width_multiplier = r.f32()?;
        let n = data.len();
        if n < r.pos + 6 {
            return Err("TrailRenderer: no tail".into());
        }
        let tail = &data[n - 6..];
        if tail[4] > 1 || tail[5] > 1 {
            return Err("TrailRenderer: unexpected tail".into());
        }
        let min_vertex_distance = f32::from_le_bytes(tail[..4].try_into().unwrap());
        Ok(Self { enabled, time, width_multiplier, min_vertex_distance, autodestruct: tail[4] == 1, emitting: tail[5] == 1 })
    }
}

/// ParentConstraint (class id 1773428102, the Animation module's hashed id): Behaviour, then m_Weight, m_TranslationAtRest, m_RotationAtRest, m_TranslationOffsets,
/// m_RotationOffsets, the six m_Affect flags, m_Active, m_IsLocked and m_Sources (sourceTransform, weight).
#[derive(Clone, Debug)]
pub struct ParentConstraintData {
    pub enabled: bool,
    pub weight: f32,
    pub translation_at_rest: [f32; 3],
    pub rotation_at_rest: [f32; 3],
    pub translation_offsets: Vec<[f32; 3]>,
    pub rotation_offsets: Vec<[f32; 3]>,
    /// AffectTranslationX/Y/Z, AffectRotationX/Y/Z.
    pub affect: [bool; 6],
    pub active: bool,
    pub locked: bool,
    pub sources: Vec<(PPtr, f32)>,
}

impl ParentConstraintData {
    pub const CLASS: i32 = 1773428102;

    pub fn read(data: &[u8]) -> Result<Self, String> {
        let r = &mut Reader::new(data);
        let _game_object = PPtr::read(r)?;
        let enabled = flag(r)?;
        let weight = r.f32()?;
        let translation_at_rest = floats(r)?;
        let rotation_at_rest = floats(r)?;
        let translation_offsets = vec(r, floats)?;
        let rotation_offsets = vec(r, floats)?;
        let mut affect = [false; 6];
        for a in &mut affect {
            *a = r.bool()?;
        }
        let active = r.bool()?;
        let locked = r.bool()?;
        r.align();
        let sources = vec(r, |r| Ok((PPtr::read(r)?, r.f32()?)))?;
        done(r, "ParentConstraint", Self { enabled, weight, translation_at_rest, rotation_at_rest, translation_offsets, rotation_offsets, affect, active, locked, sources })
    }
}
