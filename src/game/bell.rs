//! BellBehaviour
#![allow(non_snake_case)]

use super::{mathf, script, Game};

script! {
    Bell {
        Counter: i32 = 0,
        Intensity: f32 = 1.0,
        Offset: f32 = 0.1,
    }
    links {
        /// PhysicalBehaviour, on the bell's own GameObject.
        physical: usize = 0,
        /// The component's instance id (the checker loads a recording's Counter by it).
        id: i32 = 0,
    }
}

impl Game {
    // BellClipContainer.constructor: BellClips = new List<BellClip>() (the scene's list replaces it: `bell_clips`).
    /// BellBehaviour.Use, BellClipContainer..ctor: OverrideImpactSounds.PickRandom() played once through the PhysicalBehaviour's PlayClipOnce.
    pub fn bell_use(&mut self, b: usize) {
        self.bells[b].Counter += 1;
        let p = self.bells[b].physical;
        let clip = self.pick_clip(self.physicals[p].id, "OverrideImpactSounds", self.physicals[p].OverrideImpactSounds);
        self.play_clip_once(p, clip, 1.0);
        let x = if self.bells[b].Counter % 2 == 0 { -1.0 } else { 1.0 };
        let (k, offset) = (self.bells[b].Intensity, self.bells[b].Offset);
        let dir = self.transform_direction(p, [x, 0.0, 0.0]);
        let t = self.physicals[p].transform;
        let up = mathf::rotate(t.rotation, [0.0, 1.0, 0.0]);
        let force = [dir[0] * k, dir[1] * k];
        let at = [t.position[0] - up[0] * offset, t.position[1] - up[1] * offset];
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_force_at_position(&mut self.world, force, at);
    }
}
