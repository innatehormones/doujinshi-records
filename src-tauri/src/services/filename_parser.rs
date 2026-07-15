use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ParsedFilename {
    pub circle: Option<String>,
    pub title: String,
    pub series: Option<String>,
    pub translator: Option<String>,
}

/// Skip Comiket event markers like `(C105)` — users rarely want the event
/// number as the "series". Top-level parens matching `^C\d+$` are dropped;
/// the first remaining paren wins.
fn pick_series(series_caps: Vec<String>) -> Option<String> {
    let c_event_re = regex::Regex::new(r"^C\d+$").unwrap();
    series_caps
        .into_iter()
        .find(|s| !c_event_re.is_match(s.trim()))
}

/// Fallback when no bracket tag claimed translator. Cheap heuristics:
/// 1. 中国 / 中國 / 汉化 → Chinese
/// 2. hiragana / katakana present → Japanese
/// 3. pure ASCII with letters → English
/// 4. else None (CJK-only kanji could be either — don't guess)
fn detect_translator_from_title(title: &str) -> Option<String> {
    if title.contains("中国") || title.contains("中國") || title.contains("汉化") {
        return Some("Chinese".into());
    }
    let has_jp = title.chars().any(|c| {
        matches!(c,
            '\u{3040}'..='\u{309F}' |  // hiragana
            '\u{30A0}'..='\u{30FF}'     // katakana
        )
    });
    if has_jp {
        return Some("Japanese".into());
    }
    let has_letter = title.chars().any(|c| c.is_ascii_alphabetic());
    if has_letter && title.chars().all(|c| c.is_ascii() || c.is_whitespace()) {
        return Some("English".into());
    }
    None
}

pub fn parse(filename: &str) -> ParsedFilename {
    let mut out = ParsedFilename::default();
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);

    let working = stem.to_string();

    let bracket_re = regex::Regex::new(r"\[([^\[\]]+)\]").unwrap();
    let paren_re = regex::Regex::new(r"\(([^()]+)\)").unwrap();

    let mut bracket_matches: Vec<String> = bracket_re
        .captures_iter(&working)
        .map(|c| c[1].to_string())
        .collect();

    // title_no_brackets: brackets removed so the series regex only sees
    // top-level parens. Parens that lived inside [A (B)] are eaten
    // along with their containing brackets, never reaching series capture.
    let mut title_no_brackets = working.clone();
    for cap in bracket_re.captures_iter(&working) {
        title_no_brackets = title_no_brackets.replace(&cap[0], " ");
    }

    let series_caps: Vec<String> = paren_re
        .captures_iter(&title_no_brackets)
        .map(|c| c[1].to_string())
        .collect();

    // Final title: brackets AND parens both stripped.
    let mut title_only = title_no_brackets.clone();
    for cap in paren_re.captures_iter(&title_no_brackets) {
        title_only = title_only.replace(&cap[0], " ");
    }
    let title = title_only.split_whitespace().collect::<Vec<_>>().join(" ");
    out.title = if title.is_empty() { stem.to_string() } else { title };

    if !bracket_matches.is_empty() {
        out.circle = Some(bracket_matches.remove(0));
    }
    for chunk in bracket_matches {
        if chunk.contains("翻訳") || chunk.contains("Chinese") {
            if out.translator.is_none() {
                out.translator = Some(chunk);
            }
        } else if out.circle.is_none() {
            out.circle = Some(chunk);
        }
    }

    out.series = pick_series(series_caps);

    if out.translator.is_none() {
        out.translator = detect_translator_from_title(&out.title);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_japanese_typical() {
        let p = parse("[A (B)] sample title (series) [C] [DL]");
        assert_eq!(p.circle.as_deref(), Some("A (B)"));
        assert!(p.title.contains("sample"));
        assert!(p.title.contains("title"));
        assert_eq!(p.series.as_deref(), Some("series"));
        // [DL] 现在不再被特殊对待（version_tag 已移除），落进 circle 分支但 circle
        // 已被 [A (B)] 占用，于是 [DL] 整个被忽略。[C] 同理。
        // title 是纯 ASCII → translator 兜底为 English。
        assert_eq!(p.translator.as_deref(), Some("English"));
    }

    #[test]
    fn falls_back_to_full_filename() {
        let p = parse("random_file");
        assert_eq!(p.title, "random_file");
        assert_eq!(p.circle, None);
        assert_eq!(p.translator.as_deref(), Some("English"));
    }

    #[test]
    fn series_does_not_eat_paren_inside_bracket() {
        // Regression: the first implementation captured (B) from inside
        // [A (B)] as the series, masking the real (series) later.
        let p = parse("[Circle (Author)] Some Title (RealSeries) [Tag]");
        assert_eq!(p.series.as_deref(), Some("RealSeries"));
    }

    #[test]
    fn skips_comiket_c_event_paren() {
        // 实际文件名：(C105) [...] title (series) [...]。series 应是括号里
        // 不是 C 编号的那个，而不是 (C105)。
        let p = parse("(C105) [e＊haz (春原)] たまにはこういう日も。 (16bitセンセーション ANOTHER LAYER) [中国翻訳]");
        assert_eq!(
            p.series.as_deref(),
            Some("16bitセンセーション ANOTHER LAYER")
        );
    }

    #[test]
    fn c_numbered_only_yields_no_series() {
        // 没有非 C 编号括号 → series 应为 None，而不是落到 (C107)。
        let p = parse("(C107) [Circle] some title");
        assert_eq!(p.series, None);
    }

    #[test]
    fn chinese_translator_via_chinese_keyword() {
        let p = parse("[SomeCircle] My Title [Chinese] [DL]");
        assert_eq!(p.circle.as_deref(), Some("SomeCircle"));
        // [Chinese] 命中 bracket 规则，优先级高于纯 ASCII 兜底。
        assert_eq!(p.translator.as_deref(), Some("Chinese"));
        assert_eq!(p.title, "My Title");
    }

    #[test]
    fn chinese_translator_via_jp_keyword() {
        // [中国翻訳] 真实 UTF-8 写法（之前的 Latin1 byte 版本是 bug）。
        let p = parse("[SomeCircle] title [中国翻訳]");
        assert_eq!(p.translator.as_deref(), Some("中国翻訳"));
    }

    #[test]
    fn japanese_fallback_when_no_bracket_tag() {
        // 没有 [中国翻訳] 这种 bracket 标签：靠 title 里的平假名/片假名兜底。
        let p = parse("[Circle] タイトル (Series)");
        assert_eq!(p.translator.as_deref(), Some("Japanese"));
    }

    #[test]
    fn chinese_fallback_via_keyword_in_title() {
        // title 里出现汉化 → Chinese（覆盖日文优先，因为是更明确的标志）。
        let p = parse("[Circle] 漢化版标题");
        assert_eq!(p.translator.as_deref(), Some("Chinese"));
    }

    #[test]
    fn pure_kanji_title_yields_no_translator() {
        // 纯汉字标题（中日语都可能）→ 兜底不出错，避免误判。
        let p = parse("[Circle] 标题");
        assert_eq!(p.translator, None);
    }
}