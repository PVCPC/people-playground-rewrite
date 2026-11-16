//! DEFAULT POST PROCESSING PROFILE's AutoExposure: LogHistogram.Generate and AutoExposureRenderer.Render, run on the
//! lit picture between LightsEffect and the bloom. The result is a 1x1 texture, Unity's _AutoExposureTex.

use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    core_pipeline::schedule::{Core2d, Core2dSystems},
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_asset::RenderAssets,
        render_resource::{binding_types::*, *},
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        texture::GpuImage,
        view::{ExtractedView, ViewTarget},
        Extract, ExtractSchedule, RenderApp, RenderStartup,
    },
};
use std::num::NonZero;

// The profile's filtering, minLuminance, maxLuminance, keyValue, speedUp and speedDown.
const FILTERING: [f32; 2] = [66.63303, 99.0];
const MIN_LUMINANCE: f32 = 0.0;
const MAX_LUMINANCE: f32 = 9.0;
const KEY_VALUE: f32 = 1.0;
const SPEED_UP: f32 = 4.0;
const SPEED_DOWN: f32 = 0.8;
const PARAM_SIZE: u64 = 64;

/// On the camera whose picture is measured; holds the texture the result goes to.
#[derive(Component, Clone, ExtractComponent)]
#[extract_app(RenderApp)]
pub struct ExposureView(pub Handle<Image>);

/// A 1x1 exposure of 1: Unity's whiteTexture until the first measurement.
pub fn exposure_image() -> Image {
    let one = [0u8, 0x3c].repeat(4);
    let extent = Extent3d { width: 1, height: 1, depth_or_array_layers: 1 };
    let mut image = Image::new(extent, TextureDimension::D2, one, TextureFormat::Rgba16Float, RenderAssetUsages::RENDER_WORLD);
    image.texture_descriptor.usage |= TextureUsages::STORAGE_BINDING;
    image
}

#[derive(Resource)]
struct Exposure {
    layout: BindGroupLayoutDescriptor,
    histogram: CachedComputePipelineId,
    average: CachedComputePipelineId,
    bins: Buffer,
    history: Buffer,
    params: Buffer,
    sampler: Sampler,
    /// m_ResetHistory.
    first: bool,
    delta: f32,
}

pub fn add(app: &mut App) {
    embedded_asset!(app, "exposure.wesl");
    app.add_plugins(ExtractComponentPlugin::<ExposureView>::default());
    let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
    render
        .add_systems(RenderStartup, setup)
        .add_systems(ExtractSchedule, delta)
        .add_systems(Core2d, measure.in_set(Core2dSystems::PostProcess));
}

fn setup(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "exposure",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                storage_buffer_sized(false, NonZero::new(128 * 4)),
                storage_buffer_sized(false, NonZero::new(4)),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
                uniform_buffer_sized(false, NonZero::new(PARAM_SIZE)),
            ),
        ),
    );
    let shader: Handle<Shader> = assets.load("embedded://people_playground_bevy/exposure.wesl");
    let pipeline = |entry: &'static str| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(entry.into()),
            layout: vec![layout.clone()],
            shader: shader.clone(),
            entry_point: Some(entry.into()),
            ..default()
        })
    };
    let (histogram, average) = (pipeline("histogram"), pipeline("average"));
    let buffer = |size, usage| device.create_buffer(&BufferDescriptor { label: Some("exposure"), size, usage, mapped_at_creation: false });
    commands.insert_resource(Exposure {
        histogram,
        average,
        bins: buffer(128 * 4, BufferUsages::STORAGE | BufferUsages::COPY_DST),
        history: buffer(4, BufferUsages::STORAGE),
        params: buffer(PARAM_SIZE, BufferUsages::UNIFORM | BufferUsages::COPY_DST),
        sampler: device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() }),
        layout,
        first: true,
        delta: 0.0,
    });
}

/// Time.unscaledDeltaTime.
fn delta(time: Extract<Res<Time<Real>>>, exposure: Option<ResMut<Exposure>>) {
    if let Some(mut exposure) = exposure {
        exposure.delta = time.delta_secs();
    }
}

fn measure(
    view: ViewQuery<(&ViewTarget, &ExposureView, &ExtractedView)>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
    mut exposure: ResMut<Exposure>,
    mut ctx: RenderContext,
) {
    let (target, ExposureView(output), extracted) = view.into_inner();
    let (Some(histogram), Some(average), Some(output)) = (cache.get_compute_pipeline(exposure.histogram), cache.get_compute_pipeline(exposure.average), images.get(output)) else {
        return;
    };
    let (w, h) = (extracted.viewport.z, extracted.viewport.w);
    let values = [
        FILTERING[0] * 0.01, FILTERING[1] * 0.01, MIN_LUMINANCE.exp2(), MAX_LUMINANCE.exp2(),
        SPEED_DOWN, SPEED_UP, KEY_VALUE, exposure.delta,
        1.0 / 18.0, 9.0 / 18.0, w as f32, h as f32,
        exposure.first as u32 as f32, 0.0, 0.0, 0.0,
    ];
    let bytes: Vec<u8> = values.iter().flat_map(|v: &f32| v.to_le_bytes()).collect();
    queue.write_buffer(&exposure.params, 0, &bytes);
    let group = ctx.render_device().create_bind_group(
        None,
        &cache.get_bind_group_layout(&exposure.layout),
        &BindGroupEntries::sequential((
            target.main_texture_view(),
            &exposure.sampler,
            exposure.bins.as_entire_buffer_binding(),
            exposure.history.as_entire_buffer_binding(),
            &output.texture_view,
            exposure.params.as_entire_buffer_binding(),
        )),
    );
    let encoder = ctx.command_encoder();
    encoder.clear_buffer(&exposure.bins, 0, None);
    let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor { label: Some("exposure"), timestamp_writes: None });
    pass.set_bind_group(0, &group, &[]);
    pass.set_pipeline(histogram);
    pass.dispatch_workgroups((w / 2).div_ceil(16).max(1), (h / 2).div_ceil(16).max(1), 1);
    pass.set_pipeline(average);
    pass.dispatch_workgroups(1, 1, 1);
    drop(pass);
    exposure.first = false;
}
