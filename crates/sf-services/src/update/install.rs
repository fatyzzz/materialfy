//! Applying a verified update. Each kind returns what the app does next: quit (a helper or the installer
//! replaces the files and starts the new version) or keep running (macOS: the disk image is opened).
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::release::InstallKind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// the app must quit now; the new version starts by itself
    Quit,
    /// the installer was opened; the app keeps running
    HandedOff,
}

/// Inno Setup flags of a silent update; /RELAUNCH starts the app again (materialfy.iss, [Run]).
pub const SETUP_ARGS: [&str; 6] = ["/SILENT", "/SP-", "/NOCANCEL", "/NORESTART", "/CLOSEAPPLICATIONS", "/RELAUNCH"];

pub fn install(kind: InstallKind, file: &Path, work_dir: &Path) -> Result<Outcome, String> {
    match kind {
        InstallKind::WindowsInstaller => run_setup(file),
        InstallKind::WindowsPortable => replace_portable(file, work_dir),
        InstallKind::AppImage => replace_appimage(file),
        InstallKind::MacApp => open_dmg(file),
        InstallKind::Manual | InstallKind::Dev => Err(mf_i18n::t("This copy of Materialfy can't update itself")),
    }
}

fn io_text(error: std::io::Error) -> String {
    error.to_string()
}

/// A hidden console process that outlives the app. Not DETACHED_PROCESS: powershell.exe without a console
/// exits at once without running the script (0.2.1 and 0.3.0 portable updates just closed the app).
#[cfg(windows)]
fn detached(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
}

#[cfg(not(windows))]
fn detached(command: &mut Command) -> &mut Command {
    command
}

fn run_setup(file: &Path) -> Result<Outcome, String> {
    let mut command = Command::new(file);
    command.args(SETUP_ARGS).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // the setup window (progress of a /SILENT install) must stay visible: no CREATE_NO_WINDOW here
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP
    }
    command.spawn().map_err(io_text)?;
    Ok(Outcome::Quit)
}

/// Directory with materialfy(.exe) inside an unpacked archive: the root or its only subdirectory.
pub fn app_root(extracted: &Path, exe_name: &str) -> Option<PathBuf> {
    if extracted.join(exe_name).is_file() {
        return Some(extracted.to_path_buf());
    }
    let dirs: Vec<PathBuf> = std::fs::read_dir(extracted).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    match dirs.as_slice() {
        [only] if only.join(exe_name).is_file() => Some(only.clone()),
        _ => None,
    }
}

/// PowerShell helper of a portable update: waits for the app to exit, copies the new files over the old
/// ones (qml\ is replaced whole so files dropped in the new version go away), starts the app, cleans up.
/// Copy-Item rather than robocopy: robocopy skips a changed file of the same size and time unless given
/// /IM, which older Windows 10 doesn't know.
pub const PORTABLE_HELPER: &str = r#"param([int]$ProcId, [string]$Source, [string]$Target, [string]$Exe, [string]$Log)
$ErrorActionPreference = 'Continue'
function Write-Log($text) { Add-Content -LiteralPath $Log -Value ("{0:u} {1}" -f (Get-Date), $text) }
Write-Log "update: waiting for process $ProcId"
try { Wait-Process -Id $ProcId -Timeout 60 -ErrorAction SilentlyContinue } catch {}
Start-Sleep -Milliseconds 500
$ok = $false
for ($i = 0; $i -lt 30 -and -not $ok; $i++) {
    try {
        $qml = Join-Path $Target 'qml'
        if ((Test-Path -LiteralPath (Join-Path $Source 'qml')) -and (Test-Path -LiteralPath $qml)) {
            Remove-Item -LiteralPath $qml -Recurse -Force -ErrorAction Stop
        }
        Get-ChildItem -LiteralPath $Source -Force | Copy-Item -Destination $Target -Recurse -Force -ErrorAction Stop
        $ok = $true
    } catch {
        Write-Log ("copy: " + $_.Exception.Message + ", retrying")
        Start-Sleep -Seconds 1
    }
}
Write-Log ("update: copy " + $(if ($ok) { 'done' } else { 'FAILED' }))
Start-Process -FilePath $Exe -WorkingDirectory $Target
if ($ok) { Remove-Item -LiteralPath $Source -Recurse -Force -ErrorAction SilentlyContinue }
"#;

fn replace_portable(file: &Path, work_dir: &Path) -> Result<Outcome, String> {
    let exe = std::env::current_exe().map_err(io_text)?;
    let target = exe.parent().ok_or("no exe directory")?.to_path_buf();
    let exe_name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "materialfy.exe".into());
    // the app directory must be writable, or the helper would fail after the app is gone
    let probe = target.join(".materialfy-update-probe");
    std::fs::write(&probe, b"").map_err(|e| mf_i18n::tr!("Can't write to {0}: {1}", target.display(), e))?;
    let _ = std::fs::remove_file(&probe);

    let extract = work_dir.join("portable");
    let _ = std::fs::remove_dir_all(&extract);
    std::fs::create_dir_all(&extract).map_err(io_text)?;
    unzip(file, &extract)?;
    let source = app_root(&extract, &exe_name).ok_or_else(|| mf_i18n::t("The archive doesn't contain Materialfy"))?;

    let script = work_dir.join("apply-portable-update.ps1");
    std::fs::write(&script, PORTABLE_HELPER).map_err(io_text)?;
    let log = work_dir.join("update.log");
    let mut command = Command::new(system_tool("WindowsPowerShell\\v1.0\\powershell.exe", "powershell.exe"));
    command
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
        .arg(&script)
        .arg("-ProcId")
        .arg(std::process::id().to_string())
        .arg("-Source")
        .arg(&source)
        .arg("-Target")
        .arg(&target)
        .arg("-Exe")
        .arg(&exe)
        .arg("-Log")
        .arg(&log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detached(&mut command).spawn().map_err(io_text)?;
    Ok(Outcome::Quit)
}

/// %SystemRoot%\System32\<relative>, or the bare name through PATH.
fn system_tool(relative: &str, fallback: &str) -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join(relative))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(fallback))
}

/// Unpacks a zip with the tools Windows ships: tar.exe (bsdtar, Windows 10 1803+), else Expand-Archive.
fn unzip(file: &Path, into: &Path) -> Result<(), String> {
    let quiet = |command: &mut Command| {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
    };
    let mut tar = Command::new(system_tool("tar.exe", "tar"));
    tar.arg("-xf").arg(file).arg("-C").arg(into);
    quiet(&mut tar);
    if tar.output().is_ok_and(|out| out.status.success()) {
        return Ok(());
    }
    let mut ps = Command::new(system_tool("WindowsPowerShell\\v1.0\\powershell.exe", "powershell.exe"));
    ps.args(["-NoProfile", "-NonInteractive", "-Command", "Expand-Archive -LiteralPath $args[0] -DestinationPath $args[1] -Force"])
        .arg(file)
        .arg(into);
    quiet(&mut ps);
    let out = ps.output().map_err(io_text)?;
    if out.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(mf_i18n::tr!("Couldn't unpack the update: {0}", detail))
    }
}

/// New AppImage next to the old one, made executable, renamed over it (atomic on one file system);
/// a shell waits for this process to exit and starts it.
fn replace_appimage(file: &Path) -> Result<Outcome, String> {
    let current = std::env::var_os("APPIMAGE").map(PathBuf::from).ok_or("APPIMAGE is not set")?;
    let dir = current.parent().ok_or("no AppImage directory")?;
    let name = current.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let staged = dir.join(format!(".{name}.update"));
    std::fs::copy(file, &staged).map_err(|e| mf_i18n::tr!("Can't write to {0}: {1}", dir.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(io_text)?;
    }
    if let Err(error) = std::fs::rename(&staged, &current) {
        let _ = std::fs::remove_file(&staged);
        return Err(io_text(error));
    }
    let script = format!(
        "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; exec \"$0\"",
        pid = std::process::id()
    );
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(&current)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(io_text)?;
    Ok(Outcome::Quit)
}

fn open_dmg(file: &Path) -> Result<Outcome, String> {
    let status = Command::new("open").arg(file).status().map_err(io_text)?;
    if status.success() { Ok(Outcome::HandedOff) } else { Err(format!("open: {status}")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_app_root() {
        let base = std::env::temp_dir().join(format!("mf-update-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let flat = base.join("flat");
        std::fs::create_dir_all(flat.join("qml")).unwrap();
        std::fs::write(flat.join("materialfy.exe"), b"").unwrap();
        assert_eq!(app_root(&flat, "materialfy.exe"), Some(flat.clone()));

        let nested = base.join("nested");
        std::fs::create_dir_all(nested.join("Materialfy")).unwrap();
        std::fs::write(nested.join("Materialfy").join("materialfy.exe"), b"").unwrap();
        assert_eq!(app_root(&nested, "materialfy.exe"), Some(nested.join("Materialfy")));

        let empty = base.join("empty");
        std::fs::create_dir_all(empty.join("a")).unwrap();
        std::fs::create_dir_all(empty.join("b")).unwrap();
        assert_eq!(app_root(&empty, "materialfy.exe"), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn setup_runs_silently_and_relaunches() {
        assert!(SETUP_ARGS.contains(&"/SILENT"));
        assert!(SETUP_ARGS.contains(&"/RELAUNCH"));
        assert!(!SETUP_ARGS.contains(&"/VERYSILENT"));
        assert!(PORTABLE_HELPER.contains("Wait-Process -Id $ProcId"));
    }

    /// The helper started the way the app starts it: it must run, replace changed files (also of the same
    /// size), drop files gone from qml\, keep the rest and launch the exe.
    #[cfg(windows)]
    #[test]
    fn portable_helper_replaces_files_and_relaunches() {
        let base = std::env::temp_dir().join(format!("mf-portable-helper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (source, target) = (base.join("new"), base.join("app"));
        for dir in [source.join("qml"), target.join("qml")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(source.join("data.txt"), "v2").unwrap();
        std::fs::write(source.join("qml").join("Main.qml"), "new").unwrap();
        std::fs::write(target.join("data.txt"), "v1").unwrap();
        std::fs::write(target.join("qml").join("Main.qml"), "old").unwrap();
        std::fs::write(target.join("qml").join("Gone.qml"), "x").unwrap();
        std::fs::write(target.join("settings.ini"), "keep").unwrap();
        // the "app": a cmd that writes a marker, so the relaunch is visible
        let marker = base.join("relaunched.txt");
        let exe = base.join("app.cmd");
        std::fs::write(&exe, format!("@echo ok> \"{}\"\r\n", marker.display())).unwrap();
        let script = base.join("helper.ps1");
        std::fs::write(&script, PORTABLE_HELPER).unwrap();
        let log = base.join("update.log");

        let mut app = Command::new("cmd").args(["/c", "ping -n 2 127.0.0.1 >nul"]).spawn().unwrap();
        let mut command = Command::new(system_tool("WindowsPowerShell\\v1.0\\powershell.exe", "powershell.exe"));
        command
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
            .arg(&script)
            .arg("-ProcId")
            .arg(app.id().to_string())
            .args(["-Source".as_ref(), source.as_os_str(), "-Target".as_ref(), target.as_os_str()])
            .args(["-Exe".as_ref(), exe.as_os_str(), "-Log".as_ref(), log.as_os_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let status = detached(&mut command).status().unwrap();
        let _ = app.wait();
        for _ in 0..50 {
            if marker.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        let log_text = read(log.clone());
        assert!(status.success(), "{log_text}");
        assert!(log_text.contains("copy done"), "{log_text}");
        assert_eq!(read(target.join("data.txt")), "v2");
        assert_eq!(read(target.join("qml").join("Main.qml")), "new");
        assert!(!target.join("qml").join("Gone.qml").exists());
        assert_eq!(read(target.join("settings.ini")), "keep");
        assert!(marker.is_file(), "the app was not started again");
        assert!(!source.exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
