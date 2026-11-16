//! BackgroundItemLoader: runs the launch steps (MapLoadStep, ModLoadStep, SteamContraptionLoadStep) one after the other while the loading overlay
//! (BackgroundItemLoaderStatusBehaviour) shows each one's name, and again whenever a step asks for a load.
//! LoadingCoroutine runs on to each `yield return new WaitForEndOfFrame()` of a step's Load, and `end_of_frame` resumes it
//! (the caller calls it before the next frame's Update, which is where the frame's end lands for everything that reads it).

/// ILaunchStep
pub trait LaunchStep {
    /// ILaunchStep.DisplayName
    fn display_name(&self) -> String;
    /// ILaunchStep.RequestingLoad
    fn requesting_load(&self) -> bool;
    /// ILaunchStep.Thumbnail: only SteamContraptionLoadStep's is set (its SteamWorkshopThumbnail).
    fn workshop_thumbnail(&self) -> bool {
        false
    }
    /// ILaunchStep.Load, nested in LoadingCoroutine: the first call starts it, each call runs it on to its next yield
    /// or to its end. Its Show, Hide and SetDisplayState calls write `status`.
    fn resume(&mut self, status: &mut Status) -> Yield;
    /// ILaunchStep.Initialise (1.28.41): BackgroundItemLoader.Start calls it on every step before LoadingCoroutine. Only
    /// ModLoadStep's has a body (the work its Awake did before), and that registers mod loaders and sources only.
    fn initialise(&mut self, _loaders: &mut Vec<Box<dyn ModLoader>>) {}
    /// The step's own MonoBehaviour Update (ModLoadStep.Update polls the mod sources).
    fn update(&mut self) {}
    /// The step's part of BackgroundItemLoader.OnApplicationQuit (ModLoaderRegistry.Unload disposes the mod sources).
    fn on_application_quit(&mut self) {}
}

/// Where a step's Load stopped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Yield {
    /// It ended.
    Done,
    /// `yield return new WaitForEndOfFrame()`.
    EndOfFrame,
    /// A nested coroutine that goes on in the next frame's delayed calls (BackgroundItemLoader.DoAsyncTask's
    /// `WaitUntil(() => task.IsCompleted)` on a task that is done by then).
    NextFrame,
}

/// IModLoader: ModLoaderRegistry.Loaders' members (OnUpdate runs every frame).
pub trait ModLoader {
    fn on_update(&mut self);
    /// ModLoaderRegistry.Unload's per-loader part.
    fn unload(&mut self);
}

/// BackgroundItemLoaderStatusBehaviour as the loader drives it: Show's name (the overlay is active while Some) and
/// SetDisplayState's state, and Show's thumbnail: SteamContraptionLoadStep.SteamWorkshopThumbnail when `workshop`, else
/// null (MapLoadStep.Thumbnail, ModLoadStep's, DoAsyncTask's default), which shows DefaultThumbnailSprite.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub shown: Option<String>,
    pub state: String,
    pub workshop: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Wait {
    /// In launch step i, at its WaitForEndOfFrame.
    EndOfFrame(usize),
    /// At `yield return launchStep.Load()` of a step that ended in its first MoveNext: the nested coroutine is gone at
    /// once and Coroutine::ProcessCoroutineCurrent (UnityPlayer 1808b2230) queues the continuation the way `yield return
    /// null` does (CallDelayed, 0 s, Update.ScriptRunDelayedDynamicFrameRate of the next frame); it goes on at step i.
    NextFrame(usize),
    /// Step i waits in a nested coroutine that goes on in the next frame's delayed calls (Yield::NextFrame).
    Resume(usize),
}

pub struct BackgroundItemLoader {
    /// BuiltInLoadSteps that are ILaunchSteps (collected by Start).
    pub BuiltInLoadSteps: Vec<Box<dyn LaunchStep>>,
    launchSteps: Vec<Box<dyn LaunchStep>>,
    isBusy: bool,
    /// Where a running LoadingCoroutine waits.
    running: Option<Wait>,
    /// ModLoaderRegistry.Loaders
    pub Loaders: Vec<Box<dyn ModLoader>>,
    pub status: Status,
    /// ModLoaderRegistry.Unload has run.
    pub unloaded: bool,
}

impl Default for BackgroundItemLoader {
    fn default() -> Self {
        Self {
            BuiltInLoadSteps: Default::default(),
            launchSteps: Default::default(),
            isBusy: true,
            running: Default::default(),
            Loaders: Default::default(),
            status: Default::default(),
            unloaded: Default::default(),
        }
    }
}

/// Its fields but the steps and loaders (trait objects): their counts.
impl std::fmt::Debug for BackgroundItemLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundItemLoader")
            .field("BuiltInLoadSteps", &self.BuiltInLoadSteps.len())
            .field("launchSteps", &self.launchSteps.len())
            .field("isBusy", &self.isBusy)
            .field("running", &self.running)
            .field("Loaders", &self.Loaders.len())
            .field("status", &self.status)
            .field("unloaded", &self.unloaded)
            .finish()
    }
}

impl BackgroundItemLoader {
    /// BackgroundItemLoader.constructor: `isBusy = true`, no launch steps.
    pub fn new() -> Self {
        BackgroundItemLoader { isBusy: true, ..Default::default() }
    }

    /// BackgroundItemLoader.Awake: `Instance = this` (the single loader the Game or app holds) and DontDestroyOnLoad (the
    /// scene change keeps it). Since 1.28.41 a second one logs "Multiple BackgroundItemLoader components found in scene."
    /// (Debug.LogError, console only), disables itself and DestroyImmediate's its own GameObject: the first stays.
    pub fn Awake(slot: &mut Option<BackgroundItemLoader>, this: BackgroundItemLoader) {
        if slot.is_none() {
            *slot = Some(this);
        }
    }

    /// BackgroundItemLoader.Start: the BuiltInLoadSteps that are ILaunchSteps, each one's Initialise, then StartCoroutine(LoadingCoroutine()).
    pub fn Start(&mut self) {
        self.launchSteps.append(&mut self.BuiltInLoadSteps);
        for step in &mut self.launchSteps {
            step.initialise(&mut self.Loaders);
        }
        self.LoadingCoroutine();
    }

    /// BackgroundItemLoader.Update: another LoadingCoroutine when idle and a step is requesting a load; every loader's OnUpdate.
    pub fn Update(&mut self) {
        if !self.isBusy && self.launchSteps.iter().any(|l| l.requesting_load()) {
            self.LoadingCoroutine();
        }
        for loader in &mut self.Loaders {
            loader.on_update();
        }
        for step in &mut self.launchSteps {
            step.update();
        }
    }

    /// BackgroundItemLoader.LoadingCoroutine: busy; each step shown with its name and an empty state, then `yield return
    /// launchStep.Load()` (a nested coroutine; see Wait); finally hidden and not busy.
    pub fn LoadingCoroutine(&mut self) {
        self.isBusy = true;
        self.run_from(0);
    }

    fn run_from(&mut self, first: usize) {
        for i in first..self.launchSteps.len() {
            let step = &mut self.launchSteps[i];
            // BackgroundItemLoaderStatusBehaviour.Show(DisplayName, Thumbnail), SetDisplayState(string.Empty)
            self.status = Status { shown: Some(step.display_name()), state: String::new(), workshop: step.workshop_thumbnail() };
            self.running = Some(match step.resume(&mut self.status) {
                Yield::Done => Wait::NextFrame(i + 1),
                Yield::EndOfFrame => Wait::EndOfFrame(i),
                Yield::NextFrame => Wait::Resume(i),
            });
            return;
        }
        // finally: Hide, isBusy = false
        self.running = None;
        self.status.shown = None;
        self.isBusy = false;
    }

    /// The end of the frame: a LoadingCoroutine waiting in a step's WaitForEndOfFrame goes on; a step that ends here
    /// continues LoadingCoroutine at once (Coroutine::Run, UnityPlayer 1808b2bc0, runs the waiting coroutine).
    pub fn end_of_frame(&mut self) {
        let Some(Wait::EndOfFrame(i)) = self.running else { return };
        self.go_on(i);
    }

    /// Step i runs on from a later yield; ending there continues LoadingCoroutine at once.
    fn go_on(&mut self, i: usize) {
        match self.launchSteps[i].resume(&mut self.status) {
            Yield::Done => self.run_from(i + 1),
            Yield::EndOfFrame => self.running = Some(Wait::EndOfFrame(i)),
            Yield::NextFrame => self.running = Some(Wait::Resume(i)),
        }
    }

    /// Update.ScriptRunDelayedDynamicFrameRate, after every Update: a LoadingCoroutine behind a step that ended at once goes on.
    pub fn after_update(&mut self) {
        match self.running {
            Some(Wait::NextFrame(i)) => self.run_from(i),
            Some(Wait::Resume(i)) => self.go_on(i),
            _ => {}
        }
    }

    /// BackgroundItemLoader.DoAsyncTask: the overlay shows `name` and `description` while the task runs (the action sees it) (Task.Run; the caller waits for it); the log lines are Logger output.
    pub fn DoAsyncTask(&mut self, action: impl FnOnce(&Status) -> Result<(), String>, name: &str, description: &str) -> Vec<String> {
        self.status = Status { shown: Some(name.to_string()), state: description.to_string(), workshop: false };
        let mut log = vec![format!("Started background loading task: {name}, {description}")];
        let result = action(&self.status);
        log.push(format!("Finished background loading task: {name}, {description}"));
        // task.IsFaulted: each inner exception is logged as an error
        if let Err(e) = result {
            log.push(e);
        }
        self.status.shown = None;
        log
    }

    /// BackgroundItemLoader.OnApplicationQuit: ModLoaderRegistry.Unload.
    pub fn OnApplicationQuit(&mut self) {
        for loader in &mut self.Loaders {
            loader.unload();
        }
        for step in &mut self.launchSteps {
            step.on_application_quit();
        }
        self.unloaded = true;
    }

    pub fn is_busy(&self) -> bool {
        self.isBusy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    struct Step(Rc<RefCell<Vec<String>>>, bool);
    impl LaunchStep for Step {
        fn display_name(&self) -> String {
            crate::il!(str "MapLoadStep::get_DisplayName" 0).into()
        }
        fn requesting_load(&self) -> bool {
            self.1
        }
        fn resume(&mut self, status: &mut Status) -> Yield {
            // Two maps: one each frame.
            let n = self.0.borrow().len();
            self.0.borrow_mut().push("load".into());
            self.1 = false;
            status.state = format!("map{}", n % 2);
            if n % 2 == 0 { Yield::EndOfFrame } else { Yield::Done }
        }
    }

    #[test]
    fn start_loads_then_update_reloads_on_request() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut l = BackgroundItemLoader::new();
        assert!(l.is_busy());
        l.BuiltInLoadSteps.push(Box::new(Step(log.clone(), false)));
        l.Start();
        // Shown through the first frame with the first map's state; the frame's end ends the step (no second yield).
        assert_eq!(l.status, Status { shown: Some(crate::il!(str "MapLoadStep::get_DisplayName" 0).into()), state: "map0".into(), workshop: false });
        assert!(l.is_busy());
        l.Update();
        assert_eq!(log.borrow().len(), 1);
        l.end_of_frame();
        assert!(!l.is_busy() && l.status.shown.is_none() && log.borrow().len() == 2);
        l.Update();
        assert_eq!(log.borrow().len(), 2);
        l.launchSteps[0] = Box::new(Step(log.clone(), true));
        l.Update();
        assert_eq!(log.borrow().len(), 3);
        assert_eq!(l.DoAsyncTask(|_| Err("bad".into()), crate::il!(str "ModLoadStep/<Load>d__11::MoveNext" 0), "x").len(), 3);
    }

    struct Instant;
    impl LaunchStep for Instant {
        fn display_name(&self) -> String {
            crate::il!(str "ModLoadStep::get_DisplayName" 0).into()
        }
        fn requesting_load(&self) -> bool {
            false
        }
        fn resume(&mut self, _: &mut Status) -> Yield {
            Yield::Done
        }
    }

    #[test]
    fn a_step_without_yields_holds_the_coroutine_a_frame() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut l = BackgroundItemLoader::new();
        l.BuiltInLoadSteps.push(Box::new(Instant));
        l.BuiltInLoadSteps.push(Box::new(Step(log.clone(), false)));
        l.Start();
        assert_eq!(l.status.shown.as_deref(), Some(crate::il!(str "ModLoadStep::get_DisplayName" 0)));
        l.end_of_frame();
        assert_eq!(l.status.shown.as_deref(), Some(crate::il!(str "ModLoadStep::get_DisplayName" 0)));
        l.Update();
        l.after_update();
        assert_eq!(l.status, Status { shown: Some(crate::il!(str "MapLoadStep::get_DisplayName" 0).into()), state: "map0".into(), workshop: false });
        l.end_of_frame();
        assert!(!l.is_busy() && l.status.shown.is_none());
    }
}
