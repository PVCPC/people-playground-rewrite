//! The maps and the catalog, read from the game's own assets.

use crate::assets::{db::{AssetDb, ObjRef}, mono::Value};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// ItemPersistence (cachedLines): the lines of "People Playground_Data/persistence0", each an upper case MD5 hex of
/// name + Environment.MachineName.
#[derive(Default)]
pub struct ItemPersistence {
    lines: Vec<String>,
}

/// ItemPersistence's static cachedLines, and the folder its file is in: our save folder, like NonSteamStatManager's
/// "stats" (the game's relative "People Playground_Data/persistence0"). None (tests, replays) reads and writes nothing.
static PERSISTENCE: LazyLock<Mutex<(Option<PathBuf>, ItemPersistence)>> = LazyLock::new(Default::default);

/// Where ItemPersistence's file lives, set at start-up of the real game.
pub fn persistence_init(dir: &Path) {
    PERSISTENCE.lock().unwrap().0 = Some(dir.to_path_buf());
}

/// ItemPersistence.Deserialise: cachedLines is replaced only when the file exists.
pub fn persistence_deserialise() {
    let mut s = PERSISTENCE.lock().unwrap();
    if let Some(dir) = s.0.clone().filter(|d| d.join("persistence0").exists()) {
        s.1 = ItemPersistence::deserialise(&dir);
    }
}

/// ItemPersistence.Has
pub fn persistence_has(name: &str) -> bool {
    PERSISTENCE.lock().unwrap().1.has(name)
}

/// ItemPersistence.Add
pub fn persistence_add(name: &str) {
    PERSISTENCE.lock().unwrap().1.add(name);
}

/// ItemPersistence.Serialise (File.WriteAllLines; a failure throws).
pub fn persistence_serialise() {
    let s = PERSISTENCE.lock().unwrap();
    if let Some(dir) = &s.0 {
        if let Err(e) = s.1.serialise(dir) {
            // The IOException leaves Global.OnDestroy, whose caller (Unity) logs it with Debug.LogException.
            println!("exception	IOException: {} ({e})", dir.join("persistence0").display());
        }
    }
}

/// Utils.GetMD5(string): MD5 of Encoding.ASCII.GetBytes(input) (the replacement fallback gives one '?' per non-ASCII
/// character, a surrogate pair included).
pub(crate) fn get_md5(input: &str) -> [u8; 16] {
    let bytes: Vec<u8> = input.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).collect();
    md5(&bytes)
}

/// MD5 (RFC 1321) of `data`.
fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 16] = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let mut m = data.to_vec();
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&((data.len() as u64) * 8).to_le_bytes());
    let mut h: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    for chunk in m.chunks(64) {
        let w: Vec<u32> = chunk.chunks(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let [mut a, mut b, mut c, mut d] = h;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let t = d;
            d = c;
            c = b;
            b = b.wrapping_add(a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(w[g]).rotate_left(S[i / 16 * 4 + i % 4]));
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 16];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    out
}

/// Utils.GetMD5AsString: each byte as two upper case hex digits.
pub(crate) fn md5_string(input: &str) -> String {
    get_md5(input).iter().map(|b| format!("{b:02X}")).collect()
}

impl ItemPersistence {
    /// ItemPersistence.Deserialise: the file's lines when it exists (File.ReadAllLines splits at CRLF, LF and CR).
    pub fn deserialise(data_dir: &Path) -> Self {
        let Ok(text) = std::fs::read(data_dir.join("persistence0")) else { return Self::default() };
        let text = String::from_utf8_lossy(text.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&text)).into_owned();
        let mut lines: Vec<String> = text.replace("\r\n", "\n").replace('\r', "\n").split('\n').map(String::from).collect();
        // A final line break ends the last line instead of starting another.
        if lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        ItemPersistence { lines }
    }

    /// ItemPersistence.Has: a line starts with the (case-insensitive) hash of name + Environment.MachineName.
    pub fn has(&self, name: &str) -> bool {
        if self.lines.is_empty() {
            return false;
        }
        let hash = md5_string(&format!("{name}{}", machine_name()));
        self.lines.iter().any(|l| l.to_ascii_uppercase().starts_with(&hash))
    }

    /// ItemPersistence.Add: the name's hash appended unless `has` it.
    pub fn add(&mut self, name: &str) {
        if !self.has(name) {
            self.lines.push(md5_string(&format!("{name}{}", machine_name())));
        }
    }

    /// ItemPersistence.Serialise: File.WriteAllLines(path, cachedLines, Encoding.UTF8): a byte order mark, each line ended by Environment.NewLine.
    pub fn serialise(&self, data_dir: &Path) -> std::io::Result<()> {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        for l in &self.lines {
            bytes.extend_from_slice(l.as_bytes());
            bytes.extend_from_slice(if cfg!(windows) { b"\r\n" } else { b"\n" });
        }
        std::fs::write(data_dir.join("persistence0"), bytes)
    }
}

/// Environment.MachineName (ItemPersistence's static Key): Mono's icall, GetComputerNameW (the NetBIOS name) on Windows, elsewhere
/// gethostname cut at the first '.'; a failed call gives null, which concatenates as "".
/// --ui-screens on another machine: the name of the machine the recording was made on (PPG_MACHINE_NAME), whose
/// persistence0 the harness reads.
pub(crate) static MACHINE_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// --ui-screens: the recording comes from the Windows player, whose Environment.NewLine is CRLF on any machine.
pub(crate) static WINDOWS_NEW_LINE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Environment.NewLine: CRLF on Windows (or as the Windows recording has it), LF elsewhere.
pub(crate) fn new_line() -> &'static str {
    if cfg!(windows) || WINDOWS_NEW_LINE.load(std::sync::atomic::Ordering::Relaxed) { "\r\n" } else { "\n" }
}

pub(crate) fn machine_name() -> String {
    if let Some(n) = MACHINE_NAME.get() {
        return n.clone();
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetComputerNameW(buffer: *mut u16, size: *mut u32) -> i32;
        }
        // MAX_COMPUTERNAME_LENGTH + 1
        let mut buffer = [0u16; 16];
        let mut size = buffer.len() as u32;
        // SAFETY: the buffer holds `size` UTF-16 units and both pointers are valid for the call.
        if unsafe { GetComputerNameW(buffer.as_mut_ptr(), &mut size) } == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..size as usize])
    }
    #[cfg(not(windows))]
    {
        let out = std::process::Command::new("hostname").output().map(|o| o.stdout).unwrap_or_default();
        String::from_utf8_lossy(&out).trim().split('.').next().unwrap_or_default().to_string()
    }
}

#[derive(Clone, Debug)]
pub struct MapEntry {
    pub name: String,
    pub description: String,
}

#[derive(Debug)]
pub struct Category {
    pub name: String,
    pub description: String,
    /// Its Icon sprite.
    pub icon: Option<ObjRef>,
}

#[derive(Debug)]
pub struct Item {
    pub name: String,
    pub description: String,
    pub category: usize,
    /// NameToOrderBy, or the name.
    pub order: String,
    /// Its ViewSprite.
    pub view: Option<ObjRef>,
    /// The saved contraption of a button that is not a spawnable (ItemButtonBehaviour.Local).
    pub local: Option<crate::game::contraption::ContraptionMetaData>,
}

#[derive(Debug, Default)]
pub struct Catalog {
    pub categories: Vec<Category>,
    pub items: Vec<Item>,
}

fn string(v: &Value, field: &str) -> String {
    match v.get(field) {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// ModLoadStep as the launch step BackgroundItemLoader runs. Initialise (Awake's work before 1.28.41) registers a
/// LuaModLoader and a ModFolderSource on the game's "./Mods" folder; a change the source reports asks for a load.
#[derive(Debug)]
pub struct ModLoadStep {
    game: std::path::PathBuf,
    /// ModLoadStep.RequestingLoad
    pub(crate) requesting: bool,
    /// ModLoaderRegistry.Sources: the one ModFolderSource (Initialise makes it).
    source: Option<crate::game::mods::ModFolderSource>,
    /// A running Load: the source's folders still to go, and whether the current one's DoAsyncTask is running.
    load: Option<(std::vec::IntoIter<std::path::PathBuf>, bool)>,
}

impl Default for ModLoadStep {
    fn default() -> Self {
        Self {
            game: Default::default(),
            requesting: true,
            source: Default::default(),
            load: Default::default(),
        }
    }
}

impl ModLoadStep {
    /// ModLoadStep..ctor: RequestingLoad = true.
    pub fn new(game: std::path::PathBuf) -> Self {
        ModLoadStep { game, requesting: true, source: None, load: None }
    }

    // ModLoadStep.OnDestroy: OnModDiscovery.RemoveListener(OnModDiscovered); the step lives as long as the app.
    /// ModLoadStep.OnModDiscovered (ModLoaderRegistry.OnModDiscovery's listener): RequestingLoad = true.
    pub(crate) fn OnModDiscovered(&mut self) {
        self.requesting = true;
    }
}

impl crate::game::background_loader::LaunchStep for ModLoadStep {
    fn display_name(&self) -> String {
        crate::il!(str "ModLoadStep::get_DisplayName" 0).into()
    }
    fn requesting_load(&self) -> bool {
        self.requesting
    }
    /// ModLoadStep.Initialise, ModLoaderRegistry.AddLoader, ModLoaderRegistry.AddSource: AddLoader(new LuaModLoader()),
    /// AddSource(new ModFolderSource("./Mods")), whose OnChange invokes OnModDiscovery, and OnModDiscovered on OnModDiscovery.
    fn initialise(&mut self, loaders: &mut Vec<Box<dyn crate::game::background_loader::ModLoader>>) {
        loaders.push(Box::new(crate::game::mods::LuaModLoader));
        let source = crate::game::mods::ModFolderSource::new(self.game.join("Mods")).unwrap_or_else(|e| panic!("ModFolderSource..ctor: {e}"));
        self.source = Some(source);
    }
    /// ModLoadStep.Update, ModLoaderRegistry.Sources: each source's OnUpdate (ModFolderSource.OnUpdate invoking OnChange reaches OnModDiscovered).
    fn update(&mut self) {
        if self.source.as_mut().is_some_and(|s| s.OnUpdate()) {
            self.OnModDiscovered();
        }
    }
    /// ModLoadStep.Load: RequestingLoad = false, then for each source folder and each loader (a ModSteamSource only
    /// with the LuaModLoader: the rewrite has none) while the folder exists `yield return BackgroundItemLoader.DoAsyncTask(
    /// loader.TryLoadAsync, "Loading mod", dir.Name)`: Show("Loading mod"), SetDisplayState(dir.Name), the task (done at
    /// once) waited on with WaitUntil, which goes on in the next frame's delayed calls, then Hide. TryLoadAsync is
    /// false, so no OnModLoaded and no break; then `yield return new WaitForEndOfFrame()`. The folders are listed when
    /// the load starts (EnumerateDirectories reads them as it goes; a folder made or removed during the load is the only
    /// difference). The Logger lines are empty.
    fn resume(&mut self, status: &mut crate::game::background_loader::Status) -> crate::game::background_loader::Yield {
        use crate::game::background_loader::Yield;
        if self.load.is_none() {
            self.requesting = false;
            let folders = self.source.as_ref().map(|s| s.GetFolders()).unwrap_or_default();
            self.load = Some((folders.into_iter(), false));
        }
        let (folders, in_task) = self.load.as_mut().unwrap();
        if *in_task {
            // DoAsyncTask after its WaitUntil: Logger lines, BackgroundItemLoaderStatusBehaviour.Hide.
            *in_task = false;
            status.shown = None;
            // ModLoadStep.b__0: the task, `if (await loader.TryLoadAsync(source, dir)) success = true`: false here.
            let _ = crate::game::mods::LuaModLoader.TryLoadAsync(std::path::Path::new(""));
            return Yield::EndOfFrame;
        }
        let Some(dir) = folders.next() else {
            self.load = None;
            return Yield::Done;
        };
        if !dir.is_dir() {
            return Yield::EndOfFrame;
        }
        *status = crate::game::background_loader::Status { shown: Some(crate::il!(str "ModLoadStep/<Load>d__11::MoveNext" 0).into()), state: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), workshop: false };
        *in_task = true;
        Yield::NextFrame
    }
    /// BackgroundItemLoader.OnApplicationQuit's ModLoaderRegistry.Unload, the source part: ModFolderSource.Dispose.
    fn on_application_quit(&mut self) {
        if let Some(s) = &mut self.source {
            s.Dispose();
        }
    }
}

/// SteamContraptionLoadStep as the launch step BackgroundItemLoader runs (the Menu scene's third BuiltInLoadStep).
#[derive(Debug)]
pub struct SteamContraptionLoadStep {
    /// SteamContraptionLoadStep.RequestingLoad
    requesting: bool,
    /// ContraptionSerialiser.Sources' SteamContraptionSource (GetContraptions gives nothing while it is inactive).
    source: Option<crate::game::mods::SteamContraptionSource>,
    /// Load is at its WaitUntil.
    waiting: bool,
}

impl Default for SteamContraptionLoadStep {
    fn default() -> Self {
        Self {
            requesting: true,
            source: Default::default(),
            waiting: Default::default(),
        }
    }
}

impl SteamContraptionLoadStep {
    pub fn new() -> Self {
        SteamContraptionLoadStep { requesting: true, source: None, waiting: false }
    }
}

impl crate::game::background_loader::LaunchStep for SteamContraptionLoadStep {
    /// SteamContraptionLoadStep.DisplayName
    fn display_name(&self) -> String {
        crate::il!(str "SteamContraptionLoadStep::get_DisplayName" 0).into()
    }
    fn requesting_load(&self) -> bool {
        self.requesting
    }
    /// SteamContraptionLoadStep.Thumbnail: SteamWorkshopThumbnail.
    fn workshop_thumbnail(&self) -> bool {
        true
    }
    // SteamContraptionLoadStep.Initialise: empty.
    /// SteamContraptionLoadStep.Load: RequestingLoad = false; the SteamContraptionSource already in ContraptionSerialiser.Sources,
    /// else a new one added; Task.Run(UpdateCache) (done at once without Steam) and `yield return new WaitUntil(task.IsCompleted)`,
    /// which goes on in the next frame's delayed calls; a faulted task's exceptions go to the Logger (empty). Then it ends.
    fn resume(&mut self, _status: &mut crate::game::background_loader::Status) -> crate::game::background_loader::Yield {
        use crate::game::background_loader::Yield;
        if std::mem::take(&mut self.waiting) {
            return Yield::Done;
        }
        self.requesting = false;
        self.source.get_or_insert_with(crate::game::mods::SteamContraptionSource::new).UpdateCache();
        // The Workshop folder, created when missing and read once (`mods::workshop_items`).
        crate::game::mods::workshop_items();
        self.waiting = true;
        Yield::NextFrame
    }
}

/// MapLoadStep as the launch step BackgroundItemLoader runs: Load fills MapRegistry, one map a frame.
#[derive(Debug)]
pub struct MapLoadStep {
    game: std::path::PathBuf,
    /// MapLoadStep.RequestingLoad
    requesting: bool,
    // MapLoadStep.Initialise: empty (BackgroundItemLoader.Start calls it before Load).
    /// MapRegistry.GetAllMaps after each change (the reader takes it).
    pub registry: std::sync::Arc<std::sync::Mutex<Option<Result<Vec<MapEntry>, String>>>>,
    /// MapRegistry
    maps: Vec<MapEntry>,
    /// A running Load: the BuiltInMaps still to go (`built_in_maps`).
    load: Option<std::vec::IntoIter<(Option<String>, Option<MapEntry>)>>,
}

impl Default for MapLoadStep {
    fn default() -> Self {
        Self {
            game: Default::default(),
            requesting: true,
            registry: Default::default(),
            maps: Default::default(),
            load: Default::default(),
        }
    }
}

impl MapLoadStep {
    pub fn new(game: std::path::PathBuf) -> Self {
        MapLoadStep { game, requesting: true, registry: Default::default(), maps: Vec::new(), load: None }
    }
}

impl crate::game::background_loader::LaunchStep for MapLoadStep {
    fn display_name(&self) -> String {
        crate::il!(str "MapLoadStep::get_DisplayName" 0).into()
    }
    fn requesting_load(&self) -> bool {
        self.requesting
    }
    /// MapLoadStep.Load: RequestingLoad = false and MapRegistry.Clear, then per BuiltInMap SetDisplayState(map.name) and
    /// MapRegistry.Register (a null map throws at map.name and a second identity in Register: both caught and logged),
    /// then `yield return new WaitForEndOfFrame()`.
    fn resume(&mut self, status: &mut crate::game::background_loader::Status) -> crate::game::background_loader::Yield {
        if self.load.is_none() {
            self.requesting = false;
            self.maps.clear();
            match built_in_maps(&self.game) {
                Ok(v) => self.load = Some(v.into_iter()),
                Err(e) => {
                    *self.registry.lock().unwrap() = Some(Err(e));
                    return crate::game::background_loader::Yield::Done;
                }
            }
            *self.registry.lock().unwrap() = Some(Ok(Vec::new()));
        }
        let Some((name, entry)) = self.load.as_mut().unwrap().next() else {
            self.load = None;
            return crate::game::background_loader::Yield::Done;
        };
        if let Some(name) = name {
            status.state = name;
        }
        if let Some(e) = entry {
            self.maps.push(e);
            *self.registry.lock().unwrap() = Some(Ok(self.maps.clone()));
        }
        crate::game::background_loader::Yield::EndOfFrame
    }
}
/// Logger.LogFormat: empty (the game's Logger class has no body in any method).
fn logger_log_format(_format: &str, _args: &[&str]) {}

/// Logger.LogWarning: empty.
pub fn logger_log_warning<T: ?Sized>(_m: &T) {}

/// Logger.LogWarningFormat: empty.
#[allow(dead_code)]
pub fn logger_log_warning_format(_format: &str, _args: &[&str]) {}

/// Logger.LogError: empty.
#[allow(dead_code)]
pub fn logger_log_error<T: ?Sized>(_m: &T) {}

/// Logger.LogErrorFormat: empty.
#[allow(dead_code)]
pub fn logger_log_error_format(_format: &str, _args: &[&str]) {}

/// MapLoadStep.Load, MapRegistry.Clear, MapRegistry.Register and MapRegistry.GetAllMaps: MapLoadStep.BuiltInMaps registered in order, a duplicate identity skipped, listed in registration order for the map selection menu.
pub fn maps(game: &Path) -> Result<Vec<MapEntry>, String> {
    Ok(built_in_maps(game)?.into_iter().filter_map(|(_, e)| e).collect())
}

/// MapLoadStep.BuiltInMaps in order: each one's Object.name (None for a null entry) and its MapRegistry entry when Register takes it.
pub fn built_in_maps(game: &Path) -> Result<Vec<(Option<String>, Option<MapEntry>)>, String> {
    let mut db = AssetDb::open(game)?;
    let mut out = Vec::new();
    // MapRegistry: the maps by UniqueIdentity, in the order they were registered.
    let mut identities: Vec<String> = Vec::new();
    for menu in db.find_scripts("MapLoadStep")? {
        let Some((_, _, v)) = db.mono(menu)? else { continue };
        for p in v.get("BuiltInMaps").and_then(Value::array).unwrap_or(&[]).iter().filter_map(Value::pptr) {
            // A null map: map.name throws NullReferenceException, which the catch logs (Logger.LogWarning).
            let Some((_, mb, v)) = (match db.resolve(menu.file, p) { Some(m) => db.mono(m)?, None => None }) else {
                out.push((None, None));
                continue;
            };
            // MapRegistry.Register throws on a second map with the same identity (MapLoadStep.Load logs it and goes on).
            let identity = string(&v, "UniqueIdentity");
            if identities.contains(&identity) {
                // MapLoadStep.Load's catch: Logger.LogWarning(m).
                logger_log_warning(&identity);
                out.push((Some(mb.name), None));
                continue;
            }
            logger_log_format(crate::il!(str "MapRegistry::Register" 1), &[&mb.name, &identity]);
            identities.push(identity);
            out.push((Some(mb.name.clone()), Some(MapEntry { name: mb.name, description: string(&v, "Description") })));
        }
        if !out.is_empty() {
            break;
        }
    }
    Ok(out)
}

/// CatalogData (each item a SpawnableAsset..ctor with NameToOrderBy "" and VisibleInCatalog true, then its serialized fields):
/// the categories in order and the items the catalog shows (VisibleInCatalog, not Locked), each in its category, in CatalogBehaviour's order (NameToOrderBy when set, else the name).
pub fn catalog(db: &mut AssetDb) -> Result<Catalog, String> {
    let data = *db.find_scripts("CatalogData")?.first().ok_or("no CatalogData")?;
    let (_, _, root) = db.mono(data)?.ok_or("CatalogData")?;
    let mut catalog = Catalog::default();
    persistence_deserialise();
    let mut category_ids = Vec::new();
    for c in root.get("Categories").and_then(Value::array).ok_or("CatalogData.Categories")? {
        let Some(r) = c.pptr().and_then(|p| db.resolve(data.file, p)) else { continue };
        let (_, mb, v) = db.mono(r)?.ok_or("Category")?;
        category_ids.push(r);
        let icon = v.get("Icon").and_then(Value::pptr).and_then(|p| db.resolve(r.file, p));
        catalog.categories.push(Category { name: mb.name, description: string(&v, "Description"), icon });
    }
    // CatalogBehaviour.Populate: Catalog.Items = Resources.LoadAll<SpawnableAsset>("SpawnableAssets"), not the serialized list.
    let all = db.resources_in("SpawnableAssets", 114)?;
    let n = all.len();
    super::uifn("CatalogBehaviour.Populate", || n.to_string());
    for r in all {
        let (_, mb, v) = db.mono(r)?.ok_or("SpawnableAsset")?;
        let visible = v.get("VisibleInCatalog").and_then(Value::bool).unwrap_or(true);
        // SpawnableAsset.get_IsLocked: the private Locked field.
        let locked = v.get("Locked").and_then(Value::bool).unwrap_or(false);
        let category = v.get("Category").and_then(Value::pptr).and_then(|p| db.resolve(r.file, p)).and_then(|c| category_ids.iter().position(|&x| x == c));
        // CatalogBehaviour.CreateItemButtons: VisibleInCatalog and (!IsLocked or ItemPersistence.Has(name)).
        let (true, Some(category)) = (visible && (!locked || persistence_has(&mb.name)), category) else { continue };
        let order = Some(string(&v, "NameToOrderBy")).filter(|s| !s.is_empty()).unwrap_or_else(|| mb.name.clone());
        let view = v.get("ViewSprite").and_then(Value::pptr).and_then(|p| db.resolve(r.file, p));
        catalog.items.push(Item { name: mb.name, description: string(&v, "Description"), category, order, view, local: None });
    }
    // extras: new items that are not in the real game, in the existing categories they name (src/extras_hook.rs).
    for (name, description, category, view) in crate::extras_hook::catalog_items(db) {
        if let Some(category) = catalog.categories.iter().position(|c| c.name == category) {
            catalog.items.push(Item { order: name.clone(), name, description, category, view, local: None });
        }
    }
    // The categories are `catalog.categories` in order (a null or icon-less one is skipped above, CreateCategoryButtons' "non-existent category" case);
    // `category_button_text` is each button's tooltip, `view::catalog` lays the buttons out and stops where the dropdown would be needed.
    // Enumerable.OrderBy with string keys: Comparer<string>.Default, string.CompareTo (the current culture), stable.
    catalog.items.sort_by(|a, b| culture_cmp(&a.order, &b.order));
    // CatalogBehaviour.CreateItemButtons, the contraptions: ContraptionSerialiser.GetAllContraptions (the "Contraptions" folder, ContraptionFolderSource),
    // a button each after the spawnables, in the sources' order; the LocalCategory shows them.
    catalog.create_contraption_buttons();
    Ok(catalog)
}

impl Catalog {
    /// CatalogBehaviour.CreateItemButtons: the contraption half (run again after a save or a delete): the old contraption buttons go, ContraptionSerialiser.GetAllContraptions
    /// (the "Contraptions" folder, ContraptionFolderSource) gives one each after the spawnables, in the sources' order; the LocalCategory shows them.
    pub fn create_contraption_buttons(&mut self) {
        self.items.retain(|i| i.local.is_none());
        let mut saved = crate::game::contraption::get_all_contraptions(&[crate::game::contraption::ContraptionFolderSource::new(&crate::game::contraption::save_directory())]);
        // The SteamContraptionSource's place, after the folder source: the Workshop folder's items (`mods::workshop_contraptions`).
        saved.extend(crate::game::mods::workshop_contraptions());
        if !saved.is_empty() {
            let Some(local) = self.categories.iter().position(|c| c.name == "Local") else { unimplemented!("mod-only: CatalogBehaviour.LocalCategory: the Local category is not in the catalog") };
            for m in saved {
                let name = m.DisplayName.clone().unwrap_or_default();
                self.items.push(Item { description: String::new(), category: local, order: name.clone(), name, view: None, local: Some(m) });
            }
        }
        // The buttons' texts in order: the spawnables' names, then the contraptions' DisplayNames.
        super::uifn("CatalogBehaviour.CreateItemButtons", || format!("{} {}", self.items.len(), super::ui_text(&self.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join("|"))));
    }
}

pub use super::collation::culture_cmp;

/// CatalogBehaviour.CreateCategoryButtons: a button per category for the first prevCategoryCount (9), the rest in the CatalogDropdownBehaviour's
/// containers (`catalog::category_containers`); each button's Image is the category's Icon and its HasTooltipBehaviour.Text is "<b>" + name + "</b>" +
/// newline + Description (the line break is "\n, as `state::tooltip` splits it). A category without an Icon is skipped (the loop in `catalog`).
pub fn category_button_text(c: &Category) -> String {
    format!("<b>{}</b>\n{}", c.name, c.description)
}

#[cfg(test)]
mod category_button_tests {
    #[test]
    fn category_button_tooltip_text() {
        let c = super::Category { name: "Humans".into(), description: "People".into(), icon: None };
        assert_eq!(super::category_button_text(&c), "<b>Humans</b>\nPeople");
    }
}

impl Catalog {
    /// CatalogBehaviour.SetFilter and CatalogBehaviour.set_SelectedCategory: the items to list: the filter (case-insensitive, against name and category) when set, else the category.
    pub fn shown(&self, category: usize, filter: &str) -> Vec<(usize, &Item)> {
        let f = filter.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| match f.is_empty() {
                true => i.category == category,
                // A contraption button's text is its DisplayName.
                false if i.local.is_some() => i.name.to_lowercase().contains(&f),
                // The text SetFilter searches: item.Item.name + item.Item.Category, the Category's Object.ToString being "name (Category)".
                false => format!("{}{} (Category)", i.name, self.categories[i.category].name).to_lowercase().contains(&f),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        let item = |name: &str, category, order: &str| Item { name: name.into(), description: String::new(), category, order: order.into(), view: None, local: None };
        Catalog {
            categories: ["Biological", "Firearms"].map(|n| Category { name: n.into(), description: String::new(), icon: None }).into(),
            items: vec![item("Human", 0, "Human"), item("Pistol", 1, "Pistol"), item("Rifle", 1, "Rifle")],
        }
    }

    #[test]
    fn culture_order_is_not_ordinal() {
        // The collation tables are read from the game's mscorlib.
        if crate::game_assets::find_game(None).is_err() {
            return;
        }
        let mut v = vec!["brick", "Rod", "Apple", "9mm", "apple", "Large Crate", "Large-Crate", "Largest"];
        v.sort_by(|a, b| culture_cmp(a, b));
        assert_eq!(v, ["9mm", "apple", "Apple", "brick", "Large Crate", "Large-Crate", "Largest", "Rod"]);
        // Hyphen and apostrophe are skipped first and only break a tie.
        use std::cmp::Ordering::*;
        assert_eq!(culture_cmp("Large-Crate", "LargeCrate"), Greater);
        assert_eq!(culture_cmp("it's", "its"), Greater);
        assert_eq!(culture_cmp("co-op", "coop"), Greater);
        assert_eq!(culture_cmp("co-op", "cool"), Greater);
    }

    #[test]
    fn md5_known_values() {
        assert_eq!(md5_string(""), "D41D8CD98F00B204E9800998ECF8427E");
        assert_eq!(md5_string("The quick brown fox jumps over the lazy dog"), "9E107D9D372BB6826BD81D3542A419D6");
    }

    #[test]
    fn shown_by_category_or_filter() {
        let c = cat();
        assert_eq!(c.shown(1, "").iter().map(|(_, i)| i.name.as_str()).collect::<Vec<_>>(), ["Pistol", "Rifle"]);
        assert_eq!(c.shown(0, "FIREARM").len(), 2);
        assert_eq!(c.shown(0, "pist").len(), 1);
    }

    /// Reads the installed game (cargo test -- --ignored).
    #[test]
    #[ignore]
    fn the_installed_game_lists_maps_and_a_catalog() {
        let game = crate::game_assets::find_game(None).unwrap();
        let maps = maps(&game).unwrap();
        assert!(maps.iter().any(|m| m.name == "Default"), "{:?}", maps.iter().map(|m| &m.name).collect::<Vec<_>>());
        let mut db = AssetDb::open(&game).unwrap();
        let c = catalog(&mut db).unwrap();
        println!("{} maps, {} categories, {} items", maps.len(), c.categories.len(), c.items.len());
        assert!(c.items.iter().any(|i| i.name == "Pistol"));
        assert!(!c.categories.is_empty());
    }

    /// Prints the scene values the catalog classes read (cargo test -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn probe_catalog_values() {
        let game = crate::game_assets::find_game(None).unwrap();
        let mut db = AssetDb::open(&game).unwrap();
        for class in ["CatalogBehaviour", "CatalogDropdownBehaviour", "FollowingViewBehaviour", "ConditionalTextBehaviour"] {
            for r in db.find_scripts(class).unwrap() {
                let (_, mb, v) = db.mono(r).unwrap().unwrap();
                println!("PROBE {class} file {} {}", r.file, mb.name);
                for k in ["AutomaticallyPopulateCatalog", "ModdedCatalog", "Property", "TrueText", "FalseText"] {
                    if let Some(x) = v.get(k) { println!("PROBE   {k} = {}", format!("{x:?}").chars().take(200).collect::<String>()); }
                }
            }
        }
        for class in ["SliderUserPreferenceBehaviour", "RadioUserPreferenceControllerBehaviour", "TextMeshProHyperlink", "ItemButtonBehaviour", "BackgroundItemLoader", "BackgroundItemLoaderStatusBehaviour", "ThumbnailCreator", "MapPreviewLargeImageBehaviour", "tankController", "RenderScaleController", "BathroomMirrorController", "TextureSwapTest", "ReverbControllerBehaviour", "GoToScene", "ShowDialogBehaviour", "DisableWhenControlTrigger", "Toggle", "ControlSchemeEditorBehaviour", "PlayButtonMapNameBehaviour", "WindowModeRadioButton", "PressureGridController", "RadiationGridController", "AutomaticSentryController", "TimeToggle", "SlowmotionToggle", "PageSwitchButtonBehaviour", "NotificationControllerBehaviour", "MapEditorSelectionOutlineBehaviour", "ScreenUIEventEmitter", "GameSettingsInitBehaviour", "TexturepackInitialisationBehaviour", "InvisibleOptionInContextMenu", "ToolControllerBehaviour", "SelectionController", "EnvironmentalSettings", "MapBounds"] {
            let rs = db.find_scripts(class).unwrap();
            let mut files: Vec<usize> = rs.iter().map(|r| r.file).collect();
            files.sort();
            files.dedup();
            println!("PROBE COUNT {class} {} in files {:?}", rs.len(), files);
        }
        for r in db.resources_in("SpawnableAssets", 114).unwrap() {
            let (_, mb, v) = db.mono(r).unwrap().unwrap();
            if mb.name.contains("Normal Sized") {
                println!("PROBE spawnable {} file {} Locked {:?} Visible {:?} Category {:?}", mb.name, r.file, v.get("Locked"), v.get("VisibleInCatalog"), v.get("Category"));
            }
        }
        let c = catalog(&mut db).unwrap();
        println!("PROBE categories {} items {}", c.categories.len(), c.items.len());
        let data = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, root) = db.mono(data).unwrap().unwrap();
        println!("PROBE raw categories {} items {}", root.get("Categories").and_then(Value::array).unwrap().len(), root.get("Items").and_then(Value::array).unwrap().len());
    }

    /// ItemPersistence.Add, Serialise and Deserialise on the machine's own name (Windows).
    #[cfg(windows)]
    #[test]
    fn item_persistence_round_trips() {
        let dir = std::env::temp_dir().join(format!("ppr-persistence-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = ItemPersistence::default();
        assert!(!p.has("Egg"));
        p.add("Egg");
        p.add("Egg");
        assert!(p.has("Egg") && !p.has("Other") && p.lines.len() == 1);
        p.serialise(&dir).unwrap();
        let again = ItemPersistence::deserialise(&dir);
        assert!(again.has("Egg") && again.lines == p.lines);
    }
}
