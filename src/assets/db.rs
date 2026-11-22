//! The installed game's object graph: every SerializedFile of the build, PPtrs resolved across them, scripts read
//! with their classes, and prefabs as GameObject trees.

use super::classes::{GameObject, MonoBehaviour, MonoScript, PPtr, Transform};
use super::components::{Collider2D, Joint2D, ParentConstraintData, PointEffector2D, Rigidbody2D, SpriteRenderer};
use super::mono::{Managed, Value};
use super::serialized::{Reader, SerializedFile};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// An object anywhere in the build: (file index, path id).
/// Default: fileID 0 / pathID 0, a null PPtr.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjRef {
    pub file: usize,
    pub path_id: i64,
}

pub struct AssetDb {
    pub data_dir: PathBuf,
    pub files: Vec<SerializedFile>,
    names: HashMap<String, usize>,
    pub managed: Managed,
    scripts: HashMap<ObjRef, MonoScript>,
    /// `mono` read once per MonoBehaviour (the data never changes).
    monos: HashMap<ObjRef, Option<(MonoScript, MonoBehaviour, Value)>>,
    load_order: std::sync::OnceLock<std::sync::Arc<HashMap<ObjRef, i32>>>,
    /// CatalogBehaviour.Populate's two lookups (by name, by prior name), built on the first spawn.
    /// Prefabs decoded once (the data never changes).
    prefabs: std::sync::Mutex<HashMap<ObjRef, std::sync::Arc<Prefab>>>,
    /// Texture2Ds decoded ahead of their first sprite (`preload_textures`): the texture and its RGBA rows, bottom first.
    decoded: std::sync::Mutex<HashMap<ObjRef, std::sync::Arc<(super::components::Texture2D, Vec<u8>)>>>,
    pub catalog_lookup: Option<(HashMap<String, ObjRef>, HashMap<String, ObjRef>)>,
    /// gapC r6: every Sprite (213) by its m_Name, the first of a name (`sprite_by_name`).
    sprite_names: std::sync::OnceLock<HashMap<String, ObjRef>>,
    material_names: std::sync::OnceLock<HashMap<String, ObjRef>>,
    clip_names: std::sync::OnceLock<HashMap<String, ObjRef>>,
    texture_names: std::sync::OnceLock<HashMap<String, ObjRef>>,
    /// `shader_id`'s map, built on its first call.
    shader_ids: std::sync::OnceLock<HashMap<ObjRef, i64>>,
    /// Unity's builtin resource files (Resources/unity_builtin_extra, ...), opened on the first `resolve_builtin` into one
    /// and appended to `files`; `resolve` never reaches them.
    builtin: HashMap<String, usize>,
}

#[derive(Clone, Debug)]
pub enum Component {
    Transform(Transform),
    Rigidbody2D(Rigidbody2D),
    Collider2D(Collider2D),
    Joint2D(Joint2D),
    SpriteRenderer(SpriteRenderer),
    PointEffector2D(PointEffector2D),
    ParentConstraint(ParentConstraintData),
    /// A MonoBehaviour: its script class, MonoScript.m_ExecutionOrder, Behaviour.m_Enabled and fields.
    Script { class: String, execution_order: i32, enabled: bool, fields: Value },
    /// An AudioSource, its m_audioClip resolved, and the name of its OutputAudioMixerGroup (None when null).
    AudioSource { source: Box<super::components::AudioSource>, clip: Option<ObjRef>, group: Option<String> },
    /// A WindZone (182) and a ParticleSystemForceField (330), for ExternalForcesModule.
    WindZone(super::particles::WindZone),
    ForceField(std::sync::Arc<super::particles::ForceField>),
    /// A component class the rewrite does not read yet (particles, UI...).
    Other(i32),
}

#[derive(Clone, Debug)]
pub struct PrefabObject {
    pub game_object: ObjRef,
    pub name: String,
    pub layer: u32,
    pub tag: u16,
    pub active: bool,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub components: Vec<(ObjRef, Component)>,
}

/// A GameObject hierarchy, root first, children in Transform order (depth first).
#[derive(Clone, Debug, Default)]
pub struct Prefab {
    pub objects: Vec<PrefabObject>,
    /// Component or GameObject -> (object index, component index; usize::MAX for the GameObject itself).
    pub index: HashMap<ObjRef, (usize, usize)>,
    /// The PhysicsMaterial2D a Collider2D or Rigidbody2D component points at, by (object, component): (friction,
    /// bounciness). A material that does not resolve reads as null and has no entry.
    pub materials: HashMap<(usize, usize), (f32, f32)>,
}

impl AssetDb {
    pub fn open(game: &Path) -> Result<Self, String> {
        let data_dir = game.join("People Playground_Data");
        let mut names: Vec<String> = std::fs::read_dir(&data_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| (n.ends_with(".assets") || n.starts_with("level") || n == "globalgamemanagers") && !n.contains('.') || n.ends_with(".assets"))
            .collect();
        names.sort();
        let mut files = Vec::new();
        let mut index = HashMap::new();
        for n in names {
            index.insert(n.clone(), files.len());
            files.push(SerializedFile::open(&data_dir.join(&n))?);
        }
        Ok(Self { data_dir, files, names: index, managed: Managed::new(game), scripts: HashMap::new(), monos: HashMap::new(), catalog_lookup: None, prefabs: Default::default(), decoded: Default::default(), load_order: Default::default(), sprite_names: Default::default(), material_names: Default::default(), clip_names: Default::default(), texture_names: Default::default(), shader_ids: Default::default(), builtin: HashMap::new() })
    }

    pub fn file_index(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    /// A PPtr written in `from`, resolved; None for null or targets outside the build (Unity's builtin resources).
    pub fn resolve(&self, from: usize, p: PPtr) -> Option<ObjRef> {
        if p.is_null() {
            return None;
        }
        let file = if p.file == 0 {
            from
        } else {
            let ext = self.files[from].externals.get(p.file as usize - 1)?;
            self.file_index(ext.path.rsplit('/').next()?)?
        };
        self.files[file].object(p.path_id)?;
        Some(ObjRef { file, path_id: p.path_id })
    }

    /// `resolve`, and a target in one of Unity's builtin resource files (People Playground_Data/Resources) as well.
    pub fn resolve_builtin(&mut self, from: usize, p: PPtr) -> Option<ObjRef> {
        if let Some(r) = self.resolve(from, p) {
            return Some(r);
        }
        if p.is_null() || p.file == 0 {
            return None;
        }
        let name = self.files[from].externals.get(p.file as usize - 1)?.path.rsplit('/').next()?.to_string();
        let file = match self.builtin.get(&name) {
            Some(&f) => f,
            None => {
                let f = SerializedFile::open(&self.data_dir.join("Resources").join(&name)).ok()?;
                self.files.push(f);
                self.builtin.insert(name, self.files.len() - 1);
                self.files.len() - 1
            }
        };
        self.files[file].object(p.path_id)?;
        Some(ObjRef { file, path_id: p.path_id })
    }

    pub fn class(&self, r: ObjRef) -> i32 {
        self.files[r.file].object(r.path_id).map_or(-1, |o| o.class_id)
    }

    /// The order of the persistent instance ids Unity gives the Resources objects (every Instantiate-able prefab
    /// and its dependencies), as ids 2, 4, ... in that order. They are allocated while globalgamemanagers'
    /// ResourceManager is read at startup: each PPtr it transfers (m_Container's targets, then each
    /// m_DependentAssets entry's object and dependencies) takes the next id on its first appearance
    /// (Remapper::GetOrGenerateInstanceID). The managers read before it (MonoManager's scripts, GraphicsSettings'
    /// shaders, ...) already gave a few of these targets lower ids, but none of them is a GameObject or a
    /// component, so the order among a prefab's own objects (all `instantiate_native` needs) is exact.
    pub fn resource_load_order(&self) -> Result<HashMap<ObjRef, i32>, String> {
        let file = self.file_index("globalgamemanagers").ok_or("no globalgamemanagers")?;
        let manager = self.files[file].objects.iter().find(|o| o.class_id == 147).ok_or("no ResourceManager")?;
        let r = &mut Reader::new(self.data(ObjRef { file, path_id: manager.path_id }));
        let mut order = HashMap::new();
        let mut take = |p: PPtr| {
            if let Some(o) = self.resolve(file, p) {
                let next = 2 * order.len() as i32 + 2;
                order.entry(o).or_insert(next);
            }
        };
        for _ in 0..r.i32()? {
            r.string()?;
            take(PPtr::read(r)?);
        }
        for _ in 0..r.i32()? {
            take(PPtr::read(r)?);
            for _ in 0..r.i32()? {
                take(PPtr::read(r)?);
            }
        }
        Ok(order)
    }

    /// `resource_load_order`, computed once (the data never changes).
    pub fn load_order(&self) -> Result<std::sync::Arc<HashMap<ObjRef, i32>>, String> {
        if let Some(o) = self.load_order.get() {
            return Ok(o.clone());
        }
        Ok(self.load_order.get_or_init(|| std::sync::Arc::new(self.resource_load_order().expect("the resource load order"))).clone())
    }

    /// Resources.Load(path) of an object of `class_id`: the ResourceManager's m_Container entry with that path
    /// (Unity lowercases both sides) and class.
    pub fn resource(&self, path: &str, class_id: i32) -> Result<Option<ObjRef>, String> {
        let file = self.file_index("globalgamemanagers").ok_or("no globalgamemanagers")?;
        let manager = self.files[file].objects.iter().find(|o| o.class_id == 147).ok_or("no ResourceManager")?;
        let r = &mut Reader::new(self.data(ObjRef { file, path_id: manager.path_id }));
        let want = path.to_lowercase();
        for _ in 0..r.i32()? {
            let name = r.string()?;
            let target = PPtr::read(r)?;
            if name == want {
                if let Some(t) = self.resolve(file, target).filter(|&t| self.class(t) == class_id) {
                    return Ok(Some(t));
                }
            }
        }
        Ok(None)
    }

    /// Every ResourceManager m_Container entry of `class_id`: its path (lowercased, as Unity keeps it) and object.
    pub fn resource_entries(&self, class_id: i32) -> Result<Vec<(String, ObjRef)>, String> {
        let file = self.file_index("globalgamemanagers").ok_or("no globalgamemanagers")?;
        let manager = self.files[file].objects.iter().find(|o| o.class_id == 147).ok_or("no ResourceManager")?;
        let r = &mut Reader::new(self.data(ObjRef { file, path_id: manager.path_id }));
        let mut out = Vec::new();
        for _ in 0..r.i32()? {
            let name = r.string()?;
            let target = PPtr::read(r)?;
            out.extend(self.resolve(file, target).filter(|&t| self.class(t) == class_id).map(|t| (name.clone(), t)));
        }
        Ok(out)
    }

    /// Resources.LoadAll(folder) of the objects of `class_id`: the ResourceManager's m_Container entries under the
    /// folder (Unity lowercases both sides).
    pub fn resources_in(&self, folder: &str, class_id: i32) -> Result<Vec<ObjRef>, String> {
        let file = self.file_index("globalgamemanagers").ok_or("no globalgamemanagers")?;
        let manager = self.files[file].objects.iter().find(|o| o.class_id == 147).ok_or("no ResourceManager")?;
        let r = &mut Reader::new(self.data(ObjRef { file, path_id: manager.path_id }));
        let prefix = format!("{}/", folder.to_lowercase());
        let mut out = Vec::new();
        for _ in 0..r.i32()? {
            let name = r.string()?;
            let target = PPtr::read(r)?;
            if name.starts_with(&prefix) {
                out.extend(self.resolve(file, target).filter(|&t| self.class(t) == class_id));
            }
        }
        Ok(out)
    }

    pub fn data(&self, r: ObjRef) -> &[u8] {
        let f = &self.files[r.file];
        f.data(f.object(r.path_id).expect("object"))
    }

    /// A Sprite with the texture data its SpriteAtlas holds, when it is packed in one (Sprite::GetRenderData).
    pub fn sprite(&self, r: ObjRef) -> Result<super::components::Sprite, String> {
        let mut s = super::components::Sprite::read(self.data(r))?;
        if s.texture.is_null() {
            if let Some(atlas) = self.resolve(r.file, s.atlas) {
                let map = super::components::sprite_atlas_data(self.data(atlas))?;
                let (_, d) = map.into_iter().find(|(k, _)| *k == s.render_data_key).ok_or("a sprite missing from its atlas")?;
                s.texture = d.texture;
                s.alpha_texture = d.alpha_texture;
                s.texture_rect = d.texture_rect;
                s.texture_rect_offset = d.texture_rect_offset;
                s.atlas_rect_offset = d.atlas_rect_offset;
                s.uv_transform = d.uv_transform;
                s.downscale_multiplier = d.downscale_multiplier;
                s.settings_raw = d.settings_raw;
            }
        }
        Ok(s)
    }

    /// gapC r6: a Sprite asset by name (the fn-check loads a recorded `ItemSprite sprite=<name>` into ours): the first
    /// Sprite object of that m_Name over the files in order (names repeat across files; the compare is by name).
    pub fn sprite_by_name(&self, name: &str) -> Option<ObjRef> {
        self.named(&self.sprite_names, 213, name)
    }

    /// gapB r24: a Material asset by name, as sprite_by_name (a recorded `SharedDeflatedMaterial=@<name>#<id>`).
    pub fn material_by_name(&self, name: &str) -> Option<ObjRef> {
        self.named(&self.material_names, 21, name)
    }

    /// Rewrite extra (extras_glue's sounds): an AudioClip asset by name, as sprite_by_name.
    pub fn clip_by_name(&self, name: &str) -> Option<ObjRef> {
        self.named(&self.clip_names, 83, name)
    }

    /// A Texture2D asset by name, as sprite_by_name (lights.rs: the FireEffect profile's textures).
    pub fn texture_by_name(&self, name: &str) -> Option<ObjRef> {
        self.named(&self.texture_names, 28, name)
    }

    /// A Shader's id as the code knows it (`shader_ids.rs`: 1.28.41's path id, or 10000 + the first build's for a later
    /// shader), matched by file and name; a shader in a file the table leaves out (a builtin resource file) keeps its own.
    pub fn shader_id(&self, r: ObjRef) -> i64 {
        use super::shader_ids::{shader_name, SHADER_IDS};
        let ids = self.shader_ids.get_or_init(|| {
            let mut m = HashMap::new();
            for (fi, f) in self.files.iter().enumerate() {
                let file = f.path.file_name().unwrap().to_string_lossy();
                if !SHADER_IDS.iter().any(|t| t.0 == file) {
                    continue;
                }
                for o in f.objects.iter().filter(|o| o.class_id == 48) {
                    let n = shader_name(f.data(o)).unwrap_or_else(|e| panic!("{file} shader {}: {e}", o.path_id));
                    // Not a game path: every shader of every accepted build has a row (test shader_ids_for_every_shader), and a mod's
                    // shaders are in other files, skipped above. Only a new game build with a new shader lands here: add its row.
                    let t = SHADER_IDS.iter().find(|t| t.0 == file && t.1 == n).unwrap_or_else(|| panic!("shader_ids.rs has no row for shader {n} in {file}"));
                    m.insert(ObjRef { file: fi, path_id: o.path_id }, t.2);
                }
            }
            m
        });
        ids.get(&r).copied().unwrap_or(r.path_id)
    }

    /// A Material's shader (`p`, written in `file`) as `shader_id` gives it; one outside the loaded files (Unity's builtin
    /// resources, globalgamemanagers' Particles/Standard Unlit 34 and the rest) by its PPtr path id.
    pub fn material_shader_id(&self, file: usize, p: PPtr) -> i64 {
        self.resolve(file, p).map_or(p.path_id, |s| self.shader_id(s))
    }

    fn named(&self, cell: &std::sync::OnceLock<HashMap<String, ObjRef>>, class_id: i32, name: &str) -> Option<ObjRef> {
        cell
            .get_or_init(|| {
                let mut m = HashMap::new();
                for (file, f) in self.files.iter().enumerate() {
                    for o in f.objects.iter().filter(|o| o.class_id == class_id) {
                        let r = ObjRef { file, path_id: o.path_id };
                        if let Ok(n) = Reader::new(self.data(r)).string() {
                            m.entry(n).or_insert(r);
                        }
                    }
                }
                m
            })
            .get(name)
            .copied()
    }

    pub fn game_object(&self, r: ObjRef) -> Result<GameObject, String> {
        GameObject::read(self.data(r))
    }

    /// A MonoBehaviour's script class, or None for a missing script.
    pub fn script(&mut self, r: ObjRef) -> Result<Option<MonoScript>, String> {
        let mb = MonoBehaviour::read(self.data(r))?;
        let Some(s) = self.resolve(r.file, mb.script) else { return Ok(None) };
        if !self.scripts.contains_key(&s) {
            let script = MonoScript::read(self.data(s))?;
            self.scripts.insert(s, script);
        }
        Ok(self.scripts.get(&s).cloned())
    }

    /// A MonoBehaviour's header and fields; None for a missing script.
    pub fn mono(&mut self, r: ObjRef) -> Result<Option<(MonoScript, MonoBehaviour, Value)>, String> {
        if let Some(m) = self.monos.get(&r) {
            return Ok(m.clone());
        }
        let m = self.read_mono(r)?;
        self.monos.insert(r, m.clone());
        Ok(m)
    }

    fn read_mono(&mut self, r: ObjRef) -> Result<Option<(MonoScript, MonoBehaviour, Value)>, String> {
        let Some(script) = self.script(r)? else { return Ok(None) };
        let asm = self.managed.assembly(&script.assembly)?;
        if asm.type_def(&script.namespace, &script.class_name).is_none() {
            return Ok(None);
        }
        let f = &self.files[r.file];
        let data = f.data(f.object(r.path_id).unwrap());
        let mb = MonoBehaviour::read(data)?;
        let mut reader = Reader::new(data);
        reader.pos = mb.fields;
        let v = self.managed.read_script(&script.assembly, &script.namespace, &script.class_name, &mut reader)?;
        if reader.pos != data.len() {
            return Err(format!("{}: read {} of {} bytes", script.class_name, reader.pos, data.len()));
        }
        Ok(Some((script, mb, v)))
    }

    /// Every MonoBehaviour / ScriptableObject whose script class is `class`.
    pub fn find_scripts(&mut self, class: &str) -> Result<Vec<ObjRef>, String> {
        let mut out = Vec::new();
        for file in 0..self.files.len() {
            let ids: Vec<i64> = self.files[file].objects.iter().filter(|o| o.class_id == 114).map(|o| o.path_id).collect();
            for path_id in ids {
                let r = ObjRef { file, path_id };
                if self.script(r)?.is_some_and(|s| s.class_name == class) {
                    out.push(r);
                }
            }
        }
        Ok(out)
    }

    fn component(&mut self, r: ObjRef) -> Result<Component, String> {
        let d = self.data(r);
        Ok(match self.class(r) {
            // RectTransform (224) is a Transform with its UI layout fields after.
            4 | 224 => Component::Transform(Transform::read(d)?),
            50 => Component::Rigidbody2D(Rigidbody2D::read(d)?),
            212 => Component::SpriteRenderer(SpriteRenderer::read(d)?),
            ParentConstraintData::CLASS => Component::ParentConstraint(ParentConstraintData::read(d)?),
            PointEffector2D::CLASS => Component::PointEffector2D(PointEffector2D::read(d)?),
            PointEffector2D::AREA_CLASS => Component::PointEffector2D(PointEffector2D::read_area(d)?),
            c if Collider2D::CLASSES.contains(&c) => Component::Collider2D(Collider2D::read(c, d)?),
            c if Joint2D::CLASSES.contains(&c) => Component::Joint2D(Joint2D::read(c, d)?),
            114 => match self.mono(r)? {
                Some((script, mb, fields)) => Component::Script { class: script.class_name, execution_order: script.execution_order, enabled: mb.enabled, fields },
                None => Component::Other(114),
            },
            super::components::AudioSource::CLASS => {
                let source = super::components::AudioSource::read(d)?;
                let clip = self.resolve(r.file, source.clip);
                // AudioMixerGroup (273): m_Name first.
                let group = match self.resolve(r.file, source.output) {
                    Some(g) => Some(Reader::new(self.data(g)).string()?),
                    None => None,
                };
                Component::AudioSource { source: Box::new(source), clip, group }
            }
            182 => Component::WindZone(super::particles::WindZone::read(d)?),
            330 => Component::ForceField(std::sync::Arc::new(super::particles::ForceField::read(d)?)),
            c => Component::Other(c),
        })
    }

    /// extras: a file made in memory, after the game's files; its index.
    pub fn add_file(&mut self, name: &str, file: SerializedFile) -> usize {
        self.files.push(file);
        self.names.insert(name.to_string(), self.files.len() - 1);
        self.files.len() - 1
    }

    /// extras: a prefab built in code, served by `prefab_cached` under `root`.
    pub fn cache_prefab(&self, root: ObjRef, p: Prefab) {
        self.prefabs.lock().unwrap().insert(root, std::sync::Arc::new(p));
    }

    /// `prefab`, decoded once.
    pub fn prefab_cached(&mut self, root: ObjRef) -> Result<std::sync::Arc<Prefab>, String> {
        if let Some(p) = self.prefabs.lock().unwrap().get(&root) {
            return Ok(p.clone());
        }
        let p = std::sync::Arc::new(self.prefab(root)?);
        self.prefabs.lock().unwrap().insert(root, p.clone());
        Ok(p)
    }

    /// Decodes the textures `list` names on every core, for `decoded_texture` (Unity has them loaded before play).
    pub fn preload_textures(&self, list: &[ObjRef]) {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&t) = list.get(i) else { break };
                    if let Ok(tex) = super::components::Texture2D::read(self.data(t)) {
                        if let Ok(px) = tex.rgba(&self.data_dir) {
                            self.decoded.lock().unwrap().insert(t, std::sync::Arc::new((tex, px)));
                        }
                    }
                });
            }
        });
    }

    /// The textures `preload_textures` decoded, handed over once (in file order).
    pub fn take_decoded(&self) -> Vec<(ObjRef, std::sync::Arc<(super::components::Texture2D, Vec<u8>)>)> {
        let mut v: Vec<_> = self.decoded.lock().unwrap().drain().collect();
        v.sort_by_key(|(t, _)| (t.file, t.path_id));
        v
    }

    /// The GameObject tree rooted at `root` (a GameObject).
    pub fn prefab(&mut self, root: ObjRef) -> Result<Prefab, String> {
        let mut p = Prefab { objects: Vec::new(), index: HashMap::new(), materials: HashMap::new() };
        self.add_object(&mut p, root, None)?;
        Ok(p)
    }

    fn add_object(&mut self, p: &mut Prefab, go: ObjRef, parent: Option<usize>) -> Result<usize, String> {
        let g = self.game_object(go)?;
        let me = p.objects.len();
        p.index.insert(go, (me, usize::MAX));
        p.objects.push(PrefabObject {
            game_object: go,
            name: g.name.clone(),
            layer: g.layer,
            tag: g.tag,
            active: g.active,
            parent,
            children: Vec::new(),
            components: Vec::new(),
        });
        let mut children = Vec::new();
        for c in &g.components {
            let r = self.resolve(go.file, *c).ok_or_else(|| format!("{}: unresolved component", g.name))?;
            let comp = self.component(r).map_err(|e| format!("{}: {e}", g.name))?;
            if let Component::Transform(t) = &comp {
                children = t.children.clone();
            }
            let material = match &comp {
                Component::Collider2D(c) => c.material,
                Component::Rigidbody2D(r) => r.material,
                _ => PPtr::default(),
            };
            if let Some(m) = self.resolve(go.file, material) {
                let m = super::components::PhysicsMaterial2D::read(self.data(m))?;
                p.materials.insert((me, p.objects[me].components.len()), (m.friction, m.bounciness));
            }
            p.index.insert(r, (me, p.objects[me].components.len()));
            p.objects[me].components.push((r, comp));
        }
        for c in children {
            let t = self.resolve(go.file, c).ok_or("unresolved child transform")?;
            let child_go = self.resolve(t.file, Transform::read(self.data(t))?.game_object).ok_or("child without GameObject")?;
            let idx = self.add_object(p, child_go, Some(me))?;
            p.objects[me].children.push(idx);
        }
        Ok(me)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn depth(p: &Prefab, o: &PrefabObject) -> usize {
        o.parent.map_or(0, |i| 1 + depth(p, &p.objects[i]))
    }

    /// Every Shader in the build's files reads its name (shader_ids.rs) and has a row, and the ids the code tests come back
    /// as 1.28.41's whatever this build numbers them.
    #[test]
    fn shader_ids_for_every_shader() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = AssetDb::open(&game).unwrap();
        let ids: Vec<i64> = db.files.iter().enumerate().flat_map(|(file, f)| f.objects.iter().filter(|o| o.class_id == 48).map(move |o| ObjRef { file, path_id: o.path_id })).map(|r| db.shader_id(r)).collect();
        assert!(ids.len() >= 115, "{} shaders", ids.len());
        println!("{} shaders, {} with ids", ids.len(), ids.iter().filter(|&&i| i > 0).count());
        for id in [34, 37, 130, 888, 891, 898] {
            assert!(ids.contains(&id), "no shader takes id {id}");
        }
    }

    /// Prints every CatalogData item's asset name, one per line after "ITEM " (for the catalog sweep runner).
    #[test]
    #[ignore]
    fn catalog_item_names() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            println!("ITEM {}", db.mono(asset).unwrap().unwrap().1.name);
        }
    }

    /// Lists the catalog prefabs' Rigidbody2Ds under another Rigidbody2D: child (type), nearest body above (type),
    /// the child's scripts and joints. Run with --ignored --nocapture.
    #[test]
    #[ignore]
    fn catalog_nested_bodies() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab(prefab).unwrap();
            let body = |o: usize| p.objects[o].components.iter().find_map(|(_, c)| match c { Component::Rigidbody2D(rb) => Some(rb.body_type), _ => None });
            for o in 0..p.objects.len() {
                let Some(t) = body(o) else { continue };
                let mut up = p.objects[o].parent;
                while let Some(u) = up {
                    if let Some(tu) = body(u) {
                        let what: Vec<String> = p.objects[o].components.iter().filter_map(|(_, c)| match c {
                            Component::Script { class, .. } => Some(class.clone()),
                            Component::Joint2D(j) => Some(format!("{:?}", std::mem::discriminant(&j.kind)).chars().take(0).collect::<String>() + "Joint"),
                            _ => None,
                        }).collect();
                        println!("NEST {} | {} ({t}) under {} ({tu}) | {}", p.objects[0].name, p.objects[o].name, p.objects[u].name, what.join(","));
                        break;
                    }
                    up = p.objects[u].parent;
                }
            }
        }
    }

    /// Every asset instance of the scripts the rewrite stops on when off their PhysicalBehaviour's object
    /// (`PPG_CLASSES=A,B -- --ignored --nocapture`): "OFF class.field" where the field's component is on another
    /// GameObject, "BARE class" where the script's own GameObject has no PhysicalBehaviour.
    #[test]
    #[ignore]
    fn scripts_off_their_physical() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let classes = std::env::var("PPG_CLASSES").unwrap_or_default();
        let fields = ["PhysicalBehaviour", "physicalBehaviour", "trigger", "Slider", "ColliderToDisable", "SliderJoint", "joint", "SpriteRenderer"];
        for class in classes.split(',').filter(|c| !c.is_empty()) {
            for r in db.find_scripts(class).unwrap() {
                let Ok(Some((_, mb, v))) = db.mono(r) else { continue };
                let Some(go) = db.resolve(r.file, mb.game_object) else { continue };
                let g = db.game_object(go).unwrap();
                let mut has_pb = false;
                for c in &g.components {
                    if let Some(c) = db.resolve(go.file, *c).filter(|c| db.class(*c) == 114) {
                        has_pb |= db.script(c).unwrap().is_some_and(|s| s.class_name == "PhysicalBehaviour");
                    }
                }
                if !has_pb {
                    println!("BARE {class} on {} ({}:{})", g.name, go.file, go.path_id);
                }
                for f in fields {
                    let Some(p) = v.get(f).and_then(|x| x.pptr()).filter(|p| !p.is_null()) else { continue };
                    let Some(t) = db.resolve(r.file, p) else { continue };
                    let tgo = PPtr::read(&mut Reader::new(db.data(t))).ok().and_then(|q| db.resolve(t.file, q));
                    if tgo != Some(go) {
                        println!("OFF {class}.{f} on {}", g.name);
                    }
                }
            }
        }
    }

    /// Every root Transform's tree in the data: "BREAKBELOW" for a GameObject with a Collider2D and no PhysicalBehaviour
    /// that has a descendant with a Break receiver (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn bare_colliders_with_break_below() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let receivers = ["BalloonBehaviour", "BulbBehaviour", "CathodeLightBehaviour", "DamagableMachineryBehaviour", "DestroyableBehaviour", "VaseBehaviour", "GlowtubeBehaviour",
            "OldSyringeBehaviour", "SyringeBehaviour", "TelevisionBehaviour", "TyreDeflationBehaviour", "AcidSyringe", "AdrenalineSyringe", "BoneEatingPoisonSyringe", "CoagulationSyringe",
            "DeathSyringe", "EmptySyringe", "FreezeSyringe", "KnockoutSyringe", "LifeSyringe", "MendingSyringe", "PainKillerSyringe", "PinkSyringe", "UltraStrengthSyringe",
            "WaterBreathingSyringe", "ZombieSyringe"];
        let mut roots = Vec::new();
        for file in 0..db.files.len() {
            for o in db.files[file].objects.iter().filter(|o| o.class_id == 4) {
                let r = ObjRef { file, path_id: o.path_id };
                if let Ok(t) = Transform::read(db.data(r)) && t.father.is_null() && let Some(go) = db.resolve(file, t.game_object) {
                    roots.push(go);
                }
            }
        }
        for root in roots {
            let Ok(p) = db.prefab(root) else { continue };
            let has = |o: usize, f: &dyn Fn(&Component) -> bool| p.objects[o].components.iter().any(|(_, c)| f(c));
            for o in 0..p.objects.len() {
                if !has(o, &|c| matches!(c, Component::Collider2D(_))) || has(o, &|c| matches!(c, Component::Script { class, .. } if class == "PhysicalBehaviour")) {
                    continue;
                }
                let mut stack = p.objects[o].children.clone();
                while let Some(x) = stack.pop() {
                    if has(x, &|c| matches!(c, Component::Script { class, .. } if receivers.contains(&class.as_str()))) {
                        println!("BREAKBELOW {} | {} above {}", p.objects[0].name, p.objects[o].name, p.objects[x].name);
                    }
                    stack.extend(p.objects[x].children.iter().copied());
                }
            }
        }
    }

    /// Non-trigger colliders on objects without a PhysicalBehaviour, per catalog item (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn catalog_bare_colliders() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab(prefab).unwrap();
            let has = |o: usize, class: &str| p.objects[o].components.iter().any(|(_, c)| matches!(c, Component::Script { class: k, .. } if k == class));
            for o in 0..p.objects.len() {
                if has(o, "PhysicalBehaviour") {
                    continue;
                }
                for (_, c) in &p.objects[o].components {
                    let Component::Collider2D(col) = c else { continue };
                    if col.is_trigger {
                        continue;
                    }
                    let rb = p.objects[o].components.iter().any(|(_, c)| matches!(c, Component::Rigidbody2D(_)));
                    let parent = p.objects[o].parent.map_or("-".into(), |u| format!("{}{}", p.objects[u].name, if has(u, "PhysicalBehaviour") { "[PB]" } else { "" }));
                    println!("BARE {} | {} layer {} active {} rb {rb} | parent {parent} | {:?}", p.objects[0].name, p.objects[o].name, p.objects[o].layer, p.objects[o].active, std::mem::discriminant(&col.shape));
                }
            }
        }
    }

    /// One catalog item's prefab (`PPG_ITEM=name`): per object its layer, parent and components, with the raw bytes
    /// of the classes the rewrite does not read (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn catalog_dump_item() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let Ok(name) = std::env::var("PPG_ITEM") else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab(prefab).unwrap();
            if p.objects[0].name != name {
                continue;
            }
            for (i, o) in p.objects.iter().enumerate() {
                println!("OBJ {i} {} layer {} active {} parent {:?}", o.name, o.layer, o.active, o.parent);
                for (r, c) in &o.components {
                    match c {
                        Component::Collider2D(c) => println!("  COLLIDER enabled {} trigger {} effector {} {:?}", c.enabled, c.is_trigger, c.used_by_effector, c.shape),
                        Component::Other(id) => println!("  OTHER {id} {:02x?}", db.data(*r)),
                        Component::Script { class, .. } => println!("  SCRIPT {class}"),
                        Component::Rigidbody2D(_) => println!("  RIGIDBODY"),
                        Component::Joint2D(_) => println!("  JOINT"),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Every world-space TextMeshPro in the catalog: item, object, its fields (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn catalog_world_texts() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        let mut first = true;
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab(prefab).unwrap();
            for o in &p.objects {
                for (_, c) in &o.components {
                    let Component::Script { class, fields, enabled, .. } = c else { continue };
                    if class != "TextMeshPro" { continue }
                    if first {
                        if let Value::Struct(f) = fields { println!("KEYS {:?}", f.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()); }
                        first = false;
                    }
                    let g = |n: &str| fields.get(n).map(|v| format!("{v:?}")).unwrap_or_default();
                    for (r, x) in &o.components {
                        if matches!(x, Component::Other(23)) {
                            println!("MR {} {:?} rect {:?}", o.name, crate::assets::components::MeshRenderer::read(db.data(*r)), crate::assets::classes::RectTransform::read(db.data(o.components[0].0)).map(|r| (r.size_delta, r.pivot, r.anchor_min, r.anchor_max)));
                        }
                    }
                    let others: Vec<String> = o.components.iter().filter_map(|(_, x)| match x { Component::Other(i) => Some(i.to_string()), Component::Script { class, .. } => Some(class.clone()), _ => None }).collect();
                    println!("TMP {} / {} en {} active {} | text {} | size {} auto {} | color {} | align {} {} {} | wrap {} overflow {} | style {} | rich {} | font {} | comps {:?}", p.objects[0].name, o.name, enabled, o.active, g("m_text"), g("m_fontSize"), g("m_enableAutoSizing"), g("m_fontColor"), g("m_textAlignment"), g("m_HorizontalAlignment"), g("m_VerticalAlignment"), g("m_enableWordWrapping"), g("m_overflowMode"), g("m_fontStyle"), g("m_isRichText"), g("m_fontAsset"), others);
                }
            }
        }
    }

    /// Every ParentConstraint (class 1773428102) in the game's files reads to its end; the Killzone prefab's one is printed.
    #[test]
    fn parent_constraints_read() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = AssetDb::open(&game).unwrap();
        let mut n = 0;
        for f in 0..db.files.len() {
            for o in db.files[f].objects.iter().filter(|o| o.class_id == ParentConstraintData::CLASS) {
                ParentConstraintData::read(db.data(ObjRef { file: f, path_id: o.path_id })).unwrap();
                n += 1;
            }
        }
        assert!(n > 0);
        let mut db = db;
        let k = db.resource("Prefabs/Killzone", 1).unwrap().unwrap();
        let p = db.prefab(k).unwrap();
        for o in &p.objects {
            println!("KILLZONE {} active {} parent {:?}", o.name, o.active, o.parent);
            for (_, c) in &o.components {
                match c {
                    Component::Script { class, .. } => println!("  SCRIPT {class}"),
                    Component::ParentConstraint(x) => println!("  {x:?}"),
                    Component::Other(id) => println!("  OTHER {id}"),
                    _ => println!("  {:?}", std::mem::discriminant(c)),
                }
            }
        }
    }

    /// A TMP_FontAsset's fields (`PPG_FONT=path_id`, `-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn font_asset_dump() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let Ok(id) = std::env::var("PPG_FONT") else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let r = ObjRef { file: catalog.file, path_id: id.parse().unwrap() };
        let (_, _, v) = db.mono(r).unwrap().unwrap();
        let Value::Struct(f) = &v else { return };
        for (n, x) in f {
            let s = format!("{x:?}");
            println!("FIELD {n} = {}", &s[..s.len().min(600)]);
        }
    }

    /// The component class ids the rewrite does not read, per catalog item (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn catalog_other_components() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        let mut classes: BTreeMap<i32, Vec<String>> = BTreeMap::new();
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab(prefab).unwrap();
            for o in &p.objects {
                for (_, c) in &o.components {
                    if let Component::Other(id) = c {
                        let items = classes.entry(*id).or_default();
                        if items.last() != Some(&p.objects[0].name) {
                            items.push(p.objects[0].name.clone());
                        }
                    }
                }
            }
        }
        for (class, items) in &classes {
            println!("OTHER {class} {} | {}", items.len(), items.iter().take(6).cloned().collect::<Vec<_>>().join(", "));
        }
    }

    /// Every MonoBehaviour class in the catalog prefabs, with how many items use it (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn catalog_script_classes() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        let mut classes: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab(prefab).unwrap();
            for o in &p.objects {
                for (_, c) in &o.components {
                    if let Component::Script { class, .. } = c {
                        let items = classes.entry(class.clone()).or_default();
                        if items.last() != Some(&p.objects[0].name) {
                            items.push(p.objects[0].name.clone());
                        }
                    }
                }
            }
        }
        for (class, items) in &classes {
            println!("CLASS {class} {} | {}", items.len(), items.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
        }
    }

    /// Rigidbody2Ds on objects without a PhysicalBehaviour, with their scripts and the joints connected to them.
    #[test]
    #[ignore]
    fn catalog_bare_bodies() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab(prefab).unwrap();
            let has = |o: usize, class: &str| p.objects[o].components.iter().any(|(_, c)| matches!(c, Component::Script { class: k, .. } if k == class));
            for o in 0..p.objects.len() {
                let Some(rb) = p.objects[o].components.iter().find_map(|(_, c)| if let Component::Rigidbody2D(rb) = c { Some(rb) } else { None }) else { continue };
                if has(o, "PhysicalBehaviour") {
                    continue;
                }
                let scripts: Vec<&str> = p.objects[o].components.iter().filter_map(|(_, c)| if let Component::Script { class, .. } = c { Some(class.as_str()) } else { None }).collect();
                let comps = p.objects[o].components.len();
                let parent = p.objects[o].parent.map_or("-".into(), |u| format!("{}{}", p.objects[u].name, if has(u, "PhysicalBehaviour") { "[PB]" } else { "" }));
                println!("BODY {} | {} type {} active {} comps {comps} children {} | parent {parent} | {scripts:?}", p.objects[0].name, p.objects[o].name, rb.body_type, p.objects[o].active, p.objects[o].children.len());
            }
        }
    }

    /// The game's catalog (CatalogData.Items) resolves, and every spawnable prefab's tree reads completely.
    #[test]
    fn catalog_prefabs_load() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalogs = db.find_scripts("CatalogData").unwrap();
        assert_eq!(catalogs.len(), 1, "one CatalogData");
        let (_, _, catalog) = db.mono(catalogs[0]).unwrap().unwrap();
        let items = catalog.get("Items").and_then(Value::array).unwrap().to_vec();
        assert!(items.len() >= 284, "{} items", items.len());
        let mut other: BTreeMap<i32, usize> = BTreeMap::new();
        let mut names = Vec::new();
        for item in &items {
            let asset = db.resolve(catalogs[0].file, item.pptr().unwrap()).expect("SpawnableAsset");
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else {
                continue;
            };
            let p = db.prefab(prefab).unwrap();
            names.push((p.objects[0].name.clone(), prefab));
            for o in &p.objects {
                for (_, c) in &o.components {
                    if let Component::Other(c) = c {
                        *other.entry(*c).or_default() += 1;
                    }
                }
            }
        }
        eprintln!("{} prefabs; unread component classes: {other:?}", names.len());
        assert!(names.iter().any(|(n, _)| n == "Knife"), "no Knife prefab in the catalog");
        // Person's limbs: each has a LimbBehaviour, a Rigidbody2D and a collider.
        let person = names.iter().find(|(n, _)| n == "Person").expect("no Person prefab in the catalog").1;
        let p = db.prefab(person).unwrap();
        let limbs: Vec<&PrefabObject> = p
            .objects
            .iter()
            .filter(|o| o.components.iter().any(|(_, c)| matches!(c, Component::Script { class, .. } if class == "LimbBehaviour")))
            .collect();
        assert_eq!(limbs.len(), 14, "{:?}", limbs.iter().map(|l| &l.name).collect::<Vec<_>>());
        for l in limbs {
            assert!(l.components.iter().any(|(_, c)| matches!(c, Component::Rigidbody2D(_))), "{}", l.name);
            assert!(l.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_))), "{}", l.name);
        }
    }

    /// Every object in the install's serialized files, one "OBJ" line each: file, path id, class id, size, name (the
    /// script class for a MonoBehaviour) and a hash of its bytes, and for a MonoBehaviour one of its fields read with the
    /// installed DLL's layout; then each FirearmBehaviour's StandardParticleOverride (1.28.52's new field). For comparing two
    /// builds' data (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn object_survey() {
        use std::hash::{Hash, Hasher};
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let h = |x: &dyn Fn(&mut std::collections::hash_map::DefaultHasher)| {
            let mut s = std::collections::hash_map::DefaultHasher::new();
            x(&mut s);
            s.finish()
        };
        let names: Vec<(String, usize)> = db.names.iter().map(|(n, &i)| (n.clone(), i)).collect();
        for (fname, fi) in names {
            let objects = db.files[fi].objects.clone();
            for o in objects {
                let r = ObjRef { file: fi, path_id: o.path_id };
                let bytes = h(&|s| db.data(r).hash(s));
                let named = [28, 213, 83, 21, 43, 74, 49, 48, 128, 90, 91, 95, 150, 152, 156, 187];
                let (name, fields) = if o.class_id == 114 {
                    match db.mono(r) {
                        Ok(Some((sc, mb, v))) => {
                            if sc.class_name == "FirearmBehaviour" {
                                let go = db.resolve(fi, mb.game_object).and_then(|g| db.game_object(g).ok()).map(|g| g.name).unwrap_or_default();
                                println!("SPO {fname} {} {go} {:?}", o.path_id, v.get("StandardParticleOverride"));
                            }
                            let mut v = v;
                            if let Value::Struct(f) = &mut v {
                                f.retain(|(n, _)| n != "StandardParticleOverride");
                            }
                            (format!("{}:{}", sc.class_name, mb.name), format!("{:x}", h(&|s| format!("{v:?}").hash(s))))
                        }
                        Ok(None) => ("missing-script".into(), String::new()),
                        Err(e) => (format!("ERR {e}"), String::new()),
                    }
                } else if o.class_id == 198 {
                    let ps = crate::assets::particles::ParticleSystem::read(db.data(r)).unwrap();
                    let go = PPtr::read(&mut crate::assets::serialized::Reader::new(db.data(r))).ok().and_then(|p| db.resolve(fi, p)).and_then(|g| db.game_object(g).ok()).map(|g| g.name).unwrap_or_default();
                    (format!("{go} color{}", ps.start_color.state), String::new())
                } else if o.class_id == 1 {
                    (db.game_object(r).map(|g| g.name).unwrap_or_default(), String::new())
                } else if named.contains(&o.class_id) {
                    (crate::assets::serialized::Reader::new(db.data(r)).string().unwrap_or_default(), String::new())
                } else {
                    (String::new(), String::new())
                };
                println!("OBJ	{fname}	{}	{}	{}	{name}	{bytes:x}	{fields}", o.path_id, o.class_id, o.byte_size);
            }
        }
    }
}
