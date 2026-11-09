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
        // У обычного окна Wayland нет «не брать фокус»: KWin активирует островок при появлении и по клику, и окно, где
        // печатали, теряет фокус. Возвращаем фокус тому, кто был активен до островка (previous).
        // Окно островка исчезло (закрыли или процесс упал, пока курсор был над ним) — снимаем его клавиши в kglobalaccel:
        // иначе ← → ↑ ↓ и пробел остались бы захвачены во всех окнах до перезапуска сеанса
        let key_actions = KEYS.iter().map(|k| format!("\"{}\"", k.1)).collect::<Vec<_>>().join(", ");
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
             let previous = null;\n\
             function giveBack(w) {{\n\
             \x20   if (!w || w.caption !== title) return;\n\
             \x20   // в stackingOrder есть и закрытые окна (ещё анимируются) — у них пустой заголовок\n\
             \x20   const ok = v => v && v.normalWindow && !v.deleted && v.wantsInput && !v.minimized && v.caption !== \"\"\n\
             \x20       && !v.caption.startsWith(\"Materialfy Island\")\n\
             \x20       && (v.onAllDesktops || v.desktops.includes(workspace.currentDesktop));\n\
             \x20   // нет прежнего (островок активен с самого появления) — верхнее обычное окно этого стола\n\
             \x20   const back = ok(previous) && workspace.windowList().includes(previous) ? previous : workspace.stackingOrder.filter(ok).pop();\n\
             \x20   if (back) workspace.activeWindow = back;\n\
             }}\n\
             workspace.windowActivated.connect(w => {{\n\
             \x20   if (w && !w.caption.startsWith(\"Materialfy Island\")) previous = w;\n\
             \x20   giveBack(w);\n\
             }});\n\
             giveBack(workspace.activeWindow);\n\
             workspace.windowList().forEach(place);\n\
             workspace.windowAdded.connect(w => {{ place(w); update(); }});\n\
             workspace.windowRemoved.connect(w => {{\n\
             \x20   if (w.caption === title) [{key_actions}].forEach(a => callDBus(\"{KGA}\", \"{KGA_PATH}\", \"{KGA_IFACE}\", \"unregister\", \"{KEYS_COMPONENT}\", a));\n\
             \x20   update();\n\
             }});\n\
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

    // последнее место каждого окна: KWin перезапустился — все скрипты пропали, грузим их заново
    static SPOTS: std::sync::Mutex<Vec<(String, super::Spot)>> = std::sync::Mutex::new(Vec::new());
    static WATCH: std::sync::Once = std::sync::Once::new();

    fn remember(title: &str, spot: Option<&super::Spot>) {
        let mut spots = SPOTS.lock().unwrap_or_else(|e| e.into_inner());
        spots.retain(|(t, _)| t != title);
        if let Some(spot) = spot {
            spots.push((title.to_owned(), spot.clone()));
        }
    }

    fn signals(interface: &'static str, member: Option<&'static str>, arg0: &'static str) -> zbus::Result<zbus::MatchRule<'static>> {
        let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(interface)?;
        let rule = match member {
            Some(member) => rule.member(member)?,
            None => rule,
        };
        Ok(rule.arg(0, arg0)?.build())
    }

    async fn each_signal(conn: &zbus::Connection, rule: zbus::MatchRule<'static>, mut handle: impl FnMut(zbus::Message)) {
        use zbus::export::futures_core::Stream;
        let stream = match zbus::MessageStream::for_match_rule(rule, conn, None).await {
            Ok(stream) => stream,
            Err(error) => return tracing::warn!("KWin: D-Bus subscription failed: {error}"),
        };
        let mut stream = std::pin::pin!(stream);
        while let Some(message) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            if let Ok(message) = message {
                handle(message);
            }
        }
    }

    // org.kde.KWin сменил владельца (KWin перезапустился, окна переподключились): ставим окна заново, клавиши
    // наведения — тоже (kglobalaccel живёт в KWin и всё забыл)
    async fn watch_kwin(conn: zbus::Connection) {
        let Ok(rule) = signals("org.freedesktop.DBus", Some("NameOwnerChanged"), SERVICE) else { return };
        each_signal(&conn, rule, |message| {
            let Ok((_, _, owner)) = message.body().deserialize::<(String, String, String)>() else { return };
            if owner.is_empty() {
                return;
            }
            tokio::spawn(async {
                // имя KWin занимает раньше, чем поднимает /Scripting
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let spots = SPOTS.lock().unwrap_or_else(|e| e.into_inner()).clone();
                for (title, spot) in spots {
                    if let Err(error) = load(&title, &spot).await {
                        tracing::warn!("KWin script for {title} after a KWin restart: {error}");
                    }
                }
                *KEYS_ON.lock().await = false;
                let _ = keys(KEYS_WANTED.load(std::sync::atomic::Ordering::SeqCst)).await;
            });
        })
        .await
    }

    pub async fn place(title: &str, spot: &super::Spot) -> Result<(), PlatformError> {
        let conn = connection().await?;
        WATCH.call_once(|| {
            tokio::spawn(watch_kwin(conn.clone()));
        });
        load(title, spot).await
    }

    // отдельно от place: watch_kwin зовёт эту, а не place (иначе тип future place ссылался бы сам на себя)
    async fn load(title: &str, spot: &super::Spot) -> Result<(), PlatformError> {
        let _guard = LOCK.lock().await;
        remember(title, Some(spot));
        let name = script_name(title);
        let conn = connection().await?;
        let report = conn.unique_name().map(|n| n.to_string()).unwrap_or_default();
        // только XDG_RUNTIME_DIR (0700, наш): в общий /tmp кто угодно подложит свой файл, и KWin выполнит его
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|dir| !dir.is_empty())
            .map(std::path::PathBuf::from)
            .ok_or_else(|| PlatformError::Io(std::io::Error::other("XDG_RUNTIME_DIR is not set")))?;
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
        // не run на /Scripting/Script{id}: id у KWin — число загруженных скриптов, после выгрузки не последнего он
        // совпадает с id живого скрипта, run уходит тому, и наш не запускается. start запускает все незапущенные
        conn.call_method(Some(SERVICE), SCRIPTING, Some(SCRIPTING_IFACE), "start", &())
            .await
            .map_err(dbus_err)?;
        Ok(())
    }

    pub async fn release(title: &str) {
        let _guard = LOCK.lock().await;
        remember(title, None);
        let name = script_name(title);
        if let Ok(conn) = zbus::Connection::session().await {
            let _ = conn.call_method(Some(SERVICE), SCRIPTING, Some(SCRIPTING_IFACE), "unloadScript", &(name.as_str(),)).await;
        }
    }

    // Клавиши без фокуса: окно островка на Wayland клавиатуру не получает, поэтому ← → ↑ ↓ и пробел — глобальные
    // сочетания kglobalaccel (он внутри KWin), и только пока курсор над островком: on — захватить, false — отпустить
    const KGA: &str = "org.kde.kglobalaccel";
    const KGA_PATH: &str = "/kglobalaccel";
    const KGA_IFACE: &str = "org.kde.KGlobalAccel";
    const KEYS_COMPONENT: &str = "materialfy-island";
    // (код для floatingKey в FloatingIsland.qml, действие, подпись в «Комбинациях клавиш» Plasma, Qt::Key)
    const KEYS: [(i32, &str, &str, i32); 5] = [
        (1, "previous", "Previous track", 0x0100_0012),
        (2, "next", "Next track", 0x0100_0014),
        (3, "volumeUp", "Volume up", 0x0100_0013),
        (4, "volumeDown", "Volume down", 0x0100_0015),
        (5, "playPause", "Play / pause", 0x20),
    ];

    type KeyFn = Box<dyn Fn(i32) + Send + Sync>;
    static ON_KEY: std::sync::OnceLock<KeyFn> = std::sync::OnceLock::new();
    static LISTEN: std::sync::Once = std::sync::Once::new();
    static KEYS_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static KEYS_ON: tokio::sync::Mutex<bool> = tokio::sync::Mutex::const_new(false);

    pub fn on_key(callback: KeyFn) {
        let _ = ON_KEY.set(callback);
    }

    pub async fn keys(on: bool) -> Result<(), PlatformError> {
        KEYS_WANTED.store(on, std::sync::atomic::Ordering::SeqCst);
        let mut active = KEYS_ON.lock().await;
        // вызовы гонятся друг с другом в пуле: ставим последнее желание, а не своё
        let want = KEYS_WANTED.load(std::sync::atomic::Ordering::SeqCst);
        if *active == want {
            return Ok(());
        }
        let conn = connection().await?;
        LISTEN.call_once(|| {
            let conn = conn.clone();
            tokio::spawn(async move {
                let Ok(rule) = signals("org.kde.kglobalaccel.Component", None, KEYS_COMPONENT) else { return };
                each_signal(&conn, rule, |message| {
                    // автоповтор (globalShortcutRepeated): громкость идёт шагами дальше, трек и пауза — один раз на нажатие
                    let member = message.header().member().map(|m| m.to_string()).unwrap_or_default();
                    let Ok((_, action, _)) = message.body().deserialize::<(String, String, i64)>() else { return };
                    let Some(&(code, ..)) = KEYS.iter().find(|k| k.1 == action) else { return };
                    let fire = member == "globalShortcutPressed" || (member == "globalShortcutRepeated" && (code == 3 || code == 4));
                    if let (true, Some(callback)) = (fire, ON_KEY.get()) {
                        callback(code);
                    }
                })
                .await
            });
        });
        for (_, action, text, key) in KEYS {
            let id = [KEYS_COMPONENT, action, "Materialfy", text];
            let id = id.as_slice();
            if want {
                conn.call_method(Some(KGA), KGA_PATH, Some(KGA_IFACE), "doRegister", &(id,)).await.map_err(dbus_err)?;
                // SetPresent | NoAutoloading: клавиша — эта, а не сохранённая в kglobalshortcutsrc
                let reply = conn
                    .call_method(Some(KGA), KGA_PATH, Some(KGA_IFACE), "setShortcutKeys", &(id, vec![(vec![key],)], 6u32))
                    .await
                    .map_err(dbus_err)?;
                let set: Vec<(Vec<i32>,)> = reply.body().deserialize().unwrap_or_default();
                if !set.iter().any(|(k,)| k.first() == Some(&key)) {
                    tracing::warn!("kglobalaccel did not take the key for {action} (taken by another shortcut?)");
                }
            } else {
                conn.call_method(Some(KGA), KGA_PATH, Some(KGA_IFACE), "setInactive", &(id,)).await.map_err(dbus_err)?;
            }
        }
        *active = want;
        Ok(())
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
            // окно пропало — клавиши kglobalaccel снимает сам KWin (наш процесс мог упасть)
            assert!(text.contains("[\"previous\", \"next\", \"volumeUp\", \"volumeDown\", \"playPause\"].forEach(a => callDBus(\"org.kde.kglobalaccel\", \"/kglobalaccel\", \"org.kde.KGlobalAccel\", \"unregister\", \"materialfy-island\", a));"));
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

    pub async fn keys(_on: bool) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    pub fn on_key(_callback: Box<dyn Fn(i32) + Send + Sync>) {}
}

/// Сеанс KDE Plasma на Wayland: островок можно поставить скриптом KWin
pub fn available() -> bool {
    imp::available()
}

/// Где окно и когда его прятать: (x, y, width, height) — логические пиксели рабочего стола
#[derive(Clone)]
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

/// KDE: ← → ↑ ↓ и пробел без фокуса — глобальные сочетания kglobalaccel, пока on (курсор над островком)
pub async fn keys(on: bool) -> Result<(), crate::PlatformError> {
    imp::keys(on).await
}

/// Нажали клавишу островка: код 1 ←, 2 →, 3 ↑, 4 ↓, 5 пробел. Вызывается из потока D-Bus; задаётся один раз
pub fn on_key(callback: Box<dyn Fn(i32) + Send + Sync>) {
    imp::on_key(callback)
}
