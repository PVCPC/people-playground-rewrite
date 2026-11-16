//! DiskLogger (on Main's and Menu's scene objects): Logs/<unix seconds>-<scene>.log, keeping at most 25 old logs, with
//! every message Application.logMessageReceived carries after its Awake. "Logs" is relative to the working directory in
//! the game; the rewrite keeps it in its own user-data folder (next to the executable, beside ppr-preferences.json),
//! never in the game's folders.
use super::menu::LogType;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_FILE_COUNT: usize = 25;

/// One DiskLogger: its StreamWriter (None when the file could not be made: `new StreamWriter` would throw).
pub struct DiskLogger {
    writer: Option<Box<dyn Write + Send + Sync>>,
}

/// The rewrite's user-data folder: the executable's.
pub fn user_data_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).unwrap_or_default()
}

impl DiskLogger {
    /// DiskLogger.Awake in scene `scene` under `root` (the user-data folder). Its Debug.Log / LogError calls reach no
    /// listener of the rewrite: its own subscribes at the end, NotificationControllerBehaviour in Start (after every Awake).
    pub fn awake(root: &Path, scene: &str) -> Self {
        let dir = root.join("Logs");
        if !dir.is_dir() {
            let _ = std::fs::create_dir_all(&dir);
        } else {
            // Directory.GetFiles("Logs", "*.log")
            let files: Vec<PathBuf> = std::fs::read_dir(&dir).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "log")).collect()).unwrap_or_default();
            if files.len() > MAX_LOG_FILE_COUNT {
                let mut num = 0;
                // OrderBy(File.GetCreationTime): a stable sort.
                let mut by_time: Vec<(std::time::SystemTime, &PathBuf)> = files.iter().map(|f| (std::fs::metadata(f).and_then(|m| m.created()).unwrap_or(std::time::UNIX_EPOCH), f)).collect();
                by_time.sort_by_key(|(t, _)| *t);
                for (_, item) in by_time {
                    match std::fs::remove_file(item) {
                        Ok(()) => {
                            num += 1;
                            if files.len() - num <= MAX_LOG_FILE_COUNT {
                                break;
                            }
                        }
                        // Debug.LogError(message)
                        Err(_) => {}
                    }
                }
                // Debug.Log($"{num} old log files deleted"): Unity's player log only.
            }
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        let writer = std::fs::File::create(Self::get_path(&dir, secs, scene)).ok().map(|f| Box::new(std::io::BufWriter::new(f)) as Box<dyn Write + Send + Sync>);
        // Debug.Log("Timestamp: " + DateTime.UtcNow.ToString()); Debug.Log("Current version: 1.28"): Unity's player log only.
        Self { writer }
    }

    /// A snapshot's DiskLogger (fn-check): its log file lies outside the snapshot, so it writes nowhere.
    pub fn loaded() -> Self {
        Self { writer: Some(Box::new(std::io::sink())) }
    }

    /// DiskLogger.GetPath: Logs/{DateTimeOffset.Now.ToUnixTimeSeconds()}-{scene name}.log
    fn get_path(dir: &Path, secs: i64, scene: &str) -> PathBuf {
        dir.join(format!("{secs}-{scene}.log"))
    }

    /// DiskLogger.GetPrefix
    pub(crate) fn get_prefix(ty: LogType) -> &'static str {
        match ty {
            LogType::Error => "[ERROR]",
            LogType::Assert => "[ASSERT]",
            LogType::Warning => "[WARNING]",
            LogType::Log => "[LOG]",
            LogType::Exception => "[EXCEPTION]",
        }
    }

    /// DiskLogger.LogMessageReceived. The rewrite has no managed stack trace, so `stack_trace` is the empty string Unity
    /// passes for a message without one.
    pub fn log_message_received(&mut self, condition: &str, stack_trace: &str, ty: LogType) {
        let Some(w) = self.writer.as_mut() else { return };
        let _ = write!(w, " {} {condition}", Self::get_prefix(ty));
        if ty != LogType::Log && !stack_trace.trim().is_empty() {
            // StreamWriter.WriteLine: Environment.NewLine.
            let _ = write!(w, "{NL}\t{stack_trace}{NL}");
        }
        let _ = write!(w, "{NL}");
    }
}

impl DiskLogger {
    /// StreamWriter.Dispose: flushes and closes (the writer is gone).
    fn dispose(&mut self) {
        if let Some(mut w) = self.writer.take() {
            let _ = w.flush();
        }
    }
}

impl Drop for DiskLogger {
    /// A logger dropped without its OnDestroy (the rewrite's process exit; Unity runs OnDestroy at quit): the same Dispose.
    fn drop(&mut self) {
        self.dispose();
    }
}

impl super::Game {
    /// DiskLogger.OnDestroy (Main's): writer.Dispose() and `Application.logMessageReceived -= LogMessageReceived` (it
    /// leaves `disk_logger`, through which alone `log_message_received` reaches it).
    pub fn disk_logger_on_destroy(&mut self) {
        if let Some(mut l) = self.disk_logger.take() {
            l.dispose();
        }
    }

    /// Main's DiskLogger for the fn-check's " st=dl:<writer open>,<listener on>" (Recorder.cs FnTiny.StateOf).
    pub fn disk_logger_state(&self) -> String {
        match &self.disk_logger {
            Some(l) => format!("dl:{},1", l.writer.is_some() as u8),
            None => "dl:0,0".into(),
        }
    }
}

#[cfg(windows)]
const NL: &str = "\r\n";
#[cfg(not(windows))]
const NL: &str = "\n";
