//! ThumbnailCreator: the picture ContraptionSerialiser.SaveThumbnail writes beside a saved contraption. ThumbnailCreator.CreateThumbnail converts the
//! states at its Center, shows only those objects to its own camera (their layer set to the camera's), and TakePicture renders them into a 512 x 512
//! RGB24 texture: the camera and Background centred on the objects, orthographicSize from `contraption::thumbnail_framing`, then EncodeToPNG. Here the saved objects' sprites join a render layer
//! only the thumbnail camera draws, and the picture is read back from a Screenshot of its target image.

use crate::{
    game::contraption::{ThumbnailJob, THUMBNAIL_RESOLUTION},
    play::{LimbSprite, ObjectSprite, Play, RendererSprite},
};
use bevy::{
    camera::{visibility::RenderLayers, ClearColorConfig, ImageRenderTarget, RenderTarget, ScalingMode},
    prelude::*,
    render::{render_resource::TextureFormat, view::screenshot::{Screenshot, ScreenshotCaptured}},
};
use std::sync::{Arc, Mutex};

/// The render layer ThumbnailCreator.Camera draws (ThumbnailCreator's own culling mask).
const THUMBNAIL_LAYER: usize = 29;

/// ThumbnailCreator's scene fields: ThumbnailPadding, the Camera's solid background colour and the Background's SpriteRenderer.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub padding: f32,
    pub clear: [f32; 4],
    background: crate::play::RectPrefab,
    /// Center.position: where CreateThumbnail places the states (Convert's newOrigin), so TakePicture's camera sits there + the framing.
    pub center: [f32; 2],
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            padding: 0.5,
            clear: Default::default(),
            background: Default::default(),
            center: [0.0; 2],
        }
    }
}

/// ThumbnailCreator.Awake: Main = this (the scene's one ThumbnailCreator, found by class here), and its scene fields.
pub fn read_scene(db: &mut crate::assets::db::AssetDb) -> Option<Scene> {
    use crate::assets::{classes::{PPtr, Transform}, components::Camera, mono::Value};
    let s = *db.find_scripts("ThumbnailCreator").ok()?.first()?;
    let (_, _, v) = db.mono(s).ok()??;
    let field = |db: &crate::assets::db::AssetDb, n: &str| v.get(n).and_then(Value::pptr).and_then(|p| db.resolve(s.file, p));
    let padding = v.get("ThumbnailPadding")?.f32()?;
    let camera = Camera::read(db.data(field(db, "Camera")?)).ok()?;
    // CameraClearFlags.SolidColor; the culling mask is the thumbnail layer alone.
    if camera.clear_flags != 2 || camera.culling_mask.count_ones() != 1 {
        unimplemented!("mod-only: Camera.clearFlags: a ThumbnailCreator camera that does not clear to a solid colour and cull to one layer");
    }
    let background = field(db, "Background")?;
    let go = db.resolve(background.file, Transform::read(db.data(background)).ok()?.game_object)?;
    // Center's world position: its local TRS, then each parent's.
    let mut chain = Vec::new();
    let mut t = field(db, "Center")?;
    loop {
        let tr = Transform::read(db.data(t)).ok()?;
        let w = |v: [f32; 3]| [v[0], v[1], v[2], 0.0];
        chain.push(crate::unity::transform::Trs { t: w(tr.local_position), q: tr.local_rotation, s: w(tr.local_scale) });
        if tr.father.is_null() {
            break;
        }
        t = db.resolve(t.file, tr.father)?;
    }
    let m = crate::unity::transform::local_to_world(&chain);
    let _: PPtr;
    Some(Scene { padding, clear: camera.background, background: crate::play::scene_sprite(db, go)?, center: [m[12], m[13]] })
}

pub struct Active {
    job: ThumbnailJob,
    camera: Entity,
    image: Handle<Image>,
    frames: u8,
    shot: Option<Entity>,
    done: Arc<Mutex<Option<Result<(), String>>>>,
    shown: Vec<Entity>,
    background: Entity,
}

/// EncodeToPNG of an RGB24 picture: `rgba` rows from the top.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, width, height);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().map_err(|e| e.to_string())?.write_image_data(&rgb).map_err(|e| e.to_string())?;
    Ok(out)
}

/// ThumbnailCreator.CreateThumbnail for the oldest pending SaveThumbnail: frame 0 shows the objects to the camera, frame 2 reads the picture, then the camera goes.
pub fn thumbnails(
    mut commands: Commands,
    mut play: Option<ResMut<Play>>,
    mut images: ResMut<Assets<Image>>,
    mut active: Local<Option<Active>>,
    mut sprites: Query<(Entity, &mut RenderLayers, Option<&LimbSprite>, Option<&ObjectSprite>, Option<&RendererSprite>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut glows: ResMut<Assets<crate::lights::Glow>>,
    mut panels: ResMut<Assets<crate::lights::Panels>>,
    mut parts: ResMut<Assets<crate::particles::ParticleMaterial>>,
    mut textures: ResMut<crate::play::Textures>,
) {
    let Some(play) = play.as_deref_mut() else {
        *active = None;
        return;
    };
    if active.is_none() {
        if play.game.contraption.thumbnails.is_empty() {
            return;
        }
        let job = play.game.contraption.thumbnails.remove(0);
        let mut image = Image::new_target_texture(THUMBNAIL_RESOLUTION, THUMBNAIL_RESOLUTION, TextureFormat::Rgba8UnormSrgb, None);
        image.data = Some(vec![0; (THUMBNAIL_RESOLUTION * THUMBNAIL_RESOLUTION * 4) as usize]);
        let image = images.add(image);
        // Camera.orthographicSize = size / 2
        let camera = commands
            .spawn((
                Camera2d,
                Camera { order: -10, clear_color: ClearColorConfig::Custom(Color::srgba(job.scene.clear[0], job.scene.clear[1], job.scene.clear[2], job.scene.clear[3])), ..default() },
                RenderTarget::Image(ImageRenderTarget { handle: image.clone(), scale_factor: 1.0 }),
                Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::FixedVertical { viewport_height: job.size }, ..OrthographicProjection::default_2d() }),
                Transform::from_xyz(job.centre[0], job.centre[1], 0.0),
                RenderLayers::layer(THUMBNAIL_LAYER),
            ))
            .id();
        let mut shown = Vec::new();
        for (e, mut layers, limb, object, renderer) in &mut sprites {
            let p = limb.map(|l| l.0).or(object.map(|o| o.0));
            if p.is_some_and(|p| job.physicals.contains(&p)) && renderer.is_none() {
                *layers = layers.clone().with(THUMBNAIL_LAYER);
                shown.push(e);
            }
        }
        // ThumbnailCreator.Background: its sprite, scaled by the camera's orthographicSize, behind the objects.
        let mut background = commands.spawn((Transform { translation: Vec3::new(job.centre[0], job.centre[1], -0.9), scale: Vec3::splat(job.size / 2.0), ..default() }, RenderLayers::layer(THUMBNAIL_LAYER)));
        let size = [2.0, 2.0];
        let id = background.id();
        crate::play::insert_scene_sprite(&play.game, &job.scene.background, size, &mut images, &mut meshes, &mut materials, &mut glows, &mut panels, &mut parts, &mut textures, background.reborrow());
        *active = Some(Active { job, camera, image, frames: 0, shot: None, done: Arc::new(Mutex::new(None)), shown, background: id });
        return;
    }
    let a = active.as_mut().unwrap();
    a.frames += 1;
    if a.frames == 3 && a.shot.is_none() {
        let (path, done) = (a.job.path.clone(), a.done.clone());
        let e = commands
            .spawn(Screenshot::image(a.image.clone()))
            .observe(move |c: On<ScreenshotCaptured>| {
                let r = c.image.clone().try_into_dynamic().map_err(|e| e.to_string()).and_then(|d| {
                    let rgba = d.to_rgba8();
                    let png = encode_png(rgba.width(), rgba.height(), rgba.as_raw())?;
                    std::fs::write(&path, png).map_err(|e| e.to_string())
                });
                *done.lock().unwrap() = Some(r);
            })
            .id();
        a.shot = Some(e);
    }
    let finished = a.done.lock().unwrap().take();
    if let Some(r) = finished {
        // Debug.LogException
        if let Err(e) = r {
            error!("ThumbnailCreator: {e}");
        }
        for (e, mut layers, ..) in &mut sprites {
            if a.shown.contains(&e) {
                *layers = layers.clone().without(THUMBNAIL_LAYER);
            }
        }
        // Camera.gameObject.SetActive(false)
        commands.entity(a.camera).despawn();
        commands.entity(a.background).despawn();
        *active = None;
        play.game.contraption.catalog_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn png_is_rgb_and_top_down() {
        let png = super::encode_png(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        let mut r = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let i = r.next_frame(&mut buf).unwrap();
        assert_eq!(i.color_type, png::ColorType::Rgb);
        assert_eq!(buf, [255, 0, 0, 0, 255, 0]);
    }
}
