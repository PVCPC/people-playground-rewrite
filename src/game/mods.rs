//! The mod system as 1.28 ships it. ModLoadStep registers one loader, Modding.Lua.LuaModLoader, which is an
//! EmptyModLoader (it loads nothing), and one source, a ModFolderSource on the game's Mods folder. That source's
//! PollingDirectoryWatcher hashes every mod.json under the folder every 30 s and, on a change, asks for another load.
//! No mod is ever loaded, so ModLoaderRegistry.Mods stays empty.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::contraption::PollingDirectoryWatcher;
use std::time::Duration;

/// ModFolderSource
#[derive(Debug, Default)]
pub struct ModFolderSource {
    pub Folder: PathBuf,
    pub(crate) needsUpdate: Arc<AtomicBool>,
    pub(crate) watcher: PollingDirectoryWatcher,
    /// ModFolderSource.get_OnChange: `new UnityEvent()`.
    pub OnChange: UnityEvent,
}

/// A UnityEvent with no arguments made in code (`new UnityEvent()`): no persistent calls, only runtime listeners.
#[derive(Default)]
pub struct UnityEvent {
    listeners: Vec<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for UnityEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "UnityEvent({} listeners)", self.listeners.len())
    }
}

impl UnityEvent {
    /// UnityEvent.AddListener
    pub fn AddListener(&mut self, f: Arc<dyn Fn() + Send + Sync>) {
        self.listeners.push(f);
    }
    /// UnityEventBase.GetPersistentEventCount: the serialized calls, none for one made in code.
    pub fn GetPersistentEventCount(&self) -> i32 {
        0
    }
    /// UnityEvent.Invoke: each listener in the order added.
    pub fn Invoke(&self) {
        for f in &self.listeners {
            f();
        }
    }
}

impl ModFolderSource {
    /// ModFolderSource..ctor: creates the folder when missing (a failure throws), then a PollingDirectoryWatcher on its
    /// mod.json files every 30 s whose OnChange sets needsUpdate, started.
    pub fn new(source: PathBuf) -> std::io::Result<Self> {
        if !source.is_dir() {
            std::fs::create_dir_all(&source)?;
        }
        let needs = Arc::new(AtomicBool::new(false));
        let mut watcher = PollingDirectoryWatcher::new(&source, "mod.json");
        watcher.Interval = Duration::from_secs(30);
        let n = needs.clone();
        watcher.add_OnChange(Arc::new(move || n.store(true, Ordering::SeqCst)));
        watcher.Start();
        Ok(ModFolderSource { Folder: source, needsUpdate: needs, watcher, OnChange: UnityEvent::default() })
    }

    /// ModFolderSource.Initialise: empty.
    pub fn Initialise(&mut self) {}

    /// ModFolderSource.OnUpdate: when needsUpdate, clears it and invokes OnChange (true then: the game's one listener is
    /// ModLoaderRegistry.AddSource's, which invokes OnModDiscovery, and its caller does that on true).
    pub fn OnUpdate(&mut self) -> bool {
        let fire = self.needsUpdate.swap(false, Ordering::SeqCst);
        if fire {
            self.OnChange.Invoke();
        }
        fire
    }

    /// ModFolderSource.GetFolders: the folder's subfolders (EnumerateDirectories order) that hold a mod.json.
    pub fn GetFolders(&self) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(&self.Folder) else { return Vec::new() };
        rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).filter(|d| has_file(d, "mod.json")).collect()
    }

    /// ModFolderSource.Dispose: the watcher's Dispose.
    pub fn Dispose(&mut self) {
        self.watcher.Stop();
    }
}

/// `dir.EnumerateFiles(name, TopDirectoryOnly).FirstOrDefault() != null`.
fn has_file(dir: &Path, name: &str) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.file_type().is_ok_and(|t| t.is_file()) && e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
}

/// Where a mod came from (IModSource): the only kinds the game has.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ModSourceKind {
    #[default]
    Folder,
    /// ModSteamSource, with the UGC id TryGetUgcIdentity finds for the mod (the rewrite never talks to Steam, so it never makes one).
    Steam(Option<u64>),
}

/// LuaModInfo, the IModInfo of a Lua mod.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LuaModInfo {
    pub Name: Option<String>,
    pub Identifier: Option<String>,
    pub Authors: Option<Vec<String>>,
    pub Summary: Option<String>,
    pub Version: Option<String>,
    pub GameVersion: Option<String>,
    pub Tags: Option<Vec<String>>,
    pub Dependencies: Option<Vec<String>>,
    pub LoadBefore: Option<Vec<String>>,
    pub LoadAfter: Option<Vec<String>>,
    pub Modules: Option<Vec<(Option<String>, Option<String>)>>,
}

impl LuaModInfo {
    /// LuaModInfo..ctor (LuaModInfo.Name, LuaModInfo.Identifier, LuaModInfo.Authors, LuaModInfo.Summary, LuaModInfo.Version, LuaModInfo.GameVersion, LuaModInfo.Tags, LuaModInfo.Dependencies, LuaModInfo.LoadBefore, LuaModInfo.LoadAfter, LuaModInfo.Modules are plain fields here)
    /// Authors and Tags start empty, the rest null.
    pub fn new() -> Self {
        LuaModInfo { Authors: Some(Vec::new()), Tags: Some(Vec::new()), ..Self::default() }
    }
}

/// Mod
#[derive(Default, Clone, Debug)]
pub struct Mod {
    pub Info: LuaModInfo,
    pub Source: ModSourceKind,
    pub Errors: Vec<String>,
    pub Status: Option<String>,
    pub File: PathBuf,
}

impl Mod {
    /// Mod..ctor: Errors empty. Its loader is always the one LuaModLoader.
    pub fn new(info: LuaModInfo, source: ModSourceKind, file: PathBuf) -> Self {
        Mod { Info: info, Source: source, Errors: Vec::new(), Status: None, File: file }
    }

    /// Mod.IsEnabled: Loader.IsEnabled(this).
    pub fn IsEnabled(&self, loader: &LuaModLoader) -> bool {
        loader.IsEnabled(self)
    }

    /// Mod.HasErrors: Errors.Any().
    pub fn HasErrors(&self) -> bool {
        !self.Errors.is_empty()
    }

    /// ModExtensions.GetWorkshopId: a ModSteamSource's UGC id for it, as text.
    pub fn GetWorkshopId(&self) -> Option<String> {
        match self.Source {
            ModSourceKind::Steam(Some(id)) => Some(id.to_string()),
            _ => None,
        }
    }

    /// ModExtensions.IsFromWorkshop: its source is a ModSteamSource.
    pub fn IsFromWorkshop(&self) -> bool {
        matches!(self.Source, ModSourceKind::Steam(_))
    }
}

/// Modding.Lua.LuaModLoader: an EmptyModLoader with nothing added (the Lua loader has not shipped).
#[derive(Default)]
pub struct LuaModLoader;

impl LuaModLoader {
    /// EmptyModLoader.GetMods: none.
    pub fn GetMods(&self) -> Vec<Mod> {
        Vec::new()
    }
    /// EmptyModLoader.TryLoadAsync: a finished false.
    pub fn TryLoadAsync(&self, _directory: &Path) -> bool {
        false
    }
    /// EmptyModLoader.HasModWithId: false.
    pub fn HasModWithId(&self, _id: &str) -> bool {
        false
    }
    /// EmptyModLoader.TryGetModById: false, mod null.
    pub fn TryGetModById(&self, _id: &str) -> Option<Mod> {
        None
    }
    /// EmptyModLoader.GetThumbnail: null.
    pub fn GetThumbnail(&self, _m: &Mod) -> Option<()> {
        None
    }
    /// EmptyModLoader.Reload, EmptyModLoader.Unload, EmptyModLoader.Enable, EmptyModLoader.Disable: empty.
    pub fn Reload(&mut self, _m: &Mod) {}
    // EmptyModLoader.Unload: empty.
    pub fn Unload(&mut self, _m: &Mod) {}
    // EmptyModLoader.Enable: empty.
    pub fn Enable(&mut self, _m: &Mod) {}
    // EmptyModLoader.Disable: empty.
    pub fn Disable(&mut self, _m: &Mod) {}
    /// EmptyModLoader.IsEnabled: false.
    pub fn IsEnabled(&self, _m: &Mod) -> bool {
        false
    }
    /// EmptyModLoader.OnLoadMap: empty (MapLoaderBehaviour.Load calls it on every loader).
    pub fn OnLoadMap(&mut self) {}
    /// EmptyModLoader.OnCreateModEntryUi: empty.
    pub fn OnCreateModEntryUi(&mut self, _m: &Mod) {}
    /// IModLoaderExtensions.Toggle: Disable when enabled, else Enable.
    pub fn Toggle(&mut self, m: &Mod) {
        if self.IsEnabled(m) {
            self.Disable(m);
        } else {
            self.Enable(m);
        }
    }
}

impl super::background_loader::ModLoader for LuaModLoader {
    /// EmptyModLoader.OnUpdate: empty.
    fn on_update(&mut self) {}
    /// EmptyModLoader.Dispose: empty.
    fn unload(&mut self) {}
}

/// ModLoaderRegistry.Mods over its one loader: LuaModLoader's (none).
pub fn registry_mods() -> Vec<Mod> {
    LuaModLoader.GetMods()
}

/// ModLoaderRegistry.TryGetModById: false for a blank id, else the first loader that has it (the LuaModLoader has none).
pub fn registry_try_get_mod_by_id(identifier: Option<&str>) -> Option<Mod> {
    let id = identifier.filter(|s| !s.trim().is_empty())?;
    LuaModLoader.TryGetModById(id)
}

/// ModInfoUtilities.TryGetModIdFromPath: File.ReadAllText (which throws past the caller), then Json.NET's
/// DeserializeAnonymousType into `{ Identifier = "" }`: every property named Identifier in any case, the last winning,
/// read as a string (jsonnet). Anything Json.NET refuses, and a JSON null or empty text (the NullReferenceException at
/// `.Identifier`), gives null; true when the id is not blank.
pub fn TryGetModIdFromPath(path_to_json: &Path) -> std::io::Result<(bool, Option<String>)> {
    use super::jsonnet::{self, JTok};
    let text = jsonnet::read_all_text(path_to_json)?;
    let id = match jsonnet::parse(&text) {
        Ok(Some(root @ JTok::Object(_))) => root.read("Identifier", JTok::as_string).ok().flatten().flatten(),
        _ => None,
    };
    Ok((id.as_deref().is_some_and(|s| !s.trim().is_empty()), id))
}

/// ModFunctionAttribute (marks a method mods may call by `Name`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModFunctionAttribute {
    pub Name: Option<String>,
}

impl ModFunctionAttribute {
    /// ModFunctionAttribute..ctor, ModFunctionAttribute.Name
    pub fn new(name: Option<&str>) -> Self {
        ModFunctionAttribute { Name: name.map(Into::into) }
    }
}

/// ModValueAttribute (marks a value mods may read or write).
#[derive(Clone, Debug, PartialEq)]
pub struct ModValueAttribute {
    pub Name: Option<String>,
    pub Read: bool,
    pub Write: bool,
}

/// ModValueAttribute..ctor's defaults: `(name = null, read = true, write = true)`.
impl Default for ModValueAttribute {
    fn default() -> Self {
        Self::new(None, true, true)
    }
}

impl ModValueAttribute {
    /// ModValueAttribute..ctor, ModValueAttribute.Name, ModValueAttribute.Read, ModValueAttribute.Write
    pub fn new(name: Option<&str>, read: bool, write: bool) -> Self {
        ModValueAttribute { Name: name.map(Into::into), Read: read, Write: write }
    }
}

/// ModCompilationResult
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModCompilationResult {
    pub Success: bool,
    pub Errors: Option<String>,
    pub Suspicious: bool,
}

impl ModCompilationResult {
    /// ModCompilationResult..ctor
    pub fn new(success: bool, errors: Option<&str>, suspicious: bool) -> Self {
        ModCompilationResult { Success: success, Errors: errors.map(Into::into), Suspicious: suspicious }
    }
}

/// SteamTags.Tags
pub const STEAM_TAGS: [&str; 8] = ["Machine", "Vehicle", "Building", "Destructible", "Realistic", "Fun", "Electronics", "Utility"];

/// DoNotReuploadSign.Magic: the UTF-8 of "🐍🇩🇴_🇳🇴🇹_🇷🇪🇺🇵🇱🇴🇦🇩💀".
pub fn do_not_reupload_magic() -> Vec<u8> {
    "\u{1F40D}\u{1F1E9}\u{1F1F4}_\u{1F1F3}\u{1F1F4}\u{1F1F9}_\u{1F1F7}\u{1F1EA}\u{1F1FA}\u{1F1F5}\u{1F1F1}\u{1F1F4}\u{1F1E6}\u{1F1E9}\u{1F480}".as_bytes().to_vec()
}

/// DoNotReuploadSign.Mark: Magic appended to the file (FileMode.Append makes a missing one; BinaryWriter.Write(byte[]) writes
/// the bytes alone). Nothing in 1.28 calls it.
pub fn do_not_reupload_mark(path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new().append(true).create(true).open(path)?.write_all(&do_not_reupload_magic())
}

/// DoNotReuploadSign.HasMark: the file ends with Magic (opening or seeking failing throws).
pub fn do_not_reupload_has_mark(path: &Path) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let magic = do_not_reupload_magic();
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::End(-(magic.len() as i64)))?;
    let mut buf = vec![0u8; magic.len()];
    let n = f.read(&mut buf)?;
    buf[n..].fill(0);
    Ok(buf == magic)
}

/// ModMetaData: the legacy (C#) mod's mod.json. Its strings and arrays are nullable as the class's are (mod.json can set them null).
#[derive(Clone, Debug, PartialEq)]
pub struct ModMetaData {
    pub Name: Option<String>,
    pub Author: Option<String>,
    pub Description: Option<String>,
    pub RichReadme: Option<String>,
    pub ModVersion: Option<String>,
    pub GameVersion: Option<String>,
    pub ThumbnailPath: Option<String>,
    pub EntryPoint: Option<String>,
    pub Tags: Option<Vec<Option<String>>>,
    pub Scripts: Option<Vec<Option<String>>>,
    pub Active: bool,
    pub MetaLocation: Option<String>,
    pub UGCIdentity: Option<String>,
    pub CreatorUGCIdentity: Option<String>,
    pub AssemblyName: Option<String>,
    pub FromWorkshop: bool,
    pub MetaPath: Option<String>,
    pub HasErrors: bool,
    pub Errors: Option<String>,
    pub ContentHash: u32,
    pub Suspicious: bool,
    pub NeedsUpdateApproval: bool,
}

impl Default for ModMetaData {
    fn default() -> Self {
        Self::new()
    }
}

impl ModMetaData {
    /// ModMetaData..ctor (ModMetaData.Suspicious and ModMetaData.NeedsUpdateApproval are plain fields here): Name "Mod",
    /// Author and Description "", ModVersion "1.0", GameVersion "1.28", ThumbnailPath "thumb.png", EntryPoint "Mod.Mod",
    /// empty Tags and Scripts, Active.
    pub fn new() -> Self {
        let s = |v: &str| Some(v.to_string());
        ModMetaData {
            Name: s("Mod"),
            Author: s(""),
            Description: s(""),
            RichReadme: None,
            ModVersion: s("1.0"),
            // 1.28.45: GameInfo.Version.ToString().
            GameVersion: s(super::contraption::current_version()),
            ThumbnailPath: s("thumb.png"),
            EntryPoint: s("Mod.Mod"),
            Tags: Some(Vec::new()),
            Scripts: Some(Vec::new()),
            Active: true,
            MetaLocation: None,
            UGCIdentity: None,
            CreatorUGCIdentity: None,
            AssemblyName: None,
            FromWorkshop: false,
            MetaPath: None,
            HasErrors: false,
            Errors: None,
            ContentHash: 0,
            Suspicious: false,
            NeedsUpdateApproval: false,
        }
    }

    /// ModMetaData.HasDoNotReuploadSign: Path.GetFullPath(Path.Combine(MetaLocation, ThumbnailPath)) exists and carries the
    /// DoNotReuploadSign mark. Err is the exception's class (a null part: ArgumentNullException; a bad path char: ArgumentException).
    pub fn HasDoNotReuploadSign(&self) -> Result<bool, &'static str> {
        let combined = super::legacy_mods::path_combine(self.MetaLocation.as_deref(), self.ThumbnailPath.as_deref())?;
        let full = super::legacy_mods::get_full_path(&std::env::current_dir().unwrap_or_default(), &combined)?;
        if !Path::new(&full).is_file() {
            return Ok(false);
        }
        do_not_reupload_has_mark(Path::new(&full)).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "FileNotFoundException" } else { "IOException" })
    }

    /// ModMetaData.GetUniqueName: Author-Name-(CreatorUGCIdentity, "local" when null or white space), each String.Normalize'd
    /// (NFC); a null Author or Name throws NullReferenceException.
    pub fn GetUniqueName(&self) -> Result<String, &'static str> {
        use unicode_normalization::UnicodeNormalization;
        let creator = self.CreatorUGCIdentity.as_deref().filter(|s| !s.chars().all(char::is_whitespace)).unwrap_or(crate::il!(str "ModMetaData::GetUniqueName" 0));
        let author = self.Author.as_deref().ok_or("NullReferenceException")?;
        let name = self.Name.as_deref().ok_or("NullReferenceException")?;
        Ok(format!("{}-{}-{}", author.nfc(), name.nfc(), creator.nfc()))
    }

    /// ModMetaData.GetTagsForSteam: the Tags (none for null) that are SteamTags (distinct, in Tags' order), then "Mods".
    pub fn GetTagsForSteam(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in self.Tags.iter().flatten().flatten() {
            if STEAM_TAGS.contains(&t.as_str()) && !out.contains(t) {
                out.push(t.clone());
            }
        }
        out.push("Mods".into());
        out
    }
}

// SteamworksInitialiser.Awake: DontDestroyOnLoad, then SteamClient.RestartAppIfNecessary/Init throw without Steam: IsInitialised stays false.
// SteamworksInitialiser.OnDestroy: SteamClient.Shutdown, nothing to shut down.
// SteamworksInitialiser.Update: SteamClient.RunCallbacks, Steam's own callback pump (no game state; gapC r26: the old anchor was
// the frame's whole script Update, update_scripts, which the fn-check ran as this function).
// SteamTags..cctor: STEAM_TAGS.
/// SteamworksInitialiser.IsInitialised for Steamworks itself (SteamUGC, the workshop): false. SteamworksInitialiser.Awake's
/// SteamClient.Init throws without a Steam client, the catch leaves IsInitialised false (Debug.LogWarning), and the rewrite
/// never contacts Steam. (The stats and achievements read `stats::STEAMWORKS_INITIALISED`, the local store.)
pub const STEAM_UGC: bool = false;

/// ModSteamSource: never made in 1.28 (ModLoadStep adds only the ModFolderSource); without Steam it is inactive.
#[derive(Debug)]
pub struct ModSteamSource {
    active: bool,
    needsUpdate: bool,
}

impl Default for ModSteamSource {
    fn default() -> Self {
        Self {
            active: true,
            needsUpdate: Default::default(),
        }
    }
}

impl ModSteamSource {
    /// ModSteamSource..ctor: inactive when SteamworksInitialiser.IsInitialised is false (no SteamUGC handlers).
    pub fn new() -> Self {
        ModSteamSource { active: STEAM_UGC, needsUpdate: false }
    }
    /// ModSteamSource.Initialise: Task.Run(UpdateCache).
    pub fn Initialise(&mut self) {
        self.UpdateCache();
    }
    // ModSteamSource.MoveNext: UpdateCache's async state machine.
    // ModSteamSource.SetStateMachine: its builder hook.
    /// ModSteamSource.UpdateCache: the first Query.GetPageAsync throws without Steamworks (a faulted task nobody reads),
    /// before the cache or needsUpdate change.
    fn UpdateCache(&mut self) {}
    /// ModSteamSource.GetFolders: none when inactive.
    pub fn GetFolders(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    /// ModSteamSource.OnUpdate: OnChange once per needsUpdate (never set here).
    pub fn OnUpdate(&mut self) -> bool {
        std::mem::take(&mut self.needsUpdate)
    }
    /// ModSteamSource.Dispose: cacheLock.Dispose.
    pub fn Dispose(&mut self) {}
    /// ModSteamSource.TryGetUgcIdentity: ucgIdByModId has nothing (UpdateCache never filled it).
    pub fn TryGetUgcIdentity(&self, _m: &Mod) -> Option<u64> {
        let _ = self.active;
        None
    }
}

/// SteamContraptionLoadStep.SteamContraptionSource: inactive without Steam.
#[derive(Debug)]
pub struct SteamContraptionSource {
    active: bool,
}

impl Default for SteamContraptionSource {
    fn default() -> Self {
        Self {
            active: true,
        }
    }
}

impl SteamContraptionSource {
    /// SteamContraptionSource..ctor: inactive when SteamworksInitialiser.IsInitialised is false.
    pub fn new() -> Self {
        SteamContraptionSource { active: STEAM_UGC }
    }
    // SteamContraptionSource.MoveNext: UpdateCache's async state machine.
    // SteamContraptionSource.SetStateMachine: its builder hook.
    /// SteamContraptionSource.UpdateCache: returns at once when SteamworksInitialiser.IsInitialised is false.
    pub fn UpdateCache(&mut self) {}
    /// SteamContraptionSource.GetContraptions: none when inactive.
    pub fn GetContraptions(&self) -> Vec<super::contraption::ContraptionMetaData> {
        let _ = self.active;
        Vec::new()
    }
}

// The legacy (obsolete) mod list, ModListBehaviour and ModEntryBehaviour: `super::legacy_mods`.

/// Activations.LegacyActivationCache: which of the four activation messages a component's type can take.
pub mod legacy_activation_cache {
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// LegacyActivationCache.CachedResult
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct CachedResult {
        pub Use: bool,
        pub UseContinuous: bool,
        pub IsolatedActivation: bool,
        pub IsolatedContinuousActivation: bool,
    }

    /// The Messages interfaces it asks about (each has one method taking an ActivationPropagation).
    #[derive(Clone, Copy)]
    pub enum Interface {
        IUse,
        IUseContinuous,
        IIsolatedActivation,
        IIsolatedContinuousActivation,
    }

    // LegacyActivationCache..cctor: the type cache and the results list empty.
    static CACHE: Mutex<Option<HashMap<String, CachedResult>>> = Mutex::new(None);

    /// LegacyActivationCache.ProcessObject: GetLegacyActivationMethods of each component, the four flags OR-ed and then
    /// dropped (nothing reads them; only the type cache fills). Nothing in 1.28 calls it.
    pub fn ProcessObject(component_classes: &[&str]) {
        let mut any = CachedResult::default();
        for c in component_classes {
            let r = GetLegacyActivationMethods(c);
            any.Use |= r.Use;
            any.UseContinuous |= r.UseContinuous;
            any.IsolatedActivation |= r.IsolatedActivation;
            any.IsolatedContinuousActivation |= r.IsolatedContinuousActivation;
        }
        let _ = any;
    }

    /// LegacyActivationCache.GetLegacyActivationMethods: the cached result of the component's type, else
    /// CanImplementInterface for each of the four, cached.
    pub fn GetLegacyActivationMethods(class: &str) -> CachedResult {
        let mut cache = CACHE.lock().unwrap();
        let cache = cache.get_or_insert_with(HashMap::new);
        if let Some(r) = cache.get(class) {
            return *r;
        }
        let r = CachedResult {
            Use: CanImplementInterface(class, Interface::IUse),
            UseContinuous: CanImplementInterface(class, Interface::IUseContinuous),
            IsolatedActivation: CanImplementInterface(class, Interface::IIsolatedActivation),
            IsolatedContinuousActivation: CanImplementInterface(class, Interface::IIsolatedContinuousActivation),
        };
        cache.insert(class.to_string(), r);
        r
    }

    /// LegacyActivationCache.CanImplementInterface: the type implements the interface, or has an instance method (any
    /// access, inherited public ones too) of the same name, return type and parameters. A component's runtime type is never
    /// an interface or abstract. Use and UseContinuous are the classes that receive those messages (`tool::USE_RECEIVERS`,
    /// `continuous::USE_CONTINUOUS_RECEIVERS`, subclasses included); only UseWireBehaviour has IsolatedActivation and
    /// IsolatedContinuousActivation (public, and nothing derives from it).
    pub fn CanImplementInterface(class: &str, interface: Interface) -> bool {
        match interface {
            Interface::IUse => super::super::tool::USE_RECEIVERS.contains(&class),
            Interface::IUseContinuous => super::super::continuous::USE_CONTINUOUS_RECEIVERS.contains(&class),
            Interface::IIsolatedActivation | Interface::IIsolatedContinuousActivation => class == "UseWireBehaviour",
        }
    }
}

/// What SteamWorkshopController.PublishContraption leaves for the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum PublishOutcome {
    /// isBusy: nothing.
    Busy,
    /// UISoundBehaviour.Main.Error() and a dialog with one Close button.
    Error(String),
    /// UISoundBehaviour.Main.Warning() and the "Update existing" / "Upload as new" / "Cancel" dialog.
    AskUpdateOrNew,
    /// StartUploadingRoutine started (no existing id).
    Started,
}

/// SteamWorkshopController (the Main scene's; Awake sets SteamWorkshopController.Main). With SteamworksInitialiser.IsInitialised
/// false PublishButtonBehaviour.Publish refuses before the uploader dialog, so nothing reaches PublishContraption and no
/// progress box is ever made.
#[derive(Default)]
pub struct SteamWorkshopController {
    pub currentlyUploading: Option<super::contraption::ContraptionMetaData>,
    pub isBusy: bool,
    /// currentProgressBox: only StartUploadingRoutine past Steam's gate opens one.
    pub progress_box: bool,
    pub currentProgress: f32,
}

impl SteamWorkshopController {
    // SteamWorkshopController.Awake: Main = this.
    /// SteamWorkshopController.PublishContraption: nothing while busy; ConvertToDirectoryBased; a malformed directory is an
    /// error; a contraption without a CreatorUGCIdentity starts uploading, one with it asks.
    pub fn PublishContraption(&mut self, meta: &super::contraption::ContraptionMetaData) -> PublishOutcome {
        if self.isBusy {
            return PublishOutcome::Busy;
        }
        let _ = super::contraption::convert_to_directory_based(&super::contraption::save_directory(), meta);
        if !Self::IsContraptionValid(meta) {
            return PublishOutcome::Error(crate::il!(str "SteamWorkshopController::PublishContraption" 0).into());
        }
        self.currentlyUploading = Some(meta.clone());
        if meta.CreatorUGCIdentity.as_deref().map_or(true, str::is_empty) {
            return match self.StartUploadingRoutine() {
                Some(e) => PublishOutcome::Error(e),
                None => PublishOutcome::Started,
            };
        }
        // SteamWorkshopController.b__6_0: "Update existing", StartUploadingRoutine(ulong.Parse(CreatorUGCIdentity)).
        // SteamWorkshopController.b__6_1: "Upload as new", StartUploadingRoutine().
        PublishOutcome::AskUpdateOrNew
    }

    /// SteamWorkshopController.StartUploadingRoutine: isBusy, then "Contraption does not exist." (an Error and a notification)
    /// without its directory; the rest asks Steam (Utils.IsFamilyShared, Item.GetAsync, Editor.SubmitAsync).
    pub fn StartUploadingRoutine(&mut self) -> Option<String> {
        self.isBusy = true;
        let dir = self.currentlyUploading.as_ref().and_then(|m| m.Directory.clone());
        if !dir.is_some_and(|d| d.is_dir()) {
            self.isBusy = false;
            return Some(crate::il!(str "SteamWorkshopController/<StartUploadingRoutine>d__7::MoveNext" 0).into());
        }
        // SteamWorkshopController.b__3: (<>c__DisplayClass7_1) Task.Run of `item = await Item.GetAsync(existingId)`; b__4 its WaitUntil.
        // SteamWorkshopController.b__0: (<>c__DisplayClass7_0) Task.Run of `publishResult = await editor.SubmitAsync(this)`; b__1 its WaitUntil.
        // SteamWorkshopController.b__2: the "View terms of service" button, OpenURL of the workshop legal agreement.
        // SteamWorkshopController.OpenURL: SteamFriends.OpenWebOverlay or Application.OpenURL, from the agreement dialog.
        // The rewrite never starts Steam. Uninitialised, Utils.IsFamilyShared reads SteamApps.AppOwner, whose Internal
        // (SteamSharedClass.Interface) is null: a NullReferenceException ends the coroutine (Unity logs it, nothing on
        // screen), and isBusy stays true, so every later PublishContraption does nothing.
        None
    }

    /// SteamWorkshopController.IsContraptionValid: the metadata, data and thumbnail files all exist.
    pub fn IsContraptionValid(meta: &super::contraption::ContraptionMetaData) -> bool {
        [&meta.PathToMetadata, &meta.PathToDataFile, &meta.PathToThumbnail].iter().all(|p| p.as_ref().is_some_and(|p| p.is_file()))
    }

    /// SteamWorkshopController.Report: the upload's progress (IProgress<float>, from Editor.SubmitAsync).
    pub fn Report(&mut self, value: f32) {
        self.currentProgress = value;
    }

    /// SteamWorkshopController.Update: with a progress box and a contraption, its title "<Name>\n\n<percent>% uploaded...".
    pub fn Update(&self) -> Option<String> {
        let m = self.currentlyUploading.as_ref().filter(|_| self.progress_box)?;
        Some(format!("{}\n\n{}% uploaded...", m.name(), (self.currentProgress * 100.0).round_ties_even() as i32))
    }
}

/// WorkshopUploaderDialog (the Main scene's, inactive until Show): a toggle per AvailableTags entry; only
/// PublishButtonBehaviour.Publish's "Yes" (past the Steam check) shows it.
#[derive(Default)]
pub struct WorkshopUploaderDialog {
    pub Contraption: Option<super::contraption::ContraptionMetaData>,
    /// The tag toggles Start made: (tag, isOn).
    pub toggles: Vec<(String, bool)>,
    pub active: bool,
}

/// What a WorkshopUploaderDialog action leaves for the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum UploaderOutcome {
    Nothing,
    /// DialogBoxManager.Notification(text), and the dialog hides.
    Notification(String),
    Published(PublishOutcome),
}

impl WorkshopUploaderDialog {
    /// WorkshopUploaderDialog.Start: a TagEntryPrefab per tag, its toggle off (Select on, Deselect off).
    pub fn Start(&mut self, available_tags: &[String]) {
        self.toggles = available_tags.iter().map(|t| (t.clone(), false)).collect();
    }

    /// WorkshopUploaderDialog.AllEntriesDo: the action on each toggle under TagEntryContainer.
    fn AllEntriesDo(&mut self, mut action: impl FnMut(&mut (String, bool))) {
        self.toggles.iter_mut().for_each(|t| action(t));
    }

    /// WorkshopUploaderDialog.Reset: every toggle off (with no contraption set yet; the listeners see none).
    pub fn Reset(&mut self) {
        self.AllEntriesDo(|t| t.1 = false);
    }

    /// WorkshopUploaderDialog.Show: the object on and Global.main.AddUiBlocker().
    pub fn Show(&mut self) {
        self.active = true;
    }

    /// WorkshopUploaderDialog.Hide: the object off and Global.main.RemoveUiBlocker().
    pub fn Hide(&mut self) {
        self.active = false;
    }

    /// WorkshopUploaderDialog.Upload: no contraption is an error notification; else SteamWorkshopController.PublishContraption
    /// (an exception is logged); then Hide.
    pub fn Upload(&mut self, controller: &mut SteamWorkshopController) -> UploaderOutcome {
        let Some(c) = self.Contraption.clone() else {
            self.Hide();
            return UploaderOutcome::Notification(crate::il!(str "WorkshopUploaderDialog::Upload" 0).into());
        };
        let r = controller.PublishContraption(&c);
        self.Hide();
        UploaderOutcome::Published(r)
    }

    /// WorkshopUploaderDialog.Select: the tag added to the contraption's Tags.
    pub fn Select(&mut self, tag: &str) -> UploaderOutcome {
        match self.Contraption.as_mut() {
            Some(c) => {
                c.Tags.push(tag.to_string());
                UploaderOutcome::Nothing
            }
            None => {
                self.Hide();
                UploaderOutcome::Notification(crate::il!(str "WorkshopUploaderDialog::Select" 0).into())
            }
        }
    }

    /// WorkshopUploaderDialog.Deselect: every copy of the tag removed.
    pub fn Deselect(&mut self, tag: &str) -> UploaderOutcome {
        match self.Contraption.as_mut() {
            Some(c) => {
                c.Tags.retain(|t| t != tag);
                UploaderOutcome::Nothing
            }
            None => {
                self.Hide();
                UploaderOutcome::Notification(crate::il!(str "WorkshopUploaderDialog::Deselect" 0).into())
            }
        }
    }

    /// WorkshopUploaderDialog.OnDestroy: each toggle's listeners removed.
    pub fn OnDestroy(&mut self) {
        self.AllEntriesDo(|_| {});
    }
}

/// PublishButtonBehaviour.Publish: without Steam (SteamworksInitialiser.IsInitialised false) an Error and this notification;
/// with it, a Yes/No dialog whose Yes resets the uploader dialog, sets its Contraption and shows it.
pub fn publish_button_publish() -> &'static str {
    debug_assert!(!STEAM_UGC);
    crate::il!(str "PublishButtonBehaviour::Publish" 0)
}

/// The rewrite's Workshop folder (the rewrite's addition, no Steam): `Workshop` next to the exe, where Steam workshop item folders
/// (steamapps/workshop/content/1118200/<id>) are dropped in as-is. Everything in it is read as data only: nothing there
/// is ever run or written.
/// --ui-screens sets WORKSHOP_DIR to the install's steamapps/workshop/content/1118200, the folder the game's SteamContraptionSource lists.
pub static WORKSHOP_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn workshop_directory() -> PathBuf {
    if let Some(d) = WORKSHOP_DIR.get() {
        return d.clone();
    }
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("Workshop"))).unwrap_or_else(|| PathBuf::from("Workshop"))
}

/// Whether `path` is inside the Workshop folder (those files are never deleted or overwritten).
pub fn in_workshop_directory(path: &Path) -> bool {
    let w = workshop_directory();
    let w = std::fs::canonicalize(&w).unwrap_or(w);
    std::fs::canonicalize(path).map_or(false, |p| p.starts_with(&w))
}

/// The Workshop folder's item folders, read once (the folder is created first when missing), the way
/// SteamContraptionSource.UpdateCache keeps a subscribed item: a `<id>` folder with a `*.json` and a `*.jaap` file. An item
/// with C# code (a mod.json, what ModSteamSource keeps, or any .cs/.dll file) is skipped with a log line and never run; so
/// is an item that is no contraption (1.28 has no workshop maps: MapLoadStep registers only its BuiltInMaps).
pub fn workshop_items() -> &'static [(u64, PathBuf)] {
    static ITEMS: std::sync::OnceLock<Vec<(u64, PathBuf)>> = std::sync::OnceLock::new();
    ITEMS.get_or_init(|| workshop_scan(&workshop_directory()))
}

fn workshop_scan(folder: &Path) -> Vec<(u64, PathBuf)> {
    fn has_code(d: &Path) -> bool {
        std::fs::read_dir(d).into_iter().flatten().flatten().any(|e| {
            let p = e.path();
            if p.is_dir() {
                return has_code(&p);
            }
            p.extension().map_or(false, |x| x.eq_ignore_ascii_case("cs") || x.eq_ignore_ascii_case("dll"))
        })
    }
    let has_ext = |d: &Path, ext: &str| std::fs::read_dir(d).into_iter().flatten().flatten().any(|e| e.path().is_file() && e.path().extension().map_or(false, |x| x.eq_ignore_ascii_case(ext)));
    if !folder.exists() {
        let _ = std::fs::create_dir_all(folder);
    }
    let mut out = Vec::new();
    for e in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(id) = name.parse::<u64>() else {
            eprintln!("Workshop: skipped \"{name}\" (not a workshop item id folder)");
            continue;
        };
        if dir.join("mod.json").is_file() || has_code(&dir) {
            eprintln!("Workshop: skipped item {id} (C# mod; its code is never run)");
        } else if has_ext(&dir, "json") && has_ext(&dir, "jaap") {
            // Debug.Log("Loaded contraption " + entry.Title): the id stands for the title, which needs Steam.
            eprintln!("Loaded contraption {id}");
            out.push((id, dir));
        } else {
            eprintln!("Workshop: skipped item {id} (not a contraption; 1.28 has no workshop maps)");
        }
    }
    out
}

/// SteamContraptionSource.GetContraptions as it reads a cached item's folder, over the Workshop folder's items: the first
/// `*.json`'s metadata, the four paths from its file name, Name = the item id, PublishedFileID = the id, CreatorUGCIdentity
/// null. DisplayName is item.Title in the game; with no Steam the item's own metadata DisplayName stands for it, and a blank
/// one is the Name, as there.
pub fn workshop_contraptions() -> Vec<super::contraption::ContraptionMetaData> {
    // --ui-screens with the recording's Steam cache: its items in its order, each from its Workshop folder, titled as Steam had it.
    if let Some(cache) = STEAM_CACHE.get() {
        let items: Vec<(u64, PathBuf)> = cache.iter().map(|(id, _)| (*id, workshop_directory().join(id.to_string()))).collect();
        let mut out = workshop_contraptions_in(&items);
        for m in &mut out {
            m.DisplayName = cache.iter().find(|c| c.0 == m.PublishedFileID).map(|c| c.1.clone()).filter(|t| !t.trim().is_empty()).or(m.Name.clone());
        }
        return out;
    }
    workshop_contraptions_in(workshop_items())
}

/// --ui-screens: SteamContraptionSource's cache in the recording (`uisteam`: the subscribed items' ids and Titles, in the
/// cache's order; empty when Steam was not initialised). Steam's own data, which the rewrite never asks Steam for.
pub static STEAM_CACHE: std::sync::OnceLock<Vec<(u64, String)>> = std::sync::OnceLock::new();

fn workshop_contraptions_in(items: &[(u64, PathBuf)]) -> Vec<super::contraption::ContraptionMetaData> {
    let mut out = Vec::new();
    for (id, dir) in items {
        let Some(json) = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).find(|p| p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case("json"))) else { continue };
        let stem = json.file_stem().unwrap().to_string_lossy().to_string();
        // Debug.LogWarning("Failed to read contraption at ...") on an exception
        let Ok(Some(mut m)) = std::fs::read_to_string(&json).map_err(|e| e.to_string()).and_then(|t| super::contraption::ContraptionMetaData::from_json(&t)) else { continue };
        m.set_paths(dir, &stem);
        m.Name = Some(id.to_string());
        m.PublishedFileID = *id;
        m.CreatorUGCIdentity = None;
        if m.DisplayName.as_deref().map_or(true, |s| s.trim().is_empty()) {
            m.DisplayName = m.Name.clone();
        }
        out.push(m);
    }
    out
}

// ModHasher.Hash: the reads below (a missing file adds nothing).
/// ModHasher.Compute: 371317 XOR the xxHash (seed 0) of every 1024-byte read of each .cs and .dll file under the mod's
/// MetaLocation (the XOR makes the file order unobservable). ModHasher.Hash: a missing file adds nothing.
pub fn mod_hasher_compute(meta_location: &Path) -> std::io::Result<u32> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for e in std::fs::read_dir(d)? {
            let p = e?.path();
            if p.is_dir() { walk(&p, out)? } else { out.push(p) }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(meta_location, &mut files)?;
    let mut hash = 371317u32;
    let mut buffer = [0u8; 1024];
    for f in files {
        let n = f.to_string_lossy().to_lowercase();
        if !(n.ends_with(".cs") || n.ends_with(".dll")) || !f.is_file() {
            continue;
        }
        mod_hasher_hash(&mut hash, &mut buffer, &f)?;
    }
    Ok(hash)
}
/// ModHasher.Hash: XORs into `hash` the xxHash (seed 0) of each read of up to 1024 bytes of the file (none when it is missing).
pub fn mod_hasher_hash(hash: &mut u32, buffer: &mut [u8; 1024], path: &Path) -> std::io::Result<()> {
    use std::io::Read;
    if !path.is_file() {
        return Ok(());
    }
    let mut file = std::fs::File::open(path)?;
    loop {
        let num = file.read(buffer)?;
        if num == 0 {
            return Ok(());
        }
        *hash ^= super::contraption::xxhash(&buffer[..num], 0);
    }
}
// ---------------------------------------------------------------------------------------------------------------------------------
// ModAPI (the obsolete C# API), ModEvent`1's legacy hook, ModDebugDrawer and ModResourceCache.
// ---------------------------------------------------------------------------------------------------------------------------------

/// A ModAPI `EventHandler<T>` event: its subscribers in order (a multicast delegate), each called with (sender, args).
/// The sender and the component arguments are instance ids.
pub struct ModApiEvent<T: 'static>(std::sync::Mutex<Vec<Arc<dyn Fn(i32, &T) + Send + Sync>>>);

impl<T: 'static> ModApiEvent<T> {
    const fn new() -> Self {
        Self(std::sync::Mutex::new(Vec::new()))
    }

    /// ModAPI.add_OnWireCreated, ModAPI.add_OnPinCreated, ModAPI.add_OnLinkCreated, ModAPI.add_OnWireDestroyed, ModAPI.add_OnPinDestroyed, ModAPI.add_OnLinkDestroyed, ModAPI.add_OnItemSpawned, ModAPI.add_OnBeforeItemSpawned, ModAPI.add_OnItemRemoved, ModAPI.add_OnItemSelected, ModAPI.add_OnItemDeselected, ModAPI.add_OnItemActivated, ModAPI.add_OnGunShot, ModAPI.add_OnDeath
    /// (Delegate.Combine: appended; only mod code subscribes).
    pub fn add(&self, handler: Arc<dyn Fn(i32, &T) + Send + Sync>) {
        self.0.lock().unwrap().push(handler);
    }

    /// ModAPI.remove_OnWireCreated, ModAPI.remove_OnPinCreated, ModAPI.remove_OnLinkCreated, ModAPI.remove_OnWireDestroyed, ModAPI.remove_OnPinDestroyed, ModAPI.remove_OnLinkDestroyed, ModAPI.remove_OnItemSpawned, ModAPI.remove_OnBeforeItemSpawned, ModAPI.remove_OnItemRemoved, ModAPI.remove_OnItemSelected, ModAPI.remove_OnItemDeselected, ModAPI.remove_OnItemActivated, ModAPI.remove_OnGunShot, ModAPI.remove_OnDeath
    /// (Delegate.Remove: the last occurrence).
    pub fn remove(&self, handler: &Arc<dyn Fn(i32, &T) + Send + Sync>) {
        let mut v = self.0.lock().unwrap();
        if let Some(i) = v.iter().rposition(|h| Arc::ptr_eq(h, handler)) {
            v.remove(i);
        }
    }

    /// ModAPI.InvokeWireCreated, ModAPI.InvokePinCreated, ModAPI.InvokeLinkCreated, ModAPI.InvokeWireDestroyed, ModAPI.InvokePinDestroyed, ModAPI.InvokeLinkDestroyed, ModAPI.InvokeItemSpawned, ModAPI.InvokeBeforeItemSpawned, ModAPI.InvokeItemRemoved, ModAPI.InvokeItemSelected, ModAPI.InvokeItemDeselected, ModAPI.InvokeItemActivated, ModAPI.InvokeGunShot, ModAPI.InvokeDeath
    /// (`Event?.Invoke(sender, args)`: every subscriber in order, nothing when there is none).
    pub fn invoke(&self, sender: i32, args: &T) {
        let handlers = self.0.lock().unwrap().clone();
        for h in handlers {
            h(sender, args);
        }
    }

    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

/// UserSpawnEventArgs: the spawned object's root id and its SpawnableAsset's name (null for an object without one).
#[derive(Default, Clone, Debug, PartialEq)]
pub struct UserSpawnEventArgs {
    pub Instance: i32,
    pub SpawnableAsset: Option<String>,
}

impl UserSpawnEventArgs {
    /// UserSpawnEventArgs..ctor
    pub fn new(instance: i32, spawnable_asset: Option<String>) -> Self {
        Self { Instance: instance, SpawnableAsset: spawnable_asset }
    }
}

/// ModAPI's events: OnWireCreated (WireBehaviour), OnPinCreated (PinBehaviour), OnLinkCreated (LinkDeviceBehaviour), the
/// three Destroyed ones, OnItemSpawned / OnBeforeItemSpawned / OnItemRemoved (UserSpawnEventArgs), OnItemSelected /
/// OnItemDeselected / OnItemActivated (PhysicalBehaviour), OnGunShot (FirearmBehaviour), OnDeath (PersonBehaviour).
pub mod mod_api {
    use super::{ModApiEvent, UserSpawnEventArgs};
    // ModAPI..cctor: metaDataIsValid false, registeredInputsByMod empty, the indented invariant JsonSerializerSettings (only
    // the JSON functions, mod-only, read them); the events start without subscribers.
    pub static OnWireCreated: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnPinCreated: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnLinkCreated: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnWireDestroyed: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnPinDestroyed: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnLinkDestroyed: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnItemSpawned: ModApiEvent<UserSpawnEventArgs> = ModApiEvent::new();
    pub static OnBeforeItemSpawned: ModApiEvent<UserSpawnEventArgs> = ModApiEvent::new();
    pub static OnItemRemoved: ModApiEvent<UserSpawnEventArgs> = ModApiEvent::new();
    pub static OnItemSelected: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnItemDeselected: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnItemActivated: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnGunShot: ModApiEvent<i32> = ModApiEvent::new();
    pub static OnDeath: ModApiEvent<i32> = ModApiEvent::new();

    /// ModAPI.ClearEvents: every event but OnBeforeItemSpawned loses its subscribers (only ModificationManager, the legacy
    /// path, calls it).
    pub fn ClearEvents() {
        for e in [&OnWireCreated, &OnPinCreated, &OnLinkCreated, &OnWireDestroyed, &OnPinDestroyed, &OnLinkDestroyed, &OnItemSelected, &OnItemDeselected, &OnItemActivated, &OnGunShot, &OnDeath] {
            e.clear();
        }
        OnItemSpawned.clear();
        OnItemRemoved.clear();
    }

    use super::super::legacy_mods::{get_full_path, io_exc, path_combine, path_exc, Exc, ModRef, MODIFICATION_MANAGER};
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
    use std::sync::Mutex;

    /// ModAPI's statics (metaDataIsValid, metadata, registeredInputsByMod: by the metadata's hash code, which the rewrite
    /// takes as the mod's Arc address, as object.GetHashCode tells objects apart) and the flags it sets elsewhere.
    #[derive(Default)]
    pub struct State {
        pub metaDataIsValid: bool,
        pub metadata: Option<ModRef>,
        pub registeredInputsByMod: HashMap<usize, HashSet<String>>,
        /// InputSystem.Modded: (uniqueId, Name, default key); Category Modded, Universe Game.
        pub Modded: Vec<(String, String, i32)>,
        /// ControlSchemeEditorBehaviour.ShouldReinitialise, set by RegisterInput.
        pub ShouldReinitialise: bool,
        /// CatalogBehaviour.ShouldRemoveToRemoveWhenModded (true until KeepExtraObjects).
        pub ShouldRemoveToRemoveWhenModded: bool,
        /// Global.allowedToRegisterExternalLiquids.
        pub allowedToRegisterExternalLiquids: bool,
        /// CatalogBehaviour.Main.ModdedCatalog: the categories RegisterCategory made.
        pub ModdedCatalog: Vec<ModCategory>,
        /// ToolLibrary.ModdedTypes (a HashSet<Type>): the mods' tool type names, each once.
        pub ModdedTypes: Vec<String>,
        /// ToolLibrary.OnCollectionChange.
        pub OnCollectionChange: super::UnityEvent,
    }

    /// A Category ScriptableObject.CreateInstance made (RegisterCategory): its name, Description and Icon; HadPriorName
    /// false and PriorName null as made.
    #[derive(Clone, Debug, Default)]
    pub struct ModCategory {
        pub name: String,
        pub Description: Option<String>,
        pub Icon: Option<Picture>,
        pub HadPriorName: bool,
        pub PriorName: Option<String>,
    }

    /// A Category FindCategory returns: one of ModGlobals.MainCatalog's (the asset) or of the ModdedCatalog (its index).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum CategoryRef {
        Main(crate::assets::db::ObjRef),
        Modded(usize),
    }

    use super::super::contraption::LoadedTexture;
    use super::super::legacy_mods::Picture;

    pub static STATE: std::sync::LazyLock<Mutex<State>> =
        std::sync::LazyLock::new(|| Mutex::new(State { ShouldRemoveToRemoveWhenModded: true, ..Default::default() }));

    /// ModAPI.get_Metadata: InvalidOperationException outside AfterSpawn, Main and OnLoad.
    pub fn get_Metadata() -> Result<Option<ModRef>, Exc> {
        let s = STATE.lock().unwrap();
        if !s.metaDataIsValid {
            return Err(Exc::new("InvalidOperationException", crate::il!(str "ModAPI::get_Metadata" 0)));
        }
        Ok(s.metadata.clone())
    }

    /// ModAPI.set_Metadata
    pub fn set_Metadata(m: Option<ModRef>) {
        STATE.lock().unwrap().metadata = m;
    }

    /// Resources.Load<T>(path): the resource (case-insensitive) when it is a MonoBehaviour of script `class`.
    fn load_script(db: &mut crate::assets::db::AssetDb, path: &str, class: &str) -> Result<Option<crate::assets::db::ObjRef>, String> {
        let Some(r) = db.resource(path, 114)? else { return Ok(None) };
        Ok(db.mono(r)?.filter(|(s, _, _)| s.class_name == class).map(|_| r))
    }

    /// ModAPI.FindCartridge: a copy of Resources.Load<Cartridge>("Calibers/" + name) ("30-06 Springfield" is "30-06"), or null.
    pub fn FindCartridge(db: &mut crate::assets::db::AssetDb, name: &str) -> Result<Option<crate::game::ballistics::Cartridge>, String> {
        let path = format!("Calibers/{}", if name == crate::il!(str "ModAPI::FindCartridge" 0) { "30-06" } else { name });
        let Some(r) = load_script(db, &path, "Cartridge")? else { return Ok(None) };
        let (_, _, v) = db.mono(r)?.ok_or("Cartridge")?;
        crate::game::ballistics::Cartridge::read(&v).map(Some)
    }

    /// ModAPI.FindPhysicalProperties: a copy of Resources.Load<PhysicalProperties>("PhysicalProperties/" + name), or null.
    pub fn FindPhysicalProperties(db: &mut crate::assets::db::AssetDb, name: &str) -> Result<Option<crate::game::physical::PhysicalProperties>, String> {
        let Some(r) = load_script(db, &format!("PhysicalProperties/{name}"), "PhysicalProperties")? else { return Ok(None) };
        crate::game::link::read_physical_properties(db, r).map(Some)
    }

    /// ModAPI.FindMaterial: ModGlobals.LoadedMaterials[name], or null.
    pub fn FindMaterial(name: &str) -> Option<crate::assets::db::ObjRef> {
        super::mod_globals::with(|g| g.LoadedMaterials.get(name).copied())
    }

    /// ModAPI.Register(SpawnableModification): only while the metadata is valid; Modifications.Add(modification, Metadata)
    /// (`modification` is the object's identity; one already in it is Dictionary.Add's ArgumentException); the log line.
    pub fn Register(modification: usize, name_override: Option<&str>) -> Result<String, Exc> {
        let m = {
            let s = STATE.lock().unwrap();
            if !s.metaDataIsValid {
                return Err(Exc::new("InvalidOperationException", crate::il!(str "ModAPI::Register" 0)));
            }
            s.metadata.clone()
        };
        let mut mm = MODIFICATION_MANAGER.lock().unwrap();
        if mm.Modifications.iter().any(|(k, _)| *k == modification) {
            return Err(Exc::new("ArgumentException", "An item with the same key has already been added."));
        }
        mm.Modifications.push((modification, m));
        Ok(format!("{} registered as item modification", name_override.unwrap_or("")))
    }

    /// ModAPI.RegisterInput: only while the metadata is valid; a new id is listed for its mod and added to
    /// InputSystem.Modded (the scheme editor re-reads it), a repeated one only logged as an error. Added, and the log line.
    pub fn RegisterInput(name: &str, unique_id: &str, default_key: i32) -> Result<(bool, String), Exc> {
        let mut s = STATE.lock().unwrap();
        if !s.metaDataIsValid {
            return Err(Exc::new("InvalidOperationException", crate::il!(str "ModAPI::RegisterInput" 0)));
        }
        if s.Modded.iter().any(|(id, _, _)| id == unique_id) {
            return Ok((false, format!("Could not register input for {name}:{unique_id}, it is a duplicate")));
        }
        let key = s.metadata.as_ref().map(|m| std::sync::Arc::as_ptr(m) as usize).ok_or_else(Exc::nre)?;
        s.registeredInputsByMod.entry(key).or_default().insert(unique_id.to_string());
        s.Modded.push((unique_id.to_string(), name.to_string(), default_key));
        s.ShouldReinitialise = true;
        Ok((true, format!("Registered input for {name}:{unique_id}")))
    }

    /// ModAPI.KeepExtraObjects: inside AfterSpawn the RemoveWhenModded objects stay; elsewhere the warning's text.
    pub fn KeepExtraObjects() -> Option<&'static str> {
        let mut s = STATE.lock().unwrap();
        if s.metaDataIsValid {
            s.ShouldRemoveToRemoveWhenModded = false;
            return None;
        }
        Some("You cannot call KeepExtraObjects outside the AfterSpawn delegate.")
    }

    /// ModAPI.RegisterLiquid: Liquid.Register(id, liquid) only after or during map load. `registry` is Liquid.Registry's
    /// keys (ordinal); a taken id is only logged (the LogErrorFormat text returned), a new one added.
    pub fn RegisterLiquid(registry: &mut Vec<String>, id: &str) -> Result<Option<String>, Exc> {
        if STATE.lock().unwrap().allowedToRegisterExternalLiquids {
            if registry.iter().any(|k| k == id) {
                return Ok(Some(format!("A liquid that matches \"{id}\" is already registered.")));
            }
            registry.push(id.to_string());
            return Ok(None);
        }
        Err(Exc::new("InvalidOperationException", crate::il!(str "ModAPI::RegisterLiquid" 0)))
    }

    /// The JSON functions' path: it must end in ".json" and lie inside GetFullPath("Mods") (a string prefix test).
    fn json_path(base: &Path, path: &str) -> Result<String, Exc> {
        if !path.ends_with(".json") {
            return Err(Exc::new("ArgumentException", crate::il!(str "ModAPI::DeleteJSON" 1)));
        }
        let mods = get_full_path(base, "Mods").map_err(path_exc)?;
        let full = get_full_path(base, &path_combine(Some(&mods), Some(path)).map_err(path_exc)?).map_err(path_exc)?;
        if !full.starts_with(&mods) {
            return Err(Exc::new("ArgumentException", format!("given path '{full}' is not inside the game mod directory")));
        }
        Ok(full)
    }

    fn dir_missing(full: &str) -> bool {
        !Path::new(full).parent().is_some_and(Path::is_dir)
    }

    /// ModAPI.DeleteJSON: File.Delete (a missing file is no error, a missing folder is).
    pub fn DeleteJSON(base: &Path, path: &str) -> Result<(), Exc> {
        let full = json_path(base, path)?;
        match std::fs::remove_file(&full) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !dir_missing(&full) => Ok(()),
            r => r.map_err(|e| io_exc(&e, dir_missing(&full))),
        }
    }

    /// ModAPI.SerialiseJSON with the object's JSON text already made (JsonConvert of a mod's own object, a type only a C#
    /// mod has): a null object throws, the text is written as UTF-8 without a BOM.
    pub fn SerialiseJSON(base: &Path, json: Option<&str>, path: &str) -> Result<(), Exc> {
        if !path.ends_with(".json") {
            return Err(Exc::new("ArgumentException", crate::il!(str "ModAPI::SerialiseJSON" 1)));
        }
        let json = json.ok_or_else(|| Exc::new("ArgumentNullException", "Value cannot be null.\r\nParameter name: obj cannot be null"))?;
        let full = json_path(base, path)?;
        std::fs::write(&full, json).map_err(|e| io_exc(&e, dir_missing(&full)))
    }

    /// ModAPI.DeserialiseJSON up to JsonConvert: the file's text (File.ReadAllText; the mod's T reads it).
    pub fn DeserialiseJSON(base: &Path, path: &str) -> Result<String, Exc> {
        let full = json_path(base, path)?;
        let b = std::fs::read(&full).map_err(|e| io_exc(&e, dir_missing(&full)))?;
        Ok(String::from_utf8_lossy(b.strip_prefix(&[0xef, 0xbb, 0xbf][..]).unwrap_or(&b)).into_owned())
    }

    /// ModAPI.FindSpawnable: find(name) (CatalogBehaviour.Main alive, `catalog_main`: its GetSpawnable, by name then by prior
    /// name; else the first of Resources.LoadAll<SpawnableAsset>("SpawnableAssets") of that name), then, inside AfterSpawn,
    /// Main or OnLoad, the ToSpawnInstead of its first MigrationEvent applicable to Metadata.GameVersion. A null GameVersion
    /// there throws NullReferenceException (LegacyGameVersionUtils.GetDifference splits it). No mod adds a spawnable.
    pub fn FindSpawnable(db: &mut crate::assets::db::AssetDb, name: &str, catalog_main: bool) -> Result<Option<crate::assets::db::ObjRef>, Exc> {
        use crate::assets::mono::Value;
        let io = |e: String| Exc::new("Exception", e);
        let assets = db.resources_in("SpawnableAssets", 114).map_err(io)?;
        let mut by_prior = None;
        let mut found = None;
        for &r in &assets {
            let (_, mb, v) = db.mono(r).map_err(io)?.ok_or_else(Exc::nre)?;
            if mb.name == name {
                found = Some(r);
                break;
            }
            if by_prior.is_none() && v.get("HasPriorName").and_then(Value::bool) == Some(true) && matches!(v.get("PriorName"), Some(Value::Str(s)) if s == name) {
                by_prior = Some(r);
            }
        }
        let Some(asset) = found.or(if catalog_main { by_prior } else { None }) else { return Ok(None) };
        if !STATE.lock().unwrap().metaDataIsValid {
            return Ok(Some(asset));
        }
        let (_, _, v) = db.mono(asset).map_err(io)?.ok_or_else(Exc::nre)?;
        let events = v.get("MigrationEvents").and_then(Value::array).map(<[Value]>::to_vec).unwrap_or_default();
        for e in events {
            // MigrationEvent.IsApplicable: false without a ToSpawnInstead.
            let Some(target) = e.get("ToSpawnInstead").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let Some(Value::Str(version)) = e.get("Version") else { return Err(Exc::nre()) };
            use super::super::contraption::MigrationCondition::*;
            let condition = [OlderThan, OlderThanOrEqualTo, EqualTo, NewerThan, NewerThanOrEqualTo][e.get("VersionCondition").and_then(Value::i64).unwrap_or(0) as usize];
            let event = super::super::contraption::MigrationEvent { Version: version.clone(), VersionCondition: condition };
            let game_version = get_Metadata()?.ok_or_else(Exc::nre)?.lock().unwrap().GameVersion.clone();
            if event.IsApplicable(game_version.as_deref().ok_or_else(Exc::nre)?, true) {
                return Ok(Some(target));
            }
        }
        Ok(Some(asset))
    }

    /// ModAPI.FindCategory: the first of ModGlobals.MainCatalog.Categories (then, when CatalogBehaviour.Main is alive,
    /// `catalog_main`, its ModdedCatalog) named `name` or, with HadPriorName, whose PriorName is `name`; None for none. A null
    /// MainCatalog or a null category reached throws NullReferenceException.
    pub fn FindCategory(db: &mut crate::assets::db::AssetDb, name: &str, catalog_main: bool) -> Result<Option<CategoryRef>, Exc> {
        use crate::assets::mono::Value;
        let catalog = super::mod_globals::with(|g| g.MainCatalog).ok_or_else(Exc::nre)?;
        let io = |e: String| Exc::new("Exception", e);
        let (_, _, root) = db.mono(catalog).map_err(io)?.ok_or_else(Exc::nre)?;
        let list = root.get("Categories").and_then(Value::array).map(<[Value]>::to_vec).unwrap_or_default();
        for c in list {
            let r = c.pptr().and_then(|p| db.resolve(catalog.file, p)).ok_or_else(Exc::nre)?;
            let (_, mb, v) = db.mono(r).map_err(io)?.ok_or_else(Exc::nre)?;
            let prior = match v.get("PriorName") {
                Some(Value::Str(s)) => Some(s.clone()),
                _ => None,
            };
            if mb.name == name || (v.get("HadPriorName").and_then(Value::bool) == Some(true) && prior.as_deref() == Some(name)) {
                return Ok(Some(CategoryRef::Main(r)));
            }
        }
        if catalog_main {
            let s = STATE.lock().unwrap();
            if let Some(i) = s.ModdedCatalog.iter().position(|c| c.name == name || (c.HadPriorName && c.PriorName.as_deref() == Some(name))) {
                return Ok(Some(CategoryRef::Modded(i)));
            }
        }
        Ok(None)
    }

    /// ModAPI.RegisterCategory: with CatalogBehaviour.Main alive (`catalog_main`) and no modded category of that name, a new
    /// Category (name, description, icon) added to its ModdedCatalog. Else the LogError text.
    pub fn RegisterCategory(name: &str, description: Option<&str>, icon: Option<Picture>, catalog_main: bool) -> Option<&'static str> {
        if !catalog_main {
            return Some(crate::il!(str "ModAPI::RegisterCategory" 0));
        }
        let mut s = STATE.lock().unwrap();
        if s.ModdedCatalog.iter().any(|c| c.name == name) {
            return Some(crate::il!(str "ModAPI::RegisterCategory" 1));
        }
        s.ModdedCatalog.push(ModCategory { name: name.to_string(), Description: description.map(str::to_string), Icon: icon, ..Default::default() });
        None
    }

    /// The mod file a path-dependent function reads: InvalidOperationException (`message`) outside AfterSpawn, Main and
    /// OnLoad, then Path.Combine(Metadata.MetaLocation, path) (a null Metadata throws NullReferenceException) and the mod's
    /// Name.
    fn mod_file(message: &'static str, path: &str) -> Result<(String, Option<String>), Exc> {
        if !STATE.lock().unwrap().metaDataIsValid {
            return Err(Exc::new("InvalidOperationException", message));
        }
        let m = get_Metadata()?.ok_or_else(Exc::nre)?;
        let m = m.lock().unwrap();
        Ok((path_combine(m.MetaLocation.as_deref(), Some(path)).map_err(path_exc)?, m.Name.clone()))
    }

    /// Debug.LogErrorFormat("{0}: Failed to load ... at \"{1}\"", Metadata.Name, path).
    fn failed(format: &str, name: Option<&str>, path: &str) -> String {
        format.replace("{0}", name.unwrap_or("")).replace("{1}", path)
    }

    /// ModAPI.LoadSprite(path, scale = 1, pixelated = true): the ModResourceCache's Sprite for Path.Combine(MetaLocation,
    /// path), else (a missing file is logged and null) Utils.LoadSprite with Point (pixelated) or Bilinear filtering and
    /// scale * 35 pixels per unit, cached. A null sprite (an unreadable file) makes Cache throw NullReferenceException.
    /// Ok holds the sprite and the LogError text.
    pub fn LoadSprite(path: &str, scale: f32, pixelated: bool) -> Result<(Option<Picture>, Option<String>), Exc> {
        let (full, name) = mod_file(crate::il!(str "ModAPI::LoadSprite" 0), path)?;
        if let Some(s) = super::mod_resource_cache::TryGet::<Picture>(&full) {
            return Ok((Some(s), None));
        }
        if !Path::new(&full).is_file() {
            return Ok((None, Some(failed(crate::il!(str "ModAPI::LoadSprite" 1), name.as_deref(), path))));
        }
        let sprite: Picture = std::sync::Arc::new(super::super::contraption::load_sprite(Path::new(&full), pixelated, scale * 35.0).ok_or_else(Exc::nre)?);
        super::mod_resource_cache::Cache(&full, sprite.clone()).map_err(|e| Exc::new("ArgumentException", e))?;
        Ok((Some(sprite), None))
    }

    /// ModAPI.LoadTexture(path, pixelated = true): as LoadSprite, the Texture2D itself (Utils.LoadTexture), cached as one.
    pub fn LoadTexture(path: &str, pixelated: bool) -> Result<(Option<std::sync::Arc<LoadedTexture>>, Option<String>), Exc> {
        let (full, name) = mod_file(crate::il!(str "ModAPI::LoadTexture" 0), path)?;
        if let Some(t) = super::mod_resource_cache::TryGet::<std::sync::Arc<LoadedTexture>>(&full) {
            return Ok((Some(t), None));
        }
        if !Path::new(&full).is_file() {
            return Ok((None, Some(failed(crate::il!(str "ModAPI::LoadTexture" 1), name.as_deref(), path))));
        }
        let texture = std::sync::Arc::new(super::super::contraption::load_texture(Path::new(&full), pixelated).ok_or_else(Exc::nre)?);
        super::mod_resource_cache::Cache(&full, texture.clone()).map_err(|e| Exc::new("ArgumentException", e))?;
        Ok((Some(texture), None))
    }

    /// The asset keys of the clips LoadSound makes (negative: the jukebox's are 0 up).
    static SOUND_KEYS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);

    /// ModAPI.LoadSound(path): the ModResourceCache's AudioClip for Path.Combine(MetaLocation, path), else
    /// Utils.FileToAudioClip (a missing file throws its IO exception; a file the reader refuses, Exception with the
    /// reader's text: unchecked against the game's message), cached.
    pub fn LoadSound(path: &str) -> Result<crate::game::audio::ClipRef, Exc> {
        let (full, _) = mod_file(crate::il!(str "ModAPI::LoadSound" 0), path)?;
        if let Some(c) = super::mod_resource_cache::TryGet::<crate::game::audio::ClipRef>(&full) {
            return Ok(c);
        }
        std::fs::metadata(&full).map_err(|e| io_exc(&e, dir_missing(&full)))?;
        let key = SOUND_KEYS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        let clip = crate::game::audio::file_to_audio_clip(Path::new(&full), key).map_err(|e| Exc::new("Exception", e))?;
        super::mod_resource_cache::Cache(&full, clip.clone()).map_err(|e| Exc::new("ArgumentException", e))?;
        Ok(clip)
    }

    /// ModAPI.RegisterCustomTool<T>: with ToolLibrary.Instance alive (`alive`), `new Tool(name, description, icon,
    /// typeof(T).Name, parent)` added to `coll`, T to ModdedTypes and BroadcastCollectionChange (OnCollectionChange's
    /// listeners, called with the state unlocked); else the LogError text.
    fn RegisterCustomTool(name: &str, description: &str, icon: Option<Picture>, type_name: &str, parent: Option<&str>, coll: &mut Vec<crate::game::tool::ToolEntry>, alive: bool) -> Option<&'static str> {
        if !alive {
            return Some(crate::il!(str "ModAPI::RegisterCustomTool" 0));
        }
        coll.push(crate::game::tool::ToolEntry { name: name.to_string(), ty: type_name.to_string(), parent: parent.unwrap_or("").to_string(), description: description.to_string(), icon });
        let event = {
            let mut s = STATE.lock().unwrap();
            if !s.ModdedTypes.iter().any(|t| t == type_name) {
                s.ModdedTypes.push(type_name.to_string());
            }
            std::mem::take(&mut s.OnCollectionChange)
        };
        event.Invoke();
        let mut s = STATE.lock().unwrap();
        let added = std::mem::replace(&mut s.OnCollectionChange, event);
        s.OnCollectionChange.listeners.extend(added.listeners);
        None
    }

    /// ModAPI.RegisterTool<T>(name, description, icon, parent = null): RegisterCustomTool into ToolLibrary.Instance.Tools (a
    /// null Instance throws NullReferenceException first; `library` is its Tools and whether it is alive), then the log
    /// line "<T> registered as tool". Ok holds the LogError text (if any) and the log line.
    pub fn RegisterTool(name: &str, description: &str, icon: Option<Picture>, type_name: &str, parent: Option<&str>, library: Option<(&mut Vec<crate::game::tool::ToolEntry>, bool)>) -> Result<(Option<&'static str>, String), Exc> {
        let (coll, alive) = library.ok_or_else(Exc::nre)?;
        let error = RegisterCustomTool(name, description, icon, type_name, parent, coll, alive);
        Ok((error, format!("{type_name}{}", crate::il!(str "ModAPI::RegisterTool" 0))))
    }

    /// ModAPI.RegisterPower<T>: RegisterTool's, into ToolLibrary.Instance.Powers, "<T> registered as power".
    pub fn RegisterPower(name: &str, description: &str, icon: Option<Picture>, type_name: &str, parent: Option<&str>, library: Option<(&mut Vec<crate::game::tool::ToolEntry>, bool)>) -> Result<(Option<&'static str>, String), Exc> {
        let (coll, alive) = library.ok_or_else(Exc::nre)?;
        let error = RegisterCustomTool(name, description, icon, type_name, parent, coll, alive);
        Ok((error, format!("{type_name}{}", crate::il!(str "ModAPI::RegisterPower" 0))))
    }

    /// What ModAPI.CreatePhysicalObject makes, in its order: new GameObject(name, SpriteRenderer, AudioSourceTimeScaleBehaviour,
    /// Optout) on the Objects layer (9) with the sprite, AddComponent BoxCollider2D (fitted to the sprite) and Rigidbody2D,
    /// a PhysicalBehaviour whose Properties are FindPhysicalProperties("Metal") with SpawnSpawnParticles false, a second
    /// AudioSourceTimeScaleBehaviour, and empty OverrideShotSounds and OverrideImpactSounds.
    #[derive(Clone, Debug)]
    pub struct PhysicalObjectSpec {
        pub name: String,
        pub layer: i32,
        pub sprite: Option<Picture>,
        pub properties: Option<crate::game::physical::PhysicalProperties>,
        pub SpawnSpawnParticles: bool,
    }

    /// ModAPI.CreatePhysicalObject(name, sprite): the object's spec (a missing "Metal" asset leaves Properties null);
    /// `Game::mod_api_create_physical_object` puts it in the map.
    pub fn CreatePhysicalObject(db: &mut crate::assets::db::AssetDb, name: &str, sprite: Option<Picture>) -> Result<PhysicalObjectSpec, String> {
        Ok(PhysicalObjectSpec { name: name.to_string(), layer: 9, sprite, properties: FindPhysicalProperties(db, "Metal")?, SpawnSpawnParticles: false })
    }
}

/// What ModAPI.CreateParticleEffect returned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParticleEffect {
    /// PoolGenerator.Instance.RequestPrefab's clone (the pool's own item).
    Pooled,
    /// Object.Instantiate's clone: its root Transform id.
    Spawned(i32),
}

impl super::Game {
    /// ModAPI.CreateParticleEffect(name, position): ModGlobals.LoadedParticleEffects[name] (None for an unknown name), a
    /// prefab tagged Poolable from PoolGenerator.Instance.RequestPrefab (None when its full pool hands out nothing), any
    /// other Object.Instantiate'd at the position with no rotation.
    pub fn mod_api_create_particle_effect(&mut self, name: &str, position: [f32; 2]) -> Option<ParticleEffect> {
        let prefab = mod_globals::with(|g| g.LoadedParticleEffects.get(name).copied())?;
        let poolable = self.assets.as_ref().expect("ModAPI.CreateParticleEffect without the game's data").game_object(prefab).is_ok_and(|g| g.tag == super::water::POOLABLE_TAG);
        if poolable {
            return self.pool_generator_request_prefab(prefab, position).then_some(ParticleEffect::Pooled);
        }
        Some(ParticleEffect::Spawned(self.spawn_prefab(prefab, [position[0], position[1], 0.0])))
    }

    /// ModAPI.CreatePhysicalObject in a map: the spec's GameObject made (see `mod_api::PhysicalObjectSpec`).
    pub fn mod_api_create_physical_object(&mut self, spec: &mod_api::PhysicalObjectSpec) -> usize {
        let _ = spec;
        unimplemented!("mod-only: ModAPI.CreatePhysicalObject's GameObject: a mod file's sprite in the world (the renderers and BoxCollider2D fit take the game's sprite assets only)")
    }

    /// ModdingApi.Events.<event>.Invoke(component) for an event over a behaviour: the component's instance id is the
    /// sender and the argument (`mod_event_invoke`).
    pub(super) fn mod_event(&self, event: &ModApiEvent<i32>, id: i32) {
        mod_event_invoke(event, id, &id);
    }

    /// PhysicalBehaviour.InvokeOnItemRemoved: OnItemRemoved with a UserSpawnEventArgs of the object and its own
    /// SerialiseInstructions' OriginalSpawnableAsset (a root's; GetComponent on a child object finds none).
    pub(crate) fn physical_invoke_on_item_removed(&self, p: usize) {
        let go = self.physicals[p].transform_id;
        let asset = (self.root_id(p) == go).then(|| self.object_names.get(&go).cloned()).flatten();
        let args = UserSpawnEventArgs::new(go, asset);
        mod_event_invoke(&mod_api::OnItemRemoved, go, &args);
    }
}

/// ModEvent`1.Invoke of an event with a legacy hook (ModEvents..ctor sets LegacyEvent to the ModAPI.Invoke* of each): its
/// listeners (only Lua mods add any; the LuaModLoader loads none), the persistent ones (nothing adds any), then
/// LegacyEvent(argument, argument).
pub fn mod_event_invoke<T: 'static>(legacy: &ModApiEvent<T>, sender: i32, argument: &T) {
    legacy.invoke(sender, argument);
}

/// One ModDebugDrawer action: a GL.Begin(mode) vertex list (1 lines, 2 line strip).
#[derive(Clone, Debug, PartialEq)]
pub struct DebugDrawAction {
    pub mode: i32,
    pub vertices: Vec<[f32; 3]>,
}

/// ModdingApi.Draw (and ModAPI.Draw): Global.Awake makes a new one over an empty stack.
#[derive(Debug, Default)]
pub struct ModDebugDrawer {
    pub Actions: std::sync::Mutex<Vec<DebugDrawAction>>,
}

impl ModDebugDrawer {
    /// ModDebugDrawer..ctor
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&self, mode: i32, vertices: Vec<[f32; 3]>) {
        self.Actions.lock().unwrap().push(DebugDrawAction { mode, vertices });
    }

    /// ModDebugDrawer.Line
    pub fn Line(&self, from: [f32; 3], to: [f32; 3]) {
        self.push(1, vec![from, to]);
    }

    /// ModDebugDrawer.Rect: the four edges about `center`.
    pub fn Rect(&self, center: [f32; 3], size: [f32; 3]) {
        let h = [size[0] / 2.0, size[1] / 2.0];
        let at = |x: f32, y: f32| [center[0] + x, center[1] + y, center[2]];
        let (tl, tr, br, bl) = (at(-h[0], h[1]), at(h[0], h[1]), at(h[0], -h[1]), at(-h[0], -h[1]));
        self.push(1, vec![tl, tr, tr, br, br, bl, bl, tl]);
    }

    /// ModDebugDrawer.Circle: a 65-vertex line strip, 5.625 degrees a step.
    pub fn Circle(&self, center: [f32; 3], radius: f32) {
        self.push(2, circle(center, radius));
    }

    /// ModDebugDrawer.Collider
    #[allow(dead_code)]
    pub fn Collider(&self) -> ! {
        unimplemented!("mod-only: ModDebugDrawer.Collider: only mod code draws a collider, and 1.28 runs none")
    }

    // ModDebugRenderer.Awake: the Hidden/Internal-Colored material and Camera.main (the rewrite draws with gizmos).
    /// ModDebugRenderer.OnPostRender: nothing when the stack is empty, else every action popped (last first) and drawn in
    /// magenta.
    pub fn take_for_render(&self) -> Vec<DebugDrawAction> {
        let mut a = std::mem::take(&mut *self.Actions.lock().unwrap());
        a.reverse();
        a
    }
}

fn circle(center: [f32; 3], radius: f32) -> Vec<[f32; 3]> {
    let add = |p: [f32; 3]| [p[0] + center[0], p[1] + center[1], p[2] + center[2]];
    let mut v = vec![add([radius, 0.0, 0.0])];
    for i in 1..64 {
        v.push(add(super::value_types::utils_rotate([radius, 0.0], 5.625 * i as f32, [0.0, 0.0])));
    }
    v.push(add([radius, 0.0, 0.0]));
    v
}

/// ModResourceCache: what ModAPI.LoadSprite/LoadTexture/LoadSound loaded, by file name and type.
pub mod mod_resource_cache {
    use std::any::{Any, TypeId};
    use std::collections::HashMap;
    use std::sync::Mutex;

    // ModResourceCache..cctor: an empty cache.
    static CACHE: Mutex<Option<HashMap<(String, TypeId), Box<dyn Any + Send>>>> = Mutex::new(None);

    /// ModResourceCache.TryGet
    pub fn TryGet<T: Any + Clone>(key: &str) -> Option<T> {
        CACHE.lock().unwrap().as_ref()?.get(&(key.to_string(), TypeId::of::<T>()))?.downcast_ref::<T>().cloned()
    }

    /// ModResourceCache.Cache: Dictionary.Add, which throws for a key already there.
    /// Err is the throw (class: message); the cache stays as it was.
    pub fn Cache<T: Any + Send>(key: &str, obj: T) -> Result<(), String> {
        let mut guard = CACHE.lock().unwrap();
        let c = guard.get_or_insert_with(HashMap::new);
        let k = (key.to_string(), TypeId::of::<T>());
        if c.contains_key(&k) {
            return Err(format!("ArgumentException: An item with the same key has already been added. Key: {key}"));
        }
        c.insert(k, Box::new(obj));
        Ok(())
    }

    /// ModResourceCache.Remove
    pub fn Remove(key: &str, ty: TypeId) {
        if let Some(c) = CACHE.lock().unwrap().as_mut() {
            c.remove(&(key.to_string(), ty));
        }
    }

    /// ModResourceCache.Clean (Global.Awake)
    pub fn Clean() {
        if let Some(c) = CACHE.lock().unwrap().as_mut() {
            c.clear();
        }
    }
}

/// ModGlobals.alreadyRan before the first Awake (ModGlobals..cctor).
pub const ALREADY_RAN: bool = false;

/// ModGlobals: the Main scene's materials, particle effects and catalog that ModAPI's Find* look up by name.
pub mod mod_globals {
    use crate::assets::db::ObjRef;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct Globals {
        pub LoadedMaterials: HashMap<String, ObjRef>,
        pub LoadedParticleEffects: HashMap<String, ObjRef>,
        pub MainCatalog: Option<ObjRef>,
    }

    // ModGlobals..cctor: alreadyRan false, the two dictionaries empty.
    static STATE: Mutex<(bool, Option<Globals>)> = Mutex::new((super::ALREADY_RAN, None));

    /// ModGlobals.Awake: the first time in the process only (alreadyRan), MainCatalog = Catalog, then every Material and
    /// every particle effect by name into its dictionary. Dictionary.Add throws on a repeated name and `val.name` on a
    /// missing one (None), which ends Awake there.
    pub fn Awake(catalog: Option<ObjRef>, materials: &[Option<(String, ObjRef)>], effects: &[Option<(String, ObjRef)>]) {
        let mut st = STATE.lock().unwrap();
        if st.0 {
            return;
        }
        st.0 = true;
        let g = st.1.get_or_insert_with(Globals::default);
        g.MainCatalog = catalog;
        for (list, dict) in [(materials, &mut g.LoadedMaterials), (effects, &mut g.LoadedParticleEffects)] {
            for v in list {
                let Some((name, r)) = v else { return };
                if dict.contains_key(name) {
                    return;
                }
                dict.insert(name.clone(), *r);
            }
        }
    }

    /// The dictionaries and catalog Awake filled.
    pub fn with<R>(f: impl FnOnce(&Globals) -> R) -> R {
        let st = STATE.lock().unwrap();
        f(st.1.as_ref().unwrap_or(&Globals::default()))
    }
}

/// WorkshopItem (a struct nothing in 1.28 makes but its own constructor).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkshopItem {
    pub Name: String,
    pub FileId: u64,
    pub Path: String,
}

impl WorkshopItem {
    /// WorkshopItem..ctor
    pub fn new(name: String, file_id: u64, path: String) -> Self {
        Self { Name: name, FileId: file_id, Path: path }
    }
}


// The legacy C# mod path (ModLoader, ModificationManager, TrustCache, CompilerClient, ModCompilerSockets / Pipes,
// CompilerServerBehaviour, ModCompilerStatusBehaviour): `super::legacy_mods`.

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn workshop_upload_offline_paths() {
        use legacy_activation_cache::*;
        let r = GetLegacyActivationMethods("UseWireBehaviour");
        assert!(r.IsolatedActivation && r.IsolatedContinuousActivation);
        assert_eq!(GetLegacyActivationMethods("Rigidbody2D"), CachedResult::default());
        ProcessObject(&["UseWireBehaviour", "Rigidbody2D"]);
        let mut d = WorkshopUploaderDialog::default();
        d.Start(&["Machines".into(), "Vehicles".into()]);
        assert_eq!(d.Select("Machines"), UploaderOutcome::Notification(crate::il!(str "WorkshopUploaderDialog::Deselect" 0).into()));
        d.Contraption = Some(Default::default());
        d.Select("Machines");
        d.Select("Machines");
        d.Select("Vehicles");
        d.Deselect("Machines");
        assert_eq!(d.Contraption.as_ref().unwrap().Tags, vec!["Vehicles".to_string()]);
        let mut c = SteamWorkshopController::default();
        d.Show();
        assert_eq!(d.Upload(&mut c), UploaderOutcome::Published(PublishOutcome::Error(crate::il!(str "SteamWorkshopController::PublishContraption" 0).into())));
        assert!(!d.active && c.Update().is_none());
        c.isBusy = true;
        assert_eq!(c.PublishContraption(&Default::default()), PublishOutcome::Busy);
        assert!(publish_button_publish().starts_with("<b>Steam is not initialised</b>"));
        // Past the directory check without Steam: Utils.IsFamilyShared throws, the routine dies busy.
        let dir = std::env::temp_dir().join(format!("ppg-upload-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = SteamWorkshopController { currentlyUploading: Some(super::super::contraption::ContraptionMetaData { Directory: Some(dir.clone()), ..Default::default() }), ..Default::default() };
        assert_eq!(c.StartUploadingRoutine(), None);
        assert!(c.isBusy && !c.progress_box);
        assert_eq!(c.PublishContraption(&Default::default()), PublishOutcome::Busy);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn workshop_folder_items() {
        let root = std::env::temp_dir().join(format!("ppg-workshop-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = root.join("Workshop");
        let item = |id: &str, files: &[(&str, &str)]| {
            std::fs::create_dir_all(w.join(id)).unwrap();
            for (f, t) in files {
                std::fs::write(w.join(id).join(f), t).unwrap();
            }
        };
        let meta = r#"{"Name":"X","DisplayName":"Our Box","Version":"1.28","PublishedFileID":0,"CreatorUGCIdentity":"me","RequiredMods":[]}"#;
        item("111", &[("box.json", meta), ("box.jaap", "x")]);
        item("222", &[("mod.json", "{}"), ("a.jaap", "x"), ("b.json", "{}")]);
        item("333", &[("c.json", meta), ("c.jaap", "x"), ("Script.cs", "class A {}")]);
        item("444", &[("readme.txt", "map")]);
        item("notanid", &[("d.json", meta), ("d.jaap", "x")]);
        let items = workshop_scan(&w);
        assert_eq!(items.iter().map(|i| i.0).collect::<Vec<_>>(), vec![111]);
        let m = workshop_contraptions_in(&items);
        assert_eq!((m.len(), m[0].name(), m[0].DisplayName.as_deref(), m[0].PublishedFileID, m[0].CreatorUGCIdentity.as_deref()), (1, "111", Some("Our Box"), 111, None));
        assert_eq!(m[0].PathToDataFile.as_deref(), Some(w.join("111").join("box.jaap").as_path()));
        assert!(workshop_scan(&root.join("Fresh")).is_empty() && root.join("Fresh").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mod_api_events_drawer_and_cache() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let h: Arc<dyn Fn(i32, &i32) + Send + Sync> = Arc::new(move |sender, a| s2.lock().unwrap().push((sender, *a)));
        mod_api::OnGunShot.add(h.clone());
        mod_event_invoke(&mod_api::OnGunShot, 7, &7);
        mod_api::OnGunShot.remove(&h);
        mod_event_invoke(&mod_api::OnGunShot, 8, &8);
        assert_eq!(*seen.lock().unwrap(), vec![(7, 7)]);
        let d = ModDebugDrawer::new();
        d.Line([0.0; 3], [1.0, 0.0, 0.0]);
        d.Rect([0.0; 3], [2.0, 2.0, 0.0]);
        d.Circle([1.0, 1.0, 0.0], 2.0);
        let a = d.take_for_render();
        assert_eq!((a.len(), a[0].vertices.len(), a[1].vertices[0], a[2].vertices.len()), (3, 65, [-1.0, 1.0, 0.0], 2));
        assert_eq!((a[0].vertices[0], a[0].vertices[64]), ([3.0, 1.0, 0.0], [3.0, 1.0, 0.0]));
        assert!(d.take_for_render().is_empty());
        mod_resource_cache::Cache("a.png", 5u32).unwrap();
        assert_eq!((mod_resource_cache::TryGet::<u32>("a.png"), mod_resource_cache::TryGet::<i64>("a.png")), (Some(5), None));
        mod_resource_cache::Remove("a.png", std::any::TypeId::of::<u32>());
        assert_eq!(mod_resource_cache::TryGet::<u32>("a.png"), None);
    }

    #[test]
    fn folder_source_lists_mod_folders_and_the_watcher_sees_a_change() {
        let root = std::env::temp_dir().join(format!("ppg-mods-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut src = ModFolderSource::new(root.join("Mods")).unwrap();
        assert!(root.join("Mods").is_dir());
        std::fs::create_dir_all(root.join("Mods/b")).unwrap();
        std::fs::create_dir_all(root.join("Mods/a")).unwrap();
        std::fs::write(root.join("Mods/a/mod.json"), br#"{"identifier": "test.a"}"#).unwrap();
        assert_eq!(src.GetFolders(), vec![root.join("Mods/a")]);
        assert_eq!(TryGetModIdFromPath(&root.join("Mods/a/mod.json")).unwrap(), (true, Some("test.a".into())));
        assert!(!src.OnUpdate());
        src.Dispose();
        std::fs::write(root.join("Mods/a/x.cs"), b"class X {}").unwrap();
        std::fs::write(root.join("Mods/a/y.txt"), b"skip").unwrap();
        assert_eq!(mod_hasher_compute(&root.join("Mods/a")).unwrap(), 371317 ^ super::super::contraption::xxhash(b"class X {}", 0));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A w x h RGBA PNG (the png crate), for a test mod's sprite.
    pub(crate) fn test_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&vec![200u8; (w * h * 4) as usize]).unwrap();
        drop(wr);
        out
    }

    /// A mono 16-bit PCM WAV of `samples` silent samples at 22050 Hz.
    fn test_wav(samples: u32) -> Vec<u8> {
        let data = samples * 2;
        let mut b = b"RIFF".to_vec();
        b.extend((36 + data).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(22050u32.to_le_bytes());
        b.extend(44100u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend(data.to_le_bytes());
        b.extend(vec![0u8; data as usize]);
        b
    }

    /// ModAPI's file, category and tool functions over a mod folder built here (a sprite, a sound, its metadata); the
    /// DoNotReuploadSign mark and the watcher's OnChange subscriptions.
    #[test]
    fn mod_api_loads_a_test_mod_folder_and_registers() {
        let root = std::env::temp_dir().join(format!("ppg-modapi-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("icon.png"), test_png(4, 2)).unwrap();
        std::fs::write(root.join("bad.png"), b"not a picture").unwrap();
        std::fs::write(root.join("beep.wav"), test_wav(441)).unwrap();
        let mut meta = ModMetaData::new();
        meta.Name = Some("Test Mod".into());
        meta.MetaLocation = Some(root.display().to_string());
        assert!(mod_api::LoadSprite("icon.png", 1.0, true).is_err_and(|e| e.message.contains("LoadSprite must be called")));
        mod_api::STATE.lock().unwrap().metaDataIsValid = true;
        mod_api::set_Metadata(Some(Arc::new(std::sync::Mutex::new(meta))));
        let (s, err) = mod_api::LoadSprite("icon.png", 2.0, true).unwrap();
        let s = s.unwrap();
        assert_eq!((err, s.rect, s.pixels_per_unit, s.texture.point_filter), (None, [0.0, 0.0, 4.0, 2.0], 70.0, true));
        assert!(Arc::ptr_eq(&s, &mod_api::LoadSprite("icon.png", 1.0, false).unwrap().0.unwrap()));
        assert_eq!(mod_api::LoadSprite("gone.png", 1.0, true).unwrap(), (None, Some("Test Mod: Failed to load sprite at \"gone.png\"".into())));
        let (t, _) = mod_api::LoadTexture("icon.png", false).unwrap();
        assert_eq!((t.as_ref().map(|t| (t.image.width, t.point_filter))), Some((4, false)));
        assert_eq!(mod_api::LoadTexture("gone.png", true).unwrap().1.unwrap(), "Test Mod: Failed to load texture at \"gone.png\"");
        let c = mod_api::LoadSound("beep.wav").unwrap();
        assert_eq!((c.clip.frequency, c.pcm.as_ref().unwrap().data.len()), (22050, 441));
        assert!(Arc::ptr_eq(&c, &mod_api::LoadSound("beep.wav").unwrap()));
        assert!(mod_api::LoadSound("gone.wav").is_err());
        // RegisterCategory: before a map, then once, then a second of the same name.
        assert_eq!(mod_api::RegisterCategory("Test cat", Some("d"), None, false), Some(crate::il!(str "ModAPI::RegisterCategory" 0)));
        assert_eq!(mod_api::RegisterCategory("Test cat", Some("d"), None, true), None);
        assert_eq!(mod_api::RegisterCategory("Test cat", None, None, true), Some(crate::il!(str "ModAPI::RegisterCategory" 1)));
        // RegisterTool / RegisterPower: no library, a dead one, a live one (its listener told).
        let told = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let t2 = told.clone();
        mod_api::STATE.lock().unwrap().OnCollectionChange.AddListener(Arc::new(move || {
            t2.fetch_add(1, Ordering::SeqCst);
        }));
        let mut tools = Vec::new();
        assert!(mod_api::RegisterTool("T", "d", None, "TestTool", None, None).is_err());
        let (e, log) = mod_api::RegisterPower("T", "d", Some(s.clone()), "TestTool", Some("P"), Some((&mut tools, false))).unwrap();
        assert_eq!((e, log.as_str(), tools.len()), (Some(crate::il!(str "ModAPI::RegisterCustomTool" 0)), "TestTool registered as power", 0));
        let (e, log) = mod_api::RegisterTool("T", "d", Some(s.clone()), "TestTool", Some("P"), Some((&mut tools, true))).unwrap();
        assert_eq!((e, log.as_str(), tools[0].ty.as_str(), tools[0].parent.as_str(), told.load(Ordering::SeqCst)), (None, "TestTool registered as tool", "TestTool", "P", 1));
        {
            let mut st = mod_api::STATE.lock().unwrap();
            assert!(st.ModdedTypes == ["TestTool"] && st.ModdedCatalog.len() == 1);
            st.ModdedTypes.clear();
            st.ModdedCatalog.clear();
            st.OnCollectionChange = Default::default();
            st.metaDataIsValid = false;
        }
        mod_api::set_Metadata(None);
        // DoNotReuploadSign.Mark appends the mark (a new file gets one too).
        std::fs::write(root.join("c.json"), b"{}").unwrap();
        do_not_reupload_mark(&root.join("c.json")).unwrap();
        do_not_reupload_mark(&root.join("new.json")).unwrap();
        assert!(do_not_reupload_has_mark(&root.join("c.json")).unwrap() && do_not_reupload_has_mark(&root.join("new.json")).unwrap());
        assert!(std::fs::read(root.join("c.json")).unwrap().starts_with(b"{}"));
        // PollingDirectoryWatcher.add_OnChange / remove_OnChange: the last subscription of a handler goes.
        let w = PollingDirectoryWatcher::new(&root, "mod.json");
        let n = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let n2 = n.clone();
        let h: super::super::contraption::WatcherHandler = Arc::new(move || {
            n2.fetch_add(1, Ordering::SeqCst);
        });
        w.add_OnChange(h.clone());
        w.add_OnChange(h.clone());
        w.remove_OnChange(&h);
        w.invoke_on_change();
        w.remove_OnChange(&h);
        w.remove_OnChange(&h);
        w.invoke_on_change();
        assert_eq!(n.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn meta_data_names_and_tags() {
        let mut m = ModMetaData::new();
        m.Author = Some("me".into());
        m.Tags = Some(vec![Some("Fun".into()), None, Some("Weird".into()), Some("Fun".into())]);
        assert_eq!(m.GetUniqueName(), Ok("me-Mod-local".into()));
        assert_eq!(m.GetTagsForSteam(), vec!["Fun".to_string(), "Mods".into()]);
        assert!(registry_try_get_mod_by_id(Some("x")).is_none() && registry_mods().is_empty());
    }
}
