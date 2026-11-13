//! Materialfy localisation. Source strings are English; the dictionaries (`i18n/ru/*.json`, `i18n/pt/*.json`
//! at the repo root) are merged and embedded at compile time by build.rs. The current language is a global
//! atomic, so `t`/`tf`/`n` are callable from any thread.
//!
//! * `t("Settings")` — the translation of an English string (or the string itself in English / when missing).
//! * `tf("Played {0} of {1}", &[&a, &b])` — translate, then substitute positional `{0}`, `{1}`…
//! * `n(count, "{0} track", "{0} tracks")` — plural: English one/many, Russian one/few/many
//!   (dictionary key `"{0} track|{0} tracks"`, value `"{0} трек|{0} трека|{0} треков"`), Portuguese one/many
//!   (value `"{0} faixa|{0} faixas"`), `{0}` → count.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(test)]
mod merge;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    #[default]
    En,
    Ru,
    Pt,
}

/// The languages with an embedded dictionary (English is the source strings themselves).
pub const DICTIONARIES: &[Lang] = &[Lang::Ru, Lang::Pt];

impl Lang {
    /// Settings / QML code: "en", "ru" or "pt".
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::Pt => "pt",
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::Ru,
        2 => Lang::Pt,
        _ => Lang::En,
    }
}

/// "pt", "pt-BR", "ru", "ru-RU" (any case) → that language, anything else → En.
pub fn parse_lang(s: &str) -> Lang {
    let s = s.trim().to_ascii_lowercase();
    match s.split(['-', '_']).next().unwrap_or("") {
        "ru" => Lang::Ru,
        "pt" => Lang::Pt,
        _ => Lang::En,
    }
}

/// The Language setting: follow the OS UI language, or one fixed language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LangSetting {
    #[default]
    System,
    Fixed(Lang),
}

impl LangSetting {
    /// Settings code: "system", "en", "ru" or "pt".
    pub fn code(self) -> &'static str {
        match self {
            LangSetting::System => "system",
            LangSetting::Fixed(lang) => lang.code(),
        }
    }

    /// The language to show: a fixed one as is; System — from the OS UI languages (most preferred first).
    pub fn resolve<S: AsRef<str>>(self, os_languages: &[S]) -> Lang {
        match self {
            LangSetting::System => system_lang(os_languages),
            LangSetting::Fixed(lang) => lang,
        }
    }
}

/// The `language` setting value: "en"/"ru"/"pt" (any case, with a region) → that language; "system", empty or
/// anything unknown → System.
pub fn parse_setting(s: &str) -> LangSetting {
    let code = s.trim().to_ascii_lowercase();
    let primary = code.split(['-', '_']).next().unwrap_or("");
    match primary {
        "en" => LangSetting::Fixed(Lang::En),
        "ru" => LangSetting::Fixed(Lang::Ru),
        "pt" => LangSetting::Fixed(Lang::Pt),
        _ => LangSetting::System,
    }
}

/// The app language for the OS UI languages (BCP 47 tags, most preferred first, as QLocale::uiLanguages gives
/// them): a language we have a dictionary for when the OS UI language is that language, English otherwise
/// (also for an empty list).
pub fn system_lang<S: AsRef<str>>(os_languages: &[S]) -> Lang {
    os_languages.first().map_or(Lang::En, |first| parse_lang(first.as_ref()))
}

/// The merged Russian dictionary as a JSON object string (for the QML side).
pub const RU_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/ru.json"));
/// The merged Portuguese dictionary as a JSON object string (for the QML side).
pub const PT_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/pt.json"));

fn ru() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| serde_json::from_str(RU_JSON).unwrap_or_default())
}

fn pt() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| serde_json::from_str(PT_JSON).unwrap_or_default())
}

/// The dictionary of `lang` (English key → translation; plural values keep the `one|few|many` / `one|many` form).
/// None for English.
pub fn dictionary(lang: Lang) -> Option<&'static HashMap<String, String>> {
    match lang {
        Lang::En => None,
        Lang::Ru => Some(ru()),
        Lang::Pt => Some(pt()),
    }
}

/// Whether a Rust-made UI text is one of the dictionary keys in any language (such a text arrives already
/// translated, so it must not be treated as a message from the network).
pub fn in_any_dictionary(text: &str) -> bool {
    DICTIONARIES.iter().any(|l| dictionary(*l).is_some_and(|d| d.contains_key(text)))
}

/// Whether `text` is the `en` string translated in any dictionary (the language may have changed since a
/// placeholder was stored).
pub fn translated_in_any(en: &str, text: &str) -> bool {
    DICTIONARIES.iter().any(|l| dictionary(*l).and_then(|d| d.get(en)).is_some_and(|t| t == text))
}

fn lookup(en: &str) -> Option<&'static str> {
    dictionary(lang())?.get(en).map(String::as_str)
}

static PHONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Phone build / layout: texts speak of "this phone" instead of "this computer" (QML: I18n.phone).
pub fn set_phone(on: bool) {
    PHONE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// "computer" → "phone" in the current language when the phone wording is on; the nouns decline alike, so one
/// replacement per stem is enough.
fn phone(text: &str) -> String {
    if !PHONE.load(std::sync::atomic::Ordering::Relaxed) {
        return text.to_owned();
    }
    let pairs: &[(&str, &str)] = match lang() {
        Lang::Ru => &[("компьютер", "телефон"), ("Компьютер", "Телефон")],
        Lang::Pt => &[
            ("computadores", "celulares"),
            ("computador", "celular"),
            ("Computadores", "Celulares"),
            ("Computador", "Celular"),
        ],
        _ => &[("computers", "phones"), ("computer", "phone"), ("Computer", "Phone")],
    };
    pairs.iter().fold(text.to_owned(), |acc, (from, to)| acc.replace(from, to))
}

/// Translates an English source string into the current language (unchanged if English or untranslated).
pub fn t(en: &str) -> String {
    phone(lookup(en).unwrap_or(en))
}

/// Substitutes `{0}`, `{1}`… in `pattern` (unknown or out-of-range placeholders stay as is).
pub fn format(pattern: &str, args: &[&dyn Display]) -> String {
    if args.is_empty() || !pattern.contains('{') {
        return pattern.to_owned();
    }
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let idx = after.find('}').and_then(|close| after[..close].parse::<usize>().ok().map(|i| (i, close)));
        match idx {
            Some((i, close)) if i < args.len() => {
                use std::fmt::Write;
                let _ = write!(out, "{}", args[i]);
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Translates, then substitutes `{0}`, `{1}`…
pub fn tf(en: &str, args: &[&dyn Display]) -> String {
    format(&phone(lookup(en).unwrap_or(en)), args)
}

/// Russian plural category: 0 = one (1, 21, 101…), 1 = few (2–4, 22–24…), 2 = many (0, 5–20, 11–14, 25…).
pub fn ru_plural_index(count: i64) -> usize {
    let n = count.unsigned_abs();
    let (m10, m100) = (n % 10, n % 100);
    if m10 == 1 && m100 != 11 {
        0
    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
        1
    } else {
        2
    }
}

/// Plural category for `count` in `lang`: Russian has three forms (one|few|many), every other language two
/// (one|many).
pub fn plural_index(lang: Lang, count: i64) -> usize {
    match lang {
        Lang::Ru => ru_plural_index(count),
        _ => usize::from(count.unsigned_abs() != 1),
    }
}

/// Plural form for `count`; `{0}` in the chosen form is replaced by `count`.
pub fn n(count: i64, one: &str, many: &str) -> String {
    let current = lang();
    let form: String = match dictionary(current).and_then(|d| d.get(&format!("{one}|{many}"))) {
        Some(text) => {
            let forms: Vec<&str> = text.split('|').collect();
            forms.get(plural_index(current, count)).or(forms.last()).copied().unwrap_or(many).to_owned()
        }
        None if count == 1 => one.to_owned(),
        None => many.to_owned(),
    };
    format(&phone(&form), &[&count])
}

/// `tr!("Played {0} of {1}", a, b)` — `t` without arguments, `tf` with them.
#[macro_export]
macro_rules! tr {
    ($en:expr) => {
        $crate::t($en)
    };
    ($en:expr, $($arg:expr),+ $(,)?) => {
        $crate::tf($en, &[$(&$arg as &dyn ::std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // the language is global: tests that switch it run one at a time
    static GUARD: Mutex<()> = Mutex::new(());

    fn with_lang<R>(l: Lang, f: impl FnOnce() -> R) -> R {
        let _g = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        set_lang(l);
        let r = f();
        set_lang(Lang::En);
        r
    }

    #[test]
    fn plural_rules() {
        let cases = [
            (0, 2),
            (1, 0),
            (2, 1),
            (4, 1),
            (5, 2),
            (11, 2),
            (12, 2),
            (14, 2),
            (21, 0),
            (22, 1),
            (25, 2),
            (101, 0),
            (111, 2),
            (112, 2),
            (-1, 0),
        ];
        for (count, idx) in cases {
            assert_eq!(ru_plural_index(count), idx, "count {count}");
        }
        // two-form languages: one for ±1, many for everything else
        for (count, idx) in [(0, 1), (1, 0), (2, 1), (11, 1), (21, 1), (101, 1), (-1, 0)] {
            assert_eq!(plural_index(Lang::Pt, count), idx, "count {count}");
            assert_eq!(plural_index(Lang::En, count), idx, "count {count}");
        }
    }

    #[test]
    fn parse() {
        assert_eq!(parse_lang("ru"), Lang::Ru);
        assert_eq!(parse_lang("RU-ru"), Lang::Ru);
        assert_eq!(parse_lang("pt"), Lang::Pt);
        assert_eq!(parse_lang("pt-BR"), Lang::Pt);
        assert_eq!(parse_lang("PT_br"), Lang::Pt);
        assert_eq!(parse_lang("en"), Lang::En);
        assert_eq!(parse_lang(""), Lang::En);
        assert_eq!(parse_lang("de"), Lang::En);
        assert_eq!(Lang::Ru.code(), "ru");
        assert_eq!(Lang::Pt.code(), "pt");
    }

    #[test]
    fn parse_setting_values() {
        assert_eq!(parse_setting("system"), LangSetting::System);
        assert_eq!(parse_setting(" System "), LangSetting::System);
        assert_eq!(parse_setting(""), LangSetting::System);
        assert_eq!(parse_setting("de"), LangSetting::System);
        assert_eq!(parse_setting("english"), LangSetting::System);
        assert_eq!(parse_setting("en"), LangSetting::Fixed(Lang::En));
        assert_eq!(parse_setting("EN-us"), LangSetting::Fixed(Lang::En));
        assert_eq!(parse_setting("ru"), LangSetting::Fixed(Lang::Ru));
        assert_eq!(parse_setting("ru_RU"), LangSetting::Fixed(Lang::Ru));
        assert_eq!(parse_setting("pt"), LangSetting::Fixed(Lang::Pt));
        assert_eq!(parse_setting("pt-BR"), LangSetting::Fixed(Lang::Pt));
        assert_eq!(LangSetting::default(), LangSetting::System);
        for setting in [LangSetting::System, LangSetting::Fixed(Lang::En), LangSetting::Fixed(Lang::Ru), LangSetting::Fixed(Lang::Pt)] {
            assert_eq!(parse_setting(setting.code()), setting);
        }
    }

    #[test]
    fn system_language() {
        let none: [&str; 0] = [];
        assert_eq!(system_lang(&none), Lang::En);
        assert_eq!(system_lang(&["ru-RU", "en-US"]), Lang::Ru);
        assert_eq!(system_lang(&["ru"]), Lang::Ru);
        assert_eq!(system_lang(&["pt-BR", "en-US"]), Lang::Pt);
        assert_eq!(system_lang(&["pt"]), Lang::Pt);
        assert_eq!(system_lang(&["en-US", "ru-RU"]), Lang::En);
        assert_eq!(system_lang(&["de-DE", "ru-RU"]), Lang::En);
        assert_eq!(system_lang(&["uk-UA"]), Lang::En);
        assert_eq!(system_lang(&["be-BY"]), Lang::En);
        assert_eq!(system_lang(&["kk-KZ"]), Lang::En);
        assert_eq!(system_lang(&["rue"]), Lang::En);
        assert_eq!(system_lang(&[String::from("ru-UA")]), Lang::Ru);
        assert_eq!(system_lang(&[String::from("pt-PT")]), Lang::Pt);
    }

    #[test]
    fn resolve_setting() {
        let russian_os = ["ru-RU", "en-US"];
        let english_os = ["en-GB"];
        let portuguese_os = ["pt-BR", "en-US"];
        assert_eq!(parse_setting("system").resolve(&russian_os), Lang::Ru);
        assert_eq!(parse_setting("system").resolve(&english_os), Lang::En);
        assert_eq!(parse_setting("system").resolve(&portuguese_os), Lang::Pt);
        assert_eq!(parse_setting("en").resolve(&russian_os), Lang::En);
        assert_eq!(parse_setting("ru").resolve(&english_os), Lang::Ru);
        assert_eq!(parse_setting("pt").resolve(&english_os), Lang::Pt);
    }

    #[test]
    fn formatting() {
        assert_eq!(format("{0} of {1}", &[&1, &"two"]), "1 of two");
        assert_eq!(format("{1}{0}{1}", &[&"a", &"b"]), "bab");
        assert_eq!(format("{x} {5} {", &[&1]), "{x} {5} {");
        assert_eq!(format("no args", &[]), "no args");
    }

    #[test]
    fn english_passthrough() {
        with_lang(Lang::En, || {
            assert_eq!(t("Language"), "Language");
            assert_eq!(tf("Untranslated {0}", &[&7]), "Untranslated 7");
            assert_eq!(n(1, "{0} track", "{0} tracks"), "1 track");
            assert_eq!(n(5, "{0} track", "{0} tracks"), "5 tracks");
            assert_eq!(tr!("{0}+{1}", 1, 2), "1+2");
            assert_eq!(tr!("Language"), "Language");
        });
    }

    #[test]
    fn russian_lookup() {
        with_lang(Lang::Ru, || {
            assert_eq!(t("Language"), "Язык");
            assert_eq!(t("No such string here"), "No such string here");
            let forms = [
                (1, "1 трек"),
                (2, "2 трека"),
                (5, "5 треков"),
                (11, "11 треков"),
                (21, "21 трек"),
                (22, "22 трека"),
                (25, "25 треков"),
                (111, "111 треков"),
            ];
            for (c, want) in forms {
                assert_eq!(n(c, "{0} track", "{0} tracks"), want);
            }
            // an untranslated plural falls back to the English forms
            assert_eq!(n(2, "{0} zork", "{0} zorks"), "2 zorks");
        });
    }

    #[test]
    fn portuguese_lookup() {
        with_lang(Lang::Pt, || {
            assert_eq!(t("Language"), "Idioma");
            assert_eq!(t("No such string here"), "No such string here");
            let forms = [(1, "1 faixa"), (2, "2 faixas"), (0, "0 faixas"), (21, "21 faixas")];
            for (c, want) in forms {
                assert_eq!(n(c, "{0} track", "{0} tracks"), want);
            }
            // an untranslated plural falls back to the English forms
            assert_eq!(n(2, "{0} zork", "{0} zorks"), "2 zorks");
        });
    }

    #[test]
    fn embedded_dictionaries_are_valid() {
        for (lang, json, forms) in [(Lang::Ru, RU_JSON, 3), (Lang::Pt, PT_JSON, 2)] {
            let parsed: HashMap<String, String> = serde_json::from_str(json).expect("dictionary");
            assert!(!parsed.is_empty(), "{lang:?} dictionary is empty");
            assert_eq!(dictionary(lang).map(|d| d.len()), Some(parsed.len()), "{lang:?}");
            for (k, v) in &parsed {
                if k.contains('|') {
                    assert_eq!(v.split('|').count(), forms, "{lang:?} plural {k}");
                }
            }
        }
    }

    fn files(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn duplicate_keys() {
        // identical duplicates across files are fine
        let ok = merge::merge(
            &files(&[
                ("a.json", r#"{"Play": "Играть"}"#),
                ("b.json", r#"{"Play": "Играть", "Pause": "Пауза"}"#),
            ]),
            3,
        );
        assert_eq!(ok.unwrap().len(), 2);
        // conflicting values across files
        let err = merge::merge(&files(&[("a.json", r#"{"Play": "Играть"}"#), ("b.json", r#"{"Play": "Воспроизвести"}"#)]), 3)
            .unwrap_err();
        assert!(err.contains("duplicate key \"Play\"") && err.contains("a.json") && err.contains("b.json"), "{err}");
        // conflicting values inside one file
        let err = merge::merge(&files(&[("a.json", r#"{"Play": "Играть", "Play": "Пуск"}"#)]), 3).unwrap_err();
        assert!(err.contains("duplicate key"), "{err}");
        // malformed plural: two forms where the language needs three
        let err = merge::merge(&files(&[("ru.json", r#"{"{0} track|{0} tracks": "{0} трек|{0} треков"}"#)]), 3).unwrap_err();
        assert!(err.contains("plural"), "{err}");
        // bad JSON
        assert!(merge::merge(&files(&[("a.json", "{")]), 3).is_err());
        // two-form languages accept one|many and reject the Russian three
        let ok = merge::merge(&files(&[("pt.json", r#"{"{0} track|{0} tracks": "{0} faixa|{0} faixas"}"#)]), 2);
        assert_eq!(ok.unwrap().len(), 1);
        let err = merge::merge(&files(&[("pt.json", r#"{"{0} track|{0} tracks": "{0} faixa|{0} faixas|{0} faixas"}"#)]), 2)
            .unwrap_err();
        assert!(err.contains("plural"), "{err}");
    }
}
