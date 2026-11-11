// Materialfy — клиент Spotify: QML (src/materialfy/qml) + бэкенд на Rust (cxx-qt, librespot).
// Оболочка: cli, context, display, engine, icons, images, models, qt, winframe; мосты QML — bridges.
// релиз на Windows — GUI-подсистема, без чёрного окна консоли; отладочная сборка оставляет консоль для логов
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// Android: бинарник собирается общей библиотекой (build.rs), QtActivity находит в ней C-функцию main
#![cfg_attr(target_os = "android", no_main)]

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

#[cfg(not(target_os = "android"))]
fn main() {
    install_crash_log();
    let args: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    let code = engine::run(cli::Cli::parse(&args));
    std::process::exit(code);
}

/// Точка входа на Android: её зовёт QtActivity (QtLoader → dlsym "main") в своём потоке после загрузки библиотеки.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn main(argc: std::ffi::c_int, argv: *const *const std::ffi::c_char) -> std::ffi::c_int {
    android::prepare_env();
    install_crash_log();
    let mut args = vec!["materialfy".to_owned()];
    for i in 1..usize::try_from(argc).unwrap_or(0) {
        // SAFETY: argv — argc строк от QtLoader, живут до конца main
        let arg = unsafe { std::ffi::CStr::from_ptr(*argv.add(i)) };
        args.push(arg.to_string_lossy().into_owned());
    }
    let mut cli = cli::Cli::parse(&args);
    cli.mobile = true;
    // один процесс на приложение — Android сам не даст запустить второй
    cli.new_instance = true;
    engine::run(cli)
}

#[cfg(target_os = "android")]
mod android {
    /// До всего остального: каталоги XDG внутри песочницы приложения (crate dirs и mf_platform::dirs читают их)
    /// и логи в logcat.
    pub fn prepare_env() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/data/local/tmp".to_owned());
        let cache = std::env::var("TMPDIR").unwrap_or_else(|_| format!("{home}/cache"));
        // имя в Spotify Connect по умолчанию — «Materialfy · <модель телефона>» (sf_services::Devices)
        let model = system_property(c"ro.product.model");
        // SAFETY: самое начало main, других потоков Rust ещё нет
        unsafe {
            if std::env::var_os("HOSTNAME").is_none() && !model.is_empty() {
                std::env::set_var("HOSTNAME", &model);
            }
            std::env::set_var("XDG_CONFIG_HOME", format!("{home}/config"));
            std::env::set_var("XDG_DATA_HOME", format!("{home}/data"));
            std::env::set_var("XDG_STATE_HOME", format!("{home}/state"));
            // temp_dir() без TMPDIR на Android — /data/local/tmp, приложению туда не писать (обычно Qt его задаёт)
            if std::env::var_os("TMPDIR").is_none() {
                std::env::set_var("TMPDIR", &cache);
            }
            std::env::set_var("XDG_CACHE_HOME", cache);
            // журнал — для отчёта об ошибке без adb: свои крейты подробнее (info), сторонние — как на компьютере
            if std::env::var_os("RUST_LOG").is_none() {
                std::env::set_var(
                    "RUST_LOG",
                    "warn,materialfy=info,sf_services=info,sf_player=info,sf_auth=info,sf_api=info,mf_lyrics=info",
                );
            }
            // журнал для «Поделиться журналом» в настройках (MobileShell::shareLog читает его же)
            std::env::set_var("MATERIALFY_LOG_FILE", format!("{home}/materialfy.log"));
            // корневые сертификаты: rustls-native-certs (reqwest, librespot) ищет их в /etc/ssl и т. п., а на
            // Android они в своём каталоге — без него любой HTTPS падает, и вход висит на «Входим…».
            // Android 14+ обновляет их через модуль conscrypt, раньше — только /system.
            if std::env::var_os("SSL_CERT_DIR").is_none()
                && let Some(dir) = ["/apex/com.android.conscrypt/cacerts", "/system/etc/security/cacerts"]
                    .into_iter()
                    .find(|dir| std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some()))
            {
                std::env::set_var("SSL_CERT_DIR", dir);
            }
        }
        redirect_stderr_to_logcat();
    }

    unsafe extern "C" {
        fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> std::ffi::c_int;
    }

    /// Системное свойство Android (ro.product.model и т. п.); пустая строка — нет такого.
    fn system_property(name: &std::ffi::CStr) -> String {
        // PROP_VALUE_MAX = 92
        let mut buf = [0 as std::ffi::c_char; 92];
        // SAFETY: буф PROP_VALUE_MAX байт, имя — C-строка
        let len = unsafe { __system_property_get(name.as_ptr(), buf.as_mut_ptr()) };
        if len <= 0 {
            return String::new();
        }
        // SAFETY: __system_property_get пишет строку с нулём в конце
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().trim().to_owned()
    }

    unsafe extern "C" {
        fn __android_log_write(prio: std::ffi::c_int, tag: *const std::ffi::c_char, text: *const std::ffi::c_char)
        -> std::ffi::c_int;
    }

    /// Журнал больше 1 МБ при старте уходит в .old: в файле — этот запуск и предыдущий
    const LOG_ROTATE_BYTES: u64 = 1 << 20;

    /// eprintln! и tracing пишут в stderr, которого на Android никто не читает: труба → logcat (тег materialfy)
    /// и файл журнала (MATERIALFY_LOG_FILE) — его пользователь отправляет из настроек без adb. Сообщения Qt и QML
    /// сюда же направляет обработчик из cpp/mobile.cpp.
    fn redirect_stderr_to_logcat() {
        use std::io::BufRead as _;
        use std::os::fd::FromRawFd as _;
        let mut fds = [0 as std::ffi::c_int; 2];
        // SAFETY: обычные pipe/dup2 над своими дескрипторами
        unsafe {
            if pipe(fds.as_mut_ptr()) != 0 {
                return;
            }
            dup2(fds[1], 2);
        }
        // SAFETY: fds[0] — только что созданный конец трубы, владеет им поток ниже
        let reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        let mut file = std::env::var_os("MATERIALFY_LOG_FILE").and_then(|path| {
            let path = std::path::PathBuf::from(path);
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
                let _ = std::fs::rename(&path, path.with_extension("log.old"));
            }
            std::fs::OpenOptions::new().create(true).append(true).open(&path).ok()
        });
        if let Some(file) = file.as_mut() {
            use std::io::Write as _;
            let _ = writeln!(file, "---- Materialfy {} started ----", env!("CARGO_PKG_VERSION"));
        }
        let _ = std::thread::Builder::new().name("logcat".into()).spawn(move || {
            use std::io::Write as _;
            let tag = c"materialfy";
            for line in std::io::BufReader::new(reader).lines().map_while(Result::ok) {
                if let Some(file) = file.as_mut() {
                    let _ = writeln!(file, "{line}");
                }
                if let Ok(text) = std::ffi::CString::new(line) {
                    // 5 — ANDROID_LOG_WARN
                    // SAFETY: строки C с нулём в конце
                    unsafe { __android_log_write(5, tag.as_ptr(), text.as_ptr()) };
                }
            }
        });
    }

    unsafe extern "C" {
        fn pipe(fds: *mut std::ffi::c_int) -> std::ffi::c_int;
        fn dup2(old: std::ffi::c_int, new: std::ffi::c_int) -> std::ffi::c_int;
    }
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
