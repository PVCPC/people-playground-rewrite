//! ToggleableMirrorBehaviour: its FixedUpdate sets the physical's ReflectsLasers and AbsorbsLasers to whether its Charge is
//! under 0.67 (the two flags are equal) and puts its Charge in its property block's `_Progress`, which OnWillRenderObject
//! hands the sprite's DissolveMirror shader (custom_material.rs).
#![allow(non_snake_case)]

use super::{script, Game};


script! {
    ToggleableMirror {}
    links {
        /// PhysicalBehaviour (GetComponent in Awake).
        physical: usize = 0,
        /// The property block `Awake` read from the SpriteRenderer (`GetPropertyBlock`).
        block: super::props::PropBlock = Default::default(),
        /// No PhysicalBehaviour on the GameObject (FixedUpdate dereferences it).
        physical_null: bool = false,
    }
}

impl Game {
    /// ToggleableMirrorBehaviour.FixedUpdate
    pub fn toggleable_mirror_fixed_update(&mut self, m: usize) {
        if self.toggleable_mirrors[m].physical_null {
            crate::game::menu::npe();
        }
        let p = self.toggleable_mirrors[m].physical;
        // (double)Charge < 0.67
        let reflects = (self.physicals[p].Charge() as f64) < 0.67;
        self.physicals[p].ReflectsLasers = reflects;
        self.physicals[p].AbsorbsLasers = reflects;
        // propertyBlock.SetFloat("_Progress", physicalBehaviour.Charge)
        let charge = self.physicals[p].Charge();
        self.toggleable_mirrors[m].block.set_float("_Progress", charge);
    }

    /// ToggleableMirrorBehaviour.Awake: physicalBehaviour and spriteRenderer are the same object's `physical`; the block is read
    /// from the sprite (`GetPropertyBlock`).
    pub fn toggleable_mirror_awake(&mut self, m: usize) {
        self.toggleable_mirrors[m].block = self.get_property_block(super::visual::SpriteAt::Physical(self.toggleable_mirrors[m].physical));
    }

    /// ToggleableMirrorBehaviour.OnWillRenderObject for each mirror whose sprite the camera draws: SetPropertyBlock(propertyBlock).
    pub fn toggleable_mirrors_will_render(&mut self) {
        for m in 0..self.toggleable_mirrors.len() {
            let at = super::visual::SpriteAt::Physical(self.toggleable_mirrors[m].physical);
            // No component id here (script! without one): fn-check cannot pick the target, so the camera's gate stays.
            if !self.sprite_drawn(at) {
                continue;
            }
            let block = std::mem::take(&mut self.toggleable_mirrors[m].block);
            self.set_property_block(at, &block);
            self.toggleable_mirrors[m].block = block;
        }
    }
}
