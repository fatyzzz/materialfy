//! Аргументы командной строки — как parse_args в app.py (неизвестные аргументы игнорируются, как parse_known_args).
//! Как argparse: `--opt VALUE` и `--opt=VALUE`, однозначные сокращения длинных имён, -h/--help.
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Cli {
    /// --gallery: галерея компонентов MD3 (qml/gallery/Gallery.qml)
    pub gallery: bool,
    /// --mobile: раскладка телефона (MobileMain.qml) на десктопе; на Android — всегда
    pub mobile: bool,
    /// --qml FILE: загрузить произвольный QML
    pub qml: Option<PathBuf>,
    /// --screenshot PNG: сохранить снимок окна и выйти
    pub screenshot: Option<PathBuf>,
    /// --delay MS (1500): задержка перед снимком
    pub delay: u64,
    /// --seed-image PATH
    pub seed_image: Option<PathBuf>,
    /// --light
    pub light: bool,
    /// --offline
    pub offline: bool,
    /// --new-instance
    pub new_instance: bool,
    /// --debug-wheel: QT_LOGGING_RULES += ";qml.debug=true"
    pub debug_wheel: bool,
    /// --debug-fps
    pub debug_fps: bool,
    /// --size WxH
    pub size: Option<(i32, i32)>,
    /// --config-dir DIR (для --screenshot по умолчанию — временный каталог)
    pub config_dir: Option<PathBuf>,
    /// --set KEY=VALUE (можно несколько): начальные свойства корня; "true"/"false" → bool, int, float, иначе строка
    pub set: Vec<(String, InitialValue)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InitialValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl InitialValue {
    /// _parse_value из app.py.
    pub fn parse(value: &str) -> InitialValue {
        if value == "true" || value == "false" {
            return InitialValue::Bool(value == "true");
        }
        let trimmed = value.trim();
        let underscores_ok = !trimmed.contains("__") && !trimmed.starts_with('_') && !trimmed.ends_with('_');
        if underscores_ok && let Ok(i) = trimmed.replace('_', "").parse::<i64>() {
            return InitialValue::Int(i);
        }
        if let Some(f) = parse_python_float(trimmed) {
            return InitialValue::Float(f);
        }
        InitialValue::Str(value.to_owned())
    }
}

/// float() Python: «1.5», «1e3», «inf», «nan», «1_000.5».
fn parse_python_float(text: &str) -> Option<f64> {
    if text.is_empty() || text.contains("__") || text.starts_with('_') || text.ends_with('_') {
        return None;
    }
    text.replace('_', "").parse().ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Opt {
    Gallery,
    Qml,
    Screenshot,
    Delay,
    SeedImage,
    Light,
    Offline,
    NewInstance,
    DebugWheel,
    DebugFps,
    Mobile,
    Size,
    ConfigDir,
    Set,
    Help,
}

const OPTIONS: &[(&str, Opt, bool)] = &[
    ("--gallery", Opt::Gallery, false),
    ("--qml", Opt::Qml, true),
    ("--screenshot", Opt::Screenshot, true),
    ("--delay", Opt::Delay, true),
    ("--seed-image", Opt::SeedImage, true),
    ("--light", Opt::Light, false),
    ("--offline", Opt::Offline, false),
    ("--new-instance", Opt::NewInstance, false),
    ("--debug-wheel", Opt::DebugWheel, false),
    ("--debug-fps", Opt::DebugFps, false),
    ("--mobile", Opt::Mobile, false),
    ("--size", Opt::Size, true),
    ("--config-dir", Opt::ConfigDir, true),
    ("--set", Opt::Set, true),
    ("--help", Opt::Help, false),
];

const USAGE: &str = "usage: materialfy [-h] [--gallery] [--qml FILE] [--screenshot PNG] [--delay DELAY] [--seed-image PATH]
               [--light] [--offline] [--new-instance] [--debug-wheel] [--debug-fps] [--mobile] [--size SIZE]
               [--config-dir DIR] [--set KEY=VALUE]";

const HELP: &str = "
options:
  -h, --help         show this help message and exit
  --gallery          галерея компонентов MD3
  --qml FILE         загрузить произвольный QML (стенды для отладки)
  --screenshot PNG   сохранить снимок окна и выйти
  --delay DELAY      задержка перед снимком, мс
  --seed-image PATH  взять цвет темы из картинки
  --light            светлая тема
  --offline          не восстанавливать сессию из keyring
  --new-instance     запустить второй экземпляр, даже если плеер уже работает
  --debug-wheel      печатать сырые события колеса мыши
  --debug-fps        счётчик кадров в углу окна
  --mobile           раскладка телефона (как в сборке для Android)
  --size SIZE        размер окна, ШxВ (по умолчанию — из настроек)
  --config-dir DIR   каталог настроек (для --screenshot по умолчанию временный, чтобы не трогать настоящие)
  --set KEY=VALUE    начальное свойство корневого окна (число или строка)";

/// Ошибка разбора — как argparse: usage и сообщение в stderr, код 2.
fn fail(message: &str) -> ! {
    eprintln!("{USAGE}\nmaterialfy: error: {message}");
    std::process::exit(2);
}

/// Длинное имя по точному совпадению или однозначному префиксу (allow_abbrev argparse).
fn lookup(name: &str) -> Option<(&'static str, Opt, bool)> {
    if name == "-h" {
        return Some(("--help", Opt::Help, false));
    }
    if let Some(&exact) = OPTIONS.iter().find(|(n, ..)| *n == name) {
        return Some(exact);
    }
    if !name.starts_with("--") || name.len() < 3 {
        return None;
    }
    let mut found = OPTIONS.iter().filter(|(n, ..)| n.starts_with(name));
    match (found.next(), found.next()) {
        (Some(&only), None) => Some(only),
        (Some(_), Some(_)) => {
            let all: Vec<&str> = OPTIONS.iter().filter(|(n, ..)| n.starts_with(name)).map(|(n, ..)| *n).collect();
            fail(&format!("ambiguous option: {name} could match {}", all.join(", ")))
        }
        _ => None,
    }
}

fn parse_size(text: &str) -> Option<(i32, i32)> {
    let lower = text.to_lowercase();
    let (w, h) = lower.split_once('x')?;
    if h.contains('x') {
        return None;
    }
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

impl Cli {
    /// Разбор std::env::args() (первый элемент — программа).
    pub fn parse(args: &[String]) -> Cli {
        let mut cli = Cli { delay: 1500, ..Cli::default() };
        let mut rest = args.iter().skip(1);
        let mut only_positional = false;
        while let Some(arg) = rest.next() {
            if only_positional || !arg.starts_with('-') || arg == "-" {
                continue;
            }
            if arg == "--" {
                only_positional = true;
                continue;
            }
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) if n.starts_with("--") => (n, Some(v.to_owned())),
                _ => (arg.as_str(), None),
            };
            let Some((full, opt, takes_value)) = lookup(name) else { continue };
            let value = if takes_value {
                match inline {
                    Some(v) => v,
                    None => match rest.next() {
                        // argparse не берёт значением то, что похоже на опцию
                        Some(v) if !(v.starts_with('-') && v.len() > 1 && v.parse::<f64>().is_err()) => v.clone(),
                        _ => fail(&format!("argument {full}: expected one argument")),
                    },
                }
            } else {
                if inline.is_some() {
                    fail(&format!("argument {full}: ignored explicit argument '{}'", inline.unwrap_or_default()));
                }
                String::new()
            };
            match opt {
                Opt::Gallery => cli.gallery = true,
                Opt::Qml => cli.qml = Some(PathBuf::from(value)),
                Opt::Screenshot => cli.screenshot = Some(PathBuf::from(value)),
                Opt::Delay => match value.trim().parse::<i64>() {
                    // QTimer.singleShot с отрицательной задержкой срабатывает сразу
                    Ok(ms) => cli.delay = ms.max(0) as u64,
                    Err(_) => fail(&format!("argument --delay: invalid int value: '{value}'")),
                },
                Opt::SeedImage => cli.seed_image = Some(PathBuf::from(value)),
                Opt::Light => cli.light = true,
                Opt::Offline => cli.offline = true,
                Opt::NewInstance => cli.new_instance = true,
                Opt::DebugWheel => cli.debug_wheel = true,
                Opt::DebugFps => cli.debug_fps = true,
                Opt::Mobile => cli.mobile = true,
                Opt::Size => match parse_size(&value) {
                    Some(size) => cli.size = Some(size),
                    None => fail(&format!("argument --size: expected WxH, got '{value}'")),
                },
                Opt::ConfigDir => cli.config_dir = Some(PathBuf::from(value)),
                Opt::Set => {
                    let (key, value) = value.split_once('=').unwrap_or((value.as_str(), ""));
                    cli.set.push((key.to_owned(), InitialValue::parse(value)));
                }
                Opt::Help => {
                    println!("{USAGE}\n{HELP}");
                    std::process::exit(0);
                }
            }
        }
        cli
    }

    /// Главный режим (не снимок, не --qml, не галерея): трей, единственный экземпляр.
    pub fn main_mode(&self) -> bool {
        self.screenshot.is_none() && self.qml.is_none() && !self.gallery
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        let mut all = vec!["materialfy".to_owned()];
        all.extend(args.iter().map(|s| s.to_string()));
        Cli::parse(&all)
    }

    #[test]
    fn defaults_and_unknown() {
        let cli = parse(&["--unknown", "x", "pos"]);
        assert_eq!(cli.delay, 1500);
        assert!(cli.main_mode());
    }

    #[test]
    fn values_and_abbrev() {
        let cli = parse(&["--screenshot=a.png", "--del", "200", "--size", "800X600", "--gal", "--set", "k=1", "--set=s=hi"]);
        assert_eq!(cli.screenshot, Some(PathBuf::from("a.png")));
        assert_eq!(cli.delay, 200);
        assert_eq!(cli.size, Some((800, 600)));
        assert!(cli.gallery);
        assert_eq!(cli.set, vec![("k".into(), InitialValue::Int(1)), ("s".into(), InitialValue::Str("hi".into()))]);
    }

    #[test]
    fn initial_values() {
        assert_eq!(InitialValue::parse("true"), InitialValue::Bool(true));
        assert_eq!(InitialValue::parse("-3"), InitialValue::Int(-3));
        assert_eq!(InitialValue::parse("1.5"), InitialValue::Float(1.5));
        assert_eq!(InitialValue::parse("1e3"), InitialValue::Float(1000.0));
        assert_eq!(InitialValue::parse("True"), InitialValue::Str("True".into()));
        assert_eq!(InitialValue::parse("settings"), InitialValue::Str("settings".into()));
    }
}
