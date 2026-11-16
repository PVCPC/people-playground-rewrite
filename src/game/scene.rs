//! The Main scene (level1) as SceneManager loads it: the serialized state of the scene objects the simulation
//! reads, with the Awake/Start effects that touch it. A scene object's instance id is the scene's base id plus
//! twice its index in the file (traced under cdb), so the file order is the id order.
use super::{temperature::AmbientTemperatureGridBehaviour, tool::Tool, Game, PlayAudioAtRandom, Prefs};
use crate::b2::settings::Physics2DSettings;
use crate::b2::world::{BodyId, World};
use crate::assets::classes::{MonoBehaviour, Transform};
use crate::assets::db::{AssetDb, ObjRef};
use crate::assets::mono::Value;
use crate::assets::serialized::Reader;
use std::path::Path;

/// A scene MonoBehaviour: its class, whether Unity runs its Awake/Start (enabled, on an object active in the
/// hierarchy), and its serialized fields.
struct SceneScript {
    class: String,
    runs: bool,
    fields: Value,
    game_object: ObjRef,
    /// The behaviour's index among the file's objects (its instance id is the scene's base id plus twice this).
    at: usize,
    /// Its GameObject active in the hierarchy (Awake runs; Start also needs `enabled`).
    active: bool,
}

/// The scene singletons with a static Instance (or Main) property that their Awake sets.
pub const SINGLETONS: &[&str] = &["BulletTracerPool", "ClipboardControllerBehaviour", "ContinuousActivationBehaviour", "GlobalShrapnelEmitter", "LimbSpriteCache",
    "PoolGenerator", "ProceduralShatterMaterialPool", "ThermalVisionBehaviour", "ToolLibrary", "ToolWheelBehaviour", "PianoDialogBox", "BackgroundItemLoader",
    "BackgroundItemLoaderStatusBehaviour", "LayerEditorController"];

/// Every MonoBehaviour of a scene file, in file order.
/// The scene's BulletTracerPool (its Awake only sets Instance): MaxPoolSize, InactivityThresholdInSeconds and the ids an
/// Instantiate of its Prefab takes.
fn tracer_pool(db: &mut AssetDb, s: &SceneScript) -> Result<super::ballistics::TracerPool, String> {
    let v = &s.fields;
    let prefab = v.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(s.game_object.file, p)).ok_or("BulletTracerPool.Prefab")?;
    let prefab = db.prefab(prefab)?;
    for o in &prefab.objects {
        for (_, c) in &o.components {
            if let crate::assets::db::Component::Script { class, .. } = c {
                if class != "PoolableSizeHelper" {
                    unimplemented!("mod-only: a {class} on the BulletTracerPool prefab");
                }
            }
        }
    }
    Ok(super::ballistics::TracerPool {
        clone_ids: prefab.objects.iter().map(|o| 1 + o.components.len()).sum(),
        MaxPoolSize: v.get("MaxPoolSize").and_then(Value::i64).ok_or("BulletTracerPool.MaxPoolSize")? as usize,
        InactivityThresholdInSeconds: v.get("InactivityThresholdInSeconds").and_then(Value::f32).ok_or("BulletTracerPool.InactivityThresholdInSeconds")?,
        ..Default::default()
    })
}

fn scene_scripts(db: &mut AssetDb, scene: &str) -> Result<Vec<SceneScript>, String> {
    let file = db.file_index(scene).ok_or_else(|| format!("no {scene}"))?;
    let ids: Vec<(usize, i64)> = db.files[file].objects.iter().enumerate().filter(|(_, o)| o.class_id == 114).map(|(i, o)| (i, o.path_id)).collect();
    let mut out = Vec::new();
    for (at, path_id) in ids {
        let r = ObjRef { file, path_id };
        let Some((script, mb, fields)) = db.mono(r)? else { continue };
        let active = active_in_hierarchy(db, file, &mb)?;
        let runs = mb.enabled && active;
        let game_object = db.resolve(file, mb.game_object).ok_or("a behaviour without its GameObject")?;
        out.push(SceneScript { class: script.class_name, runs, fields, game_object, at, active });
    }
    Ok(out)
}

/// BellClipContainer.BellClips: each BellClip's Clip.
fn bell_clip_refs(db: &AssetDb, cache: &mut std::collections::HashMap<ObjRef, super::audio::ClipRef>, file: usize, v: &Value) -> Result<Vec<Option<super::audio::ClipRef>>, String> {
    v.get("BellClips").and_then(Value::array).ok_or("BellClipContainer.BellClips")?.iter().map(|c| super::audio::clip_ref(db, cache, c.get("Clip").and_then(Value::pptr).and_then(|p| db.resolve(file, p)))).collect()
}

/// BellClipContainer.BellClips: each BellClip's Name and Key.
fn bell_clips(v: &Value) -> Result<Vec<(String, i32)>, String> {
    v.get("BellClips").and_then(Value::array).ok_or("BellClipContainer.BellClips")?.iter().map(|c| {
        let name = match c.get("Name") { Some(Value::Str(s)) => s.clone(), _ => return Err("BellClip.Name".to_string()) };
        Ok((name, c.get("Key").and_then(Value::i64).ok_or("BellClip.Key")? as i32))
    }).collect()
}

/// GameObject.activeInHierarchy of a behaviour's object: it and every ancestor active. A RectTransform (224)
/// starts with Transform's fields.
fn active_in_hierarchy(db: &AssetDb, file: usize, mb: &MonoBehaviour) -> Result<bool, String> {
    let mut go = db.resolve(file, mb.game_object).ok_or("a behaviour without its GameObject")?;
    loop {
        let g = db.game_object(go)?;
        if !g.active {
            return Ok(false);
        }
        let t = g.components.iter().filter_map(|&c| db.resolve(go.file, c)).find(|&c| matches!(db.class(c), 4 | 224)).ok_or_else(|| format!("{}: no Transform", g.name))?;
        let t = Transform::read(db.data(t))?;
        if t.father.is_null() {
            return Ok(true);
        }
        let father = db.resolve(go.file, t.father).ok_or("a Transform parent outside the data")?;
        go = db.resolve(go.file, Transform::read(db.data(father))?.game_object).ok_or("a Transform without its GameObject")?;
    }
}

impl Game {
    /// globalgamemanagers' TimeManager (class 5), as its StreamedBinaryRead transfer (1805359f0) reads it:
    /// Fixed Timestep, Maximum Allowed Timestep (SetMaximumDeltaTime keeps it at least the fixed step), m_TimeScale,
    /// and Maximum Particle Timestep (particles only).
    pub fn load_time_manager(&mut self, db: &AssetDb) -> Result<(), String> {
        let file = db.file_index("globalgamemanagers").ok_or("no globalgamemanagers")?;
        let manager = db.files[file].objects.iter().find(|o| o.class_id == 5).ok_or("no TimeManager")?;
        let r = &mut Reader::new(db.data(ObjRef { file, path_id: manager.path_id }));
        self.time.fixedDeltaTime = r.f32()?;
        self.time.maximumDeltaTime = super::mathf::max(r.f32()?, self.time.fixedDeltaTime);
        self.time.timeScale = r.f32()?;
        Ok(())
    }

    /// globalgamemanagers' Physics2DSettings (class 19).
    pub fn load_physics2d_settings(db: &AssetDb) -> Result<Physics2DSettings, String> {
        let file = db.file_index("globalgamemanagers").ok_or("no globalgamemanagers")?;
        let settings = db.files[file].objects.iter().find(|o| o.class_id == 19).ok_or("no Physics2DSettings")?;
        let r = &mut Reader::new(db.data(ObjRef { file, path_id: settings.path_id }));
        Physics2DSettings::read(r)
    }

    /// PhysicsManager2D at startup: the b2 globals (Physics2DSettings::UpdateBox2D), a world at the settings' gravity
    /// with their layer collision matrix and CallbacksOnDisable. What the rewrite does not model is refused.
    pub fn apply_physics2d_settings(&mut self, s: &Physics2DSettings) {
        if s.job_options.use_multithreading {
            unimplemented!("mod-only: b2World with PhysicsJobOptions2D.useMultithreading");
        }
        if !s.default_material.is_null() {
            unimplemented!("mod-only: Physics2DSettings.m_DefaultMaterial");
        }
        if s.simulation_mode != 0 {
            unimplemented!("mod-only: Physics2D.simulationMode other than FixedUpdate");
        }
        if s.queries_hit_triggers || s.queries_start_in_colliders {
            unimplemented!("mod-only: Physics2D.queriesHitTriggers / queriesStartInColliders");
        }
        if !s.reuse_collision_callbacks || !s.auto_sync_transforms {
            unimplemented!("mod-only: Physics2D.reuseCollisionCallbacks / autoSyncTransforms off");
        }
        s.update_box2d();
        self.world = World::new(s.gravity);
        self.world.layer_collision_matrix = s.layer_collision_matrix.clone();
        self.world.contacts2d.callbacks_on_disable = s.callbacks_on_disable;
    }

    /// ClearButtonBehaviour.ToRemove (LayerMask.m_Bits) of the scene's running ClearButtonBehaviour.
    pub fn read_clear_mask(db: &mut AssetDb) -> Result<u32, String> {
        let scripts = scene_scripts(db, "level1")?;
        let button = scripts.iter().find(|s| s.runs && s.class == "ClearButtonBehaviour").ok_or("level1 has no running ClearButtonBehaviour")?;
        Ok(button.fields.get("ToRemove").and_then(|m| m.get("m_Bits")).and_then(Value::i64).ok_or("ClearButtonBehaviour.ToRemove")? as u32)
    }

    /// ToolLibrary.Tools and ToolLibrary.Powers of the scene's ToolLibrary.
    pub fn read_tool_library(db: &mut AssetDb) -> Result<(Vec<super::tool::ToolEntry>, Vec<super::tool::ToolEntry>), String> {
        let scripts = scene_scripts(db, "level1")?;
        let library = scripts.iter().find(|s| s.runs && s.class == "ToolLibrary").ok_or("level1 has no running ToolLibrary")?;
        let read = |field: &str| -> Result<Vec<super::tool::ToolEntry>, String> {
            let list = library.fields.get(field).and_then(Value::array).ok_or_else(|| format!("ToolLibrary.{field}"))?;
            let text = |t: &Value, key: &str| match t.get(key) {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            };
            Ok(list.iter().map(|t| super::tool::ToolEntry { name: text(t, "Name"), ty: text(t, "Type"), parent: text(t, "Parent") }).collect())
        };
        Ok((read("Tools")?, read("Powers")?))
    }

    /// The scene's ToolController after ToolControllerBehaviour.Start (`body` is its Rigidbody2D). SetTool(0)
    /// adds ToolLibrary.Tools[0]'s component and calls OnToolChosen: the DragTool starts at its field
    /// initializers (rotationSpeed 1) with isActive set, and its Awake only subscribes to
    /// ContinuousActivationBehaviour. The SelectionController's selection starts empty.
    pub fn scene_tool(db: &mut AssetDb, body: BodyId) -> Result<Tool, String> {
        let scripts = scene_scripts(db, "level1")?;
        let running = |class: &str| scripts.iter().find(|s| s.runs && s.class == class).ok_or_else(|| format!("level1 has no running {class}"));
        let library = running("ToolLibrary")?;
        let first = library.fields.get("Tools").and_then(Value::array).and_then(|t| t.first()).ok_or("ToolLibrary.Tools is empty")?;
        let Some(Value::Str(ty)) = first.get("Type") else { return Err("ToolLibrary.Tool.Type".into()) };
        if ty != "DragTool" {
            unimplemented!("mod-only: ToolControllerBehaviour.SetTool(0) of a {ty}");
        }
        let controller = running("ToolControllerBehaviour")?;
        let mask = controller.fields.get("SelectionLayer").and_then(|m| m.get("m_Bits")).and_then(Value::i64).ok_or("SelectionLayer")?;
        let g = db.game_object(controller.game_object)?;
        let t = g.components.iter().filter_map(|&c| db.resolve(controller.game_object.file, c)).find(|&c| db.class(c) == 4).ok_or("ToolController without a Transform")?;
        let t = Transform::read(db.data(t))?;
        if !t.father.is_null() {
            return Err("ToolController is not a root object".into());
        }
        Ok(Tool {
            body,
            position: t.local_position,
            rotation: t.local_rotation,
            SelectionLayer: mask as u32,
            isCurrentlyHolding: false,
            currentlyHovering: None,
            isSelecting: false,
            selectionDragStart: [0.0; 2],
            rawSelection: Vec::new(),
            hover: None,
            offset: [0.0; 2],
            dragJoint: None,
            destroying: Vec::new(),
            controlRotation: false,
            selectedRigidbody: None,
            rotationSpeed: 1.0,
            currentRotation: 0.0,
            snappedMouseDelta: [0.0; 3],
            objectsToMoveInPauseMode: Vec::new(),
            prevMousePos: [0.0; 3],
            initialSnappingFinished: false,
            isActive: true,
            shouldSendContinuousUseMessage: false,
            initialTranslationSnapOffset: [0.0; 3],
            ActiveSingleSelected: None,
            gentle: false,
            gentle_added: false,
            parentCycleIndex: 0,
            childCycleIndex: 0,
            childCount: 0,
            tab: 0,
            invisible: false,
            chosen: String::new(),
        })
    }

    /// The rest of LoadScene("Main") after `load_main`: the scene's Physics2D natives (`load_scene_natives`),
    /// the ToolController after its Start (`scene_tool`) and the resize handles ResizeHandles.Update hides.
    pub fn load_scene(&mut self, db: &mut AssetDb) -> Result<(), String> {
        let mut scene = self.load_scene_natives(db)?;
        self.tool = Some(Game::scene_tool(db, scene.tool_body)?);
        let scripts = scene_scripts(db, "level1")?;
        self.load_resize_handles(db, &scripts, &mut scene.roots)?;
        let centers: Vec<_> = scripts.iter().filter(|s| s.runs && matches!(s.class.as_str(), "CenterOfGravity" | "CenterOfBounds")).map(|s| (s.class == "CenterOfBounds", s.fields.clone(), s.game_object)).collect();
        self.load_centers(db, &centers)?;
        let wheel = scripts.iter().find(|s| s.runs && s.class == "ToolWheelBehaviour").ok_or("level1 has no running ToolWheelBehaviour")?;
        let mono = |db: &mut AssetDb, name: &str| -> Result<Value, String> {
            let r = wheel.fields.get(name).and_then(Value::pptr).and_then(|p| db.resolve(wheel.game_object.file, p)).ok_or_else(|| format!("ToolWheelBehaviour.{name}"))?;
            Ok(db.mono(r)?.ok_or_else(|| format!("ToolWheelBehaviour.{name}: no MonoBehaviour"))?.2)
        };
        let (wave, image) = (mono(db, "EyeAnimation")?, mono(db, "EyeImage")?);
        self.eye = super::tool_wheel::EyeAnimation::read(&wave, &image)?;
        Ok(())
    }

    /// For a recording (which has no scene): level1's resize handles root alone, as `load_scene` makes it.
    pub(super) fn load_scene_resize_handles(&mut self, db: &mut AssetDb) -> Result<(), String> {
        let scripts = scene_scripts(db, "level1")?;
        let go = scripts.iter().find(|s| s.runs && s.class == "ResizeHandles").ok_or("level1 has no running ResizeHandles")?.game_object;
        let mut roots = self.scene_roots(db, |p| p.objects.iter().any(|o| o.game_object == go))?;
        self.load_resize_handles(db, &scripts, &mut roots)
    }

    /// The resize handles (taken out of `roots`) that ResizeHandles.Update hides.
    fn load_resize_handles(&mut self, db: &mut AssetDb, scripts: &[SceneScript], roots: &mut Vec<super::spawn::Instance>) -> Result<(), String> {
        let handles = scripts.iter().find(|s| s.runs && s.class == "ResizeHandles").ok_or("level1 has no running ResizeHandles")?;
        if !handles.fields.get("Targets").and_then(Value::array).ok_or("ResizeHandles.Targets")?.is_empty() {
            unimplemented!("mod-only: ResizeHandles with serialized Targets");
        }
        let root = roots.iter().position(|r| r.prefab.objects.iter().any(|o| o.game_object == handles.game_object)).ok_or("ResizeHandles outside the scene's Physics2D roots")?;
        let inst = roots.swap_remove(root);
        let o = inst.prefab.objects.iter().position(|o| o.game_object == handles.game_object).unwrap();
        let file = handles.game_object.file;
        let mut handle = |name: &str| -> Result<usize, String> {
            let r = handles.fields.get(name).and_then(Value::pptr).and_then(|p| db.resolve(file, p)).ok_or_else(|| format!("ResizeHandles.{name}"))?;
            Ok(inst.prefab.index.get(&r).ok_or_else(|| format!("ResizeHandles.{name} outside its object"))?.0)
        };
        self.resize.handles = (handle("HorizontalHandle")?, handle("VerticalHandle")?);
        // The object starts active; its first Update deactivates it while there are no Targets.
        self.resize.active = true;
        self.resize_add_visuals(db, file, &handles.fields, &inst)?;
        self.resize_handles = Some((inst, o));
        Ok(())
    }

    /// The DistantSoundBehaviour's clip counts and the bell clips, for a recording (which has no scene).
    pub(super) fn load_distant_sounds(&mut self, db: &mut AssetDb) -> Result<(), String> {
        let scripts = scene_scripts(db, "level1")?;
        let s = scripts.iter().find(|s| s.class == "BellClipContainer").ok_or("level1 has no BellClipContainer")?;
        self.bell_clips = bell_clips(&s.fields)?;
        self.bell_clip_refs = bell_clip_refs(db, &mut self.audio.clip_cache, s.game_object.file, &s.fields)?;
        let s = scripts.iter().find(|s| s.class == "DistantSoundBehaviour").ok_or("level1 has no DistantSoundBehaviour")?;
        self.distant_sound_awake(db, s.game_object.file, &s.fields)?;
        self.load_piano_keys(db, &scripts)
    }

    /// The UiPianoKeyBehaviours' Source and BellClip.Clip, in scene order (`menu::piano_keys`); keys naming the same
    /// AudioSource share it.
    fn load_piano_keys(&mut self, db: &AssetDb, scripts: &[SceneScript]) -> Result<(), String> {
        if self.piano_key_audio.len() == super::menu::piano_keys().len() {
            return Ok(());
        }
        let mut sources: std::collections::HashMap<ObjRef, Option<i32>> = Default::default();
        let mut keys = Vec::new();
        for s in scripts.iter().filter(|s| s.class == "UiPianoKeyBehaviour") {
            let file = s.game_object.file;
            let r = s.fields.get("Source").and_then(Value::pptr).and_then(|p| db.resolve(file, p));
            let source = match r {
                Some(r) => match sources.get(&r) {
                    Some(&id) => id,
                    None => {
                        let id = self.scene_audio_source(db, file, &s.fields, "Source")?;
                        sources.insert(r, id);
                        id
                    }
                },
                None => None,
            };
            let clip = s.fields.get("BellClip").and_then(|b| b.get("Clip")).and_then(Value::pptr).and_then(|p| db.resolve(file, p));
            keys.push((source, super::audio::clip_ref(db, &mut self.audio.clip_cache, clip)?));
        }
        if keys.len() != super::menu::piano_keys().len() {
            return Err(format!("level1 has {} UiPianoKeyBehaviours", keys.len()));
        }
        self.piano_key_audio = keys;
        self.ui_sound = Some(ui_sound_start(&mut self.audio, db, &scripts, "level1")?);
        Ok(())
    }

    pub(super) fn scene_audio_source(&mut self, db: &AssetDb, file: usize, v: &Value, name: &str) -> Result<Option<i32>, String> {
        scene_audio_source(&mut self.audio, db, file, v, name)
    }

    /// DistantSoundBehaviour.Play(soundType, point, volume) and its Play* methods: AudioSource.PlayOneShot of a random
    /// clip of the type's list at Clamp01(volume), with DistantSoundEffects on (None plays nothing).
    pub(super) fn distant_sound_play(&mut self, sound_type: i64, volume: f32) {
        if !self.prefs.DistantSoundEffects || !(1..=6).contains(&sound_type) {
            return;
        }
        let i = sound_type as usize - 1;
        let k = self.pick_random(self.distant_sounds[i]);
        if let Some(id) = self.distant_audio_source {
            let clip = k.and_then(|k| self.distant_sound_clips[i][k].clone());
            self.audio.play_one_shot(id, clip.as_ref(), super::mathf::clamp01(volume));
        }
    }

    /// DistantSoundBehaviour's six clip lists (Play picks from them).
    fn distant_sound_awake(&mut self, db: &AssetDb, file: usize, v: &Value) -> Result<(), String> {
        for (i, name) in ["SmallDistantExplosions", "LargeDistantExplosions", "LargerDistantExplosions", "SmallDistantFirearms", "MediumDistantFirearms", "LargeDistantFirearms"].iter().enumerate() {
            let a = v.get(name).and_then(Value::array).ok_or_else(|| format!("DistantSoundBehaviour.{name}"))?;
            self.distant_sounds[i] = a.len();
            self.distant_sound_clips[i] = a.iter().map(|x| super::audio::clip_ref(db, &mut self.audio.clip_cache, x.pptr().and_then(|p| db.resolve(file, p)))).collect::<Result<_, _>>()?;
        }
        Ok(())
    }

    /// The Map asset named `map` and its Prefab (a recording's CurrentMap).
    pub(super) fn map_prefab(db: &mut AssetDb, map: &str) -> Result<(crate::assets::db::Prefab, crate::assets::db::ObjRef), String> {
        for m in db.find_scripts("Map")? {
            let Some((_, mb, v)) = db.mono(m)? else { continue };
            if mb.name == map {
                let prefab = v.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(m.file, p)).ok_or("Map.Prefab")?;
                return Ok((db.prefab(prefab)?, m));
            }
        }
        Err(format!("no map {map:?}"))
    }

    /// MapLoaderBehaviour.Awake -> Load with CurrentMap = the Map asset named `map` (the main menu's choice;
    /// MapLoadOverride is editor-only): WORLD has no children to destroy or deactivate, InstantiateMap
    /// (`load_map`; no InstantiateOverride without mods), and no mod loaders to tell.
    pub fn map_loader_awake(&mut self, db: &mut AssetDb, map: &str) -> Result<(), String> {
        let scripts = scene_scripts(db, "level1")?;
        let loader = scripts.iter().find(|s| s.runs && s.class == "MapLoaderBehaviour").ok_or("level1 has no running MapLoaderBehaviour")?;
        let go = db.game_object(loader.game_object)?;
        let t = go.components.iter().filter_map(|&c| db.resolve(loader.game_object.file, c)).find(|&c| db.class(c) == 4).ok_or("MapLoaderBehaviour without a Transform")?;
        if !Transform::read(db.data(t))?.children.is_empty() {
            unimplemented!("mod-only: MapLoaderBehaviour.Load: Object.Destroy of WORLD's scene children");
        }
        let mut names = Vec::new();
        let mut prefab = None;
        let mut asset = None;
        for m in db.find_scripts("Map")? {
            let Some((_, mb, v)) = db.mono(m)? else { continue };
            if mb.name == map {
                asset = Some(m);
                prefab = Some(v.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(m.file, p)).ok_or("Map.Prefab")?);
            }
            names.push(mb.name);
        }
        // --play-map Uncharted: no Map asset; the map wh.Update switches to (Instantiate(wh.Uncharted), no asset).
        if prefab.is_none() && map == "Uncharted" {
            for m in db.find_scripts("wh")? {
                let Some((_, _, v)) = db.mono(m)? else { continue };
                prefab = v.get("Uncharted").and_then(Value::pptr).and_then(|p| db.resolve(m.file, p));
                break;
            }
        }
        let prefab = prefab.ok_or_else(|| format!("no map {map:?}; the maps are {names:?} (and Uncharted)"))?;
        let prefab = db.prefab(prefab)?;
        // A live map under WORLD (a recording's): destroyed and deactivated as in Load (map_loader_load).
        if let Some(root) = self.map_root {
            self.map_overlays_destroy();
            self.destroy_object(root);
            self.with_component_instance(root, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
        let (inst, _) = self.load_map(db, &prefab)?;
        self.map_root = Some(inst.objects[0].components[0]);
        self.current_map = Some(prefab);
        self.current_map_asset = asset;
        self.unsynced.push(inst);
        Ok(())
    }

    /// SceneManager.LoadScene("Main"): the scene objects' serialized state and the Awake/Start effects the
    /// simulation reads. `game` is the game folder (UserPreferenceManager's working directory).
    /// Global.MetalScrapePrefab: what its Instantiate reaches the simulation with, (instance ids, Random draws).
    fn metal_scrape_prefab(db: &mut AssetDb, s: &SceneScript) -> Result<(usize, usize, Vec<(usize, Option<usize>)>), String> {
        let scrape = s.fields.get("MetalScrapePrefab").and_then(Value::pptr).and_then(|p| db.resolve(s.game_object.file, p)).ok_or("Global.MetalScrapePrefab")?;
        let prefab = db.prefab(scrape)?;
        // Only ActivateIfFancyEffects has a message (Optout is a marker): its Awake draws Random.value
        // on each active object, and a childless object's SetActive skips no later Awake.
        let mut draws = 0;
        for (o, obj) in prefab.objects.iter().enumerate() {
            let mut active = obj.active;
            let mut up = obj.parent;
            while let Some(q) = up {
                active &= prefab.objects[q].active;
                up = prefab.objects[q].parent;
            }
            for (_, c) in &obj.components {
                let crate::assets::db::Component::Script { class, .. } = c else { continue };
                match class.as_str() {
                    "Optout" => {}
                    "ActivateIfFancyEffects" if !active => {}
                    "ActivateIfFancyEffects" if obj.children.is_empty() => draws += 1,
                    _ => unimplemented!("mod-only: a {class} on Global.MetalScrapePrefab ({o}: {})", obj.name),
                }
            }
        }
        let objects: Vec<(usize, Option<usize>)> = prefab.objects.iter().map(|o| (1 + o.components.len(), o.parent)).collect();
        Ok((objects.iter().map(|o| o.0).sum(), draws, objects))
    }

    /// fnweap: level1's SimpleLightningEmitterBehaviour.PoolableLightningPrefab.
    pub(super) fn simple_lightning_prefab(&mut self) -> ObjRef {
        if let Some(p) = self.simple_lightning_prefab {
            return p;
        }
        let game = crate::game_assets::find_game(None).expect("the game's data");
        let db = self.assets.get_or_insert_with(|| Box::new(AssetDb::open(&game).expect("the game's data")));
        let s = scene_scripts(db, "level1").and_then(|v| v.into_iter().find(|s| s.class == "SimpleLightningEmitterBehaviour").ok_or("level1: no SimpleLightningEmitterBehaviour".into())).expect("level1's SimpleLightningEmitterBehaviour");
        let p = s.fields.get("PoolableLightningPrefab").and_then(Value::pptr).and_then(|p| db.resolve(s.game_object.file, p)).expect("SimpleLightningEmitterBehaviour.PoolableLightningPrefab");
        self.simple_lightning_prefab = Some(p);
        p
    }

    /// The Main scene's singletons as their Awake leaves the static Instance (ClipboardControllerBehaviour: Main):
    /// `Instance = this` on the level1 object whose GameObject is active (PianoDialogBox sets it in Start, which also
    /// needs the behaviour enabled; ThermalVisionBehaviour's Awake returns first on a GPU without RG16, which the rewrite
    /// takes as supported). The value is the reference as the recorder writes it, `@<GameObject name>#<instance id>`,
    /// the id being level1's base id plus twice the behaviour's file index; `anchor` is one level1 object's (class,
    /// instance id) that fixes the base (the snapshot's WORLD PlayAudioAtRandom).
    /// BackgroundItemLoader is level0's (the Menu scene, DontDestroyOnLoad): its Awake's `Instance = this` the same way
    /// from level0's base id, which `l0` (level0's SteamworksInitialiser's instance id, BigProbeB's l0=) fixes; None leaves it unset.
    pub(crate) fn scene_singletons_awake(&mut self, anchor: (&str, i32), l0: Option<i32>) -> Result<(), String> {
        let game = crate::game_assets::find_game(None)?;
        let db = self.assets.get_or_insert_with(|| Box::new(AssetDb::open(&game).expect("the game's data")));
        let scripts = scene_scripts(db, "level1")?;
        let mut a = scripts.iter().filter(|s| s.class == anchor.0);
        let at = match (a.next(), a.next()) { (Some(s), None) => s.at, _ => return Err(format!("level1: not one {}", anchor.0)) };
        let base = anchor.1 - 2 * at as i32;
        for &class in SINGLETONS.iter().filter(|c| **c != "BackgroundItemLoader") {
            let start = class == "PianoDialogBox";
            let mut v = "null".to_string();
            for s in scripts.iter().filter(|s| s.class == class && if start { s.runs } else { s.active }) {
                let name = db.game_object(s.game_object)?.name;
                v = format!("@{}#{}", name.replace(' ', "_"), base + 2 * s.at as i32);
            }
            self.statics.insert(class.into(), v);
        }
        if let Some(id) = l0 {
            let scripts = scene_scripts(db, "level0")?;
            let one = |class: &str| {
                let mut a = scripts.iter().filter(|s| s.class == class);
                match (a.next(), a.next()) { (Some(s), None) => Ok(s), _ => Err(format!("level0: not one {class}")) }
            };
            let base = id - 2 * one("SteamworksInitialiser")?.at as i32;
            let s = one("BackgroundItemLoader")?;
            let v = if s.active { format!("@{}#{}", db.game_object(s.game_object)?.name.replace(' ', "_"), base + 2 * s.at as i32) } else { "null".into() };
            self.statics.insert("BackgroundItemLoader".into(), v);
        }
        Ok(())
    }

    /// A scene singleton's static get_Instance / get_Main.
    pub fn singleton_instance(&self, class: &str) -> String {
        self.statics.get(class).cloned().unwrap_or_else(|| "null".into())
    }

    /// A scene singleton's static set_Instance / set_Main (the reference, `null` for none).
    pub fn set_singleton_instance(&mut self, class: &str, v: &str) {
        self.statics.insert(class.into(), v.into());
    }

    /// level1's BulletTracerPool, for a game not loaded from the scene (a recording's replay that fires tracers).
    pub(super) fn load_tracer_pool(&mut self) {
        let game = crate::game_assets::find_game(None).expect("the game's data");
        let db = self.assets.get_or_insert_with(|| Box::new(AssetDb::open(&game).expect("the game's data")));
        let s = scene_scripts(db, "level1").and_then(|v| v.into_iter().find(|s| s.class == "BulletTracerPool").ok_or("level1: no BulletTracerPool".into())).expect("level1's BulletTracerPool");
        // A replay's recorded counts and timer (from_snapshot) stay.
        let (created, available, timer) = (self.tracer_pool.created, self.tracer_pool.available, self.tracer_pool.inactivityTimer);
        self.tracer_pool = tracer_pool(db, &s).expect("level1's BulletTracerPool");
        (self.tracer_pool.created, self.tracer_pool.available, self.tracer_pool.inactivityTimer) = (created, available, timer);
    }

    /// Global.MetalScrapePrefab of level1's Global, for a game not loaded from the scene (a recording's replay).
    pub(super) fn load_metal_scrape(&mut self) -> (usize, usize, Vec<(usize, Option<usize>)>) {
        let game = crate::game_assets::find_game(None).expect("the game's data");
        let db = self.assets.get_or_insert_with(|| Box::new(AssetDb::open(&game).expect("the game's data")));
        let global = scene_scripts(db, "level1").and_then(|v| v.into_iter().find(|s| s.class == "Global").ok_or("level1: no Global".into())).expect("level1's Global");
        let m = Self::metal_scrape_prefab(db, &global).expect("Global.MetalScrapePrefab");
        self.metal_scrape = Some(m.clone());
        m
    }

    /// ProceduralShatterMaterialPool.ShatterMasks (`s` the scene's pool; the Textures' red channel is all GetPolygon reads).
    fn load_shatter_masks(&mut self, db: &mut AssetDb, s: &SceneScript) -> Result<(), String> {
        for t in s.fields.get("ShatterMasks").and_then(Value::array).ok_or("ProceduralShatterMaterialPool.ShatterMasks")? {
            let r = t.pptr().and_then(|p| db.resolve(s.game_object.file, p)).ok_or("ProceduralShatterMaterialPool.ShatterMasks: a null texture")?;
            let tex = crate::assets::components::Texture2D::read(db.data(r))?;
            let rgba = tex.rgba(&db.data_dir)?;
            // GetPixels(): byte / 255f, rows from the bottom.
            let red = rgba.chunks(4).map(|c| c[0] as f32 / 255.0).collect();
            self.shatter.shatter_masks.push(super::shatter::Texture { instance_id: r.path_id as i32, width: tex.width as usize, height: tex.height as usize, red });
        }
        Ok(())
    }

    /// A snapshot-loaded Game: the Main scene's ProceduralShatterMaterialPool masks.
    pub(crate) fn load_main_shatter_masks(&mut self, db: &mut AssetDb) -> Result<(), String> {
        for s in scene_scripts(db, "level1")? {
            if s.class == "ProceduralShatterMaterialPool" {
                self.load_shatter_masks(db, &s)?;
            }
        }
        Ok(())
    }

    /// gapC r7: the Main scene's ThermalVisionTempDisplayBehaviour alone, into a game built from a snapshot (the fn-check's).
    /// ThermalVisionBehaviour.Awake: Instance, ThermalVisionEnabled as serialised; its material and texture (rendering).
    fn thermal_vision_load(&mut self, db: &mut AssetDb, s: &SceneScript) -> Result<(), String> {
        let v = &s.fields;
        self.thermal_vision = Some(v.get("ThermalVisionEnabled").and_then(Value::bool).ok_or("ThermalVisionBehaviour.ThermalVisionEnabled")?);
        let f = |name: &str| v.get(name).and_then(Value::f32).ok_or_else(|| format!("ThermalVisionBehaviour.{name}"));
        let to_read = v.get("ToRead").and_then(|m| m.get("m_Bits")).and_then(Value::i64).ok_or("ThermalVisionBehaviour.ToRead")? as u32;
        let trs = super::thermal_display::trs_of(db, s.game_object)?;
        // Awake: materialInstance = Instantiate(SpriteRenderer.sharedMaterial); SetupTexture sets its _HeatData.
        let sr = v.get("SpriteRenderer").and_then(Value::pptr).and_then(|p| db.resolve(s.game_object.file, p)).ok_or("ThermalVisionBehaviour.SpriteRenderer")?;
        let r = crate::assets::components::SpriteRenderer::read(db.data(sr))?;
        let material = r.materials.first().and_then(|m| db.resolve(sr.file, *m)).ok_or("ThermalVisionBehaviour's sharedMaterial")?;
        let mut t = super::explosion::ThermalTexture::new(f("MinTemp")?, f("MaxTemp")?, to_read, [trs.t[0], trs.t[1]], [trs.s[0], trs.s[1]]);
        t.material = Some(material);
        self.thermal_texture = Some(t);
        Ok(())
    }

    /// gapC r8: for a recording (which has no Main scene): every Main scene singleton the snapshot game lacks, from level1 as
    /// `load_main` makes it: ThermalVisionBehaviour (Instance, its texture), the thermal display and its container, Global's
    /// CameraAudioSource / HolleManPrefab / metal scrape. The ones a recording already holds (grid, pools, ambience, shrapnel,
    /// camera) are left as loaded. Returns the classes it loaded.
    pub(crate) fn load_main_singletons(&mut self, db: &mut AssetDb) -> Result<Vec<&'static str>, String> {
        let mut done = Vec::new();
        for s in scene_scripts(db, "level1")? {
            match s.class.as_str() {
                // gapA r9: the scene components the rewrite keeps under another id (load_main's).
                // The Main scene ran its Start (the listener) before any recorded case.
                "ExistInThermalVision" => {
                    self.scene_alias.insert(super::spawn::level1_index_id(s.at), super::thermal_display::CONTAINER);
                    if s.runs && !self.thermal_views.contains(&super::thermal_display::CONTAINER) {
                        self.thermal_views.push(super::thermal_display::CONTAINER);
                    }
                }
                // gapC r16: DistantSoundBehaviour.AudioSource (Play's PlayOneShot), as load_main sets it.
                "DistantSoundBehaviour" if self.distant_audio_source.is_none() => {
                    self.distant_audio_source = self.scene_audio_source(db, s.game_object.file, &s.fields, "AudioSource")?;
                    done.push("DistantSoundBehaviour.AudioSource");
                }
                "ThermalVisionBehaviour" if self.thermal_vision.is_none() => { self.thermal_vision_load(db, &s)?; done.push("ThermalVisionBehaviour") }
                "ThermalVisionTempDisplayBehaviour" if self.thermal_display.is_none() => { self.thermal_display_load(db, s.game_object, &s.fields)?; done.push("ThermalVisionTempDisplayBehaviour") }
                "Global" => {
                    if self.camera_audio_source.is_none() && let Some(a) = self.scene_audio_source(db, s.game_object.file, &s.fields, "CameraAudioSource")? { self.camera_audio_source = Some(a); done.push("Global.CameraAudioSource") }
                    if self.holle_man_prefab.is_none() { self.holle_man_prefab = s.fields.get("HolleManPrefab").and_then(Value::pptr).and_then(|p| db.resolve(s.game_object.file, p)); done.push("Global.HolleManPrefab") }
                    if self.metal_scrape.is_none() { self.metal_scrape = Some(Self::metal_scrape_prefab(db, &s)?); done.push("Global.metal_scrape") }
                }
                _ => {}
            }
        }
        Ok(done)
    }

    pub fn load_main(db: &mut AssetDb, game: &Path) -> Result<Game, String> {
        let mut g = Game::default();
        // Global.Awake: ModResourceCache.Clean (ModdingApi.Draw is the new Game's).
        super::mods::mod_resource_cache::Clean();
        // EggBehaviour's static initialiser: canEnter = true (Awake sets it again, but only on an active keypad).
        g.egg_can_enter = super::acts::CAN_ENTER;
        // UserPreferenceLoader (level0) ran UserPreferenceManager.Load.
        g.prefs = Prefs::load(game);
        g.load_time_manager(db)?;
        g.apply_physics2d_settings(&Self::load_physics2d_settings(db)?);
        let mut found = [false; 4];
        // The game boots level0 (the menu) first: a ModGlobals there ran its Awake before the Main scene (alreadyRan keeps the first).
        let menu_globals: Vec<_> = scene_scripts(db, "level0")?.into_iter().filter(|s| s.class == "ModGlobals").collect();
        for s in menu_globals.into_iter().chain(scene_scripts(db, "level1")?) {
            let v = &s.fields;
            match s.class.as_str() {
                "Global" => {
                    found[0] = true;
                    let f = v.get("DynamicSqrdVelocityThreshold").and_then(Value::f32).ok_or("Global.DynamicSqrdVelocityThreshold")?;
                    g.DynamicSqrdVelocityThreshold.0 = f;
                    g.metal_scrape = Some(Self::metal_scrape_prefab(db, &s)?);
                    g.holle_man_prefab = v.get("HolleManPrefab").and_then(Value::pptr).and_then(|p| db.resolve(s.game_object.file, p));
                    g.bullet_crack_clips = v.get("BulletCrackClips").and_then(Value::array).ok_or("Global.BulletCrackClips")?.len();
                    g.audio.clip_fields.insert((0, "BulletCrackClips".into()), v.get("BulletCrackClips").and_then(Value::array).unwrap().iter().map(|x| super::audio::clip_ref(db, &mut g.audio.clip_cache, x.pptr().and_then(|p| db.resolve(s.game_object.file, p)))).collect::<Result<_, _>>()?);
                    for name in ["WoodStutSnapClips", "WoodStrutStressClips"] {
                        let clips = v.get(name).and_then(Value::array).ok_or_else(|| format!("Global.{name}"))?.iter().map(|x| super::audio::clip_ref(db, &mut g.audio.clip_cache, x.pptr().and_then(|p| db.resolve(s.game_object.file, p)))).collect::<Result<_, _>>()?;
                        g.audio.clip_fields.insert((0, name.into()), clips);
                    }
                    g.camera_audio_source = g.scene_audio_source(db, s.game_object.file, v, "CameraAudioSource")?;
                }
                "ModGlobals" if s.runs => {
                    let file = s.game_object.file;
                    // Sprites-Default is one of Unity's builtin materials (unity_builtin_extra).
                    let pptrs: Vec<_> = v.get("Materials").and_then(Value::array).map(|a| a.iter().map(|x| x.pptr()).collect()).unwrap_or_default();
                    let materials: Vec<_> = pptrs.into_iter().map(|p| p.and_then(|p| db.resolve_builtin(file, p)).and_then(|r| Some((crate::assets::serialized::Reader::new(db.data(r)).string().ok()?, r)))).collect();
                    let refs = |name: &str| v.get(name).and_then(Value::array).map(|a| a.iter().map(|x| x.pptr().and_then(|p| db.resolve(file, p))).collect::<Vec<_>>()).unwrap_or_default();
                    let effects: Vec<_> = refs("ParticleEffects").into_iter().map(|r| r.and_then(|r| Some((db.game_object(r).ok()?.name, r)))).collect();
                    let catalog = v.get("Catalog").and_then(Value::pptr).and_then(|p| db.resolve(file, p));
                    super::mods::mod_globals::Awake(catalog, &materials, &effects);
                }
                // VideoSettingsInitBehaviour.Start: Sync.
                "VideoSettingsInitBehaviour" if s.runs => {
                    found[1] = true;
                    (g.positionIterations, g.velocityIterations) = (g.prefs.PhysicsIterations, g.prefs.PhysicsIterations);
                }
                // Global.Awake: camera = Camera.main, the MainCamera-tagged camera this one shakes.
                "CameraShakeBehaviour" => {
                    let go = db.game_object(s.game_object)?;
                    if go.tag != 5 {
                        unimplemented!("mod-only: CameraShakeBehaviour off the MainCamera");
                    }
                    let camera = go.components.iter().filter_map(|&p| db.resolve(s.game_object.file, p)).find(|&r| db.class(r) == 20).ok_or("the main camera's Camera")?;
                    let camera = crate::assets::components::Camera::read(db.data(camera)).map_err(|e| format!("Camera: {e}"))?;
                    if !camera.orthographic {
                        unimplemented!("mod-only: a perspective main camera");
                    }
                    g.camera.orthographic_size = camera.orthographic_size;
                    // The AudioListener's height over the world (z): the camera's and its ancestors' local z.
                    let mut t = go.components.iter().filter_map(|&p| db.resolve(s.game_object.file, p)).find(|&r| db.class(r) == 4).ok_or("the main camera's Transform")?;
                    let mut z = 0.0;
                    loop {
                        let tr = Transform::read(db.data(t))?;
                        if tr.local_rotation != [0.0, 0.0, 0.0, 1.0] {
                            unimplemented!("mod-only: a scene whose main camera starts rotated (Game::camera.rotation starts as the identity)");
                        }
                        z += tr.local_position[2];
                        if tr.father.is_null() {
                            break;
                        }
                        t = db.resolve(t.file, tr.father).ok_or("the camera's parent")?;
                    }
                    g.camera.z = z;
                    crate::play::CAMERA_Z.store(z.to_bits(), std::sync::atomic::Ordering::Relaxed);
                }
                // CameraListenerBehaviour.Update's parameters.
                "CameraListenerBehaviour" => {
                    let f = |n: &str| v.get(n).and_then(Value::f32).ok_or(format!("CameraListenerBehaviour.{n}"));
                    let c = |n: &str| super::link::curve(v.get(n).ok_or(format!("CameraListenerBehaviour.{n}"))?);
                    // Awake: reverbFilter = GetComponent<AudioReverbFilter>() (AddToManager creates its DSP when enabled).
                    let go = db.game_object(s.game_object)?;
                    if let Some(r) = go.components.iter().filter_map(|&p| db.resolve(s.game_object.file, p)).find(|&r| db.class(r) == 164) {
                        let f = crate::assets::components::AudioReverbFilter::read(db.data(r)).map_err(|e| format!("AudioReverbFilter: {e}"))?;
                        // The shipped Main Camera's filter is disabled (no DSP, so no reverb is heard); its properties are still set.
                        // AwakeFromLoad's Update runs ChangeProperties.
                        let mut p = f.params;
                        crate::sfx_reverb::change_properties(&mut p, f.preset);
                        g.camera.reverb = Some(p);
                        g.camera.reverb_preset = f.preset;
                        g.camera.reverb_enabled = f.enabled;
                    }
                    g.camera.listener = Some(super::ballistics::CameraListener { orthosizeMin: f("orthosizeMin")?, orthosizeMax: f("orthosizeMax")?, masterVolumeCurve: c("masterVolumeCurve")?, dryVolumeCurve: c("dryVolumeCurve")?, muted: false });
                }
                // Awake only sets Instance.
                "BulletTracerPool" => g.tracer_pool = tracer_pool(db, &s)?,
                // Awake makes its BallisticsEmitter and sets Instance.
                "GlobalShrapnelEmitter" if s.runs => {
                    let cartridge = v.get("Cartridge").and_then(Value::pptr).and_then(|p| db.resolve(s.game_object.file, p)).ok_or("GlobalShrapnelEmitter.Cartridge")?;
                    let (_, _, cv) = db.mono(cartridge)?.ok_or("GlobalShrapnelEmitter.Cartridge is not a MonoBehaviour")?;
                    let mask = |name: &str| v.get(name).and_then(|m| m.get("m_Bits")).and_then(Value::i64).map(|b| b as u32).ok_or_else(|| format!("GlobalShrapnelEmitter.{name}"));
                    g.shrapnel = Some(super::ballistics::global_shrapnel_awake(super::ballistics::Cartridge::read(&cv)?, mask("LayersToHit")?, mask("WaterLayer")?));
                }
                // Global.main.BellClips: no messages.
                "BellClipContainer" => {
                    g.bell_clips = bell_clips(v)?;
                    g.bell_clip_refs = bell_clip_refs(db, &mut g.audio.clip_cache, s.game_object.file, v)?;
                }
                // Awake only sets Instance; Play draws from these.
                "DistantSoundBehaviour" => {
                    g.distant_sound_awake(db, s.game_object.file, v)?;
                    g.distant_audio_source = g.scene_audio_source(db, s.game_object.file, v, "AudioSource")?;
                }
                // Awake sets Instance; Get draws from ShatterMasks (the Textures' red channel is all GetPolygon reads).
                "ProceduralShatterMaterialPool" => g.load_shatter_masks(db, &s)?,
                // ThermalVisionBehaviour.Awake: sets Instance (ThermalVisionEnabled as serialised); its material and texture are rendering.
                // The "Temperature display container" and its display (thermal_display.rs). Both carry an
                // ExistInThermalVision that sets the same activeSelf; the container's Start comes first and the
                // display's waits for it, so one listener stands for both.
                "ThermalVisionTempDisplayBehaviour" => g.thermal_display_load(db, s.game_object, v)?,
                "ExistInThermalVision" if s.runs => {
                    g.scene_alias.insert(super::spawn::level1_index_id(s.at), super::thermal_display::CONTAINER);
                    let p = super::misc::PendingStart::ExistInThermalVision(super::thermal_display::CONTAINER);
                    if !g.pending_starts.contains(&p) {
                        g.pending_starts.push(p);
                    }
                }
                "ThermalVisionBehaviour" => g.thermal_vision_load(db, &s)?,
                // Awake only sets Instance.
                "AmbientTemperatureGridBehaviour" => {
                    found[2] = true;
                    let mut grid = AmbientTemperatureGridBehaviour::default();
                    for (name, slot) in [("ObjectToAirTransferRate", &mut grid.ObjectToAirTransferRate), ("AirToObjectTransferRate", &mut grid.AirToObjectTransferRate), ("AmbientTemperatureTransferRate", &mut grid.AmbientTemperatureTransferRate)] {
                        *slot = v.get(name).and_then(Value::f32).ok_or_else(|| format!("AmbientTemperatureGridBehaviour.{name}"))?;
                    }
                    grid.GridSize = v.get("GridSize").and_then(Value::i64).and_then(|x| i32::try_from(x).ok()).ok_or("AmbientTemperatureGridBehaviour.GridSize")?;
                    g.ambient_grid = super::temperature::SceneGrid(grid);
                }
                // WORLD's ambience emitter; MapConfig.Reinitialise sets PositionCenter and Radius when a map loads.
                "GearManagerBehaviour" => g.gear_manager_awake(),
                "PlayAudioAtRandom" => {
                    found[3] = true;
                    let mut a = PlayAudioAtRandom::default();
                    super::spawn::load_values(a.fields(), v)?;
                    a.clips = v.get("Clips").and_then(Value::array).map(<[Value]>::len);
                    a.source = g.scene_audio_source(db, s.game_object.file, v, "AudioSource")?;
                    g.ambient_audio_clips.push(v.get("Clips").and_then(Value::array).map_or(Ok(Vec::new()), |c| c.iter().map(|x| super::audio::clip_ref(db, &mut g.audio.clip_cache, x.pptr().and_then(|p| db.resolve(s.game_object.file, p)))).collect::<Result<Vec<_>, _>>())?);
                    g.ambient_audio.push(a);
                }
                _ => {}
            }
        }
        if found != [true; 4] {
            return Err(format!("level1 lacks a scene script the simulation reads ({found:?})"));
        }
        Ok(g)
    }
}

/// `scene`'s running UISoundBehaviour, then its Start (Main = this).
fn ui_sound_start(audio: &mut super::audio::Audio, db: &AssetDb, scripts: &[SceneScript], scene: &str) -> Result<super::audio::UiSound, String> {
    let s = scripts.iter().find(|s| s.runs && s.class == "UISoundBehaviour").ok_or_else(|| format!("{scene} has no running UISoundBehaviour"))?;
    let file = s.game_object.file;
    let source = scene_audio_source(audio, db, file, &s.fields, "AudioSource")?.ok_or("UISoundBehaviour.AudioSource is null")?;
    let mut clips: [Option<super::audio::ClipRef>; 6] = Default::default();
    for (k, name) in ["DefaultClip", "WarningClip", "ErrorClip", "ScrollClip", "UpClip", "DownClip"].iter().enumerate() {
        clips[k] = super::audio::clip_ref(db, &mut audio.clip_cache, s.fields.get(name).and_then(Value::pptr).and_then(|p| db.resolve(file, p)))?;
    }
    let mut main = None;
    ui_sound_behaviour_start(audio, &mut main, super::audio::UiSound { source, clips, timeOfLastScrollClip: 0.0 })?;
    Ok(main.expect("Main = this"))
}

/// UISoundBehaviour.Start: Main = this; Refresh() (RefreshButtons: the Blip listeners, ui/mod.rs); then
/// AudioSource.bypassListenerEffects = true.
pub(super) fn ui_sound_behaviour_start(audio: &mut super::audio::Audio, main: &mut Option<super::audio::UiSound>, this: super::audio::UiSound) -> Result<(), String> {
    let source = this.source;
    *main = Some(this);
    audio.sources.get_mut(&source).ok_or("UISoundBehaviour.AudioSource")?.bypass_listener_effects = true;
    Ok(())
}

/// The Menu scene (level0) as far as its UISoundBehaviour goes: the scene's AudioSources it plays on, Main after its
/// Start, and the AudioListener's world position. No Game runs there; `ui::MenuAudio` holds a fresh copy per load.
pub fn menu_ui_sound(db: &mut AssetDb) -> Result<(super::audio::Audio, super::audio::UiSound, [f32; 3]), String> {
    let scripts = scene_scripts(db, "level0")?;
    let mut audio = super::audio::Audio::default();
    let u = ui_sound_start(&mut audio, db, &scripts, "level0")?;
    let file = db.file_index("level0").ok_or("no level0")?;
    let listeners: Vec<i64> = db.files[file].objects.iter().filter(|o| o.class_id == 81).map(|o| o.path_id).collect();
    let [path_id] = listeners[..] else { return Err(format!("level0 has {} AudioListeners", listeners.len())) };
    // An AudioListener starts with Component's m_GameObject.
    let go = crate::assets::classes::PPtr::read(&mut Reader::new(db.data(ObjRef { file, path_id })))?;
    let listener = super::audio::root_position(&scene_chain(db, db.resolve(file, go))?);
    Ok((audio, u, listener))
}

/// A scene object's Transform and its ancestors', the object's first.
fn scene_chain(db: &AssetDb, mut go: Option<ObjRef>) -> Result<Vec<crate::unity::transform::Trs>, String> {
    let mut chain = Vec::new();
    while let Some(g) = go {
        let o = db.game_object(g)?;
        let t = o.components.iter().filter_map(|c| db.resolve(g.file, *c)).find(|&c| matches!(db.class(c), 4 | 224)).ok_or("a scene GameObject without a Transform")?;
        let t = crate::assets::classes::Transform::read(db.data(t))?;
        let (p, q, sc) = (t.local_position, t.local_rotation, t.local_scale);
        chain.push(crate::unity::transform::Trs { t: [p[0], p[1], p[2], 0.0], q, s: [sc[0], sc[1], sc[2], 0.0] });
        go = match db.resolve(g.file, t.father) {
            Some(f) => db.resolve(f.file, crate::assets::classes::Transform::read(db.data(f))?.game_object),
            None => None,
        };
    }
    Ok(chain)
}

/// A scene AudioSource a script field names (the camera's and DistantSoundBehaviour's; both 2D), under its own id
/// from i32::MIN up (no instance id of the rewrite's).
fn scene_audio_source(audio: &mut super::audio::Audio, db: &AssetDb, file: usize, v: &Value, name: &str) -> Result<Option<i32>, String> {
    let Some(r) = v.get(name).and_then(Value::pptr).and_then(|p| db.resolve(file, p)) else { return Ok(None) };
    let s = crate::assets::components::AudioSource::read(db.data(r))?;
    let clip = super::audio::clip_ref(db, &mut audio.clip_cache, db.resolve(r.file, s.clip))?;
    let group = match db.resolve(r.file, s.output) {
        Some(g) => Some(Reader::new(db.data(g)).string()?),
        None => None,
    };
    // Its Transform and the ancestors' (scene objects do not move; PlayAudioAtRandom moves its own, `chain[0]`).
    let chain = scene_chain(db, db.resolve(r.file, s.game_object))?;
    let source = super::audio::AudioSource::from_component(&s, clip, group.as_deref(), super::BareAnchor::Root, chain, Vec::new(), true);
    let id = match audio.sources.range(..i32::MIN / 2).next_back().map(|(k, _)| k) {
        Some(&k) => k + 1,
        None => i32::MIN,
    };
    audio.add(id, source);
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Menu scene's UI Sound Controller: a 2D source in User interface, the blip clip, and Blip plays it once.
    #[test]
    fn menu_ui_sound_blips() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let (mut audio, mut u, _) = menu_ui_sound(&mut db).unwrap();
        let s = audio.get(u.source);
        assert_eq!((s.spatial_blend(), s.group, s.play_on_awake), (0.0, Some(super::super::audio::MixerGroup::UserInterface), false));
        assert_eq!(u.clips[0].as_ref().unwrap().clip.name, "blip");
        u.play(&mut audio, 1.0, super::super::audio::UiSoundKind::Blip);
        assert_eq!(audio.get(u.source).one_shots.len(), 1);
        u.play(&mut audio, 1.0, super::super::audio::UiSoundKind::Slider);
        assert_eq!(u.timeOfLastScrollClip, 1.0);
    }

    /// The b2 globals the code starts with (before load_main) are what the data and UpdateBox2D give, and the
    /// recorder's `physics` header (the game's own Physics2D readings) says the same.
    #[test]
    fn physics2d_settings_match_data_and_recording() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = AssetDb::open(&game).unwrap();
        let s = Game::load_physics2d_settings(&db).unwrap();
        assert_eq!(crate::b2::settings::globals(), s.globals());
        let dump = super::super::dump::read_recording(&Path::new(env!("CARGO_MANIFEST_DIR")).join("reference-probe/game/dump22drag.txt"));
        let Ok(dump) = dump else { return };
        let header = dump.lines().find(|l| l.contains("velThreshold=")).unwrap();
        // Each reading is `value|bits`; gravity's two components are separated by a space.
        let get = |k: &str| -> f32 { f32::from_bits(u32::from_str_radix(header.split(' ').find_map(|kv| kv.strip_prefix(&format!("{k}="))).unwrap().split('|').nth(1).unwrap(), 16).unwrap()) };
        // The recorded gravity is MapConfig's (Gravity * Vector2.up: x is -0), the data's default is +0.
        assert_eq!((s.gravity.x, s.gravity.y.to_bits()), (get("gravity"), 0xc11cf5c3));
        let got = [s.velocity_threshold, s.max_linear_correction, s.max_angular_correction, s.max_translation_speed, s.max_rotation_speed, s.baumgarte_scale, s.baumgarte_toi_scale, s.time_to_sleep, s.linear_sleep_tolerance, s.angular_sleep_tolerance, s.default_contact_offset];
        let want = ["velThreshold", "maxLin", "maxAng", "maxTrans", "maxRot", "baum", "baumToi", "sleepTime", "linSleep", "angSleep", "contactOffset"].map(get);
        assert_eq!(got, want);
    }

    /// The scene state against a recording made with the same config.json: the fields the map leaves alone.
    #[test]
    fn main_scene_matches_recording() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let g = Game::load_main(&mut db, &game).unwrap();
        let dump = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference-probe/game/dump31spawnrand.txt");
        let Ok(mut snaps) = super::super::dump::snapshots(&dump) else { return };
        let mut first = snaps.next().unwrap();
        // The map's and pool's splats are not loaded from snapshots.
        first.records.retain(|r| r.ty != "FancyBloodSplatController" && r.ty != "BleedingParticleBehaviour");
        let Ok(rig) = crate::game_assets::HumanRig::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("local-cache"), &game) else { return };
        let want = Game::from_snapshot(&first, &rig).unwrap();
        assert_eq!(g.DynamicSqrdVelocityThreshold, want.DynamicSqrdVelocityThreshold);
        assert_eq!((g.time.fixedDeltaTime, g.time.maximumDeltaTime, g.time.timeScale), (want.time.fixedDeltaTime, want.time.maximumDeltaTime, want.time.timeScale));
        assert_eq!((g.velocityIterations, g.positionIterations), (want.velocityIterations, want.positionIterations));
        let (a, b) = (&g.ambient_audio[0], &want.ambient_audio[0]);
        assert_eq!((a.ChooseRandomClip, a.PlaceInRandomPositionInCircle, a.OnlyOnEdge, a.clips), (b.ChooseRandomClip, b.PlaceInRandomPositionInCircle, b.OnlyOnEdge, b.clips));
        let d = crate::game::temperature::SceneGrid::default();
        let x = &g.ambient_grid;
        assert_eq!((x.ObjectToAirTransferRate, x.AirToObjectTransferRate, x.AmbientTemperatureTransferRate, x.GridSize), (d.ObjectToAirTransferRate, d.AirToObjectTransferRate, d.AmbientTemperatureTransferRate, d.GridSize));
    }

    /// ToolControllerBehaviour.Start's state against the first snapshot of a recording that dumps the tool.
    #[test]
    fn scene_tool_matches_recording() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let dump = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference-probe/game/dump22drag.txt");
        let Ok(mut snaps) = super::super::dump::snapshots(&dump) else { return };
        let first = snaps.next().unwrap();
        let t = Game::scene_tool(&mut db, 0).unwrap();
        let (r, d) = (first.of("ToolControllerBehaviour").next().unwrap(), first.of("DragTool").next().unwrap());
        assert_eq!(format!("LayerMask(m_Mask={})", t.SelectionLayer), r.req("SelectionLayer").unwrap());
        assert_eq!((t.isCurrentlyHolding, t.isSelecting), (r.req("isCurrentlyHolding").unwrap() == "1", r.req("isSelecting").unwrap() == "1"));
        assert_eq!((t.isActive, format!("{:08x}", t.rotationSpeed.to_bits())), (d.req("isActive").unwrap() == "1", d.req("rotationSpeed").unwrap().to_string()));
        assert_eq!(d.req("dragJoint").unwrap(), "null");
    }
}

