//! CameraControlBehaviour (the Main scene's camera): pan, zoom and follow in LateUpdate. It owns
//! Global.CameraPosition (the main camera's localPosition under the shake root) and the camera's orthographicSize; the
//! window copies both onto its camera after the frame. FixedUpdate/Update only keep MovementDelta, which feeds
//! Global.CameraSpeedCompensatingMousePosition (written by Global.Update, read by no class), and PostRender has no listener.
#![allow(non_snake_case)]

use super::{mathf, Game};

/// The serialized fields of the Main scene's CameraControlBehaviour.
const ZOOM_SPEED: f32 = 1.0;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 1000.0;
const EXTEND: f32 = 50.0;
const KEYBOARD_PAN_SPEED_MULTIPLIER: f32 = 4.0;
const KEYBOARD_ZOOM_SPEED_MULTIPLIER: f32 = 10.0;

#[derive(Clone, Debug, Default)]
pub struct CameraControl {
    pub started: bool,
    oldMousePos: [f32; 3],
    oldCamPos: [f32; 3],
    pub targetZoom: f32,
    pub(crate) targetPos: [f32; 3],
    intialZoom: f32,
    /// PhysicalBehaviours, in the order they were added.
    pub CurrentlyFollowing: Vec<usize>,
    /// Global.CameraPosition
    pub position: [f32; 3],
    /// MovementDelta and LastPos (world positions of the camera's Transform).
    pub MovementDelta: [f32; 3],
    LastPos: [f32; 3],
}

/// What LateUpdate reads from the window: Screen.width/height, Application.isFocused, Input.mouseScrollDelta.y and
/// Global.main.UILock (the pointer over the UI).
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowInput {
    pub size: [f32; 2],
    pub focused: bool,
    pub scroll: f32,
    pub ui_lock: bool,
}

impl Game {
    /// FollowingViewBehaviour.Update (while its object is active: CameraControlBehaviour.LateUpdate's SetActive): the
    /// followed objects' names in the follow status item.
    pub fn following_view_update(&mut self) {
        if !self.following_view_active {
            return;
        }
        let list = &self.camera_control.CurrentlyFollowing;
        if list.is_empty() {
            return;
        }
        self.following_view_text = list.iter().map(|&p| Self::following_view_transform(&self.physicals[p].name)).collect::<Vec<_>>().join(", ");
    }

    /// FollowingViewBehaviour.Transform: a space before each capital but the first, then "(Clone)" removed.
    pub(crate) fn following_view_transform(name: &str) -> String {
        let mut out = String::new();
        for (i, c) in name.chars().enumerate() {
            if c.is_uppercase() && i != 0 {
                out.push(' ');
            }
            out.push(c);
        }
        out.replace("(Clone)", "")
    }

    /// CameraControlBehaviour.Start: intialZoom = targetZoom = the camera's orthographicSize (FindObjectOfType of the
    /// FollowingViewBehaviour is the status bar's).
    pub fn camera_control_start(&mut self) {
        let c = &mut self.camera_control;
        c.intialZoom = self.camera.orthographic_size;
        c.targetZoom = c.intialZoom;
        c.started = true;
    }

    /// CameraControlBehaviour.FixedUpdate: unless paused, MovementDelta = transform.position - LastPos and LastPos =
    /// transform.position (the camera's world position: the shake root plus Global.CameraPosition).
    pub fn camera_control_fixed_update(&mut self) {
        if self.paused {
            return;
        }
        let (r, at) = (self.camera.root, self.camera_control.position);
        let position = [r[0] + at[0], r[1] + at[1], r[2] + at[2]];
        let c = &mut self.camera_control;
        c.MovementDelta = [position[0] - c.LastPos[0], position[1] - c.LastPos[1], position[2] - c.LastPos[2]];
        c.LastPos = position;
    }

    /// CameraControlBehaviour.Update: MovementDelta = default while paused.
    pub fn camera_control_update(&mut self) {
        if self.paused {
            self.camera_control.MovementDelta = [0.0; 3];
        }
    }

    /// CameraControlBehaviour.ResetCamera
    pub fn camera_control_reset(&mut self) {
        let c = &mut self.camera_control;
        c.targetPos = [0.0; 3];
        c.targetZoom = c.intialZoom;
        c.CurrentlyFollowing.clear();
        c.position = c.targetPos;
        self.camera.orthographic_size = c.targetZoom;
    }

    /// camera.ScreenToWorldPoint of a pixel (from the bottom left) for the orthographic main camera at the shaken root.
    fn camera_screen_to_world(&self, pixel: [f32; 2], size: [f32; 2]) -> [f32; 3] {
        let unit = 2.0 * self.camera.orthographic_size as f64 / size[1] as f64;
        let at = self.camera_control.position;
        // The camera's rotation about z turns the offset from the view centre.
        let o = mathf::rotate(self.camera.rotation, [((pixel[0] as f64 - size[0] as f64 * 0.5) * unit) as f32, ((pixel[1] as f64 - size[1] as f64 * 0.5) * unit) as f32, 0.0]);
        [(self.camera.root[0] as f64 + at[0] as f64 + o[0] as f64) as f32, (self.camera.root[1] as f64 + at[1] as f64 + o[1] as f64) as f32, 0.0]
    }

    /// The local clampPos of LateUpdate: inside BoundingBox grown by Extend, at z -10.
    fn camera_clamp_pos(&self, v: [f32; 3]) -> [f32; 3] {
        let b = &self.BoundingBox;
        let axis = |k: usize| mathf::clamp(v[k], b.center[k] - b.extents[k] - EXTEND, b.center[k] + b.extents[k] + EXTEND);
        [axis(0), axis(1), -10.0]
    }

    /// CameraControlBehaviour.GetAveragePosition
    pub(crate) fn camera_average_position(&mut self) -> [f32; 3] {
        let mut zero = [0.0f32; 3];
        let mut num = self.camera_control.CurrentlyFollowing.len() as i32;
        for &p in &self.camera_control.CurrentlyFollowing {
            let ph = &self.physicals[p];
            if ph.destroyed || ph.isDisintegrated {
                num -= 1;
            } else {
                for k in 0..3 {
                    zero[k] += ph.transform.position[k];
                }
            }
        }
        if num == 0 {
            self.camera_control.CurrentlyFollowing.clear();
            return self.camera_control.targetPos;
        }
        zero.map(|v| v / num as f32)
    }

    /// CameraControlBehaviour.ZoomCamera
    pub(crate) fn camera_zoom(&mut self, i: f32, cursor_as_origin: bool) {
        if self.input.ui_block {
            return;
        }
        let num = i * ZOOM_SPEED * 0.1 * self.prefs.ZoomSensitivity;
        let size = self.camera.orthographic_size;
        let c = &mut self.camera_control;
        c.targetZoom -= num * size;
        if c.targetZoom < MAX_ZOOM && c.targetZoom > MIN_ZOOM && cursor_as_origin {
            let v = [self.MousePosition[0] - c.position[0], self.MousePosition[1] - c.position[1]];
            c.targetPos[0] += v[0] * num;
            c.targetPos[1] += v[1] * num;
        }
    }

    /// CameraControlBehaviour.LateUpdate
    pub fn camera_control_late_update(&mut self) {
        if !self.camera_control.started {
            self.camera_control_start();
        }
        // PhysicalBehaviour.OnDestroy removes a destroyed object from CurrentlyFollowing.
        let physicals = &self.physicals;
        self.camera_control.CurrentlyFollowing.retain(|&p| !physicals[p].destroyed);
        if !self.camera_control.CurrentlyFollowing.is_empty() {
            self.camera_control.targetPos = self.camera_average_position();
        }
        self.camera_control.targetPos[2] = -10.0;
        // Input.mousePosition inside the screen, the window focused, no pause menu (the frame does not run under it).
        let window = self.input.window;
        let mouse = self.input.screen.map(|s| s.mouse);
        let on_screen = match (window, mouse) {
            (Some(w), Some(m)) => m[0] > 0.0 && m[1] > 0.0 && m[0] < w.size[0] && m[1] < w.size[1],
            _ => false,
        };
        let flag2 = window.is_some_and(|w| w.focused) && on_screen;
        let c = &mut self.camera_control;
        c.targetZoom = mathf::clamp(c.targetZoom, MIN_ZOOM, MAX_ZOOM);
        self.camera.orthographic_size = c.targetZoom;
        c.position = c.targetPos;
        if !self.input.ui_block && flag2 {
            let w = window.unwrap();
            if w.scroll.abs() > 0.0 && !self.ui_lock() {
                self.camera_zoom(w.scroll, true);
            }
            if self.input.down(crate::il!(str "CameraControlBehaviour::LateUpdate" 0)) {
                let c = &mut self.camera_control;
                c.oldMousePos = self.MousePosition;
                c.oldCamPos = c.position;
                c.CurrentlyFollowing.clear();
            }
            if self.input.held(crate::il!(str "CameraControlBehaviour::LateUpdate" 0)) {
                self.camera_control.position = self.camera_control.oldCamPos;
                let world = self.camera_screen_to_world(mouse.unwrap(), w.size);
                let c = &mut self.camera_control;
                let v = [c.oldMousePos[0] - world[0], c.oldMousePos[1] - world[1]];
                c.position = [c.oldCamPos[0] + v[0], c.oldCamPos[1] + v[1], c.oldCamPos[2]];
                c.targetPos = c.position;
            }
            let dt = self.time.unscaledDeltaTime;
            for (id, dir) in [("panLeft", [-1.0f32, 0.0]), ("panRight", [1.0, 0.0]), ("panUp", [0.0, 1.0]), ("panDown", [0.0, -1.0])] {
                if self.input.held(id) {
                    let c = &mut self.camera_control;
                    c.CurrentlyFollowing.clear();
                    let s = KEYBOARD_PAN_SPEED_MULTIPLIER * c.targetZoom * dt;
                    let t = [c.targetPos[0] + s * dir[0], c.targetPos[1] + s * dir[1], c.targetPos[2]];
                    self.camera_control.targetPos = self.camera_clamp_pos(t);
                }
            }
            if self.input.held("zoomIn") {
                self.camera_zoom(dt * KEYBOARD_ZOOM_SPEED_MULTIPLIER, false);
            }
            if self.input.held("zoomOut") {
                self.camera_zoom(-dt * KEYBOARD_ZOOM_SPEED_MULTIPLIER, false);
            }
        }
        let c = &mut self.camera_control;
        c.targetZoom = mathf::clamp(c.targetZoom, MIN_ZOOM, MAX_ZOOM);
        self.following_view_active = !self.camera_control.CurrentlyFollowing.is_empty();
        self.camera_control.position = self.camera_clamp_pos(self.camera_control.position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_zoom_and_clamp() {
        let mut g = Game::default();
        g.camera.orthographic_size = 5.0;
        g.prefs.ZoomSensitivity = 1.0;
        g.camera_control_late_update();
        assert_eq!((g.camera_control.targetZoom, g.camera_control.position), (5.0, [0.0, 0.0, -10.0]));
        // Keyboard zoom in for 0.1 s: 5 - 1 * 0.1 * 5 = 4.5, applied to the camera on the next LateUpdate.
        g.input.window = Some(WindowInput { size: [100.0, 100.0], focused: true, scroll: 0.0, ui_lock: false });
        g.input.screen = Some(crate::game::tool::Screen { mouse: [50.0, 50.0], origin: [0.0, 0.0], scale: 1.0, view: None });
        g.input.controls.insert("zoomIn".into(), crate::game::tool::Control { down: false, held: true, up: false });
        g.time.unscaledDeltaTime = 0.1;
        g.camera_control_late_update();
        assert!((g.camera_control.targetZoom - 4.5).abs() < 1e-6);
        assert_eq!(g.camera.orthographic_size, 5.0);
        // Far outside the box is clamped to its edge plus Extend.
        g.camera_control.targetPos = [-5000.0, 5000.0, 0.0];
        g.input.controls.clear();
        g.camera_control_late_update();
        let b = g.BoundingBox;
        assert_eq!(g.camera_control.position, [b.center[0] - b.extents[0] - EXTEND, b.center[1] + b.extents[1] + EXTEND, -10.0]);
    }
}
