//! GlowingHotMetalBehaviour and BurnableSpriteBehaviour: each hands its own sprite's shader (Jaap/GlowingMetal, Unlit/BurnedShader;
//! custom_material.rs) one `_Progress`, from the physical's temperature or burn, through a property block in OnWillRenderObject.
//! The `_MainTex` the scripts also put on the block is the sprite's own texture, which the material already shows.
//! ResizableSliceBehaviour gives its sprite's Unlit/ResizableSliceShader the Transform's scale as `_ObjectScale`, and the
//! selection outline the same scale.
#![allow(non_snake_case)]

use super::props::PropBlock;
use super::visual::SpriteAt;
use super::{mathf, script, Game};

script! {
    GlowingHotMetal {
        UpperTemperatureBound: f32 = 1000.0,
    }
    links {
        /// The component's instance id and Behaviour.enabled.
        id: i32 = 0,
        enabled: bool = true,
        /// physicalBehaviour (GetComponent in Awake); the renderer is its sprite.
        physical: usize = 0,
        /// propertyBlock, read from the renderer in Awake.
        block: PropBlock = PropBlock::default(),
    }
}

script! {
    BurnableSprite {}
    links {
        id: i32 = 0,
        enabled: bool = true,
        physical: usize = 0,
        block: PropBlock = PropBlock::default(),
    }
}

script! {
    ResizableSlice {}
    links {
        id: i32 = 0,
        enabled: bool = true,
        /// physicalBehaviour (GetComponent in Awake); spriteRenderer is its sprite.
        physical: usize = 0,
        /// propertyBlock
        block: PropBlock = PropBlock::default(),
        /// lastScale: (float.MaxValue, 0) from Awake.
        lastScale: [f32; 3] = [f32::MAX, 0.0, 0.0],
        /// The `_ObjectScale` (xy) put on physicalBehaviour.outlineSpriteRenderer's block; None until the first.
        outline_scale: Option<[f32; 2]> = None,
    }
}

impl Game {
    /// ResizableSliceBehaviour.Awake: physicalBehaviour and spriteRenderer are `physical`'s; the block is read from the sprite.
    pub fn resizable_slice_awake(&mut self, r: usize) {
        self.resizable_slices[r].block = self.get_property_block(SpriteAt::Physical(self.resizable_slices[r].physical));
    }

    /// ResizableSliceBehaviour.OnWillRenderObject for each one whose sprite the camera draws: when the localScale's xy has moved
    /// from lastScale (squared distance from float.Epsilon), the block is read again and given `_ObjectScale` = (|x|, |y|, 1), and
    /// the outline's block `_ObjectScale` = localScale (every physical has its outline from Awake).
    pub fn resizable_slices_will_render(&mut self) {
        for r in 0..self.resizable_slices.len() {
            let s = &self.resizable_slices[r];
            let at = SpriteAt::Physical(s.physical);
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(at)) {
                continue;
            }
            let scale = self.physicals[s.physical].transform.localScale;
            let (dx, dy) = (s.lastScale[0] - scale[0], s.lastScale[1] - scale[1]);
            if ((super::d(dx) * dx + super::d(dy) * dy).f()) < f32::from_bits(1) {
                continue;
            }
            let mut block = self.get_property_block(at);
            block.set_vector("_ObjectScale", [scale[0].abs(), scale[1].abs(), 1.0, 0.0]);
            self.set_property_block(at, &block);
            let s = &mut self.resizable_slices[r];
            s.lastScale = scale;
            s.block = block;
            s.outline_scale = Some([scale[0], scale[1]]);
        }
    }

    /// GlowingHotMetalBehaviour.Awake: physicalBehaviour and the renderer are `physical`'s; the block is read from the sprite.
    /// (The Debug.LogWarning for a material not named GlowingMetal writes the log only.)
    pub fn glowing_hot_metal_awake(&mut self, g: usize) {
        self.glowing_metals[g].block = self.get_property_block(SpriteAt::Physical(self.glowing_metals[g].physical));
    }

    /// GlowingHotMetalBehaviour.OnWillRenderObject for each one whose sprite the camera draws: `_Progress` = Clamp01(Temperature /
    /// UpperTemperatureBound) squared.
    pub fn glowing_metals_will_render(&mut self) {
        for g in 0..self.glowing_metals.len() {
            let s = &self.glowing_metals[g];
            let at = SpriteAt::Physical(s.physical);
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(at)) {
                continue;
            }
            let num = mathf::clamp01(self.physicals[s.physical].Temperature / s.UpperTemperatureBound);
            let mut block = std::mem::take(&mut self.glowing_metals[g].block);
            block.set_float("_Progress", num * num);
            self.set_property_block(at, &block);
            self.glowing_metals[g].block = block;
        }
    }

    /// BurnableSpriteBehaviour.Awake: as GlowingHotMetalBehaviour.Awake (the warning is for a material not named BurnableSpriteShader).
    pub fn burnable_sprite_awake(&mut self, b: usize) {
        self.burnable_sprites[b].block = self.get_property_block(SpriteAt::Physical(self.burnable_sprites[b].physical));
    }

    /// BurnableSpriteBehaviour.OnWillRenderObject: `_Progress` = Clamp01(BurnProgress) squared.
    pub fn burnable_sprites_will_render(&mut self) {
        for b in 0..self.burnable_sprites.len() {
            let s = &self.burnable_sprites[b];
            let at = SpriteAt::Physical(s.physical);
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(at)) {
                continue;
            }
            self.burnable_sprite_will_render(b);
        }
    }

    /// BurnableSpriteBehaviour.OnWillRenderObject of `b`.
    pub(crate) fn burnable_sprite_will_render(&mut self, b: usize) {
        let s = &self.burnable_sprites[b];
        // if (base.enabled)
        if !s.enabled || self.disabled_ids.contains(&s.id) {
            return;
        }
        let p = s.physical;
        let num = mathf::clamp01(self.physicals[p].BurnProgress);
        let mut block = std::mem::take(&mut self.burnable_sprites[b].block);
        block.set_float("_Progress", num * num);
        self.set_property_block(SpriteAt::Physical(p), &block);
        self.burnable_sprites[b].block = block;
    }
}
