// Materialfy — клиент Spotify: QML (src/materialfy/qml) + бэкенд на Rust (cxx-qt, librespot).
// Оболочка: cli, context, display, engine, icons, images, models, qt, winframe; мосты QML — bridges.
// релиз на Windows — GUI-подсистема, без чёрного окна консоли; отладочная сборка оставляет консоль для логов
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod bridges;
mod cli;
mod context;
mod display;
mod engine;
mod icons;
mod images;
mod models;
mod qml_cache;
mod qt;
mod winframe;

fn main() {
    install_crash_log();
    let args: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    let code = engine::run(cli::Cli::parse(&args));
    std::process::exit(code);
}

/// Паника в GUI-потоке (или в коде, который зовёт Qt) роняет процесс без следа: у сборки для Windows нет консоли.
/// Пишем её в <app_data_dir>/crash.log (последние ~64 КиБ) — с версией, потоком, местом и стеком вызовов.
fn install_crash_log() {
    let log = mf_platform::dirs::app_data_dir().join("crash.log");
    let _ = std::fs::create_dir_all(mf_platform::dirs::app_data_dir());
    // падения в нативном коде (Qt, драйвер видеокарты) — туда же, на Windows и Linux
    qt::ffi::ffi::install_native_crash_log(&cxx_qt_lib::QString::from(log.to_string_lossy().as_ref()));
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let path = mf_platform::dirs::app_data_dir().join("crash.log");
        let thread = std::thread::current();
        let entry = format!(
            "=== {} · Materialfy {} ({}) · thread {}
{info}
{}
",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_secs())
                .unwrap_or_default(),
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            thread.name().unwrap_or("?"),
            std::backtrace::Backtrace::force_capture(),
        );
        let previous = std::fs::read_to_string(&path).unwrap_or_default();
        let keep = previous.len().saturating_sub(64 * 1024);
        let start = previous.char_indices().map(|(i, _)| i).find(|&i| i >= keep).unwrap_or(previous.len());
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(&path, format!("{}{entry}", &previous[start..]));
        default(info);
    }));
}
