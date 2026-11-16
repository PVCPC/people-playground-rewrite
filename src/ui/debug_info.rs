//! ExportGameDebugInfoBehaviour: the debug report the Information page's "Export game debug info" button writes.

use super::state::{dialog_button, DialogRun, DialogView};

/// UnityEngine.SystemInfo's four strings and numbers the report reads.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemInfo {
    pub operating_system: String,
    pub processor_type: String,
    pub graphics_device_name: String,
    /// SystemInfo.systemMemorySize, in MB.
    pub system_memory_size: u64,
}

/// The file the report is written to, relative to the working directory.
fn file() -> &'static str {
    crate::il!(str "ExportGameDebugInfoBehaviour::Export" 0)
}

/// System.Version's fields (Version.ToString with three parts: major.minor.build).
#[allow(non_snake_case)]
#[derive(Debug)]
pub(crate) struct Version {
    pub _Major: i32,
    pub _Minor: i32,
    pub _Build: i32,
    /// new Version(major, minor, build) leaves it -1.
    pub _Revision: i32,
}

/// GameInfo..cctor: Version = new Version(1, 28, 41).
// GameInfo..cctor: the Version const below (the static field's only initialiser).
pub(crate) const VERSION: Version = Version { _Major: 1, _Minor: 28, _Build: 41, _Revision: -1 };

/// ExportGameDebugInfoBehaviour.GetDebugInfoString: StringBuilder.AppendLine ends a line with Environment.NewLine (CRLF on Windows); no
/// mods are loaded, so the mod list is empty.
pub fn get_debug_info_string(sys: &SystemInfo) -> String {
    let nl = super::data::new_line();
    let mut s = String::new();
    s.push_str(crate::il!(str "ExportGameDebugInfoBehaviour::GetDebugInfoString" 0));
    // GameInfo.Version (Version.ToString: major.minor.build); IntPtr.Size is 8 in the 64 bit player.
    let Version { _Major: major, _Minor: minor, _Build: build, .. } = VERSION;
    s.push_str(&format!("## People Playground [v{major}.{minor}.{build}] [{} bit]\n", if cfg!(target_pointer_width = "64") { "64" } else { "32" }));
    s.push_str(nl);
    s.push_str(crate::il!(str "ExportGameDebugInfoBehaviour::GetDebugInfoString" 4));
    s.push_str(&format!(" - **OS**: {}\n\n", sys.operating_system));
    s.push_str(&format!(" - **CPU**: {}\n\n", sys.processor_type));
    s.push_str(&format!(" - **GPU**: {}\n\n", sys.graphics_device_name));
    s.push_str(&format!(" - **Memory**: {}\n\n", crate::game::utils::get_formatted_byte_string(sys.system_memory_size * 1_000_000)));
    s.push_str(nl);
    s.push_str(crate::il!(str "ExportGameDebugInfoBehaviour::GetDebugInfoString" 9));
    s.push_str(nl);
    s
}

/// SystemInfo of this machine. Windows: the registry-backed WMI classes the player reads through the Win32 API. macOS: the
/// player's strings, "Mac OS X <kern.osproductversion>", machdep.cpu.brand_string, the Metal device's name (the display's
/// chipset model) and hw.memsize in MB. Linux: PRETTY_NAME of os-release, /proc/cpuinfo's model name, the display
/// controller of lspci and MemTotal.
pub fn system_info() -> SystemInfo {
    #[cfg(windows)]
    {
        windows_system_info()
    }
    #[cfg(not(windows))]
    {
        let run = |program: &str, args: &[&str]| -> String {
            std::process::Command::new(program).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
        };
        if cfg!(target_os = "macos") {
            let gpu = run("system_profiler", &["SPDisplaysDataType"]);
            let chipset = gpu.lines().find_map(|l| l.trim().strip_prefix("Chipset Model:")).map(|v| v.trim().to_string()).unwrap_or_default();
            SystemInfo {
                operating_system: format!("Mac OS X {}", run("sysctl", &["-n", "kern.osproductversion"])),
                processor_type: run("sysctl", &["-n", "machdep.cpu.brand_string"]),
                graphics_device_name: chipset,
                system_memory_size: run("sysctl", &["-n", "hw.memsize"]).parse::<u64>().unwrap_or(0) / (1024 * 1024),
            }
        } else {
            let field = |text: &str, name: &str| text.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim_start_matches([' ', '\t', ':', '=']).trim_matches('"').to_string()).unwrap_or_default();
            let os = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
            let cpu = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
            let mem = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
            let gpu = run("lspci", &[]).lines().find(|l| l.contains("VGA compatible controller") || l.contains("3D controller")).and_then(|l| l.split(": ").nth(1)).unwrap_or_default().to_string();
            SystemInfo {
                operating_system: field(&os, "PRETTY_NAME"),
                processor_type: field(&cpu, "model name"),
                graphics_device_name: gpu,
                system_memory_size: field(&mem, "MemTotal").trim_end_matches("kB").trim().parse::<u64>().unwrap_or(0) / 1024,
            }
        }
    }
}

#[cfg(windows)]
fn windows_system_info() -> SystemInfo {
    let script = "$p=Get-CimInstance Win32_Processor|Select-Object -First 1;$g=Get-CimInstance Win32_VideoController|Select-Object -First 1;\
        $c=Get-CimInstance Win32_ComputerSystem;$o=[System.Environment]::OSVersion.Version;\
        $p.Name;$p.MaxClockSpeed;$g.Name;[math]::Floor($c.TotalPhysicalMemory/1MB);$o.Build";
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script]).output().expect("powershell");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let l: Vec<&str> = text.lines().collect();
    let build: u32 = l[4].trim().parse().unwrap_or(0);
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mhz: f64 = l[1].trim().parse().unwrap_or(0.0);
    SystemInfo {
        // Unity: "Windows 11  (10.0.22631) 64bit"
        operating_system: format!("{}  (10.0.{build}) 64bit", if build >= 22000 { "Windows 11" } else { "Windows 10" }),
        processor_type: format!("{} ({cpus} CPUs), ~{:.1}GHz", l[0], mhz / 1000.0),
        graphics_device_name: l[2].trim().to_string(),
        system_memory_size: l[3].trim().parse().unwrap_or(0),
    }
}

/// ExportGameDebugInfoBehaviour.Export: File.WriteAllText, then the dialog with Show in Explorer, Open file and Okay; a failed write is
/// logged (Debug.LogWarning) and says so in a Notification.
pub fn export(sys: &SystemInfo) -> DialogView {
    // DialogBoxManager.Dialog: a DialogView of a title and DialogButtons; DialogButton..ctor: each (text, ClosesDialogBox, delegate); DialogBox.SetHeight(400): `height`.
    match std::fs::write(file(), get_debug_info_string(sys)) {
        Ok(()) => {
            // Path.GetFullPath
            let full = std::path::absolute(file()).map_or_else(|_| file().to_string(), |p| p.to_string_lossy().replace('/', "\\"));
            DialogView {
                title: format!("Exported to\n{full}"),
                width: None,
                height: Some(400.0),
                buttons: vec![
                    dialog_button(crate::il!(str "ExportGameDebugInfoBehaviour::Export" 3), false, Some(DialogRun::ShowInExplorer(full.clone()))),
                    dialog_button(crate::il!(str "ExportGameDebugInfoBehaviour::Export" 4), false, Some(DialogRun::OpenFile(full))),
                    dialog_button("Okay", true, None),
                ],
                entered: None,
            }
        }
        Err(e) => {
            println!("warning\t{e}");
            // DialogBoxManager.Notification: the message as the title and one OK button that closes the box.
            DialogView::notification(format!("Could not save file.\n{e}"))
        }
    }
}

/// A dialog button's delegate: Process.Start of Explorer on the file, or of the file itself.
pub fn run(r: &DialogRun) {
    use std::process::Command;
    match r {
        DialogRun::ShowInExplorer(full) => {
            let dir = std::path::Path::new(full).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
            if cfg!(windows) {
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    let _ = Command::new("explorer.exe").raw_arg(format!("/select,\"{full}\"")).spawn();
                }
            } else if cfg!(target_os = "macos") {
                let _ = Command::new("open").arg(dir).spawn();
            } else {
                let _ = Command::new("xdg-open").arg(dir).spawn();
            }
        }
        DialogRun::OpenFolder(directory) => {
            // ItemButtonBehaviour.ShowInfoDialog's delegate: only an existing directory is opened (explorer.exe, xdg-open or open).
            if !directory.is_empty() && std::path::Path::new(directory).is_dir() {
                let program = if cfg!(windows) { "explorer.exe" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
                let _ = Command::new(program).arg(directory).spawn();
            }
        }
        DialogRun::SelectContraption(_) => unreachable!("the catalog runs Select anyway itself"),
        // Run by the Game (Game::contraption_dialog_action).
        DialogRun::Game(_) => {}
        DialogRun::OpenFile(full) => {
            if cfg!(windows) {
                let _ = Command::new("cmd").args(["/c", "start", "", full]).spawn();
            } else if cfg!(target_os = "macos") {
                let _ = Command::new("open").arg(full).spawn();
            } else {
                let _ = Command::new("xdg-open").arg(full).spawn();
            }
        }
    }
}

/// Utils.OpenURL (Application.OpenURL): the address in the default browser.
pub fn open_url(url: &str) {
    use std::process::Command;
    let _ = if cfg!(windows) {
        Command::new("cmd").args(["/c", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_text() {
        let sys = SystemInfo { operating_system: "OS".into(), processor_type: "CPU".into(), graphics_device_name: "GPU".into(), system_memory_size: 32538 };
        let s = get_debug_info_string(&sys);
        assert!(s.starts_with("# EXPORTED GAME DEBUG INFO\n## People Playground [v1.28.41] [64 bit]\n"));
        assert!(s.contains(" - **Memory**: 32 GB\n\n"));
        assert!(s.ends_with("## All loaded mods\n\n\r\n") || s.ends_with("## All loaded mods\n\n\n"));
    }
}
