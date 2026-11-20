//! Crash reports (rewrite-only, no game counterpart): when a frame panics, everything needed to find the bug is
//! written to one text file in %LOCALAPPDATA%\ppg-rewrite\crashes, and the crash screen's Report button opens a
//! pre-filled GitHub issue next to that file so the player can attach it.

use crate::game::Game;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

const REPO: &str = "PVCPC/people-playground-rewrite";
static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static LAST_PANIC: Mutex<String> = Mutex::new(String::new());
static LAST_REPORT: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

fn uptime() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// One player action (a button or key), kept with the last 40.
pub fn note(what: String) {
    let mut what = what;
    what.truncate(160);
    let Ok(mut r) = RECENT.lock() else { return };
    if r.len() == 40 {
        r.pop_front();
    }
    r.push_back(format!("{:>8.1}s  {what}", uptime()));
}

/// The panic hook's text (message, place, backtrace) for the report.
pub fn set_panic(text: String) {
    if let Ok(mut p) = LAST_PANIC.lock() {
        *p = text;
    }
}

pub fn dir() -> Option<PathBuf> {
    // Windows: %LOCALAPPDATA%\ppg-rewrite; elsewhere ~/.ppg-rewrite.
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("ppg-rewrite")).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ppg-rewrite")))
}

fn os() -> String {
    let ver = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/c", "ver"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    } else {
        String::new()
    };
    format!("{} {} {ver}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The report for the frame that stopped with `msg`; its path is kept for the Report button.
pub fn write(g: &mut Game, map: Option<&str>, msg: &str) -> Option<PathBuf> {
    let panic = LAST_PANIC.lock().map(|p| p.clone()).unwrap_or_default();
    let recent = RECENT.lock().map(|r| r.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default();
    let t = &g.time;
    let mut text = format!(
        "People Playground rewrite crash report\n\nVersion: {} (commit {})\nSystem: {}\nMap: {}\nFrame: {}  game time {:.2}s  time scale {}  paused {}\nObjects: {} physical, {} limbs\n\nError: {msg}\n\n{panic}\n\nLast actions (seconds since start):\n{recent}\n",
        env!("CARGO_PKG_VERSION"), env!("PPG_COMMIT"), os(), map.unwrap_or("?"), t.frameCount, t.timeAsDouble, t.timeScale, g.paused, g.physicals.len(), g.limbs.len(),
    );
    // The scene as a contraption of every object, if the stopped world can still be read (it may be half-updated).
    let all: Vec<usize> = (0..g.physicals.len()).collect();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let scene = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::game::contraption::serialise_states(&g.object_states_of(&all, [0.0; 3]))));
    std::panic::set_hook(hook);
    match scene {
        Ok(Ok(bytes)) => text += &format!("\nScene (contraption, base64, {} bytes):\n{}\n", bytes.len(), base64(&bytes)),
        Ok(Err(e)) => text += &format!("\nScene: not saved ({e})\n"),
        Err(_) => text += "\nScene: not saved (reading it failed too)\n",
    }
    let dir = dir()?.join("crashes");
    std::fs::create_dir_all(&dir).ok()?;
    let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let path = dir.join(format!("crash-{when}.txt"));
    std::fs::write(&path, &text).ok()?;
    if let Ok(mut l) = LAST_REPORT.lock() {
        *l = Some((path.clone(), issue_body(msg, &panic, &recent, &path)));
    }
    Some(path)
}

fn issue_body(msg: &str, panic: &str, recent: &str, path: &std::path::Path) -> String {
    // The URL stays short enough for browsers: the place and the top of the backtrace; the file has the rest.
    let stack: String = panic.lines().take(40).collect::<Vec<_>>().join("\n");
    let recent: String = recent.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    let mut b = format!(
        "**What were you doing when it crashed?**\n\n\n**Can you make it happen again? How?**\n\n\n---\nVersion {} (commit {}), {}\n\nError: `{msg}`\n\n```\n{stack}\n```\n\nLast actions:\n```\n{recent}\n```\n\n**Please drag the report file into this box:** `{}`\n",
        env!("CARGO_PKG_VERSION"), env!("PPG_COMMIT"), os(), path.display()
    );
    b.truncate(6000);
    b
}

/// The Report button: the crash file shown in Explorer and a new GitHub issue opened with the summary filled in.
pub fn open_report() {
    let Some((path, body)) = LAST_REPORT.lock().ok().and_then(|l| l.clone()) else { return };
    let title = body.lines().find_map(|l| l.strip_prefix("Error: ")).unwrap_or("Crash").trim_matches('`').to_string();
    let url = format!("https://github.com/{REPO}/issues/new?labels=crash&title={}&body={}", encode(&format!("Crash: {title}")), encode(&body));
    let _ = open(&url);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
    }
    #[cfg(not(windows))]
    if let Some(d) = path.parent() {
        let _ = open(&d.to_string_lossy());
    }
}

fn open(target: &str) -> std::io::Result<std::process::Child> {
    if cfg!(windows) {
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", target]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(target).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(target).spawn()
    }
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for (i, c) in bytes.chunks(3).enumerate() {
        if i > 0 && i % 57 == 0 {
            out.push('\n');
        }
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            out.push(if k <= c.len() { A[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
        assert_eq!(super::encode("a b&c"), "a%20b%26c");
    }

    /// A stopped game's report: the error, the panic text, the actions and a scene that loads back.
    #[test]
    fn report_has_the_scene() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut g = crate::play::new_game(&game, "Default", None, None, 0).unwrap();
        let mut db = g.assets.take().unwrap();
        for (k, item) in ["Human", "Brick"].iter().enumerate() {
            let (inst, _) = g.catalog_spawn(&mut db, item, [k as f32 * 3.0, 2.0, 0.0], false).unwrap();
            g.unsynced.push(inst);
        }
        g.assets = Some(db);
        for _ in 0..5 {
            crate::play::step(&mut g, 1.0 / 60.0).unwrap();
        }
        super::note("Spawn(test)".into());
        super::set_panic("panicked at src/x.rs:1:1".into());
        let path = super::write(&mut g, Some("Default"), "boom").expect("written");
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(text.contains("Error: boom") && text.contains("src/x.rs:1:1") && text.contains("Spawn(test)"), "{text}");
        let b64: String = text.split("base64, ").nth(1).expect("a scene").lines().skip(1).collect();
        let bytes = unbase64(&b64);
        let states = crate::game::contraption::deserialise_states(&bytes).unwrap();
        assert_eq!(states.len(), 2, "{:?}", states.iter().map(|s| &s.SpawnableAssetName).collect::<Vec<_>>());
    }

    fn unbase64(s: &str) -> Vec<u8> {
        let v = |c: u8| b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".iter().position(|&a| a == c).unwrap() as u32;
        s.as_bytes().chunks(4).flat_map(|c| {
            let n = c.iter().enumerate().fold(0, |n, (k, &x)| n | if x == b'=' { 0 } else { v(x) << (18 - 6 * k) });
            let keep = c.iter().filter(|&&x| x != b'=').count() - 1;
            [(n >> 16) as u8, (n >> 8) as u8, n as u8].into_iter().take(keep)
        }).collect()
    }
}
