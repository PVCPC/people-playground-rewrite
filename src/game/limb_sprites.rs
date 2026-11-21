//! LimbSpriteCache and the person-wide skin setters (PersonBehaviour.SetBodyTextures and the shader-colour setters).
//! The state lives here and in the limb's `Skin`; `play::limb_sprites` draws it (sprite, flesh and bone textures, colours).
#![allow(non_snake_case)]

use super::{mathf, shatter, Game};
use crate::assets::db::ObjRef;

/// The `ObjRef::file` of a sprite LimbSpriteCache made at run time (its `path_id` indexes `generated`): shards of a limb
/// whose textures SetBodyTextures replaced are drawn from it.
pub const RUNTIME_SPRITE_FILE: usize = usize::MAX - 1;

/// The ObjRef of `LimbSpriteCache.generated[i]`.
pub fn runtime_sprite(i: usize) -> ObjRef {
    ObjRef { file: RUNTIME_SPRITE_FILE, path_id: i as i64 }
}

/// A Sprite a limb's renderer shows: one of the game's, or one LimbSpriteCache made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpriteSource {
    Asset(ObjRef),
    Cached(usize),
}

/// Default: a null Sprite.
impl Default for SpriteSource {
    fn default() -> Self {
        SpriteSource::Asset(ObjRef::default())
    }
}

/// A Sprite.Create result: the whole texture region (FullRect, pivot (0.5, 0.5)) at the scaled pixels per unit.
#[derive(Clone, Debug)]
pub struct CachedSprite {
    /// The Sprite's instance id.
    pub id: i32,
    pub texture: ObjRef,
    /// The Rect in the texture (x, y, width, height in pixels).
    pub rect: [f32; 4],
    /// left, bottom, right, top
    pub border: [f32; 4],
    pub pixels_per_unit: f32,
}

/// LimbSpriteCache.Key
#[derive(Default, Clone, Copy, Debug)]
pub struct Key {
    pub original: SpriteSource,
    pub skin: ObjRef,
    pub flesh: Option<ObjRef>,
    pub bone: Option<ObjRef>,
    pub scale: f32,
}

/// LimbSpriteCache.LimbSprites, as `generated` indices.
#[derive(Default, Clone, Copy, Debug)]
pub struct LimbSprites {
    pub skin: usize,
    pub flesh: Option<usize>,
    pub bone: Option<usize>,
}

impl LimbSprites {
    /// LimbSprites..ctor (skin, flesh, bone): the readonly fields.
    pub fn new(skin: usize, flesh: Option<usize>, bone: Option<usize>) -> Self {
        LimbSprites { skin, flesh, bone }
    }

    /// LimbSprites.Equals(object): false unless `obj` is a LimbSprites, then Equals(LimbSprites).
    pub fn equals_object(&self, obj: &dyn std::any::Any) -> bool {
        obj.downcast_ref::<LimbSprites>().is_some_and(|other| self == other)
    }
}

impl PartialEq for LimbSprites {
    /// LimbSprites.Equals(LimbSprites), LimbSprites.op_Equality (left.Equals(right)): each sprite is the same object (null equals null).
    fn eq(&self, other: &Self) -> bool {
        self.skin == other.skin && self.flesh == other.flesh && self.bone == other.bone
    }

    /// LimbSprites.op_Inequality: !(left == right).
    fn ne(&self, other: &Self) -> bool {
        !(self == other)
    }
}

/// LimbSpriteCache: the Sprites dictionary and the sprites it made.
#[derive(Debug, Default)]
pub struct LimbSpriteCache {
    pub generated: Vec<CachedSprite>,
    pub sprites: Vec<(Key, LimbSprites)>,
}

impl Key {
    /// Key.Equals, Key.GetHashCode (the hash it is found by): the identities, and Mathf.RoundToInt(Scale * 10000f).
    fn same(&self, o: &Key) -> bool {
        let hash = |k: &Key| (k.scale * 10000.0).round_ties_even() as i32;
        self.original == o.original && self.skin == o.skin && self.flesh == o.flesh && self.bone == o.bone && mathf::approximately(self.scale, o.scale) && hash(self) == hash(o)
    }
}

impl PartialEq for Key {
    /// Key.op_Equality: left.Equals(right).
    fn eq(&self, other: &Key) -> bool {
        self.same(other)
    }

    /// Key.op_Inequality: !(left == right).
    fn ne(&self, other: &Key) -> bool {
        !(self == other)
    }
}

impl Game {
    /// LimbSprites.GetHashCode: ((seed * m + hash(Skin)) * m + hash(Flesh)) * m + hash(Bone), wrapping; a Sprite hashes to its
    /// instance id and null to 0.
    pub fn limb_sprites_get_hash_code(&self, s: &LimbSprites) -> i32 {
        const M: i32 = -1521134295;
        let hash = |i: Option<usize>| i.map_or(0, |i| self.limb_sprite_cache.generated[i].id);
        (((-721118468i32).wrapping_mul(M).wrapping_add(hash(Some(s.skin)))).wrapping_mul(M).wrapping_add(hash(s.flesh))).wrapping_mul(M).wrapping_add(hash(s.bone))
    }

    /// The rect, border and pixels per unit of a Sprite.
    fn sprite_metrics(&self, s: SpriteSource) -> ([f32; 4], [f32; 4], f32) {
        match s {
            SpriteSource::Cached(i) => {
                let c = &self.limb_sprite_cache.generated[i];
                (c.rect, c.border, c.pixels_per_unit)
            }
            SpriteSource::Asset(r) => {
                let sprite = self.assets.as_deref().expect("the game's data").sprite(r).expect("a limb sprite");
                (sprite.rect, sprite.border, sprite.pixels_to_units)
            }
        }
    }

    /// LimbSpriteCache.LoadFor: the cached sprites for the key, else new ones (skin, then flesh, then bone) from the textures
    /// at `scale`. The log line is presentation.
    pub fn limb_sprite_cache_load_for(&mut self, original: SpriteSource, skin: ObjRef, flesh: Option<ObjRef>, bone: Option<ObjRef>, scale: f32) -> LimbSprites {
        // LimbSpriteCache.Awake: Instance = this is `Game::limb_sprite_cache`
        // LimbSpriteCache.OnDestroy: Destroy of the generated sprites, graphics resources (nothing reads them after the scene)
        let key = Key { original, skin, flesh, bone, scale };
        if let Some((_, found)) = self.limb_sprite_cache.sprites.iter().find(|(k, _)| *k == key) {
            return *found;
        }
        let (rect, border, ppu) = self.sprite_metrics(original);
        // generateFor: Sprite.Create(t, new Rect(rect.position * scale, rect.size * scale), (0.5, 0.5), ppu * scale, 0, FullRect, border * scale).
        let generate = |g: &mut Game, texture: ObjRef| {
            let id = g.allocate_instance_id();
            g.limb_sprite_cache.generated.push(CachedSprite {
                id,
                texture,
                rect: rect.map(|x| x * scale),
                border: border.map(|x| x * scale),
                pixels_per_unit: ppu * scale,
            });
            g.limb_sprite_cache.generated.len() - 1
        };
        let made = LimbSprites { skin: generate(self, skin), flesh: flesh.map(|t| generate(self, t)), bone: bone.map(|t| generate(self, t)) };
        self.limb_sprite_cache.sprites.push((key, made));
        made
    }

    /// PersonBehaviour.SetBodyTextures: each limb's renderer shows the skin sprite, a given flesh and bone texture replaces the
    /// material's, and the limb's ShatteredObjectSpriteInitialiser hands the sprites to its generators.
    pub fn person_set_body_textures(&mut self, person: usize, skin: ObjRef, flesh: Option<ObjRef>, bone: Option<ObjRef>, scale: f32) {
        for l in self.people[person].Limbs.clone() {
            let p = self.limbs[l].PhysicalBehaviour;
            let original = match self.skins[l].sprite {
                Some(i) => SpriteSource::Cached(i),
                None => SpriteSource::Asset(self.physicals[p].sprite.expect("a limb without a SpriteRenderer").0),
            };
            let sprites = self.limb_sprite_cache_load_for(original, skin, flesh, bone, scale);
            self.skins[l].sprite = Some(sprites.skin);
            if flesh.is_some() {
                self.skins[l].flesh_texture = flesh;
            }
            if bone.is_some() {
                self.skins[l].bone_texture = bone;
            }
            // ShatteredObjectSpriteInitialiser.UpdateSprites: BoneGenerator, FleshGenerator and SkinGenerator take their sprite.
            if let Some(generators) = self.limbs[l].ShatteredObjectGenerators {
                for (generator, sprite) in [(generators[0], sprites.bone), (generators[1], sprites.flesh), (generators[2], Some(sprites.skin))] {
                    if let (Some(g), Some(s)) = (generator, sprite) {
                        let c = self.limb_sprite_cache.generated[s].clone();
                        let source = &mut self.shatter_generators[g];
                        source.g.Sprite = shatter::Sprite { id: c.id, rect_width: c.rect[2], rect_height: c.rect[3], pixels_per_unit: c.pixels_per_unit, ..Default::default() };
                        // The shards' bounds: a FullRect sprite about its centre.
                        let (w, h) = (c.rect[2] / c.pixels_per_unit, c.rect[3] / c.pixels_per_unit);
                        source.sprite_bounds = ([0.0, 0.0, 0.0], [w * 0.5, h * 0.5, 0.1]);
                        // The shards a Generate makes from now on draw this sprite.
                        source.sprite = Some(runtime_sprite(s));
                    }
                }
            }
            self.skins[l].version += 1;
        }
    }

    /// PersonBehaviour.SetShaderColour: the colour (r, g, b over 255, alpha 1) on every limb's material property `id`. The
    /// rewrite keeps five (Skin colours: 0 _BruiseColor, 1 _SecondBruiseColor, 2 _ThirdBruiseColor, 3 _Zombie, 4 _BloodColor,
    /// which SkinMaterialHandler.OnWillRenderObject writes over before every draw); any other id names a property no limb
    /// shader reads, so it leaves nothing.
    pub(crate) fn person_set_shader_colour(&mut self, person: usize, id: &str, red: u8, green: u8, blue: u8) {
        let Some(slot) = ["_BruiseColor", "_SecondBruiseColor", "_ThirdBruiseColor", "_Zombie", "_BloodColor"].iter().position(|p| *p == id) else { return };
        let value = [red as f32 / 255.0, green as f32 / 255.0, blue as f32 / 255.0, 1.0];
        for l in self.people[person].Limbs.clone() {
            self.skins[l].colours[slot] = Some(value);
        }
    }

    /// PersonBehaviour.SetBruiseColor
    pub fn person_set_bruise_color(&mut self, person: usize, red: u8, green: u8, blue: u8) {
        self.person_set_shader_colour(person, "_BruiseColor", red, green, blue);
    }

    /// PersonBehaviour.SetSecondBruiseColor
    pub fn person_set_second_bruise_color(&mut self, person: usize, red: u8, green: u8, blue: u8) {
        self.person_set_shader_colour(person, "_SecondBruiseColor", red, green, blue);
    }

    /// PersonBehaviour.SetThirdBruiseColor
    pub fn person_set_third_bruise_color(&mut self, person: usize, red: u8, green: u8, blue: u8) {
        self.person_set_shader_colour(person, "_ThirdBruiseColor", red, green, blue);
    }

    /// PersonBehaviour.SetBloodColour
    pub fn person_set_blood_colour(&mut self, person: usize, red: u8, green: u8, blue: u8) {
        self.person_set_shader_colour(person, "_BloodColor", red, green, blue);
    }

    /// PersonBehaviour.SetRottenColour
    pub fn person_set_rotten_colour(&mut self, person: usize, red: u8, green: u8, blue: u8) {
        self.person_set_shader_colour(person, "_Zombie", red, green, blue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LoadFor makes sprites once per key, scaled from the original, and finds them again at an approximately equal scale.
    #[test]
    fn load_for_caches_by_key() {
        let mut g = Game::default();
        let t = |p| ObjRef { file: 0, path_id: p };
        g.limb_sprite_cache.generated.push(CachedSprite { id: 1, texture: t(1), rect: [4.0, 8.0, 32.0, 64.0], border: [1.0; 4], pixels_per_unit: 100.0 });
        let a = g.limb_sprite_cache_load_for(SpriteSource::Cached(0), t(2), Some(t(3)), None, 2.0);
        assert_eq!(g.limb_sprite_cache.generated.len(), 3);
        let c = &g.limb_sprite_cache.generated[a.skin];
        assert_eq!((c.rect, c.border, c.pixels_per_unit), ([8.0, 16.0, 64.0, 128.0], [2.0; 4], 200.0));
        let b = g.limb_sprite_cache_load_for(SpriteSource::Cached(0), t(2), Some(t(3)), None, 2.0);
        assert_eq!((a.skin, a.flesh), (b.skin, b.flesh));
        assert_eq!(g.limb_sprite_cache.generated.len(), 3);
        g.limb_sprite_cache_load_for(SpriteSource::Cached(0), t(2), Some(t(3)), None, 3.0);
        assert_eq!(g.limb_sprite_cache.generated.len(), 5);
    }

    /// LimbSprites equality is per sprite object (null equals null); the hash folds the instance ids with the C# arithmetic.
    #[test]
    fn limb_sprites_equality_and_hash() {
        let mut g = Game::default();
        let t = |p| ObjRef { file: 0, path_id: p };
        for id in [5, 6, 7] {
            g.limb_sprite_cache.generated.push(CachedSprite { id, texture: t(1), rect: [0.0; 4], border: [0.0; 4], pixels_per_unit: 1.0 });
        }
        let (a, b) = (LimbSprites::new(0, Some(1), None), LimbSprites::new(0, Some(1), None));
        assert!(a == b && !(a != b) && a.equals_object(&b));
        assert!(a != LimbSprites::new(0, Some(2), None) && a != LimbSprites::new(0, Some(1), Some(2)));
        assert!(!a.equals_object(&5i32));
        let m = -1521134295i32;
        let want = (-721118468i32).wrapping_mul(m).wrapping_add(5).wrapping_mul(m).wrapping_add(6).wrapping_mul(m);
        assert_eq!(g.limb_sprites_get_hash_code(&a), want);
    }

    /// The colour setters reach every limb of the person, over 255.
    #[test]
    fn colours_reach_each_limb() {
        let mut g = Game::default();
        g.people.push(Default::default());
        g.skins.push(Default::default());
        g.skins.push(Default::default());
        g.people[0].Limbs = vec![0, 1];
        g.person_set_rotten_colour(0, 255, 51, 0);
        g.person_set_bruise_color(0, 0, 0, 255);
        assert_eq!(g.skins[1].colours[3], Some([1.0, 0.2, 0.0, 1.0]));
        assert_eq!(g.skins[0].colours[0], Some([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(g.skins[0].colours[1], None);
    }
}
