mod achievements;
mod crash;
mod assets;
mod audio_out;
mod b2;
mod custom_material;
mod decals;
mod delete_wire;
mod energy_blur;
mod extras_hook;
mod particles;
mod sfx_reverb;
#[cfg(test)]
mod stop_scan;
mod particle_render;
mod particle_trails;
mod game;
mod game_assets;
mod hinge;
mod integration;
mod grab_pass;
mod lights;
mod lines;
mod limb_material;
/// Extras: a spatial fold's limb clipped at its exit rim.
mod mishap_fold_view;
mod motor;
mod outline;
mod exposure;
mod first_run;
mod play;
mod shader_queues;
mod thumbnail;
mod tmp;
mod replay;
/// Extras (not the game's): the portal gun's drawing.
mod extras_view;
/// Extras: the portal mishaps' drawing.
mod mishap_phase_view;
mod mishap_view;
mod ui;
mod unity;
mod video;
mod wires;

use game_assets::HumanRig;
use std::path::PathBuf;

fn main() {
    crash_log();
    crash::note("started".into());
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

/// Crash reporter: a panic (and the backtrace) is also appended to %LOCALAPPDATA%\ppg-rewrite\crash.log, so it
/// survives the console closing. Later hooks (tests that catch panics) replace this one, as before.
fn crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("ppg-rewrite")) else { return };
        let _ = std::fs::create_dir_all(&dir);
        let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let text = format!("--- crash (unix time {when}, version {}, thread {thread})
{info}
{}
", env!("CARGO_PKG_VERSION"), std::backtrace::Backtrace::force_capture());
        crash::set_panic(text.clone());
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
            let _ = f.write_all(text.as_bytes());
        }
    }));
}

fn run() -> Result<(), String> {
    let mut explicit_game = None;
    let mut cache = None;
    let mut headless = false;
    let mut reference_probe = None;
    let mut native_probe = None;
    let mut b2_probe = None;
    let mut script = None;
    let mut looped = None;
    let mut fn_check = None;
    let mut play = None;
    let mut play_map = None;
    let mut item = None;
    let mut target = None;
    let mut ui_screens = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--game-dir" => {
                explicit_game = Some(PathBuf::from(
                    arguments.next().ok_or("--game-dir needs a path")?,
                ))
            }
            "--cache" => cache = Some(PathBuf::from(arguments.next().ok_or("--cache needs a path")?)),
            "--reference-test" => {
                reference_probe =
                    Some(PathBuf::from(arguments.next().ok_or(
                        "--reference-test needs the private probe executable path",
                    )?))
            }
            "--native-reference-test" => {
                native_probe = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or("--native-reference-test needs a probe path")?,
                ));
            }
            "--b2-reference-test" => {
                b2_probe = Some(PathBuf::from(
                    arguments.next().ok_or("--b2-reference-test needs a probe path")?,
                ));
            }
            "--random-test" => {
                let path = PathBuf::from(arguments.next().ok_or("--random-test needs a recording")?);
                return replay::compare_random(&path);
            }
            "--physics-test" => {
                let path = PathBuf::from(arguments.next().ok_or("--physics-test needs a PPG_DUMP recording")?);
                return game::dump::physics::physics_test(&path);
            }
            "--ui-screens" => ui_screens = Some(PathBuf::from(arguments.next().ok_or("--ui-screens needs a PPG_UISCREENS recording")?)),
            "--fn-check" => {
                fn_check = Some(PathBuf::from(arguments.next().ok_or("--fn-check needs a PPG_FNPROBE recording")?));
            }
            "--loop-test" => {
                looped = Some(PathBuf::from(arguments.next().ok_or("--loop-test needs a PPG_DUMP recording")?));
            }
            "--play-test" => {
                play = Some(PathBuf::from(arguments.next().ok_or("--play-test needs a PPG_DUMP recording")?));
                headless = true;
            }
            "--play" => {
                play = Some(PathBuf::from(arguments.next().ok_or("--play needs a PPG_DUMP recording")?));
            }
            "--play-map" => play_map = Some(arguments.next().ok_or("--play-map needs a map name")?),
            "--item" => item = Some(arguments.next().ok_or("--item needs a catalog item name")?),
            "--target" => target = Some(arguments.next().ok_or("--target needs a catalog item name")?),
            "--play-map-test" => {
                play_map = Some(arguments.next().ok_or("--play-map-test needs a map name")?);
                headless = true;
            }
            "--script-test" => {
                script = Some(PathBuf::from(arguments.next().ok_or("--script-test needs a PPG_DUMP recording")?));
            }
            _ => return Err(format!("Unknown argument: {argument}")),
        }
    }
    if let Some(probe) = b2_probe {
        unity::compare_trig(&probe)?;
        return b2::probe::compare_reference(&probe);
    }
    let dropped = explicit_game.is_some();
    let game = match game_assets::find_game(explicit_game) {
        Ok(game) => game,
        // Not found (or a wrong folder dropped on the launcher), on a plain launch: the window asks for it. Winit makes
        // one window loop per process, so the answer is remembered and the rewrite starts again without --game-dir.
        Err(e) => {
            let tests = headless || script.is_some() || looped.is_some() || fn_check.is_some() || reference_probe.is_some() || native_probe.is_some() || ui_screens.is_some();
            if tests || std::env::var_os("PPG_SHOT").is_some() {
                return Err(e);
            }
            let game = first_run::ask_for_game(&e).ok_or(e)?;
            first_run::check_version(&game)?;
            first_run::remember_game(&game);
            let mut args = std::env::args().skip(1);
            let mut kept = Vec::new();
            while let Some(a) = args.next() {
                if a == "--game-dir" {
                    args.next();
                } else {
                    kept.push(a);
                }
            }
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let status = std::process::Command::new(exe).args(kept).status().map_err(|e| e.to_string())?;
            std::process::exit(status.code().unwrap_or(1));
        }
    };
    first_run::check_version(&game)?;
    if dropped {
        first_run::remember_game(&game);
    }
    // --cache, else the per-user cache, built from the installed game on the first launch.
    let cache = cache.unwrap_or_else(first_run::default_cache);
    first_run::ensure_cache(&cache, &game)?;
    assets::il::load(&game)?;
    ui::collation::preload()?;
    if let Some(r) = ui_screens {
        return ui::screens::run(&r, &game, &cache.canonicalize().map_err(|e| e.to_string())?);
    }
    let rig = HumanRig::load(&cache, &game)?;
    let cache = cache.canonicalize().map_err(|e| e.to_string())?;
    let start = match (play, play_map) {
        (Some(recording), _) => Some(play::Start::Recording(recording)),
        (None, Some(map)) => Some(play::Start::Map(game.clone(), map, item, target)),
        // Without a map, and without one of the other tests, the main menu.
        (None, None) if script.is_none() && looped.is_none() && fn_check.is_none() && reference_probe.is_none() && native_probe.is_none() && !headless => Some(play::Start::Menu(game.clone())),
        (None, None) => None,
    };
    if let Some(start) = start {
        if headless {
            return play::test(&start, &rig);
        }
        return play::run(&start, rig, cache);
    }
    if let Some(dump) = script {
        return game::dump::script_test(&dump, &rig);
    }
    if let Some(probe) = fn_check {
        return game::dump::fn_check(&probe, &rig);
    }
    if let Some(dump) = looped {
        return game::dump::loop_test(&game::dump::recording_for_version(&dump), &rig);
    }
    if let Some(probe) = reference_probe {
        return motor::compare_reference(&probe);
    }
    if let Some(probe) = native_probe {
        hinge::compare_reference(&probe)?;
        return integration::compare_reference(&probe);
    }
    Ok(())
}
