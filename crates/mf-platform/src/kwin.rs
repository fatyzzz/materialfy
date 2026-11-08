//! KDE Plasma (Wayland): окно не может встать у края экрана само, а layer-shell (LayerShellQt) делает слоями
//! все окна процесса, и главное перестаёт двигаться. Поэтому островок остаётся обычным окном, а на место его ставит
//! скрипт KWin, который мы загружаем по D-Bus: позиция, поверх всех, вне панели задач и переключателя окон.
//! Вне Linux и вне KDE — ничего не делает.
#[cfg(target_os = "linux")]
mod imp {
    use crate::PlatformError;

    const SERVICE: &str = "org.kde.KWin";
    const SCRIPTING: &str = "/Scripting";
    const SCRIPTING_IFACE: &str = "org.kde.kwin.Scripting";

    pub fn available() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        desktop.split(':').any(|part| part.eq_ignore_ascii_case("KDE"))
            && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
    }

    // имя скрипта и заголовок окна — только буквы, цифры и пробелы: идут в имя файла и в текст скрипта
    fn script_name(title: &str) -> String {
        title.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
    }

    fn script_text(title: &str, x: i32, y: i32, width: i32, height: i32) -> String {
        format!(
            "const title = \"{title}\";\n\
             function place(window) {{\n\
             \x20   if (window.caption !== title) return;\n\
             \x20   window.keepAbove = true;\n\
             \x20   window.skipTaskbar = true;\n\
             \x20   window.skipPager = true;\n\
             \x20   window.skipSwitcher = true;\n\
             \x20   window.noBorder = true;\n\
             \x20   window.frameGeometry = {{ x: {x}, y: {y}, width: {width}, height: {height} }};\n\
             }}\n\
             workspace.windowList().forEach(place);\n\
             workspace.windowAdded.connect(place);\n"
        )
    }

    fn dbus_err(err: impl std::fmt::Display) -> PlatformError {
        PlatformError::DBus(err.to_string())
    }

    pub async fn place(title: &str, x: i32, y: i32, width: i32, height: i32) -> Result<(), PlatformError> {
        let name = script_name(title);
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("materialfy-{name}.js"));
        std::fs::write(&path, script_text(title, x, y, width, height))?;
        let conn = zbus::Connection::session().await.map_err(dbus_err)?;
        // тот же заголовок мог остаться от прошлого запуска или прошлого размера
        let _ = conn.call_method(Some(SERVICE), SCRIPTING, Some(SCRIPTING_IFACE), "unloadScript", &(name.as_str(),)).await;
        let reply = conn
            .call_method(Some(SERVICE), SCRIPTING, Some(SCRIPTING_IFACE), "loadScript", &(path.to_string_lossy().as_ref(), name.as_str()))
            .await
            .map_err(dbus_err)?;
        let id: i32 = reply.body().deserialize().map_err(dbus_err)?;
        if id < 0 {
            return Err(PlatformError::DBus(format!("KWin did not load {}", path.display())));
        }
        conn.call_method(Some(SERVICE), format!("/Scripting/Script{id}"), Some("org.kde.kwin.Script"), "run", &())
            .await
            .map_err(dbus_err)?;
        Ok(())
    }

    pub async fn release(title: &str) {
        let name = script_name(title);
        if let Ok(conn) = zbus::Connection::session().await {
            let _ = conn.call_method(Some(SERVICE), SCRIPTING, Some(SCRIPTING_IFACE), "unloadScript", &(name.as_str(),)).await;
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use crate::PlatformError;

    pub fn available() -> bool {
        false
    }

    pub async fn place(_title: &str, _x: i32, _y: i32, _width: i32, _height: i32) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    pub async fn release(_title: &str) {}
}

/// Сеанс KDE Plasma на Wayland: островок можно поставить скриптом KWin
pub fn available() -> bool {
    imp::available()
}

/// Загрузить (или перезагрузить) скрипт, который ставит окно с заголовком title в (x, y, width, height)
/// логических пикселей рабочего стола
pub async fn place(title: &str, x: i32, y: i32, width: i32, height: i32) -> Result<(), crate::PlatformError> {
    imp::place(title, x, y, width, height).await
}

/// Выгрузить скрипт окна (при закрытии островка и выходе)
pub async fn release(title: &str) {
    imp::release(title).await
}
