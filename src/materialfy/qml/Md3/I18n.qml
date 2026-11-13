pragma Singleton
import QtQml
import Materialfy.Core

// Localisation for QML. Source strings are English; the Russian and Portuguese dictionaries come from Rust
// (Translations.ru / Translations.pt, built from i18n/ru/*.json and i18n/pt/*.json). Every function reads
// Translations.language, so a binding that calls I18n.t(...) re-evaluates when the language changes
// (Settings.language → Translations.language).
//   I18n.t("Play")                          → "Play" / "Играть" / "Reproduzir"
//   I18n.t("{0} of {1}", a, b)              → positional placeholders
//   I18n.n(count, "{0} track", "{0} tracks") → "5 tracks" / "5 треков" / "5 faixas" (key "{0} track|{0} tracks")
// Formatting helpers (compact numbers, dates, durations, sizes) keep their locale data in _loc below.
QtObject {
    id: root

    // "en" | "ru" | "pt"
    readonly property string lang: {
        const code = Translations.language
        return code === "ru" || code === "pt" ? code : "en"
    }
    readonly property bool russian: lang === "ru"
    // the embedded dictionaries (constant: converted to JS objects once)
    readonly property var _ru: Translations.ru
    readonly property var _pt: Translations.pt
    readonly property var dict: _dictOf(lang) || ({})

    // The dictionary of a language code (undefined for English, which uses the source strings).
    function _dictOf(code) {
        return code === "ru" ? _ru : code === "pt" ? _pt : undefined
    }

    // Functions read Translations.language directly (not the `lang` binding), so they are current even inside a
    // languageChanged handler that runs before I18n's own bindings update.
    function _isRu() {
        return Translations.language === "ru"
    }
    function _cur() {
        const code = Translations.language
        return _loc[code === "ru" || code === "pt" ? code : "en"]
    }

    // Substitutes {0}, {1}… with args[0], args[1]… (missing arguments leave the placeholder as is).
    function format(pattern, args) {
        if (!args || args.length === 0)
            return pattern
        return String(pattern).replace(/\{(\d+)\}/g, (m, i) => i < args.length ? String(args[i]) : m)
    }

    // Translation of an English source string; extra arguments substitute {0}, {1}…
    // Фразы, где автозамена «компьютер → телефон» даёт «телефон виден на телефоне»: на телефоне — свой текст
    // (английский здесь, переводы — ключи "phone:<английский>" в словарях)
    readonly property var _phoneEn: ({
        "How this computer appears on phones and speakers": "How this phone appears on other devices and speakers",
        "How this computer appears in Spotify on your phone and other devices": "How this phone appears in Spotify on your other devices",
        "Materialfy becomes a Spotify Connect device: music plays here, and your phone sees this computer in its device list. Confirm in your browser; after that your library also goes through this sign-in, with no Web API limits.":
            "Materialfy becomes a Spotify Connect device: music plays here, and your computer and speakers see this phone in their device lists. Confirm in your browser; after that your library also goes through this sign-in, with no Web API limits.",
        "Remote: move music between your phone and computer": "Remote: move music between this phone, a computer and speakers",
        "Without Premium you can sign in another way — through the Web API: you get your library, search and a remote for phones and speakers, but no music on this computer. Spotify sometimes limits these requests.":
            "Without Premium you can sign in another way — through the Web API: you get your library, search and a remote for other devices and speakers, but no music on this phone. Spotify sometimes limits these requests.",
        "Open Spotify on a phone or speaker and it will show up here. Or start playing music on this computer.":
            "Open Spotify on a computer or speaker and it will show up here. Or start playing music on this phone.",
        "The playing track’s cover, blurred behind the whole window as a soft colored glow":
            "The playing track’s cover, blurred behind the whole screen as a soft colored glow"
    })

    function t(en) {
        const d = _dictOf(Translations.language)
        if (Theme.mobile && _phoneEn[en] !== undefined) {
            const ps = d ? d["phone:" + en] : undefined
            const pp = typeof ps === "string" ? ps : _phoneEn[en]
            return arguments.length > 1 ? format(pp, Array.prototype.slice.call(arguments, 1)) : pp
        }
        const s = d ? d[en] : undefined
        const pattern = phone(typeof s === "string" ? s : en)
        return arguments.length > 1 ? format(pattern, Array.prototype.slice.call(arguments, 1)) : pattern
    }

    // Phone layout (Theme.mobile): the interface speaks of "this phone", not "this computer". The nouns decline
    // alike (компьютер-а/-е/-ом → телефон-а/-е/-ом; computador(es) → celular(es)), so one replacement per stem
    // covers every dictionary entry. mf_i18n::set_phone does the same for strings coming from Rust.
    function phone(text) {
        if (!Theme.mobile || typeof text !== "string" || text.length === 0)
            return text
        switch (Translations.language) {
        case "ru":
            return text.replace(/компьютер/g, "телефон").replace(/Компьютер/g, "Телефон")
        case "pt":
            return text.replace(/computadores/g, "celulares").replace(/computador/g, "celular")
                       .replace(/Computadores/g, "Celulares").replace(/Computador/g, "Celular")
        default:
            return text.replace(/\bcomputers\b/g, "phones").replace(/\bcomputer\b/g, "phone")
                       .replace(/\bComputer\b/g, "Phone")
        }
    }

    // Any language → English for plain (non-plural) entries, built once from every dictionary.
    readonly property var _en: {
        const back = {}
        const add = d => {
            for (const k in d) {
                const v = d[k]
                if (typeof v === "string" && k.indexOf("|") < 0 && !(v in back))
                    back[v] = k
            }
        }
        add(_ru)
        add(_pt)
        return back
    }

    // Text that Rust produced (and may have cached) in any language, shown in the current one:
    // a dictionary value maps back to its English source first, then translates as usual.
    function any(text) {
        const s = String(text ?? "")
        const en = typeof _en[s] === "string" ? _en[s] : s
        return t(en)
    }

    // Russian plural category: 0 one (1, 21), 1 few (2–4, 22–24), 2 many (0, 5–20, 11–14, 25…)
    function _ruIndex(count) {
        const n = Math.abs(Math.trunc(Number(count) || 0))
        const m10 = n % 10, m100 = n % 100
        return m10 === 1 && m100 !== 11 ? 0 : m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14) ? 1 : 2
    }

    // Plural category of the current language: Russian three forms, the others one (1) / many.
    function _pluralIndex(count) {
        if (Translations.language === "ru")
            return _ruIndex(count)
        return Math.abs(Math.trunc(Number(count) || 0)) === 1 ? 0 : 1
    }

    // Plural form for count; {0} → count (shown = what replaces {0}, e.g. a compact number).
    function _plural(count, one, many, shown) {
        const d = _dictOf(Translations.language)
        const forms = d ? d[one + "|" + many] : undefined
        let form
        if (typeof forms === "string") {
            const list = String(forms).split("|")
            form = list[Math.min(_pluralIndex(count), list.length - 1)]
        } else {
            form = Number(count) === 1 ? one : many
        }
        return format(form, [shown])
    }

    function n(count, one, many) {
        return _plural(count, one, many, count)
    }

    // n() with a compact {0}: "1.2M listeners" / "1,2 млн слушателей" (after «млн»/«тыс.» Russian uses the many form)
    function nCompact(count, one, many) {
        const c = Number(count) || 0
        return _plural(c >= 1e4 ? 5 : c, one, many, compact(c))
    }

    // ---- locale data for the helpers -------------------------------------------------------------
    readonly property var _loc: ({
        en: {
            decimal: ".",
            group: ",",
            million: "M",
            thousand: "K",
            monthsShort: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
            monthsLong: ["January", "February", "March", "April", "May", "June", "July", "August", "September",
                         "October", "November", "December"],
            dayMonth: "{1} {0}",                   // {0} day, {1} month
            date: "{1} {0}, {2}",                  // + {2} year
            monthYear: "{0} {1}",
            now: "Just now",
            minAgo: "{0} min ago",
            hrAgo: "{0} hr ago",
            yesterday: "Yesterday",
            daysAgo: ["{0} day ago", "{0} days ago"],
            lastWeek: "Last week",
            weeksAgo: ["{0} week ago", "{0} weeks ago"],
            hr: "{0} hr",
            min: "{0} min",
            sec: "{0} sec",
            bytes: ["B", "KB", "MB", "GB"]
        },
        ru: {
            decimal: ",",
            group: " ",
            million: " млн",
            thousand: " тыс.",
            monthsShort: ["янв.", "февр.", "мар.", "апр.", "мая", "июн.", "июл.", "авг.", "сент.", "окт.", "нояб.", "дек."],
            // genitive: «9 октября»
            monthsLong: ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября",
                         "октября", "ноября", "декабря"],
            // nominative: «октябрь 2024»
            monthsNominative: ["январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август", "сентябрь",
                               "октябрь", "ноябрь", "декабрь"],
            dayMonth: "{0} {1}",
            date: "{0} {1} {2}",
            monthYear: "{0} {1}",
            now: "Сейчас",
            minAgo: "{0} мин назад",
            hrAgo: "{0} ч назад",
            yesterday: "Вчера",
            daysAgo: ["{0} день назад", "{0} дня назад", "{0} дней назад"],
            lastWeek: "На прошлой неделе",
            weeksAgo: ["{0} неделю назад", "{0} недели назад", "{0} недель назад"],
            hr: "{0} ч",
            min: "{0} мин",
            sec: "{0} с",
            bytes: ["Б", "КБ", "МБ", "ГБ"]
        },
        pt: {
            decimal: ",",
            group: ".",
            million: " mi",
            thousand: " mil",
            monthsShort: ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."],
            // lowercase, as in «9 de outubro de 2024»
            monthsLong: ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro",
                         "outubro", "novembro", "dezembro"],
            dayMonth: "{0} de {1}",
            date: "{0} de {1} de {2}",
            monthYear: "{0} de {1}",
            now: "Agora mesmo",
            minAgo: "{0} min atrás",
            hrAgo: "{0} h atrás",
            yesterday: "Ontem",
            daysAgo: ["{0} dia atrás", "{0} dias atrás"],
            lastWeek: "Semana passada",
            weeksAgo: ["{0} semana atrás", "{0} semanas atrás"],
            hr: "{0} h",
            min: "{0} min",
            sec: "{0} s",
            bytes: ["B", "KB", "MB", "GB"]
        }
    })
    readonly property var loc: _loc[lang]   // for bindings; functions use _cur()

    function _pick(forms, count) {
        if (_isRu())
            return forms[Math.min(_ruIndex(count), forms.length - 1)]
        return Number(count) === 1 ? forms[0] : forms[forms.length - 1]
    }

    // Decimal number with the language's separator: decimal(1.25, 1) → "1.3" / "1,3"
    function decimal(x, digits) {
        return Number(x || 0).toFixed(digits === undefined ? 1 : digits).replace(".", _cur().decimal)
    }

    // Digit grouping without abbreviation: 8765 → "8,765" / "8 765"
    function grouped(x) {
        return String(Math.round(Number(x) || 0)).replace(/\B(?=(\d{3})+(?!\d))/g, _cur().group)
    }

    // 12.3M / 450K / 8,765 — Russian: 12,3 млн / 450 тыс. / 8 765
    function compact(x) {
        const v = Number(x) || 0
        if (v >= 1e6)
            return decimal(v / 1e6, v >= 1e7 ? 0 : 1) + _cur().million
        if (v >= 1e4)
            return Math.round(v / 1e3) + _cur().thousand
        return grouped(v)
    }

    // Date from an ISO string, a millisecond timestamp or a Date; null if unparsable.
    function _date(dateLike) {
        if (dateLike === undefined || dateLike === null || dateLike === "")
            return null
        if (typeof dateLike === "string" && /^\d{4}$/.test(dateLike))
            return new Date(Number(dateLike), 0, 1)
        const d = dateLike instanceof Date ? dateLike : new Date(dateLike)
        return isNaN(d.getTime()) ? null : d
    }

    // "Oct 9" / "9 октября"
    function dayMonth(dateLike) {
        const d = _date(dateLike)
        if (!d)
            return ""
        const months = _isRu() ? _cur().monthsLong : _cur().monthsShort
        return format(_cur().dayMonth, [d.getDate(), months[d.getMonth()]])
    }

    // "Oct 9, 2024" / "9 окт. 2024"; shortDate omits the current year
    function date(dateLike) {
        const d = _date(dateLike)
        if (!d)
            return ""
        return format(_cur().date, [d.getDate(), _cur().monthsShort[d.getMonth()], d.getFullYear()])
    }
    function shortDate(dateLike) {
        const d = _date(dateLike)
        if (!d)
            return ""
        if (d.getFullYear() !== new Date().getFullYear())
            return date(d)
        return format(_cur().dayMonth, [d.getDate(), _cur().monthsShort[d.getMonth()]])
    }

    // Release date with the precision Spotify knows it to ("2001-03-12" | "2001-03" | "2001"):
    // "March 12, 2001" / "12 марта 2001", "March 2001" / "март 2001", "2001". Parsed by hand, so the time zone
    // never moves the day.
    function releaseDate(text) {
        const m = /^(\d{4})(?:-(\d{2})(?:-(\d{2}))?)?/.exec(String(text ?? ""))
        if (!m)
            return ""
        const year = Number(m[1])
        if (!m[2])
            return String(year)
        const month = Math.min(11, Math.max(0, Number(m[2]) - 1))
        if (!m[3])
            return monthYear(new Date(year, month, 1))
        return format(_cur().date, [Number(m[3]), _cur().monthsLong[month], year])
    }

    // "October 2024" / "октябрь 2024"
    function monthYear(dateLike) {
        const d = _date(dateLike)
        if (!d)
            return ""
        const months = _isRu() ? _cur().monthsNominative : _cur().monthsLong
        return format(_cur().monthYear, [months[d.getMonth()], d.getFullYear()])
    }

    // "Just now", "5 min ago", "3 hr ago", "Yesterday", "3 days ago", "Last week", "2 weeks ago", then date()
    function relative(dateLike) {
        const d = _date(dateLike)
        if (!d)
            return ""
        const min = (Date.now() - d.getTime()) / 60000
        if (min < 10)
            return _cur().now
        if (min < 60)
            return format(_cur().minAgo, [Math.round(min)])
        const h = min / 60
        if (h < 24)
            return format(_cur().hrAgo, [Math.round(h)])
        const days = Math.floor(h / 24)
        if (days === 1)
            return _cur().yesterday
        if (days < 7)
            return format(_pick(_cur().daysAgo, days), [days])
        if (days < 14)
            return _cur().lastWeek
        if (days < 31) {
            const weeks = Math.floor(days / 7)
            return format(_pick(_cur().weeksAgo, weeks), [weeks])
        }
        return date(d)
    }

    // Length from milliseconds: "1 hr 5 min" / "1 ч 5 мин", "45 min", under a minute "30 sec"
    function duration(ms) {
        const totalMin = Math.round((Number(ms) || 0) / 60000)
        if (totalMin < 1 && Number(ms) > 0)
            return format(_cur().sec, [Math.max(1, Math.round(Number(ms) / 1000))])
        if (totalMin < 60)
            return format(_cur().min, [totalMin])
        const h = Math.floor(totalMin / 60), m = totalMin % 60
        return format(_cur().hr, [h]) + (m ? " " + format(_cur().min, [m]) : "")
    }

    // Clock time m:ss or h:mm:ss (same in every language)
    function clock(ms) {
        const s = Math.max(0, Math.floor((Number(ms) || 0) / 1000))
        const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, sec = s % 60
        const pad = v => (v < 10 ? "0" : "") + v
        return h > 0 ? h + ":" + pad(m) + ":" + pad(sec) : m + ":" + pad(sec)
    }

    // Size in bytes: "1.5 GB" / "1,5 ГБ", "300 MB"
    function bytes(b) {
        b = Number(b) || 0
        const u = _cur().bytes
        if (b >= 1024 * 1024 * 1024)
            return decimal(b / 1024 / 1024 / 1024, 1) + " " + u[3]
        if (b >= 1024 * 1024)
            return Math.round(b / 1024 / 1024) + " " + u[2]
        if (b >= 1024)
            return Math.round(b / 1024) + " " + u[1]
        return b + " " + u[0]
    }
}
