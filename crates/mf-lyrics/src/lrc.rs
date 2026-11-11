//! Разбор и сборка строк (lyrics_sources.py: lines_from_lrc, build_from_timed_lines, interpolate_words,
//! word_from_parts, group_syllables, parse_yrc, add_gaps). Округление времени — round(x, 3).
use std::sync::LazyLock;

use regex::Regex;

use crate::text::{ends_with_cjk, is_meta_line, py_isspace, py_splitlines, py_strip};
use crate::{GAP_MIN, Line, Word};

static LRC_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([0-9]+):([0-9]+(?:[.:][0-9]+)?)\]").unwrap());
static LRC_WORD_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[0-9]+:[0-9]+(?:[.:][0-9]+)?>").unwrap());
static YRC_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[([0-9]+),([0-9]+)\](.*)$").unwrap());
static YRC_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(([0-9]+),([0-9]+),[0-9]+\)([^(]*)").unwrap());

/// round(x, 3) Python: точное десятичное округление (половина — к чётному), а не (x·1000).round()/1000
pub fn round3(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.3}").parse().unwrap_or(x)
}

/// [mm:ss.xx] LRC → (время, текст), по времени; enhanced-теги <mm:ss.xx> вырезаются; несколько меток на строку — несколько пар.
pub fn lines_from_lrc(lrc: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for raw in py_splitlines(lrc) {
        let stamps: Vec<(u64, f64)> = LRC_TAG
            .captures_iter(raw)
            .filter_map(|c| Some((c[1].parse().ok()?, c[2].replace(':', ".").parse().ok()?)))
            .collect();
        if stamps.is_empty() {
            continue;
        }
        let text = LRC_TAG.replace_all(raw, "");
        let text = LRC_WORD_TAG.replace_all(py_strip(&text), "");
        let text = py_strip(&text);
        for (m, s) in stamps {
            out.push(((m * 60) as f64 + s, text.to_owned()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Построчный текст → строки со словами, интерполированными по длине (i = true). Мета-строки (автор, композитор…) выкидываются.
pub fn build_from_timed_lines(pairs: &[(f64, String)], duration: f64) -> Vec<Line> {
    let pairs: Vec<&(f64, String)> = pairs.iter().filter(|(_, x)| !is_meta_line(x)).collect();
    let mut lines = Vec::new();
    for (i, &(t, text)) in pairs.iter().enumerate() {
        let t = *t;
        if text.is_empty() {
            continue;
        }
        let nxt = match pairs.get(i + 1) {
            Some(p) => p.0,
            None if duration > t => duration,
            None => t + 6.0,
        };
        let n_chars = text.chars().count() as f64;
        let sung = (n_chars * 0.085 + 0.4).clamp(1.2, 9.0);
        let mut end = (nxt - 0.05).min(t + sung);
        if end <= t {
            end = t + 0.5;
        }
        lines.push(Line::words(round3(t), round3(end), interpolate_words(text, t, end), true));
    }
    lines
}

/// \S+\s* в смысле Python
fn tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut in_space = false;
    for (i, c) in text.char_indices() {
        let space = py_isspace(c);
        match start {
            None if !space => {
                start = Some(i);
                in_space = false;
            }
            Some(s) if !space && in_space => {
                out.push(&text[s..i]);
                start = Some(i);
                in_space = false;
            }
            Some(_) if space => in_space = true,
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out
}

/// Слова строки равномерно по весу len(слово)+1.5.
pub fn interpolate_words(text: &str, start: f64, end: f64) -> Vec<Word> {
    let tokens = tokens(text);
    if tokens.is_empty() {
        return Vec::new();
    }
    let weights: Vec<f64> = tokens.iter().map(|t| py_strip(t).chars().count() as f64 + 1.5).collect();
    let total: f64 = weights.iter().sum();
    let mut out = Vec::with_capacity(tokens.len());
    let mut t = start;
    for (tok, w) in tokens.into_iter().zip(weights) {
        let d = (end - start) * w / total;
        out.push(Word { x: tok.to_owned(), s: round3(t), e: round3(t + d), p: None });
        t += d;
    }
    out
}

/// Слоги одного слова (текст, начало, конец) → слово; у составного — "p" [[символов, s, e], …].
pub fn word_from_parts(parts: &[(String, f64, f64)]) -> Word {
    let x: String = parts.iter().map(|p| p.0.as_str()).collect();
    let p = (parts.len() > 1).then(|| {
        parts.iter().map(|(x, s, e)| (py_strip(x).chars().count().max(1) as u32, round3(*s), round3(*e))).collect()
    });
    Word { x, s: round3(parts[0].1), e: round3(parts[parts.len() - 1].2), p }
}

/// Слоги (текст, начало, конец) → слова: граница — пробел в конце слога или CJK-символ.
pub fn group_syllables(syllables: &[(String, f64, f64)]) -> Vec<Word> {
    let mut words = Vec::new();
    let mut cur: Vec<(String, f64, f64)> = Vec::new();
    for syl in syllables {
        let Some(last) = syl.0.chars().last() else { continue };
        cur.push(syl.clone());
        if py_isspace(last) || ends_with_cjk(&syl.0) {
            words.push(word_from_parts(&cur));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        words.push(word_from_parts(&cur));
    }
    words
}

/// NetEase YRC: строки "[start,dur](s,d,0)слог…"; JSON-строки с авторами пропускаются.
pub fn parse_yrc(yrc: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in py_splitlines(yrc) {
        let Some(m) = YRC_LINE.captures(py_strip(raw)) else { continue };
        let (Ok(ls), Ok(ld)) = (m[1].parse::<u64>(), m[2].parse::<u64>()) else { continue };
        let (ls, ld) = (ls as f64 / 1000.0, ld as f64 / 1000.0);
        let syllables: Vec<(String, f64, f64)> = YRC_WORD
            .captures_iter(&m[3])
            .filter_map(|c| {
                let (s, d) = (c[1].parse::<u64>().ok()?, c[2].parse::<u64>().ok()?);
                Some((c[3].to_owned(), s as f64 / 1000.0, (s + d) as f64 / 1000.0))
            })
            .collect();
        let text: String = syllables.iter().map(|s| s.0.as_str()).collect();
        let text = py_strip(&text);
        if text.is_empty() || is_meta_line(text) {
            continue;
        }
        let words = group_syllables(&syllables);
        let Some(last) = words.last() else { continue };
        let e = round3((ls + ld).max(last.e));
        lines.push(Line::words(round3(ls), e, words, false));
    }
    lines
}

/// Паузы ≥ GAP_MIN между строками → {"gap": true, "t": prev_end + (0.3 если prev_end), "e": t - 0.15}.
pub fn add_gaps(lines: Vec<Line>, duration: f64) -> Vec<Line> {
    // duration в Python тоже не используется — оставлен ради сигнатуры
    let _ = duration;
    let mut out = Vec::with_capacity(lines.len() + 4);
    let mut prev_end = 0.0_f64;
    for ln in lines {
        let (t, e) = (ln.start(), ln.end());
        if t - prev_end >= GAP_MIN {
            let start = prev_end + if prev_end != 0.0 { 0.3 } else { 0.0 };
            out.push(Line::Gap { gap: true, t: round3(start), e: round3(t - 0.15) });
        }
        out.push(ln);
        prev_end = prev_end.max(e);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &Line) -> &[Word] {
        match line {
            Line::Words { w, .. } => w,
            Line::Gap { .. } => &[],
        }
    }

    fn line(t: f64, e: f64) -> Line {
        Line::words(t, e, Vec::new(), false)
    }

    // ── порт LyricsSourcesTest.kt ──

    #[test]
    fn lrc_is_parsed_and_sorted() {
        let pairs = lines_from_lrc("[00:12.50]вторая строка\n[00:05.00][01:10.00]первая и повтор\n[ar:кто-то]\nбез метки");
        assert_eq!(pairs.iter().map(|p| p.0).collect::<Vec<_>>(), vec![5.0, 12.5, 70.0]);
        assert_eq!(pairs[0].1, "первая и повтор");
    }

    #[test]
    fn words_are_interpolated_inside_the_line() {
        let pairs = [(10.0, "раз два три".to_owned()), (14.0, String::new()), (20.0, "конец".to_owned())];
        let lines = build_from_timed_lines(&pairs, 30.0);
        assert_eq!(lines.len(), 2); // пустая строка — не текст
        let w = words(&lines[0]);
        assert_eq!(w.iter().map(|w| w.x.as_str()).collect::<Vec<_>>(), vec!["раз ", "два ", "три"]);
        assert!((w[0].s - 10.0).abs() < 0.001);
        assert!(lines[0].end() <= 13.95 + 0.001);
        assert!(w.windows(2).all(|p| p[0].e <= p[1].s + 0.001));
    }

    #[test]
    fn credits_are_not_lyrics() {
        let pairs = [(0.0, "作词 : 某人".to_owned()), (1.0, "Lyrics: someone".to_owned()), (5.0, "настоящая строка".to_owned())];
        assert_eq!(build_from_timed_lines(&pairs, 10.0).len(), 1);
    }

    #[test]
    fn gaps_appear_where_nobody_sings() {
        let lines = add_gaps(vec![line(8.0, 10.0), line(11.0, 12.0), line(20.0, 22.0)], 0.0);
        let gaps: Vec<bool> = lines.iter().map(|l| matches!(l, Line::Gap { .. })).collect();
        assert_eq!(gaps, vec![true, false, false, true, false]);
        assert_eq!(lines[0], Line::Gap { gap: true, t: 0.0, e: 7.85 });
        assert_eq!(lines[3], Line::Gap { gap: true, t: 12.3, e: 19.85 });
    }

    // ── прочее ──

    #[test]
    fn round3_is_python_round() {
        assert_eq!(round3(0.0625), 0.062);
        assert_eq!(round3(2.675), 2.675);
        assert_eq!(round3(1.0005), 1.0);
        assert_eq!(round3(12.3456), 12.346);
    }

    #[test]
    fn enhanced_tags_are_stripped() {
        let pairs = lines_from_lrc("[00:01.00]<00:01.00>раз <00:01.50>два\r\n[00:02:50] три ");
        assert_eq!(pairs, vec![(1.0, "раз два".to_owned()), (2.5, "три".to_owned())]);
    }

    #[test]
    fn tokens_keep_trailing_space() {
        assert_eq!(tokens("  a  b\tc "), vec!["a  ", "b\t", "c "]);
        assert!(tokens("   ").is_empty());
    }

    #[test]
    fn yrc_is_parsed_to_words() {
        let yrc = "{\"t\":0,\"c\":[{\"tx\":\"作词: \"}]}\n[1000,2000](1000,500,0)Hel(1500,500,0)lo (2000,1000,0)world\n[4000,100](4000,100,0)你(4100,100,0)好";
        let lines = parse_yrc(yrc);
        assert_eq!(lines.len(), 2);
        let w = words(&lines[0]);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], Word { x: "Hello ".into(), s: 1.0, e: 2.0, p: Some(vec![(3, 1.0, 1.5), (2, 1.5, 2.0)]) });
        assert_eq!(w[1], Word { x: "world".into(), s: 2.0, e: 3.0, p: None });
        assert_eq!(lines[0].end(), 3.0);
        // каждый иероглиф — отдельное слово; конец строки — по последнему слову
        assert_eq!(words(&lines[1]).len(), 2);
        assert_eq!(lines[1].end(), 4.2);
    }
}
