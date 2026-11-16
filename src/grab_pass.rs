//! GrabPass { "_BackgroundTexture" } (Jaap/ForcefieldShader): Unity copies the camera's colour target into the named texture
//! once per frame, when the first renderer whose shader has that GrabPass draws; every user of the name samples that copy.
//! Here the 2D transparent pass is bevy's main_transparent_pass_2d with the copy put in: the sorted items before the first
//! `GrabPassUser` draw, the pass ends, the target is copied into `GrabTexture`, and the rest draw in a second pass.
use bevy::{
    core_pipeline::{core_2d::{main_opaque_pass_2d, main_transparent_pass_2d, Transparent2d}, schedule::{Core2d, Core2dSystems}},
    ecs::schedule::ScheduleCleanupPolicy,
    image::ImageSampler,
    prelude::*,
    render::{
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_asset::RenderAssets,
        render_phase::{PhaseItem, ViewSortedRenderPhases},
        render_resource::{Extent3d, Origin3d, RenderPassDescriptor, StoreOp, TexelCopyTextureInfo, TextureAspect, TextureDimension, TextureFormat, TextureUsages},
        renderer::{RenderContext, ViewQuery},
        sync_world::MainEntity,
        texture::GpuImage,
        view::{ExtractedView, ViewDepthStencilTexture, ViewTarget},
        RenderApp,
    },
    window::PrimaryWindow,
};

/// The `_BackgroundTexture` every GrabPass user samples: the main camera's target size, its HDR format.
#[derive(Resource, Clone, ExtractResource)]
#[extract_app(RenderApp)]
pub struct GrabTexture(pub Handle<Image>);

/// A sprite drawn with a shader that has the GrabPass.
#[derive(Component, Clone, Copy, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct GrabPassUser;

pub struct GrabPassPlugin;

impl Plugin for GrabPassPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((ExtractResourcePlugin::<GrabTexture>::default(), ExtractComponentPlugin::<GrabPassUser>::default()))
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, follow_window);
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        let world = render_app.world_mut();
        world.schedule_scope(Core2d, |world, schedule| {
            schedule.remove_systems_in_set(main_transparent_pass_2d, world, ScheduleCleanupPolicy::RemoveSystemsOnly).expect("bevy's main_transparent_pass_2d in Core2d");
        });
        render_app.add_systems(Core2d, transparent_pass_with_grab.after(main_opaque_pass_2d).in_set(Core2dSystems::MainPass));
    }
}

fn grab_image(w: u32, h: u32) -> Image {
    let mut image = Image::new_fill(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, &[0; 8], TextureFormat::Rgba16Float, default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    // The grab texture is sampled bilinear, clamped.
    image.sampler = ImageSampler::linear();
    image
}

fn setup(mut commands: Commands, window: Single<&Window, With<PrimaryWindow>>, settings: Option<Res<crate::ui::Settings>>, mut images: ResMut<Assets<Image>>) {
    // The scene's size (lights::render_size): the copy is of the scene target.
    let (w, h, _) = crate::lights::render_size(&window, settings.as_deref());
    commands.insert_resource(GrabTexture(images.add(grab_image(w, h))));
}

fn follow_window(window: Single<&Window, With<PrimaryWindow>>, settings: Option<Res<crate::ui::Settings>>, grab: Res<GrabTexture>, mut images: ResMut<Assets<Image>>) {
    let (w, h, _) = crate::lights::render_size(&window, settings.as_deref());
    let size = UVec2::new(w, h);
    if images.get(&grab.0).is_some_and(|i| i.size() != size) {
        images.insert(&grab.0, grab_image(size.x, size.y)).expect("the grab texture");
    }
}

/// main_transparent_pass_2d (bevy_core_pipeline 0.20 core_2d), split at the first GrabPass user for the copy.
fn transparent_pass_with_grab(
    world: &World,
    view: ViewQuery<(&ExtractedCamera, &ExtractedView, &ViewTarget, &ViewDepthStencilTexture)>,
    transparent_phases: Res<ViewSortedRenderPhases<Transparent2d>>,
    grab: Option<Res<GrabTexture>>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (camera, extracted_view, target, depth) = view.into_inner();
    let Some(phase) = transparent_phases.get(&extracted_view.retained_view_entity) else { return };
    if phase.items.is_empty() {
        return;
    }
    // A sorted item's entity() is a placeholder: the render-world users by their main-world entity.
    let users: Vec<MainEntity> = world.try_query_filtered::<&MainEntity, With<GrabPassUser>>().map_or(Vec::new(), |mut q| q.iter(world).copied().collect());
    let first = phase.items.values().position(|x| users.contains(&x.main_entity()));
    let grab = grab.as_ref().and_then(|g| images.get(&g.0));
    let split = first.zip(grab);
    let ranges: Vec<std::ops::Range<usize>> = match split {
        Some((k, _)) => vec![0..k, k..phase.items.len()],
        None => vec![0..phase.items.len()],
    };
    for (n, range) in ranges.into_iter().enumerate() {
        if n == 1 {
            let (_, grab) = split.unwrap();
            let size = target.main_texture().size();
            let extent = Extent3d { width: size.width.min(grab.texture.width()), height: size.height.min(grab.texture.height()), depth_or_array_layers: 1 };
            assert_eq!(target.main_texture_format(), grab.texture.format(), "GrabPass: the view's format");
            ctx.command_encoder().copy_texture_to_texture(
                TexelCopyTextureInfo { texture: target.main_texture(), mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
                TexelCopyTextureInfo { texture: &grab.texture, mip_level: 0, origin: Origin3d::ZERO, aspect: TextureAspect::All },
                extent,
            );
        }
        // An empty first range still begins the pass: the attachment's first use clears it.
        let color_attachments = [Some(target.get_color_attachment())];
        let depth_stencil_attachment = Some(depth.get_attachment(StoreOp::Store));
        let mut render_pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("main_transparent_pass_2d"),
            color_attachments: &color_attachments,
            depth_stencil_attachment,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = camera.viewport.as_ref() {
            render_pass.set_camera_viewport(viewport);
        }
        if let Err(err) = phase.render_range(&mut render_pass, world, view_entity, range) {
            error!("Error encountered while rendering the transparent 2D phase {err:?}");
        }
    }
}
