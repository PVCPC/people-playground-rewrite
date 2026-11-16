//! UnityEngine.UI.Scrollbar as the scenes author it: the bar and its Sliding Area and Handle rects and colours are read from the install, the handle's
//! place from the ScrollRect's normalised position (Scrollbar.UpdateVisuals, direction BottomToTop).

use super::{scene, view::Scroll};
use crate::assets::classes::RectTransform;
use bevy::prelude::*;

/// A scroll area's bar: AutoHide (m_VerticalScrollbarVisibility 1 and 2) bars are hidden while the content fits.
/// AutoHideAndExpandViewport (2): ScrollRect.UpdateScrollbarLayout gives the viewport the bar's width and spacing
/// (`expand`) back while it is hidden, and takes them while it shows.
#[derive(Component)]
pub struct ScrollbarBar {
    id: u8,
    permanent: bool,
    expand: Option<f32>,
}

/// The Handle, with its parent Sliding Area's rect.
#[derive(Component)]
pub struct ScrollbarHandle {
    id: u8,
    sliding: RectTransform,
    handle: RectTransform,
}

/// A RectTransform as an absolute Node inside its parent: anchors as percentages, the offsets as margins.
pub fn rect_node(r: RectTransform) -> Node {
    let mut n = Node { position_type: PositionType::Absolute, ..default() };
    // x runs right, y runs up in Unity; the node's top is the parent's top.
    let (min, max, pos, size, piv) = (r.anchor_min, r.anchor_max, r.anchored_position, r.size_delta, r.pivot);
    if min[0] == max[0] {
        n.left = percent(min[0] * 100.0);
        n.margin.left = px(pos[0] - size[0] * piv[0]);
        n.width = px(size[0]);
    } else {
        n.left = percent(min[0] * 100.0);
        n.right = percent((1.0 - max[0]) * 100.0);
        n.margin.left = px(pos[0] - size[0] * piv[0]);
        n.margin.right = px(-(pos[0] + size[0] * (1.0 - piv[0])));
    }
    if min[1] == max[1] {
        n.top = percent((1.0 - max[1]) * 100.0);
        n.margin.top = px(-pos[1] - size[1] * (1.0 - piv[1]));
        n.height = px(size[1]);
    } else {
        n.top = percent((1.0 - max[1]) * 100.0);
        n.bottom = percent(min[1] * 100.0);
        n.margin.top = px(-(pos[1] + size[1] * (1.0 - piv[1])));
        n.margin.bottom = px(pos[1] - size[1] * piv[1]);
    }
    n
}

/// A rect's offsets times `k` (the canvas units of a ScrollRect drawn at localScale k: the context menu's 1.8).
fn scaled(mut r: RectTransform, k: f32) -> RectTransform {
    r.anchored_position = r.anchored_position.map(|v| v * k);
    r.size_delta = r.size_delta.map(|v| v * k);
    r
}

/// Spawn the vertical Scrollbar at `bar` (a path in `scene`, whose ScrollRect is at `scroll_rect`, drawn at scale `k`) over the scroll area `id`.
pub fn spawn(parent: &mut ChildSpawnerCommands, id: u8, scene_name: &str, scroll_rect: &str, bar: &str, k: f32) {
    let sliding = scaled(scene::rect(scene_name, &format!("{bar}/Sliding Area")), k);
    let handle = scaled(scene::rect(scene_name, &format!("{bar}/Sliding Area/Handle")), k);
    let visibility = scene::number(scene_name, scroll_rect, "ScrollRect", "m_VerticalScrollbarVisibility");
    let permanent = visibility == 0.0;
    let bar_rect = scaled(scene::rect(scene_name, bar), k);
    // ScrollRect.m_VSliderWidth is the bar's rect width (its localScale left out), plus the spacing.
    let expand = (visibility == 2.0).then(|| bar_rect.size_delta[0] + k * scene::number(scene_name, scroll_rect, "ScrollRect", "m_VerticalScrollbarSpacing"));
    // The bar is drawn at its localScale x about its pivot (the context menu's bar: 0.5); its children span its width.
    let mut drawn = bar_rect;
    if drawn.anchor_min[0] == drawn.anchor_max[0] {
        drawn.size_delta[0] *= drawn.scale[0];
    }
    parent
        .spawn((rect_node(drawn), BackgroundColor(scene::image_color(scene_name, bar)), ScrollbarBar { id, permanent, expand }, Pickable::IGNORE))
        .with_children(|b| {
            b.spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(0), ..default() }, BackgroundColor(scene::image_color(scene_name, &format!("{bar}/Sliding Area/Handle"))), ScrollbarHandle { id, sliding, handle }, Pickable::IGNORE));
        });
}

/// Scrollbar.size = viewport / content and value = 1 - scrolled / scrollable; the handle's anchors (value * (1 - size), + size) in the Sliding Area, plus the
/// Handle's own offsets.
pub fn scrollbars(
    areas: Query<(&Scroll, &ComputedNode, &ScrollPosition)>,
    mut bars: Query<(&ScrollbarBar, &ComputedNode, &mut Node), Without<ScrollbarHandle>>,
    mut handles: Query<(&ScrollbarHandle, &mut Node), Without<ScrollbarBar>>,
    bar_sizes: Query<(&ScrollbarBar, &ComputedNode)>,
    mut viewports: Query<(&Scroll, &mut Node), (Without<ScrollbarBar>, Without<ScrollbarHandle>)>,
) {
    let area = |id: u8| areas.iter().find(|(s, _, _)| s.0 == id).map(|(_, n, p)| (n, p));
    let mut rights = Vec::new();
    for (bar, _, mut node) in &mut bars {
        let Some((a, _)) = area(bar.id) else { continue };
        let fits = a.content_size().y <= a.size().y;
        let display = if fits && !bar.permanent { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
        if let Some(w) = bar.expand {
            rights.push((bar.id, px(if display == Display::None { 0.0 } else { w })));
        }
    }
    for (s, mut node) in &mut viewports {
        if let Some(&(_, right)) = rights.iter().find(|(id, _)| *id == s.0) {
            if node.right != right {
                node.right = right;
            }
        }
    }
    for (h, mut node) in &mut handles {
        let (Some((a, pos)), Some((_, bar))) = (area(h.id), bar_sizes.iter().find(|(b, _)| b.id == h.id)) else { continue };
        let k = bar.inverse_scale_factor();
        let length = bar.size().y * k;
        let (view, content) = (a.size().y, a.content_size().y);
        let size = if content > view { view / content } else { 1.0 };
        let scrollable = ((content - view) * a.inverse_scale_factor()).max(0.0);
        let value = if scrollable > 0.0 { 1.0 - pos.y / scrollable } else { 0.0 };
        // The Sliding Area is the bar inset by its sizeDelta; the Handle anchors inside it and adds its own sizeDelta.
        let (s, hd) = (h.sliding, h.handle);
        let area_length = length + s.size_delta[1];
        let bottom = s.anchored_position[1] - s.size_delta[1] * s.pivot[1];
        let low = value * (1.0 - size);
        let (from, to) = (bottom + low * area_length - hd.size_delta[1] * hd.pivot[1], bottom + (low + size) * area_length + hd.size_delta[1] * (1.0 - hd.pivot[1]));
        node.top = px(length - to);
        node.height = px(to - from);
    }
}
