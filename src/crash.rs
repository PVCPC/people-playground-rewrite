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
    // The URL stays short enough for browsers: the place and the rewrite's own frames; the file has the rest.
    let stack = our_frames(panic);
    let recent: String = recent.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    let mut b = format!(
        "**What were you doing when it crashed?**\n\n\n**Can you make it happen again? How?**\n\n\n---\nVersion {} (commit {}), {}\n\nError: `{msg}`\n\n```\n{stack}\n```\n\nLast actions:\n```\n{recent}\n```\n\n**Please drag the report file into this box:** `{}`\n",
        env!("CARGO_PKG_VERSION"), env!("PPG_COMMIT"), os(), path.display()
    );
    b.truncate(6000);
    b
}

/// The panic's place, then only the rewrite's own frames (function and file:line), not the std/core ones around them.
fn our_frames(panic: &str) -> String {
    let mut out: Vec<&str> = panic.lines().take_while(|l| !l.trim_start().starts_with("0:")).filter(|l| !l.trim().is_empty()).collect();
    let mut lines = panic.lines().peekable();
    while let Some(l) = lines.next() {
        if (l.contains("people_playground_bevy::") || l.contains("ppg_extras::")) && !l.contains("::crash::") && !l.contains("::play::guarded") {
            out.push(l.trim());
            if let Some(at) = lines.peek().filter(|n| n.trim_start().starts_with("at ")) {
                out.push(at.trim());
            }
        }
        if out.len() >= 40 {
            break;
        }
    }
    out.join("\n")
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

    /// The issue keeps the place and the rewrite's frames, drops std/core.
    #[test]
    fn issue_keeps_our_frames() {
        let p = "panicked at src\\game\\limb.rs:9:1:\ncalled `Option::unwrap()` on a `None` value\n   0: std::backtrace::Backtrace::create\n             at /rustc/x/backtrace.rs:331\n  14: core::option::Option::unwrap\n             at /rustc/x/option.rs:1014\n  15: people_playground_bevy::game::limb::impl$3::crush\n             at .\\src\\game\\limb.rs:9\n  16: people_playground_bevy::play::guarded<tuple$<>>\n             at .\\src\\play.rs:71\n";
        assert_eq!(super::our_frames(p), "panicked at src\\game\\limb.rs:9:1:\ncalled `Option::unwrap()` on a `None` value\n15: people_playground_bevy::game::limb::impl$3::crush\nat .\\src\\game\\limb.rs:9");
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

    /// LukeFisher's crash (Discord 11-20): humans crushed under an I-beam smashed down from above. Each seed (CRUSH=<from>,<to>)
    /// stands 6 to 11 humans apart on the floor (checked: no two overlap), puts an I-beam or Small I-Beam well above them
    /// (checked clear), then drops it again and again, drives it down, or smashes it with the drag tool while grinding
    /// sideways. Runs the real stop path with a panic hook like main's and prints the report
    /// (cargo test crush -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn crush_humans_under_a_beam() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let seeds = std::env::var("CRUSH").unwrap_or_else(|_| "0,20".into());
        let (from, to) = seeds.split_once(',').unwrap();
        std::panic::set_hook(Box::new(|info| super::set_panic(format!("{info}
{}", std::backtrace::Backtrace::force_capture()))));
        let bounds = |g: &crate::game::Game, r: std::ops::Range<usize>| {
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for q in r {
                let body = g.physicals[q].rigidbody.body;
                for f in g.world.fixtures.iter().flatten().filter(|f| f.body == body) {
                    for c in 0..f.shape.child_count() {
                        let bb = f.shape.compute_aabb(&g.world.body(body).xf, c);
                        lo = [lo[0].min(bb.lower.x), lo[1].min(bb.lower.y)];
                        hi = [hi[0].max(bb.upper.x), hi[1].max(bb.upper.y)];
                    }
                }
            }
            (lo, hi)
        };
        let mut g = crate::play::new_game(&game, "Default", None, None, 0).unwrap();
        for seed in from.parse::<u64>().unwrap()..to.parse::<u64>().unwrap() {
            let mut r = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mut rnd = move || { r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); ((r >> 33) as f32) / (1u64 << 31) as f32 };
            g = crate::play::new_game(&game, "Default", None, None, 0).unwrap();
            for _ in 0..30 { crate::play::step(&mut g, 1.0 / 60.0).unwrap(); }
            let c = g.BoundingBox.center;
            let mut db = g.assets.take().unwrap();
            let gap = 1.2 + rnd() * 0.8;
            let n = 6 + (rnd() * 6.0) as usize;
            let mut people = Vec::new();
            for k in 0..n {
                let first = g.physicals.len();
                let (inst, _) = g.catalog_spawn(&mut db, "Human", [c[0] - gap * (n as f32 - 1.0) * 0.5 + k as f32 * gap, 0.0, 0.0], false).unwrap();
                g.unsynced.push(inst);
                people.push(first..g.physicals.len());
            }
            g.assets = Some(db);
            for _ in 0..2 { crate::play::step(&mut g, 1.0 / 60.0).unwrap(); }
            let boxes: Vec<_> = people.iter().map(|p| bounds(&g, p.clone())).collect();
            for (i, x) in boxes.iter().enumerate() {
                assert!(boxes[i + 1..].iter().all(|y| !(x.0[0] < y.1[0] && y.0[0] < x.1[0])), "seed {seed}: humans overlap at spawn");
            }
            let top = boxes.iter().map(|x| x.1[1]).fold(f32::MIN, f32::max);
            let beam = ["I-Beam", "Small I-Beam", "I-Beam", "I-Beam"][(rnd() * 4.0) as usize];
            let mode = (rnd() * 3.0) as usize;
            let (h, down, grind) = (6.0 + rnd() * 10.0, 20.0 + rnd() * 180.0, rnd() * 2.0);
            let mut db = g.assets.take().unwrap();
            let first = g.physicals.len();
            let (inst, _) = g.catalog_spawn(&mut db, beam, [c[0], h, 0.0], false).unwrap();
            g.unsynced.push(inst);
            g.assets = Some(db);
            let parts: Vec<usize> = (first..g.physicals.len()).collect();
            crate::play::step(&mut g, 1.0 / 60.0).unwrap();
            assert!(bounds(&g, first..g.physicals.len()).0[1] > top + 2.0, "seed {seed}: the beam is not well above the humans");
            let mut stop = None;
            for frame in 0..1800u32 {
                match mode {
                    // Lifted back up and dropped again.
                    0 => if frame % 300 == 0 && frame > 0 {
                        for &q in &parts { if !g.physicals[q].destroyed { let at = g.physicals[q].transform.position; g.move_physical_for_test(q, [0.0, h - at[1], 0.0]); } }
                    },
                    1 => { let v = if frame % 120 < 90 { -down } else { 15.0 }; for &q in &parts { if !g.physicals[q].destroyed { let rb = g.physicals[q].rigidbody.clone(); rb.set_velocity(&mut g.world, [0.0, v]); } } }
                    // The drag tool picks it up and smashes it to the floor, grinding sideways, and lifts it again.
                    _ => {
                        let t = frame % 120;
                        let y = if frame < 8 { h } else if t < 20 { h - (h - 0.1) * t as f32 / 20.0 } else if t < 80 { 0.1 } else { 0.1 + (h - 0.1) * (t - 80) as f32 / 40.0 };
                        let x = c[0] + grind * ((frame as f32) * 0.05).sin();
                        let origin = [c[0] - 19.2, -2.0];
                        g.input.mouse = [x, y, 0.0];
                        g.input.screen = Some(crate::game::tool::Screen { mouse: [(x - origin[0]) / 0.02, (y - origin[1]) / 0.02], origin, scale: 0.02, view: None });
                        g.input.controls.insert("drag".into(), crate::game::tool::Control { down: frame == 1, held: frame >= 1, up: false });
                    }
                }
                if let Err(m) = crate::play::step(&mut g, 1.0 / 60.0) { stop = Some((frame, m)); break; }
            }
            let crushed = g.physicals.iter().filter(|p| p.isDisintegrated).count();
            let case = format!("seed {seed}: {beam}, mode {mode}, height {h:.1}, speed {down:.0}, {n} humans {gap:.2} m apart");
            if let Some((frame, m)) = stop {
                let _ = std::panic::take_hook();
                let path = super::write(&mut g, Some("Default"), &m).expect("written");
                let text = std::fs::read_to_string(&path).unwrap();
                let _ = std::fs::remove_file(&path);
                panic!("{case}: STOPPED at frame {frame}: {m}
{}", text.split("Scene (").next().unwrap());
            }
            println!("{case}: no stop, {crushed} limbs crushed");
        }
        if std::env::var_os("FORCE").is_some() {
            // The same unwrap-on-None panic, raised inside the guarded step path, to see the report it makes.
            let m = crate::play::guarded(|| -> Result<(), String> { let none: Option<u8> = None; none.unwrap(); Ok(()) }).unwrap_err();
            let _ = std::panic::take_hook();
            let path = super::write(&mut g, Some("Default"), &m).expect("written");
            let text = std::fs::read_to_string(&path).unwrap();
            println!("FORCED: {m}
REPORT {}
{}", path.display(), text.split("Scene (").next().unwrap());
            println!("SCENE BYTES {}", text.split("Scene (").nth(1).map_or("none", |t| t.lines().next().unwrap_or("")));
            println!("ISSUE URL LENGTH {}", super::LAST_REPORT.lock().unwrap().as_ref().map_or(0, |(_, b)| super::encode(b).len()));
            let _ = std::fs::remove_file(&path);
            return;
        }
        let _ = std::panic::take_hook();
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
