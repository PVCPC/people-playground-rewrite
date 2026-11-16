//! MaterialPropertyBlock and Material.SetX: what the scripts hand a SpriteRenderer's shader. The game side only stores
//! names and values; `custom_material.rs` uploads them to the renderer's material (OnWillRenderObject's SetFloat,
//! SetColor, SetVector and SetTexture reach the screen through it).
//!
//! Unity semantics kept: a renderer has one block (`Renderer.SetPropertyBlock` replaces it whole; an empty block clears
//! it; `GetPropertyBlock` copies it into the caller's block) and the block beats the renderer's material, whose own
//! values (`Renderer.material.SetX`) beat the saved .mat and the shader's defaults.
use super::visual::SpriteAt;
use super::Game;
use crate::assets::db::ObjRef;
use std::collections::BTreeMap;

/// One shader property value. A Color is converted to linear when the shader declares the property a Color (as Unity
/// does); a Vector is not.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Float(f32),
    Vector([f32; 4]),
    Color([f32; 4]),
    /// A Texture2D object of the build.
    Texture(ObjRef),
    /// SetVectorArray's values (a float2 array of the shader takes each one's x and y).
    VectorArray(Vec<[f32; 4]>),
}

/// UnityEngine.MaterialPropertyBlock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropBlock(pub BTreeMap<String, Prop>);

impl PropBlock {
    pub fn set_float(&mut self, name: &str, v: f32) {
        self.0.insert(name.into(), Prop::Float(v));
    }
    pub fn set_vector(&mut self, name: &str, v: [f32; 4]) {
        self.0.insert(name.into(), Prop::Vector(v));
    }
    pub fn set_color(&mut self, name: &str, v: [f32; 4]) {
        self.0.insert(name.into(), Prop::Color(v));
    }
    /// MaterialPropertyBlock.SetVectorArray(name, values)
    pub fn set_vector_array(&mut self, name: &str, v: &[[f32; 4]]) {
        self.0.insert(name.into(), Prop::VectorArray(v.to_vec()));
    }
    pub fn set_texture(&mut self, name: &str, t: ObjRef) {
        self.0.insert(name.into(), Prop::Texture(t));
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn get_float(&self, name: &str) -> f32 {
        match self.0.get(name) {
            Some(Prop::Float(v)) => *v,
            _ => 0.0,
        }
    }
    pub fn get(&self, name: &str) -> Option<&Prop> {
        self.0.get(name)
    }
}

/// A renderer's block and material overrides; `version` counts the changes the renderer side has to upload.
#[derive(Clone, Debug, Default)]
pub struct RenderProps {
    pub block: PropBlock,
    pub material: PropBlock,
    pub version: u32,
}

impl Game {
    /// Renderer.SetPropertyBlock(block): the renderer's block becomes a copy of `block`.
    pub fn set_property_block(&mut self, at: SpriteAt, block: &PropBlock) {
        let p = self.props.entry(at).or_default();
        if p.block != *block {
            p.block = block.clone();
            p.version += 1;
        }
    }

    /// Renderer.GetPropertyBlock(): a copy of the renderer's block.
    pub fn get_property_block(&self, at: SpriteAt) -> PropBlock {
        self.props.get(&at).map(|p| p.block.clone()).unwrap_or_default()
    }

    /// Material.SetFloat/SetColor/SetVector/SetTexture on the renderer's own material (`Renderer.material`).
    pub fn set_material_prop(&mut self, at: SpriteAt, name: &str, v: Prop) {
        let p = self.props.entry(at).or_default();
        if p.material.0.get(name) != Some(&v) {
            p.material.0.insert(name.into(), v);
            p.version += 1;
        }
    }

    /// Material.GetFloat on the renderer's own material, when a script has set it (the saved value is the renderer side's).
    pub fn material_prop(&self, at: SpriteAt, name: &str) -> Option<&Prop> {
        self.props.get(&at).and_then(|p| p.material.get(name))
    }
}
