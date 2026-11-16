//! The obsolete DecalRenderer system: DecalRenderer, DecalRendererController and DecalInformation. No prefab, scene or
//! script of the game holds a DecalRenderer (the live decals are DecalControllerBehaviour's, in `decals.rs`), so these are
//! plain values; what needs a renderer or a Transform takes it as an argument or stops.
#![allow(non_snake_case)]

use super::{mathf, misc::DecalDescriptor};
use crate::assets::db::{AssetDb, ObjRef};
use crate::unity::Random;
use std::collections::HashMap;

/// The ShaderProperties.Get ids as their names: a MaterialPropertyBlock here is keyed by name, and Shader.PropertyToID is
/// one id per name.
pub type ShaderProperty = String;

/// A MaterialPropertyBlock's matrices and ints (SetMatrix, SetInt replace the entry of that name).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyBlock {
    pub matrices: HashMap<ShaderProperty, [[f32; 4]; 4]>,
    pub ints: HashMap<ShaderProperty, i32>,
}

/// DecalInstruction (the get_/set_ accessors are its auto-properties: type, globalPosition, size, colourMultiplier).
/// `ty` is an index into `Game::decal_descriptors`; DecalRenderer reads all but size.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct DecalInstruction {
    pub ty: usize,
    pub globalPosition: [f32; 2],
    pub size: f32,
    pub colourMultiplier: [f32; 4],
}

/// DecalRenderer.MaxDecals
pub const MAX_DECALS: usize = 32;

/// DecalRenderer
#[derive(Default, Clone, Debug)]
pub struct DecalRenderer {
    /// DecalType, an index into `Game::decal_descriptors`.
    pub DecalType: usize,
    /// `enabled`, which Start clears for a renderer without a parent.
    pub enabled: bool,
    /// Whether Start found a SpriteRenderer (`(bool)spriteRenderer`); nothing here attaches one.
    pub has_sprite_renderer: bool,
    pub LoopIndex: usize,
    pub CurrentDecalAmount: usize,
    pub localPositionBuffer: [[f32; 2]; MAX_DECALS],
    pub properties: Option<PropertyBlock>,
}

impl DecalRenderer {
    /// DecalRenderer..cctor: shaderPropertyIndices, "_Decal01" to "_Decal32".
    pub fn cctor() -> Vec<ShaderProperty> {
        (1..=MAX_DECALS).map(|i| format!("_Decal{i:02}")).collect()
    }

    /// DecalRenderer..ctor: the field initialisers (localPositionBuffer = new Vector2[32], all zero).
    pub fn new(decal_type: usize) -> Self {
        DecalRenderer { DecalType: decal_type, enabled: true, has_sprite_renderer: false, LoopIndex: 0, CurrentDecalAmount: 0, localPositionBuffer: [[0.0; 2]; MAX_DECALS], properties: None }
    }

    /// DecalRenderer.Start: a renderer without a parent logs the error and disables itself; otherwise the buffer is filled
    /// with float.MaxValue, the property block is made, and UpdateContainingSprite takes the parent's sprite.
    pub fn Start(&mut self, has_parent: bool) {
        if !has_parent {
            // Debug.LogErrorFormat("{0} without a parent... This is not allowed.", "DecalRenderer"): console only.
            self.enabled = false;
            return;
        }
        self.localPositionBuffer = [[f32::MAX; 2]; MAX_DECALS];
        self.properties = Some(PropertyBlock::default());
        // GetComponent<SpriteRenderer>(), then UpdateContainingSprite(parent's SpriteRenderer)
        self.UpdateContainingSprite();
        // base.name = "Decal renderer for " + DecalType.name: the name is presentation.
    }

    /// DecalRenderer.UpdateContainingSprite: the sprite, sorting order and sorting layer of the container are copied to
    /// the renderer and its property block is read into `properties`.
    pub fn UpdateContainingSprite(&mut self) {
        unimplemented!("mod-only: DecalRenderer.UpdateContainingSprite: SpriteRenderer.sprite, sortingOrder, sortingLayerID and GetPropertyBlock (no SpriteRenderer is modelled for a DecalRenderer)")
    }

    /// DecalRenderer.OnWillRenderObject: SetPropertyBlock(properties) on the renderer, when there is one.
    pub fn OnWillRenderObject(&self) {
        if self.has_sprite_renderer {
            unimplemented!("mod-only: DecalRenderer.OnWillRenderObject: SpriteRenderer.SetPropertyBlock (no SpriteRenderer is modelled for a DecalRenderer)")
        }
    }

    /// DecalRenderer.Decal: a decal within IgnoreRadius / 2 of one in the buffer is dropped; else a matrix (local position,
    /// the colour product, the sprite index) goes to the next of the 32 property slots, which wrap. `inverse_transform_point`
    /// is the renderer Transform's.
    pub fn Decal(&mut self, d: &DecalInstruction, descriptors: &[DecalDescriptor], random: &mut Random, inverse_transform_point: impl Fn([f32; 3]) -> [f32; 3]) {
        if !self.has_sprite_renderer || d.ty != self.DecalType {
            return;
        }
        let ty = &descriptors[self.DecalType];
        let p = inverse_transform_point([d.globalPosition[0], d.globalPosition[1], 0.0]);
        let vector = [p[0], p[1]];
        let num = ty.IgnoreRadius * ty.IgnoreRadius / 4.0;
        for i in 0..self.CurrentDecalAmount {
            if mathf::sqr_magnitude([self.localPositionBuffer[i][0] - vector[0], self.localPositionBuffer[i][1] - vector[1]]) < num {
                return;
            }
        }
        let num2 = random.range_int(0, ty.Sprites as i32);
        let (c, m) = (ty.Color, d.colourMultiplier);
        let mut value = [[0.0f32; 4]; 4];
        value[0][0] = vector[0];
        value[0][1] = vector[1];
        value[0][2] = 0.4;
        value[1][0] = m[0] * c[0];
        value[1][1] = m[1] * c[1];
        value[1][2] = m[2] * c[2];
        value[1][3] = m[3] * c[3];
        value[2][0] = num2 as f32;
        self.CurrentDecalAmount += 1;
        self.CurrentDecalAmount = self.CurrentDecalAmount.min(MAX_DECALS);
        let properties = self.properties.as_mut().expect("DecalRenderer.Decal before Start made the property block");
        properties.matrices.insert(format!("_Decal{:02}", self.LoopIndex + 1), value);
        properties.ints.insert("_DecalCount".into(), self.CurrentDecalAmount as i32);
        self.localPositionBuffer[self.LoopIndex] = vector;
        self.LoopIndex += 1;
        self.LoopIndex %= MAX_DECALS;
    }
}

/// DecalRendererController
#[derive(Clone, Debug, Default)]
pub struct DecalRendererController {
    /// GetComponentInChildren<DecalRenderer>(), an index into the caller's renderers.
    pub decalRenderer: Option<usize>,
}

impl DecalRendererController {
    /// DecalRendererController.Awake: decalRenderer = GetComponentInChildren<DecalRenderer>(); `found` is its result.
    pub fn Awake(&mut self, found: Option<usize>) {
        self.decalRenderer = found;
    }

    /// DecalRendererController.Decal: forwarded to the renderer when there is one.
    pub fn Decal(&self, renderers: &mut [DecalRenderer], d: &DecalInstruction, descriptors: &[DecalDescriptor], random: &mut Random, inverse_transform_point: impl Fn([f32; 3]) -> [f32; 3]) {
        if let Some(r) = self.decalRenderer {
            renderers[r].Decal(d, descriptors, random, inverse_transform_point);
        }
    }
}

/// DecalInformation.DecalIndex
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DecalIndex {
    HumanBlood = 0,
    AlienBlood = 1,
    BlastMark = 2,
}

/// DecalInformation
#[derive(Default, Clone, Debug)]
pub struct DecalInformation {
    pub Color: [f32; 4],
    pub Path: String,
    pub IgnoreRadius: f32,
}

/// DecalInformation's statics: Decals and SpriteCache.
#[derive(Clone, Debug)]
pub struct DecalStatics {
    pub Decals: [DecalInformation; 3],
    pub SpriteCache: HashMap<String, Vec<ObjRef>>,
}

/// Mono's float.GetHashCode: the bits.
pub(crate) fn float_hash(f: f32) -> i32 {
    // `if (f == 0) return 0;`: -0 hashes as 0.
    if f == 0.0 { 0 } else { f.to_bits() as i32 }
}

/// Mono's string.GetHashCode (Unity 2020.3, the reference source's 64-bit one): ((h << 5) + h) ^ c over the even and the
/// odd UTF-16 units apart, both from 5381, up to the first NUL; hash1 + hash2 * 1566083941 (checked against the DLL).
pub(crate) fn string_hash(s: &str) -> i32 {
    let (mut h1, mut h2) = (5381i32, 5381i32);
    let mut units = s.encode_utf16();
    loop {
        match units.next() { Some(c) if c != 0 => h1 = h1.wrapping_shl(5).wrapping_add(h1) ^ c as i32, _ => break }
        match units.next() { Some(c) if c != 0 => h2 = h2.wrapping_shl(5).wrapping_add(h2) ^ c as i32, _ => break }
    }
    h1.wrapping_add(h2.wrapping_mul(1566083941))
}

/// Color.GetHashCode: the Vector4's, x ^ (y << 2) ^ (z >> 2) ^ (w >> 1).
fn color_hash(c: [f32; 4]) -> i32 {
    float_hash(c[0]) ^ (float_hash(c[1]) << 2) ^ (float_hash(c[2]) >> 2) ^ (float_hash(c[3]) >> 1)
}

impl DecalInformation {
    /// DecalInformation..ctor (path, color, ignoreRadius): the primary constructor's fields; ignoreRadius defaults to 0.1.
    pub fn new(path: &str, color: [f32; 4], ignore_radius: f32) -> Self {
        DecalInformation { Color: color, Path: path.into(), IgnoreRadius: ignore_radius }
    }

    /// DecalInformation..cctor: Decals (human blood, alien blood, blast mark) and an empty SpriteCache.
    pub fn cctor() -> DecalStatics {
        DecalStatics {
            Decals: [
                Self::new("Sprites/Decals/Blood", [0.5660378, 0.0, 0.0, 1.0], 0.1),
                Self::new("Sprites/Decals/Blood", [0.0, 0.4, 0.17, 1.0], 0.1),
                Self::new("Sprites/Decals/BlastMark", [1.0, 1.0, 1.0, 1.0], 0.1),
            ],
            SpriteCache: HashMap::new(),
        }
    }

    /// DecalInformation.Get: Decals[(int)index].
    pub fn Get(s: &DecalStatics, index: DecalIndex) -> &DecalInformation {
        &s.Decals[index as usize]
    }

    /// DecalInformation.get_HumanBlood: Decals[0].
    pub fn get_HumanBlood(s: &DecalStatics) -> &DecalInformation {
        &s.Decals[0]
    }

    /// DecalInformation.get_AlienBlood: Decals[1].
    pub fn get_AlienBlood(s: &DecalStatics) -> &DecalInformation {
        &s.Decals[1]
    }

    /// DecalInformation.get_BlastMark: Decals[2].
    pub fn get_BlastMark(s: &DecalStatics) -> &DecalInformation {
        &s.Decals[2]
    }

    /// DecalInformation.GetSprites: the cached array, else Resources.LoadAll<Sprite>(path) (class 213), cached.
    pub fn GetSprites(s: &mut DecalStatics, db: &AssetDb, path: &str) -> Vec<ObjRef> {
        if let Some(found) = s.SpriteCache.get(path) {
            return found.clone();
        }
        let loaded = db.resources_in(path, 213).expect("Resources.LoadAll<Sprite>");
        // SpriteCache.Add(path, array)
        s.SpriteCache.insert(path.into(), loaded.clone());
        loaded
    }

    /// DecalInformation.Equals: the colour, the path and the ignore radius are equal.
    pub fn Equals(&self, other: &DecalInformation) -> bool {
        self.Color == other.Color && self.Path == other.Path && self.IgnoreRadius == other.IgnoreRadius
    }

    /// DecalInformation.GetHashCode: ((seed * m + hash(Color)) * m + hash(Path)) * m + hash(IgnoreRadius), wrapping.
    pub fn GetHashCode(&self) -> i32 {
        const M: i32 = -1521134295;
        (((-1088103025i32).wrapping_mul(M).wrapping_add(color_hash(self.Color))).wrapping_mul(M).wrapping_add(string_hash(&self.Path))).wrapping_mul(M).wrapping_add(float_hash(self.IgnoreRadius))
    }
}

impl PartialEq for DecalInformation {
    /// DecalInformation.op_Equality: left.Equals(right).
    fn eq(&self, other: &Self) -> bool {
        self.Equals(other)
    }

    /// DecalInformation.op_Inequality: !(left == right).
    fn ne(&self, other: &Self) -> bool {
        !(self == other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptors() -> Vec<DecalDescriptor> {
        vec![DecalDescriptor { name: "Blood".into(), Color: [0.5, 1.0, 1.0, 1.0], IgnoreRadius: 0.4, Sprites: 3, MinDripCount: 0, MaxDripCount: 0 }]
    }

    fn renderer() -> DecalRenderer {
        let mut r = DecalRenderer::new(0);
        r.has_sprite_renderer = true;
        r.localPositionBuffer = [[f32::MAX; 2]; MAX_DECALS];
        r.properties = Some(PropertyBlock::default());
        r
    }

    fn instruction(x: f32) -> DecalInstruction {
        DecalInstruction { ty: 0, globalPosition: [x, 0.0], size: 1.0, colourMultiplier: [0.5, 1.0, 1.0, 0.5] }
    }

    #[test]
    fn decal_fills_slots_and_wraps() {
        let (mut r, mut rng, d) = (renderer(), Random::new(1), descriptors());
        for i in 0..40 {
            r.Decal(&instruction(i as f32), &d, &mut rng, |p| [p[0] + 1.0, p[1], 0.0]);
        }
        assert_eq!((r.CurrentDecalAmount, r.LoopIndex), (32, 8));
        let p = r.properties.as_ref().unwrap();
        assert_eq!(p.ints["_DecalCount"], 32);
        let m = p.matrices["_Decal01"];
        assert_eq!((m[0][0], m[0][2], m[1][0], m[1][3]), (33.0, 0.4, 0.25, 0.5));
        assert!(m[2][0] < 3.0);
    }

    #[test]
    fn decal_near_another_is_dropped() {
        let (mut r, mut rng, d) = (renderer(), Random::new(1), descriptors());
        r.Decal(&instruction(0.0), &d, &mut rng, |p| p);
        let state = rng.state;
        r.Decal(&instruction(0.19), &d, &mut rng, |p| p);
        assert_eq!(r.CurrentDecalAmount, 1);
        assert_eq!(rng.state, state);
        r.Decal(&instruction(0.2), &d, &mut rng, |p| p);
        assert_eq!(r.CurrentDecalAmount, 2);
    }

    #[test]
    fn decal_ignores_other_types_and_renderers_without_sprite() {
        let (mut r, mut rng, d) = (renderer(), Random::new(1), descriptors());
        let mut other = instruction(0.0);
        other.ty = 1;
        r.Decal(&other, &d, &mut rng, |p| p);
        r.has_sprite_renderer = false;
        r.Decal(&instruction(0.0), &d, &mut rng, |p| p);
        assert_eq!(r.CurrentDecalAmount, 0);
    }

    #[test]
    fn start_without_parent_disables() {
        let mut r = DecalRenderer::new(0);
        r.Start(false);
        assert!(!r.enabled && r.properties.is_none());
        assert_eq!(DecalRenderer::cctor()[0], "_Decal01");
        assert_eq!(DecalRenderer::cctor()[31], "_Decal32");
    }

    #[test]
    fn controller_forwards_only_with_a_renderer() {
        let (mut rs, mut rng, d) = (vec![renderer()], Random::new(1), descriptors());
        let mut c = DecalRendererController::default();
        c.Decal(&mut rs, &instruction(0.0), &d, &mut rng, |p| p);
        assert_eq!(rs[0].CurrentDecalAmount, 0);
        c.Awake(Some(0));
        c.Decal(&mut rs, &instruction(0.0), &d, &mut rng, |p| p);
        assert_eq!(rs[0].CurrentDecalAmount, 1);
    }

    #[test]
    fn information_statics_equality_and_hash() {
        let s = DecalInformation::cctor();
        assert_eq!(DecalInformation::Get(&s, DecalIndex::AlienBlood), DecalInformation::get_AlienBlood(&s));
        assert_ne!(DecalInformation::get_HumanBlood(&s), DecalInformation::get_AlienBlood(&s));
        assert_eq!(*DecalInformation::get_BlastMark(&s), DecalInformation::new("Sprites/Decals/BlastMark", [1.0; 4], 0.1));
        assert_eq!(string_hash("ab"), 1093630535);
        assert_eq!((string_hash(""), float_hash(-0.0)), (371857150, 0));
        let h = DecalInformation::new("", [0.0; 4], 0.0).GetHashCode();
        assert_eq!(h, (-1088103025i32).wrapping_mul(-1521134295).wrapping_mul(-1521134295).wrapping_add(371857150).wrapping_mul(-1521134295));
        let a = DecalInformation::get_HumanBlood(&s);
        assert_eq!(a.GetHashCode(), a.clone().GetHashCode());
        assert_ne!(a.GetHashCode(), DecalInformation::get_AlienBlood(&s).GetHashCode());
    }
}
