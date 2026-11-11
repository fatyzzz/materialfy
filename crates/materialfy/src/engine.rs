//! Запуск приложения — порт main() из app.py. Порядок (важен):
//! 1. env: QSG_USE_SIMPLE_ANIMATION_DRIVER убрать; --debug-wheel → QT_LOGGING_RULES; --screenshot →
//!    QT_QPA_PLATFORM=offscreen, QT_QUICK_BACKEND=rhi, QSG_RHI_BACKEND по умолчанию d3d11 (Windows) / opengl;
//! 2. имена приложения (Materialfy / materialfy / desktop "materialfy") и QApplication (qt::ffi::app_create);
//! 3. главный режим и не --new-instance: single_instance::raise_running_instance() → true — выход 0;
//! 4. иконка окна (assets/materialfy.svg), display::prefer_fastest_screen (до первого окна), шрифты assets/fonts/*.ttf;
//! 5. tracing-subscriber (materialfy: …, уровень WARN; RUST_LOG переопределяет);
//! 6. tokio-рантайм; SettingsStore::open(config_dir); Services::start; context::init;
//! 7. qt::ffi::register_qml_types(); движок: NAM-фабрика с дисковым кэшем (images), import path = каталог qml,
//!    контекстные свойства materialfyDebugWheel, materialfyDebugFps, materialfyTray (= не снимок и не галерея);
//! 8. initial properties (--size → width/height, --set), load(Main.qml | gallery/Gallery.qml | --qml);
//!    нет корня → код 1; не --offline → services.auth.start();
//! 9. у корня есть свойство trayWanted → setQuitOnLastWindowClosed(false); Windows: winframe::install(window);
//!    install_memory_trim (в трее / свёрнуто — сцена и GPU-ресурсы освобождаются);
//!    watch_software_rendering (программный рендер — у окна softwareRendering, декоративные эффекты выключены);
//!    AppRequest::RaiseWindow → show/raise/requestActivate, AppRequest::Quit → app_quit;
//! 10. --screenshot: через delay мс grabWindow().save(путь) и выход;
//! 11. exec(); после — services.shutdown().
//!
//! Каталог QML: $MATERIALFY_QML_DIR, иначе <exe>/qml, иначе <repo>/src/materialfy/qml (поиск вверх от exe и cwd);
//! assets — соседний с qml каталог assets.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use cxx_qt_lib::{QQmlApplicationEngine, QString, QUrl, QVariant};
use tokio::sync::broadcast::error::RecvError;
use mf_platform::{SettingKey, SettingsStore};
use sf_services::{AppRequest, Services, ServicesOptions};

use crate::cli::{Cli, InitialValue};
use crate::context::{self, AppContext, StartupFlags};
use crate::qt::QVariantMap;
use crate::qt::ffi::ffi;

fn qstr(text: &str) -> QString {
    QString::from(text)
}

fn qpath(path: &Path) -> QString {
    QString::from(path.to_string_lossy().as_ref())
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// Переменные окружения до создания QApplication и любых потоков.
fn setup_env(cli: &Cli) {
    // SAFETY: вызывается в самом начале main, других потоков ещё нет
    unsafe {
        // Простой драйвер анимаций тикает таймером ~60 Гц вместо vsync: на 144 Гц всё дёргается
        std::env::remove_var("QSG_USE_SIMPLE_ANIMATION_DRIVER");
        if cli.debug_wheel {
            // QT_LOGGING_RULES пользователя глушит console.log
            let rules = std::env::var("QT_LOGGING_RULES").unwrap_or_default();
            std::env::set_var("QT_LOGGING_RULES", format!("{rules};qml.debug=true"));
        }
        if cli.screenshot.is_some() {
            // Снимок без окна на экране; RHI нужен для Shape/MultiEffect — software-бэкенд их не рисует
            // QT_QPA_PLATFORM set by the caller wins (e.g. a bundle without the offscreen plugin)
            if std::env::var_os("QT_QPA_PLATFORM").is_none() {
                std::env::set_var("QT_QPA_PLATFORM", "offscreen");
            }
            std::env::set_var("QT_QUICK_BACKEND", "rhi");
            // Windows: D3D11 работает в offscreen; OpenGL может дать белый экран
            if std::env::var_os("QSG_RHI_BACKEND").is_none() {
                let backend = if cfg!(target_os = "windows") { "d3d11" } else { "opengl" };
                std::env::set_var("QSG_RHI_BACKEND", backend);
            }
        }
    }
}

/// Производитель дискретной видеокарты для настройки PRIME offload (только Linux).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum GpuVendor {
    Nvidia,
    Other,
}

/// Определяет наличие и производителя дискретной видеокарты через sysfs.
/// На Linux признаком наличия нескольких GPU (гибридной графики) служит присутствие двух или более
/// узлов рендеринга DRM (/dev/dri/renderD*). Производитель каждого узла считывается из
/// /sys/class/drm/<node>/device/vendor (например, 0x10de — NVIDIA, 0x1002 — AMD, 0x8086 — Intel).
/// При проприетарном драйвере NVIDIA без nvidia-drm.modeset=1 узел renderD* создаётся только для
/// интегрированной графики; в этом случае о наличии NVIDIA свидетельствует модуль /sys/module/nvidia.
fn discrete_gpu_vendor() -> Option<GpuVendor> {
    #[cfg(target_os = "linux")]
    {
        let entries = std::fs::read_dir("/dev/dri").ok()?;
        let mut render_nodes = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str
                .strip_prefix("renderD")
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
            {
                render_nodes.push(name_str.into_owned());
            }
        }

        let mut vendors = Vec::new();
        for node in &render_nodes {
            let path = format!("/sys/class/drm/{node}/device/vendor");
            if let Ok(content) = std::fs::read_to_string(&path) {
                let hex = content.trim();
                let hex = hex.strip_prefix("0x").or_else(|| hex.strip_prefix("0X")).unwrap_or(hex);
                if let Ok(vendor_id) = u32::from_str_radix(hex, 16) {
                    vendors.push(vendor_id);
                }
            }
        }

        if vendors.contains(&0x10de) {
            Some(GpuVendor::Nvidia)
        } else if vendors.contains(&0x1002) && render_nodes.len() >= 2 {
            Some(GpuVendor::Other)
        } else if Path::new("/sys/module/nvidia").exists() && vendors.iter().any(|&v| v != 0x10de) {
            Some(GpuVendor::Nvidia)
        } else {
            None
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Применяет переменные PRIME offload для указанного производителя дискретного GPU.
///
/// # Safety
/// Должно вызываться строго ДО старта сторонних потоков.
unsafe fn apply_discrete_gpu(vendor: GpuVendor) {
    // SAFETY: вызывается в самом начале процесса в GUI-потоке до старта сторонних потоков
    unsafe {
        match vendor {
            GpuVendor::Nvidia => {
                // Перенаправляем рендеринг на дискретный GPU NVIDIA (PRIME offload)
                std::env::set_var("__NV_PRIME_RENDER_OFFLOAD", "1");
                std::env::set_var("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
                std::env::set_var("__VK_LAYER_NV_optimus", "NVIDIA_only");
                // Wayland: Qt берёт контекст через EGL, а не GLX — вендор EGL тоже NVIDIA
                for json in ["/usr/share/glvnd/egl_vendor.d/10_nvidia.json", "/etc/glvnd/egl_vendor.d/10_nvidia.json"] {
                    if Path::new(json).exists() {
                        std::env::set_var("__EGL_VENDOR_LIBRARY_FILENAMES", json);
                        break;
                    }
                }
                std::env::set_var("DRI_PRIME", "1");
            }
            GpuVendor::Other => {
                // Для AMD и других дискретных видеокарт достаточно offload через Mesa
                std::env::set_var("DRI_PRIME", "1");
            }
        }
    }
}

/// Применяет предпочтение графического процессора через переменные окружения.
///
/// Должно вызываться строго ДО создания QApplication и графического контекста Qt (EGL/GLX/Vulkan),
/// иначе загрузчики libglvnd / Mesa / драйвер NVIDIA зафиксируют устройство по умолчанию для процесса.
fn apply_gpu_preference(settings: &SettingsStore) {
    let pref = settings.get_str(SettingKey::GpuPreference);
    match pref.as_str() {
        "integrated" => {
            // SAFETY: вызывается в самом начале процесса в GUI-потоке до старта сторонних потоков
            unsafe {
                // Принудительно отключаем PRIME offload и заставляем использовать интегрированную графику
                std::env::remove_var("__NV_PRIME_RENDER_OFFLOAD");
                std::env::remove_var("__GLX_VENDOR_LIBRARY_NAME");
                std::env::remove_var("__VK_LAYER_NV_optimus");
                std::env::remove_var("DRI_PRIME");
                std::env::remove_var("__EGL_VENDOR_LIBRARY_FILENAMES");
            }
        }
        "discrete" => {
            // Если дискретной видеокарты нет или чтение sysfs не удалось — предупреждаем в лог
            let Some(vendor) = discrete_gpu_vendor() else {
                eprintln!("materialfy: gpu preference is 'discrete' but no dedicated GPU was detected");
                return;
            };
            // SAFETY: вызывается в самом начале процесса в GUI-потоке до старта сторонних потоков
            unsafe {
                apply_discrete_gpu(vendor);
            }
        }
        _ => {
            // "auto" — как решит система: музыкальному плееру незачем будить дискретную видеокарту
            // (батарея, вентиляторы); кто упирается в драйвер встроенной — выбирает «Дискретная» сам
        }
    }
}

/// Масштаб интерфейса из настроек: QT_SCALE_FACTOR читается QGuiApplication при создании, поэтому
/// переменная выставляется до app_create. Смена настройки применяется после перезапуска приложения.
fn apply_ui_scale(settings: &SettingsStore) {
    if cfg!(target_os = "android") {
        return;
    }
    const MARKER: &str = "MATERIALFY_UI_SCALE";
    // Перезапуск из самого приложения (обновление, AppImage) наследует переменные прошлого процесса:
    // в этом случае настройка важнее унаследованного значения, иначе новый масштаб не применится.
    let relaunched = std::env::var_os(MARKER).is_some();
    if !relaunched && std::env::var_os("QT_SCALE_FACTOR").is_some_and(|value| !value.is_empty()) {
        return; // явное значение пользователя важнее настройки
    }
    let percent = settings.get_int(SettingKey::UiScale);
    let factor = format!("{:.2}", percent as f64 / 100.0);
    let percent = percent.to_string();
    // Через Qt: на Windows он читает переменные из копии C-рантайма, std::env::set_var её не меняет
    // (см. qml_cache). std::env нужен, чтобы значение унаследовал перезапущенный процесс.
    ffi::set_qt_env(&qstr("QT_SCALE_FACTOR"), &qstr(&factor));
    // SAFETY: вызывается до создания QApplication и сторонних потоков
    unsafe {
        std::env::set_var("QT_SCALE_FACTOR", &factor);
        std::env::set_var(MARKER, &percent);
    }
}

/// Временный каталог настроек для --screenshot.
fn make_temp_dir() -> PathBuf {
    let base = std::env::temp_dir();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    for attempt in 0..100u32 {
        let dir = base.join(format!("materialfy-shot-{:x}{:x}{attempt}", std::process::id(), stamp));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    base
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn load_fonts() {
    let dir = assets_dir().join("fonts");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("no font directory {}", dir.display());
        return;
    };
    let mut fonts: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ttf")))
        .collect();
    fonts.sort();
    for font in fonts {
        if ffi::app_add_font(&qpath(&font)) < 0 {
            let name = font.file_name().unwrap_or_default().to_string_lossy();
            eprintln!("could not load font {name}");
        }
    }
}

/// «materialfy: <модуль>: <сообщение>».
struct LogFormat;

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for LogFormat
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        write!(writer, "materialfy: {}: ", event.metadata().target())?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .event_format(LogFormat)
        .try_init();
}

fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    // librespot (Session/Spirc), актор плеера, опрос Web API и задачи мостов делят этот рантайм
    let workers = std::thread::available_parallelism().map_or(3, |n| n.get()).clamp(3, 6);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .thread_name("materialfy-rt")
        .enable_all()
        .build()
}

fn initial_value(value: &InitialValue) -> QVariant {
    match value {
        InitialValue::Bool(b) => QVariant::from(b),
        InitialValue::Int(i) => match i32::try_from(*i) {
            Ok(small) => QVariant::from(&small),
            Err(_) => QVariant::from(i),
        },
        InitialValue::Float(f) => QVariant::from(f),
        InitialValue::Str(s) => QVariant::from(&qstr(s)),
    }
}

fn initial_properties(cli: &Cli) -> QVariantMap {
    let mut map = QVariantMap::default();
    if let Some((width, height)) = cli.size {
        map.insert(qstr("width"), QVariant::from(&width));
        map.insert(qstr("height"), QVariant::from(&height));
    }
    for (key, value) in &cli.set {
        map.insert(qstr(key), initial_value(value));
    }
    map
}

/// MPRIS Raise/Quit → GUI-поток (C++ сам переносит вызов в него).
/// Windows/macOS: просьбы «показать окно» от новых процессов (на Linux они приходят через MPRIS Raise).
fn spawn_instance_listener(rt: &tokio::runtime::Handle) {
    rt.spawn(async move {
        let mut listener = match mf_platform::single_instance::InstanceListener::start().await {
            Ok(listener) => listener,
            Err(e) => {
                tracing::debug!("single-instance listener not started: {e}");
                return;
            }
        };
        while listener.recv().await.is_some() {
            ffi::post_raise_window();
        }
    });
}

fn spawn_app_requests(rt: &tokio::runtime::Handle, services: &Services) {
    let mut requests = services.app_requests();
    rt.spawn(async move {
        loop {
            match requests.recv().await {
                Ok(AppRequest::RaiseWindow) => ffi::post_raise_window(),
                Ok(AppRequest::Quit) => ffi::post_quit(),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// Код выхода процесса.
pub fn run(cli: Cli) -> i32 {
    setup_env(&cli);
    let qml_guard = crate::qml_cache::prepare(cli.screenshot.is_none());
    let config_dir = cli.config_dir.clone().or_else(|| cli.screenshot.as_ref().map(|_| make_temp_dir()));
    let settings = SettingsStore::open(config_dir);
    apply_gpu_preference(&settings);
    apply_ui_scale(&settings);

    ffi::app_set_names(&qstr("Materialfy"), &qstr("materialfy"), &qstr("materialfy"));
    let args: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    ffi::app_create(&args);
    if ffi::is_android() {
        prepare_android();
    }

    let rt = match build_runtime() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("materialfy: could not start tokio: {e}");
            return 1;
        }
    };

    // Плеер уже работает (возможно, спрятан в трей или ещё запускается) — показываем его окно вместо второго
    // экземпляра. --new-instance — для разработки и снимков
    if cli.main_mode() && !cli.new_instance && !rt.block_on(mf_platform::single_instance::become_primary()) {
        return 0;
    }
    if let Some(guard) = qml_guard {
        guard.arm(rt.handle());
    }

    // иконка окна; на Windows ярлыки и панель задач берут ещё и ресурс IDI_ICON1 из exe (build.rs)
    let icon = assets_dir().join("materialfy.svg");
    if icon.is_file() {
        ffi::app_set_window_icon(&qpath(&icon));
    }
    // До создания окон: Qt Quick берёт период кадра у основного экрана (см. display.rs)
    let screen_note = crate::display::prefer_fastest_screen();
    if cli.debug_fps {
        eprintln!("{screen_note}");
    }
    load_fonts();
    init_tracing();

    mf_i18n::set_phone(cli.mobile);
    // UI language before any service or QML produces text (default: the OS UI language — Russian or English)
    crate::bridges::translations::apply_language(crate::bridges::translations::effective_language(&settings));
    let options = ServicesOptions {
        // Снимки — без сессии, кроме явного файла для тестов
        persist_session: cli.screenshot.is_none() || env_set("MATERIALFY_SESSION_FILE"),
        enable_media: cli.screenshot.is_none(),
    };
    let services = match Services::start(rt.handle(), settings, options) {
        Ok(services) => services,
        Err(e) => {
            eprintln!("materialfy: {e:#}");
            return 1;
        }
    };

    context::init(AppContext {
        rt: rt.handle().clone(),
        services: services.clone(),
        flags: StartupFlags {
            screenshot: cli.screenshot.is_some(),
            light: cli.light,
            seed_image: cli.seed_image.as_deref().map(absolute),
        },
        system_dark: crate::qt::system_dark_watch(),
    });
    // Активность окна → частота опроса удалённого плеера
    crate::qt::watch_app_state();
    crate::qt::ffi::ffi::start_gui_heartbeat();
    // Все синглтоны — до создания движка: регистрация после загрузки модуля ломает разрешение типов
    ffi::register_qml_types();

    let code = run_engine(&cli, &rt, &services);
    services.shutdown();
    rt.shutdown_timeout(Duration::from_secs(1));
    ffi::app_destroy();
    code
}

fn run_engine(cli: &Cli, rt: &tokio::runtime::Runtime, services: &Services) -> i32 {
    let mut engine = QQmlApplicationEngine::new();
    let Some(mut e) = engine.as_mut() else {
        eprintln!("materialfy: could not create QQmlApplicationEngine");
        return 1;
    };
    crate::images::install_caching_nam(e.as_mut(), rt.handle(), services);
    e.as_mut().add_import_path(&qpath(&qml_dir()));
    ffi::engine_set_context_bool(e.as_mut(), &qstr("materialfyDebugWheel"), cli.debug_wheel);
    ffi::engine_set_context_bool(e.as_mut(), &qstr("materialfyDebugFps"), cli.debug_fps);
    // Значок в трее: не для снимков и не для галереи
    ffi::engine_set_context_bool(
        e.as_mut(),
        &qstr("materialfyTray"),
        cli.screenshot.is_none() && !cli.gallery && !cli.mobile,
    );
    // Раскладка телефона: нижняя навигация, плеер-шторка, крупные цели касания (Theme.mobile)
    ffi::engine_set_context_bool(e.as_mut(), &qstr("materialfyMobile"), cli.mobile);

    let entry = match &cli.qml {
        Some(file) => absolute(file),
        None => qml_dir().join(if cli.gallery {
            "gallery/Gallery.qml"
        } else if cli.mobile {
            "MobileMain.qml"
        } else {
            "Main.qml"
        }),
    };
    e.as_mut().set_initial_properties(&initial_properties(cli));
    e.as_mut().load(&QUrl::from_local_file(&qpath(&entry)));

    let root = ffi::engine_root_object(&e);
    if root.is_null() {
        let msg = format!("QQmlApplicationEngine failed to load root component from {}\n", entry.display());
        let path = mf_platform::dirs::app_data_dir().join("crash.log");
        let _ = std::fs::create_dir_all(mf_platform::dirs::app_data_dir());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            use std::io::Write as _;
            let _ = f.write_all(msg.as_bytes());
        }
        eprintln!("{msg}");
        return 1;
    }
    if !cli.offline {
        services.auth.start();
    }

    // SAFETY: root — живой корневой объект движка, всё в GUI-потоке
    unsafe {
        if ffi::object_has_property(root, &qstr("trayWanted")) {
            // Главное окно может прятаться в трей — выход решает оно само (Main.qml, onClosing), а не Qt
            ffi::app_set_quit_on_last_window_closed(false);
        }
        ffi::set_main_window(root);
    }
    // SAFETY: root — живое окно движка, GUI-поток
    unsafe { ffi::watch_software_rendering(root) };
    if cli.screenshot.is_none() && !ffi::is_android() {
        crate::winframe::install(root);
        // SAFETY: root — живое окно движка, GUI-поток
        unsafe { ffi::install_memory_trim(root) };
    }
    if cli.screenshot.is_none() && ffi::is_android() {
        // Android: в фоне — мусор JS, кэши и свободная куча системе (без снятия сцены, см. install_memory_trim)
        // SAFETY: root — живое окно движка, GUI-поток
        unsafe { ffi::install_memory_trim(root) };
    }
    spawn_app_requests(rt.handle(), services);
    if cli.main_mode() && cfg!(any(target_os = "windows", target_os = "macos")) {
        spawn_instance_listener(rt.handle());
    }

    if let Some(path) = &cli.screenshot {
        let delay = i32::try_from(cli.delay).unwrap_or(i32::MAX);
        // SAFETY: root жив, пока жив движок; таймер держит QPointer
        unsafe { ffi::schedule_screenshot(root, &qpath(path), delay) };
    }

    let code = ffi::app_exec();
    drop(engine);
    code
}

/// Android, сразу после QApplication: QML и ассеты из APK — в каталог приложения (std::fs и движок QML читают
/// обычные файлы), JNI-контекст для cpal (AAudio), браузер для входа — через Intent.
fn prepare_android() {
    let qml = ffi::android_prepare_assets(&qstr(env!("CARGO_PKG_VERSION"))).to_string();
    if qml.is_empty() {
        eprintln!("materialfy: could not extract the QML bundle from the APK");
    } else {
        // SAFETY: до запуска рантайма tokio и до первого qml_dir(); других потоков Rust, читающих окружение, нет
        unsafe { std::env::set_var("MATERIALFY_QML_DIR", &qml) };
    }
    // на других ОС функции есть (C++ собирается везде), но не нужны
    #[cfg(not(target_os = "android"))]
    let _ = (ffi::android_java_vm, ffi::android_context, ffi::android_install_apk);
    #[cfg(target_os = "android")]
    {
        let (vm, context) = (ffi::android_java_vm(), ffi::android_context());
        if vm != 0 && context != 0 {
            // SAFETY: JavaVM* процесса и глобальная ссылка на Context живут до конца процесса
            unsafe { ndk_context::initialize_android_context(vm as *mut _, context as *mut _) };
        }
    }
    sf_services::set_browser_opener(|url| {
        ffi::open_url_external(&qstr(url));
        Ok(())
    });
    sf_services::update::install::set_apk_installer(|file| {
        match ffi::android_install_apk(&qpath(file)).to_string().as_str() {
            "" => Ok(()),
            "permission" => Err(mf_i18n::t("Allow Materialfy to install apps in the settings that opened, then press Install again")),
            other => Err(other.to_owned()),
        }
    });
}

fn find_qml_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MATERIALFY_QML_DIR").filter(|v| !v.is_empty()) {
        return absolute(Path::new(&dir));
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    if let Some(exe_dir) = &exe_dir {
        let packaged = exe_dir.join("qml");
        if packaged.join("Main.qml").is_file() {
            return packaged;
        }
    }
    let cwd = std::env::current_dir().ok();
    for start in exe_dir.iter().chain(cwd.iter()) {
        for dir in start.ancestors() {
            let candidate = dir.join("src").join("materialfy").join("qml");
            if candidate.join("Main.qml").is_file() {
                return candidate;
            }
        }
    }
    absolute(Path::new("src/materialfy/qml"))
}

/// Каталог src/materialfy/qml (см. шапку).
pub fn qml_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(find_qml_dir).clone()
}

/// Каталог src/materialfy/assets.
pub fn assets_dir() -> PathBuf {
    let qml = qml_dir();
    qml.parent().map_or_else(|| PathBuf::from("assets"), |p| p.join("assets"))
}
