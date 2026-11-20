//! GitHub Releases: parsing the API answer, picking the release for a channel, the asset for this
//! platform and install kind, and SHA256SUMS.txt. All pure functions (tested without a network).
use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::version::Version;

/// Release as the GitHub API returns it (only the fields we use).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct GhRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub html_url: String,
    pub published_at: Option<String>,
    pub assets: Vec<GhAsset>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct GhAsset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    /// "sha256:<hex>" (GitHub computes it for assets uploaded since mid-2025)
    pub digest: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    /// stable releases and pre-releases
    Beta,
}

impl Channel {
    pub fn from_setting(value: &str) -> Channel {
        if value.trim().eq_ignore_ascii_case("beta") { Channel::Beta } else { Channel::Stable }
    }
}

/// How the running copy was installed — decides which asset replaces it and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// Inno Setup install (unins000.exe next to the exe, or under Program Files / AppData\Local\Programs)
    WindowsInstaller,
    /// unpacked portable zip
    WindowsPortable,
    /// $APPIMAGE is set
    AppImage,
    /// .app bundle: the .dmg is downloaded and opened
    MacApp,
    /// tar.gz, distro package, Flatpak… — only the release page is offered
    Manual,
    /// cargo build (target/release, target/debug): never replaced automatically
    Dev,
}

impl InstallKind {
    pub fn code(self) -> &'static str {
        match self {
            InstallKind::WindowsInstaller => "installer",
            InstallKind::WindowsPortable => "portable",
            InstallKind::AppImage => "appimage",
            InstallKind::MacApp => "mac",
            InstallKind::Manual => "manual",
            InstallKind::Dev => "dev",
        }
    }

    pub fn from_code(code: &str) -> Option<InstallKind> {
        [
            InstallKind::WindowsInstaller,
            InstallKind::WindowsPortable,
            InstallKind::AppImage,
            InstallKind::MacApp,
            InstallKind::Manual,
            InstallKind::Dev,
        ]
        .into_iter()
        .find(|kind| kind.code() == code.trim())
    }

    /// The app can download and apply the update itself.
    pub fn automatic(self) -> bool {
        matches!(
            self,
            InstallKind::WindowsInstaller | InstallKind::WindowsPortable | InstallKind::AppImage | InstallKind::MacApp
        )
    }
}

/// Inputs of [`detect_install_kind`] (the real ones come from std::env and the file system).
pub struct Probe<'a> {
    pub os: &'a str,
    pub exe: &'a Path,
    /// $APPIMAGE
    pub appimage: Option<&'a str>,
    /// the Inno uninstaller sits next to the exe
    pub has_uninstaller: bool,
    /// %LOCALAPPDATA%\Programs, %ProgramFiles%, %ProgramFiles(x86)%
    pub install_roots: &'a [&'a Path],
}

fn is_dev_build(exe: &Path) -> bool {
    // split on both separators, so a Windows path reads the same whatever OS the check runs on
    let parts: Vec<String> = exe
        .to_string_lossy()
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    parts.windows(2).any(|w| w[0] == "target" && matches!(w[1].as_str(), "release" | "debug"))
        || parts.windows(3).any(|w| w[0] == "target" && matches!(w[2].as_str(), "release" | "debug"))
}

fn starts_with_ci(path: &Path, root: &Path) -> bool {
    let path = path.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    let root = root.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase();
    !root.is_empty() && path.starts_with(&format!("{root}\\"))
}

pub fn detect_install_kind(probe: &Probe) -> InstallKind {
    match probe.os {
        "linux" if probe.appimage.is_some_and(|p| !p.trim().is_empty()) => InstallKind::AppImage,
        _ if is_dev_build(probe.exe) => InstallKind::Dev,
        "windows" => {
            if probe.has_uninstaller || probe.install_roots.iter().any(|root| starts_with_ci(probe.exe, root)) {
                InstallKind::WindowsInstaller
            } else {
                InstallKind::WindowsPortable
            }
        }
        "macos" if probe.exe.to_string_lossy().contains(".app/") => InstallKind::MacApp,
        _ => InstallKind::Manual,
    }
}

/// Asset for this OS/arch/install kind: …-Setup.exe, …-windows-x64-portable.zip, …-<arch>.AppImage,
/// …-macos-<arm64|x86_64>.dmg. Windows builds are x64 only (they run on Windows on Arm through emulation).
pub fn select_asset<'a>(assets: &'a [GhAsset], kind: InstallKind, arch: &str) -> Option<&'a GhAsset> {
    let suffix = match kind {
        InstallKind::WindowsInstaller => "-setup.exe".to_owned(),
        InstallKind::WindowsPortable => "-windows-x64-portable.zip".to_owned(),
        InstallKind::AppImage => match arch {
            "x86_64" | "aarch64" => format!("-{arch}.appimage"),
            _ => return None,
        },
        InstallKind::MacApp => match arch {
            "aarch64" => "-macos-arm64.dmg".to_owned(),
            "x86_64" => "-macos-x86_64.dmg".to_owned(),
            _ => return None,
        },
        InstallKind::Manual | InstallKind::Dev => return None,
    };
    assets
        .iter()
        .find(|a| a.name.to_ascii_lowercase().ends_with(&suffix) && !a.browser_download_url.is_empty())
}

/// Newest release of the channel that is newer than `current` (drafts never count; Stable skips
/// pre-releases, even ones whose tag is a plain version).
pub fn pick_release<'a>(releases: &'a [GhRelease], channel: Channel, current: &Version) -> Option<(&'a GhRelease, Version)> {
    releases
        .iter()
        .filter(|r| !r.draft)
        .filter_map(|r| Version::parse(&r.tag_name).map(|v| (r, v)))
        .filter(|(r, v)| channel == Channel::Beta || (!r.prerelease && !v.is_prerelease()))
        .filter(|(_, v)| v > current)
        .max_by(|a, b| a.1.cmp(&b.1))
}

/// SHA256SUMS.txt (`sha256sum` output): "<hex>  <name>" or "<hex> *<name>" per line → name → lowercase hex.
/// Names keep only the file name ("./dist/x" → "x"); lines that aren't checksums are skipped.
pub fn parse_sha256sums(text: &str) -> HashMap<String, String> {
    let mut sums = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        let Some((hash, rest)) = line.split_once(char::is_whitespace) else { continue };
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let name = rest.trim_start().trim_start_matches('*').trim();
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        if !name.is_empty() {
            sums.insert(name.to_owned(), hash.to_ascii_lowercase());
        }
    }
    sums
}

/// Expected SHA-256 of an asset: SHA256SUMS.txt first, then the digest GitHub reports for the asset.
pub fn expected_sha256(sums: &HashMap<String, String>, asset: &GhAsset) -> Option<String> {
    sums.get(&asset.name).cloned().or_else(|| {
        asset
            .digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_ascii_lowercase)
    })
}

/// Asset file name safe to use as a local file name (no separators or "..").
pub fn safe_file_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control);
    ok.then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn asset(name: &str) -> GhAsset {
        GhAsset {
            name: name.into(),
            size: 10,
            browser_download_url: format!("https://example.invalid/{name}"),
            digest: None,
        }
    }

    fn release_assets() -> Vec<GhAsset> {
        [
            "Materialfy-0.2.0-aarch64.AppImage",
            "materialfy-0.2.0-linux-aarch64.tar.gz",
            "materialfy-0.2.0-linux-x86_64.tar.gz",
            "Materialfy-0.2.0-macos-arm64.dmg",
            "Materialfy-0.2.0-macos-x86_64.dmg",
            "Materialfy-0.2.0-Setup.exe",
            "Materialfy-0.2.0-windows-x64-portable.zip",
            "Materialfy-0.2.0-x86_64.AppImage",
            "SHA256SUMS.txt",
        ]
        .into_iter()
        .map(asset)
        .collect()
    }

    #[test]
    fn asset_per_platform() {
        let assets = release_assets();
        let pick = |kind, arch| select_asset(&assets, kind, arch).map(|a| a.name.as_str());
        assert_eq!(pick(InstallKind::WindowsInstaller, "x86_64"), Some("Materialfy-0.2.0-Setup.exe"));
        assert_eq!(pick(InstallKind::WindowsInstaller, "aarch64"), Some("Materialfy-0.2.0-Setup.exe"));
        assert_eq!(pick(InstallKind::WindowsPortable, "x86_64"), Some("Materialfy-0.2.0-windows-x64-portable.zip"));
        assert_eq!(pick(InstallKind::AppImage, "x86_64"), Some("Materialfy-0.2.0-x86_64.AppImage"));
        assert_eq!(pick(InstallKind::AppImage, "aarch64"), Some("Materialfy-0.2.0-aarch64.AppImage"));
        assert_eq!(pick(InstallKind::AppImage, "riscv64"), None);
        assert_eq!(pick(InstallKind::MacApp, "aarch64"), Some("Materialfy-0.2.0-macos-arm64.dmg"));
        assert_eq!(pick(InstallKind::MacApp, "x86_64"), Some("Materialfy-0.2.0-macos-x86_64.dmg"));
        assert_eq!(pick(InstallKind::Manual, "x86_64"), None);
        assert_eq!(pick(InstallKind::Dev, "x86_64"), None);
        // a release without the file
        assert_eq!(select_asset(&assets[..2], InstallKind::WindowsInstaller, "x86_64"), None);
    }

    #[test]
    fn install_kind() {
        let programs = PathBuf::from(r"C:\Users\me\AppData\Local\Programs");
        let program_files = PathBuf::from(r"C:\Program Files");
        let roots = [programs.as_path(), program_files.as_path()];
        let probe = |os, exe: &str, appimage, has_uninstaller| {
            let exe = PathBuf::from(exe);
            detect_install_kind(&Probe { os, exe: &exe, appimage, has_uninstaller, install_roots: &roots })
        };
        assert_eq!(probe("windows", r"C:\Users\me\AppData\Local\Programs\Materialfy\materialfy.exe", None, true), InstallKind::WindowsInstaller);
        assert_eq!(probe("windows", r"C:\Users\me\AppData\Local\Programs\Materialfy\materialfy.exe", None, false), InstallKind::WindowsInstaller);
        assert_eq!(probe("windows", r"c:\program files\Materialfy\materialfy.exe", None, false), InstallKind::WindowsInstaller);
        assert_eq!(probe("windows", r"D:\Apps\Materialfy\materialfy.exe", None, false), InstallKind::WindowsPortable);
        assert_eq!(probe("windows", r"D:\Apps\Materialfy\materialfy.exe", None, true), InstallKind::WindowsInstaller);
        // "C:\Program Files Extra" is not under "C:\Program Files"
        assert_eq!(probe("windows", r"C:\Program Files Extra\m\materialfy.exe", None, false), InstallKind::WindowsPortable);
        assert_eq!(probe("windows", r"C:\src\materialfy\target\release\materialfy.exe", None, false), InstallKind::Dev);
        assert_eq!(probe("windows", r"C:\src\m\target\x86_64-pc-windows-msvc\debug\materialfy.exe", None, false), InstallKind::Dev);
        assert_eq!(probe("linux", "/tmp/.mount_Materi/usr/bin/materialfy", Some("/home/me/Materialfy.AppImage"), false), InstallKind::AppImage);
        assert_eq!(probe("linux", "/usr/bin/materialfy", None, false), InstallKind::Manual);
        assert_eq!(probe("linux", "/usr/bin/materialfy", Some(""), false), InstallKind::Manual);
        assert_eq!(probe("macos", "/Applications/Materialfy.app/Contents/MacOS/materialfy", None, false), InstallKind::MacApp);
        assert_eq!(probe("macos", "/Users/me/src/materialfy/target/release/materialfy", None, false), InstallKind::Dev);
        assert_eq!(probe("freebsd", "/usr/local/bin/materialfy", None, false), InstallKind::Manual);
        for kind in ["installer", "portable", "appimage", "mac", "manual", "dev"] {
            assert_eq!(InstallKind::from_code(kind).map(InstallKind::code), Some(kind));
        }
        assert!(InstallKind::from_code("zip").is_none());
        assert!(InstallKind::WindowsPortable.automatic());
        assert!(!InstallKind::Manual.automatic());
        assert!(!InstallKind::Dev.automatic());
    }

    fn release(tag: &str, prerelease: bool, draft: bool) -> GhRelease {
        GhRelease { tag_name: tag.into(), prerelease, draft, ..GhRelease::default() }
    }

    #[test]
    fn release_per_channel() {
        let releases = vec![
            release("v0.3.0", false, true), // draft
            release("v0.2.1-beta.1", true, false),
            release("v0.2.0", false, false),
            release("v0.1.0", false, false),
            release("nightly", true, false),
        ];
        let current = Version::parse("0.1.0").unwrap();
        let tag = |channel, current: &Version| pick_release(&releases, channel, current).map(|(r, _)| r.tag_name.clone());
        assert_eq!(tag(Channel::Stable, &current).as_deref(), Some("v0.2.0"));
        assert_eq!(tag(Channel::Beta, &current).as_deref(), Some("v0.2.1-beta.1"));
        let latest = Version::parse("0.2.0").unwrap();
        assert_eq!(tag(Channel::Stable, &latest), None);
        assert_eq!(tag(Channel::Beta, &latest).as_deref(), Some("v0.2.1-beta.1"));
        let beta = Version::parse("0.2.1-beta.1").unwrap();
        assert_eq!(tag(Channel::Beta, &beta), None);
        // a pre-release flagged on GitHub but tagged like a release stays out of Stable
        let flagged = vec![release("v0.4.0", true, false)];
        assert!(pick_release(&flagged, Channel::Stable, &current).is_none());
        assert_eq!(Channel::from_setting("BETA"), Channel::Beta);
        assert_eq!(Channel::from_setting(""), Channel::Stable);
    }

    #[test]
    fn github_json() {
        let json = r#"{"tag_name":"v0.2.0","name":"Materialfy v0.2.0","body":"**Full Changelog**","draft":false,
            "prerelease":false,"html_url":"https://github.com/fatyzzz/materialfy/releases/tag/v0.2.0",
            "published_at":"2026-10-20T10:00:00Z","assets":[{"name":"Materialfy-0.2.0-Setup.exe","size":29833778,
            "browser_download_url":"https://github.com/x/Materialfy-0.2.0-Setup.exe","digest":"sha256:AB","extra":1}],
            "author":{"login":"fatyzzz"}}"#;
        let release: GhRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.2.0");
        assert_eq!(release.assets[0].size, 29833778);
        assert_eq!(release.published_at.as_deref(), Some("2026-10-20T10:00:00Z"));
        let list: Vec<GhRelease> = serde_json::from_str(&format!("[{json}]")).unwrap();
        assert_eq!(list.len(), 1);
        // null body and missing fields
        let sparse: GhRelease = serde_json::from_str(r#"{"tag_name":"v1.0.0","body":null}"#).unwrap();
        assert!(sparse.body.is_none() && sparse.assets.is_empty());
    }

    #[test]
    fn sha256sums() {
        let a = "15d2eb7d4918f286b63f4ac46e90330639326508e487c30605601e472a1ce026";
        let b = "8CEEE5886667930EF0AC862C14642A4930BCCBA73CE94239FF0E4281A7EF872E";
        let text = format!(
            "\u{feff}{a}  Materialfy-0.1.0-Setup.exe\r\n{b} *./dist/Materialfy-0.1.0-windows-x64-portable.zip\n\
             not a checksum line\nabc  short.txt\n\n{a}  name with spaces.AppImage\n"
        );
        let sums = parse_sha256sums(&text);
        assert_eq!(sums.len(), 3);
        assert_eq!(sums["Materialfy-0.1.0-Setup.exe"], a);
        assert_eq!(sums["Materialfy-0.1.0-windows-x64-portable.zip"], b.to_ascii_lowercase());
        assert_eq!(sums["name with spaces.AppImage"], a);

        let mut setup = asset("Materialfy-0.1.0-Setup.exe");
        assert_eq!(expected_sha256(&sums, &setup).as_deref(), Some(a));
        setup.name = "Other.exe".into();
        assert_eq!(expected_sha256(&sums, &setup), None);
        setup.digest = Some(format!("sha256:{b}"));
        assert_eq!(expected_sha256(&sums, &setup), Some(b.to_ascii_lowercase()));
        setup.digest = Some("sha256:nothex".into());
        assert_eq!(expected_sha256(&sums, &setup), None);
    }

    #[test]
    fn file_names() {
        assert_eq!(safe_file_name("Materialfy-0.2.0-Setup.exe"), Some("Materialfy-0.2.0-Setup.exe"));
        for bad in ["", ".", "..", "../x", r"a\b", "C:x", "a\nb"] {
            assert!(safe_file_name(bad).is_none(), "{bad:?}");
        }
    }
}
