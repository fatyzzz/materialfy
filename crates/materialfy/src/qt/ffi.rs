// Привязки к cpp/mf_shell.h и cpp/winframe.h. Владелец — агент «shell»; мосты пользуются обёртками из crate::qt.
#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qrectf.h");
        type QRectF = cxx_qt_lib::QRectF;
        include!("cxx-qt-lib/qqmlapplicationengine.h");
        type QQmlApplicationEngine = cxx_qt_lib::QQmlApplicationEngine;
        include!(<QtCore/QObject>);
        type QObject = cxx_qt::QObject;
    }

    #[namespace = "mf"]
    unsafe extern "C++" {
        include!("mf_shell.h");

        fn app_create(args: &[String]);
        fn app_exec() -> i32;
        fn app_destroy();
        fn app_set_names(app: &QString, org: &QString, desktop: &QString);
        fn set_qt_env(name: &QString, value: &QString);
        fn install_native_crash_log(path: &QString);
        fn app_set_quit_on_last_window_closed(value: bool);
        fn app_set_window_icon(path: &QString);
        fn app_add_font(path: &QString) -> i32;
        /// "en" — no translator; otherwise qtbase_<code>.qm (Qt's own dialogs and menus) + QLocale default
        fn app_set_qt_language(code: &QString);
        /// OS UI languages, most preferred first ("ru-RU", "en-US"…); QLocale::setDefault does not change them
        fn system_ui_languages() -> QStringList;

        fn copy_to_clipboard(text: &QString);
        unsafe fn set_cpp_ownership(object: *mut QObject);
        fn system_color_scheme_dark() -> bool;
        fn watch_color_scheme();
        fn watch_app_state();
        fn start_gui_heartbeat();
        fn open_local_folder(path: &QString) -> bool;

        fn engine_set_context_bool(engine: Pin<&mut QQmlApplicationEngine>, name: &QString, value: bool);
        fn engine_root_object(engine: &QQmlApplicationEngine) -> *mut QObject;
        fn engine_install_caching_nam(engine: Pin<&mut QQmlApplicationEngine>, dir: &QString, max_bytes: i64);
        fn set_image_proxy(mode: i32, host: &QString, port: i32, user: &QString, password: &QString);

        unsafe fn object_has_property(object: *mut QObject, name: &QString) -> bool;
        unsafe fn set_main_window(window: *mut QObject);
        fn post_raise_window();
        fn post_quit();
        unsafe fn schedule_screenshot(window: *mut QObject, path: &QString, delay_ms: i32);
        unsafe fn install_memory_trim(window: *mut QObject);
        unsafe fn watch_software_rendering(window: *mut QObject);

        fn prefer_fastest_screen() -> QString;
        fn icon_measure(text: &QString, size: i32) -> QRectF;

        include!("winframe.h");
        unsafe fn winframe_install(window: *mut QObject, grip: i32) -> bool;

        include!("mobile.h");
        fn is_android() -> bool;
        fn android_prepare_assets(version: &QString) -> QString;
        fn android_java_vm() -> usize;
        fn android_context() -> usize;
        fn open_url_external(url: &QString);
        fn android_install_apk(path: &QString) -> QString;

        include!("register.h");
        fn register_qml_types();
    }

    #[namespace = "mf"]
    extern "Rust" {
        /// QStyleHints::colorSchemeChanged (cpp/mf_shell.cpp, watch_color_scheme)
        fn system_dark_changed(dark: bool);
        /// QGuiApplication::applicationStateChanged (cpp/mf_shell.cpp, watch_app_state)
        fn app_active_changed(active: bool);
        /// таймер GUI-потока (cpp/mf_shell.cpp, start_gui_heartbeat)
        fn gui_heartbeat();
    }
}

fn gui_heartbeat() {
    mf_platform::single_instance::gui_alive();
}

fn system_dark_changed(dark: bool) {
    super::set_system_dark(dark);
}

fn app_active_changed(active: bool) {
    if let Some(context) = crate::context::try_ctx() {
        context.services.player.set_app_active(active);
    }
}
