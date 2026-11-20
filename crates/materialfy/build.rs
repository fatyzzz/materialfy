// Сборка: все файлы src/**/*.rs с #[cxx_qt::bridge] собираются автоматически —
// новый мост не требует правки этого файла. Иконка exe — materialfy.ico из корня репозитория (логотип «Волна-M»).
use std::path::{Path, PathBuf};

use cxx_qt_build::CxxQtBuilder;

fn collect_bridges(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_bridges(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.lines().any(|l| l.trim_start().starts_with("#[cxx_qt::bridge")) {
                out.push(path);
            }
        }
    }
}

fn main() {
    let mut bridges = Vec::new();
    collect_bridges(Path::new("src"), &mut bridges);
    println!("cargo::rerun-if-changed=src");
    println!("cargo::rerun-if-changed=cpp");

    // Settings → About shows the commit of CI builds (GitHub Actions sets GITHUB_SHA); local builds go without
    println!("cargo::rerun-if-env-changed=GITHUB_SHA");
    if let Ok(sha) = std::env::var("GITHUB_SHA") {
        let short: String = sha.chars().filter(char::is_ascii_hexdigit).take(7).collect();
        println!("cargo::rustc-env=MATERIALFY_COMMIT={short}");
    }

    // winframe.cpp, taskbar.cpp и floating.cpp собираются везде (вне Windows — заглушка), чтобы привязки
    // в qt/ffi.rs и winframe.rs не зависели от ОС
    let mut cpp = vec!["cpp/mf_shell.cpp", "cpp/register.cpp", "cpp/taskbar.cpp", "cpp/winframe.cpp", "cpp/floating.cpp"];
    // островок над строкой меню и вырез экрана — AppKit
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cpp.push("cpp/floating_mac.mm");
    }

    // Без QmlModule: он требует, чтобы все мосты лежали в одном каталоге (QTBUG-93443);
    // типы Materialfy.Core регистрирует cpp/register.cpp.
    let builder = CxxQtBuilder::new()
        .qt_module("Gui")
        .qt_module("Qml")
        .qt_module("Quick")
        .qt_module("Network")
        .qt_module("Widgets")
        .include_dir("cpp")
        .files(bridges)
        .cpp_files(cpp);
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.include("cpp");
            if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
                cc.flag("/utf-8");
            }
        })
    };
    builder.build();

    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => {
            // IDI_ICON1 — иконка exe (ярлыки, панель задач); Qt берёт её же для окон без своей иконки
            let ico = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../materialfy.ico")
                .canonicalize()
                .expect("materialfy.ico");
            println!("cargo::rerun-if-changed={}", ico.display());
            let rc = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("materialfy.rc");
            let ico = ico.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "\\\\");
            // VERSIONINFO: имя «Materialfy» в диспетчере задач и свойствах файла
            let version = env!("CARGO_PKG_VERSION");
            let num = |i: usize| version.split(['.', '-']).nth(i).and_then(|p| p.parse::<u16>().ok()).unwrap_or(0);
            let (major, minor, patch) = (num(0), num(1), num(2));
            let rc_text = format!(
                r#"IDI_ICON1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Materialfy"
      VALUE "ProductName", "Materialfy"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "materialfy.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
            );
            std::fs::write(&rc, rc_text).unwrap();
            embed_resource::compile(&rc, embed_resource::NONE).manifest_required().unwrap();
            println!("cargo::rustc-link-lib=user32");
            println!("cargo::rustc-link-lib=dwmapi");
            println!("cargo::rustc-link-lib=shell32");
            // taskbar.cpp: ITaskbarList3 (COM), тема панели задач из реестра
            println!("cargo::rustc-link-lib=ole32");
            println!("cargo::rustc-link-lib=advapi32");
            // floating.cpp: границы кнопок панели задач — UI Automation (COM, BSTR)
            println!("cargo::rustc-link-lib=oleaut32");
        }
        Ok("linux") => {
            // dlsym (display.rs: QWindowSystemInterface::handlePrimaryScreenChanged)
            println!("cargo::rustc-link-lib=dl");
            // floating.cpp: клавиши островка на X11 (XGrabKey)
            println!("cargo::rustc-link-lib=xcb");
        }
        Ok("macos") => println!("cargo::rustc-link-lib=framework=AppKit"),
        _ => {}
    }
}
