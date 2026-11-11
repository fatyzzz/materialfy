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
            // скрипт на API KWin 6 (windowList, output, desktops): Plasma 5 его не выполнит
            && std::env::var("KDE_SESSION_VERSION").as_deref() == Ok("6")
    }

    // имя скрипта и заголовок окна — только буквы, цифры и пробелы: идут в имя файла и в текст скрипта
    fn script_name(title: &str) -> String {
        title.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
    }

    fn script_text(title: &str, spot: &super::Spot, report: &str) -> String {
        // заголовок идёт в текст скрипта: оставляем только буквы, цифры и пробелы
        let title: String = title.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').collect();
        let title = title.as_str();
        let super::Spot { x, y, width, height, hide_fullscreen, hide_maximized, move_x, move_y } = spot;
        // Прячем островок (minimized), пока на его мониторе и рабочем столе есть окно на весь экран или развёрнутое.
        // Окно самого приложения (тот же pid) тоже прячет островок: на Wayland клиент не знает, что его окно свернули.
        // Развёрнутым считаем окно, занявшее всю рабочую область (без панелей): у KWin нет одного свойства на это.
        // Тащат мышью (startSystemMove) — двигает сам KWin; мы держим окно на своей оси и в пределах монитора,
        // а куда отпустили — сообщаем приложению по D-Bus (REPORT_PATH). Держим и по frameGeometryChanged: пока окно
        // перерисовывается, KWin на каждом кадре клиента возвращает ему своё место без оси
        format!(
            "const title = \"{title}\";\n\
             const hideFullscreen = {hide_fullscreen};\n\
             const hideMaximized = {hide_maximized};\n\
             const moveX = {move_x};\n\
             const moveY = {move_y};\n\
             function clamp(v, lo, hi) {{ return Math.max(lo, Math.min(hi, v)); }}\n\
             function follow(window) {{\n\
             \x20   let area = null;\n\
             \x20   window.interactiveMoveResizeStarted.connect(() => {{ area = window.output.geometry; }});\n\
             \x20   let busy = false;\n\
             \x20   function keep() {{\n\
             \x20       if (!area || busy) return;\n\
             \x20       const g = window.frameGeometry;\n\
             \x20       const nx = moveX ? clamp(g.x, area.x, area.x + area.width - g.width) : {x};\n\
             \x20       const ny = moveY ? clamp(g.y, area.y, area.y + area.height - g.height) : {y};\n\
             \x20       if (Math.abs(nx - g.x) < 1 && Math.abs(ny - g.y) < 1) return;\n\
             \x20       busy = true;\n\
             \x20       window.frameGeometry = {{ x: nx, y: ny, width: g.width, height: g.height }};\n\
             \x20       busy = false;\n\
             \x20   }}\n\
             \x20   window.interactiveMoveResizeStepped.connect(keep);\n\
             \x20   window.frameGeometryChanged.connect(keep);\n\
             \x20   window.interactiveMoveResizeFinished.connect(() => {{\n\
             \x20       area = null;\n\
             \x20       const g = window.frameGeometry;\n\
             \x20       callDBus(\"{report}\", \"{REPORT_PATH}\", \"{REPORT_IFACE}\", \"Moved\", title, Math.round(g.x) | 0, Math.round(g.y) | 0);\n\
             \x20   }});\n\
             }}\n\
             function island() {{ return workspace.windowList().find(w => w.caption === title); }}\n\
             function covers(w, isl) {{\n\
             \x20   if (w === isl || w.caption.startsWith(\"Materialfy Island\") || !w.normalWindow || w.minimized) return false;\n\
             \x20   if (w.output !== isl.output) return false;\n\
             \x20   if (!w.onAllDesktops && !w.desktops.includes(workspace.currentDesktop)) return false;\n\
             \x20   if (w.pid === isl.pid) return w.active;\n\
             \x20   if (hideFullscreen && w.fullScreen) return true;\n\
             \x20   if (!hideMaximized) return false;\n\
             \x20   const area = workspace.clientArea(KWin.MaximizeArea, w);\n\
             \x20   return w.frameGeometry.width >= area.width && w.frameGeometry.height >= area.height;\n\
             }}\n\
             function update() {{\n\
             \x20   const isl = island();\n\
             \x20   if (!isl) return;\n\
             \x20   const hidden = workspace.windowList().some(w => covers(w, isl));\n\
             \x20   if (isl.minimized !== hidden) isl.minimized = hidden;\n\
             }}\n\
             function place(window) {{\n\
             \x20   if (window.caption === title) {{\n\
             \x20       window.keepAbove = true;\n\
             \x20       window.skipTaskbar = true;\n\
             \x20       window.skipPager = true;\n\
             \x20       window.skipSwitcher = true;\n\
             \x20       window.noBorder = true;\n\
             \x20       window.frameGeometry = {{ x: {x}, y: {y}, width: {width}, height: {height} }};\n\
             \x20       follow(window);\n\
             \x20   }}\n\
             \x20   window.fullScreenChanged.connect(update);\n\
             \x20   window.minimizedChanged.connect(update);\n\
             \x20   window.frameGeometryChanged.connect(update);\n\
             \x20   window.outputChanged.connect(update);\n\
             \x20   window.desktopsChanged.connect(update);\n\
             }}\n\
             workspace.windowList().forEach(place);\n\
             workspace.windowAdded.connect(w => {{ place(w); update(); }});\n\
             workspace.windowRemoved.connect(update);\n\
             workspace.currentDesktopChanged.connect(update);\n\
             workspace.windowActivated.connect(update);\n\
             update();\n"
        )
    }

    fn dbus_err(err: impl std::fmt::Display) -> PlatformError {
        PlatformError::DBus(err.to_string())
    }

    const REPORT_PATH: &str = "/MaterialfyIsland";
    const REPORT_IFACE: &str = "org.materialfy.Island";

    type MovedFn = Box<dyn Fn(String, i32, i32) + Send + Sync>;
    static ON_MOVED: std::sync::OnceLock<MovedFn> = std::sync::OnceLock::new();
    static CONNECTION: tokio::sync::OnceCell<zbus::Connection> = tokio::sync::OnceCell::const_new();

    pub fn on_moved(callback: MovedFn) {
        let _ = ON_MOVED.set(callback);
    }

    // сюда скрипт KWin сообщает, куда окно отпустили после перетаскивания
    struct Reports;

    #[zbus::interface(name = "org.materialfy.Island")]
    impl Reports {
        fn moved(&self, title: String, x: i32, y: i32) {
            if let Some(callback) = ON_MOVED.get() {
                callback(title, x, y);
            }
        }
    }

    // одно соединение на весь запуск: на нём же объект для отчётов скрипта, его уникальное имя — в тексте скрипта
    async fn connection() -> Result<&'static zbus::Connection, PlatformError> {
        CONNECTION
            .get_or_try_init(|| async {
                zbus::connection::Builder::session()
                    .map_err(dbus_err)?
                    .serve_at(REPORT_PATH, Reports)
                    .map_err(dbus_err)?
                    .build()
                    .await
                    .map_err(dbus_err)
            })
            .await
    }

    // place и release одного окна не должны идти вперемешку: выгрузка одного залпа гасит загрузку другого
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    pub async fn place(title: &str, spot: &super::Spot) -> Result<(), PlatformError> {
        let _guard = LOCK.lock().await;
        let name = script_name(title);
        let conn = connection().await?;
        let report = conn.unique_name().map(|n| n.to_string()).unwrap_or_default();
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("materialfy-{name}.js"));
        std::fs::write(&path, script_text(title, spot, &report))?;
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
        let _guard = LOCK.lock().await;
        let name = script_name(title);
        if let Ok(conn) = zbus::Connection::session().await {
            let _ = conn.call_method(Some(SERVICE), SCRIPTING, Some(SCRIPTING_IFACE), "unloadScript", &(name.as_str(),)).await;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn script_locks_the_edge_axis_and_reports_the_drop() {
            let spot = super::super::Spot { x: 10, y: 0, width: 400, height: 200, hide_fullscreen: true, hide_maximized: false, move_x: true, move_y: false };
            let text = script_text("Materialfy Island 0", &spot, ":1.42");
            assert!(text.contains("const moveX = true;\nconst moveY = false;"));
            // поперёк края окно стоит там, куда его поставили
            assert!(text.contains("moveY ? clamp(g.y, area.y, area.y + area.height - g.height) : 0;"));
            assert!(text.contains("callDBus(\":1.42\", \"/MaterialfyIsland\", \"org.materialfy.Island\", \"Moved\""));
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use crate::PlatformError;

    pub fn available() -> bool {
        false
    }

    pub async fn place(_title: &str, _spot: &super::Spot) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    pub async fn release(_title: &str) {}

    pub fn on_moved(_callback: Box<dyn Fn(String, i32, i32) + Send + Sync>) {}
}

/// Сеанс KDE Plasma на Wayland: островок можно поставить скриптом KWin
pub fn available() -> bool {
    imp::available()
}

/// Где окно и когда его прятать: (x, y, width, height) — логические пиксели рабочего стола
pub struct Spot {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// спрятать под приложением на весь экран на том же мониторе
    pub hide_fullscreen: bool,
    /// спрятать под развёрнутым окном на том же мониторе
    pub hide_maximized: bool,
    /// куда окно можно тащить мышью: у края — только вдоль него, свободное — по обеим осям
    pub move_x: bool,
    pub move_y: bool,
}

/// Загрузить (или перезагрузить) скрипт, который ставит окно с заголовком title на место spot
pub async fn place(title: &str, spot: Spot) -> Result<(), crate::PlatformError> {
    imp::place(title, &spot).await
}

/// Окно с заголовком (title) перетащили мышью и отпустили в (x, y) логических px рабочего стола.
/// Вызывается из потока D-Bus; задаётся один раз
pub fn on_moved(callback: Box<dyn Fn(String, i32, i32) + Send + Sync>) {
    imp::on_moved(callback)
}

/// Выгрузить скрипт окна (при закрытии островка и выходе)
pub async fn release(title: &str) {
    imp::release(title).await
}
