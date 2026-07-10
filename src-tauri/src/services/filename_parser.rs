use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ParsedFilename {
    pub circle: Option<String>,
    pub title: String,
    pub series: Option<String>,
    pub translator: Option<String>,
    pub version_tag: Option<String>,
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

    // Series: top-level parens only, captured BEFORE the title-strip pass.
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
        if chunk.contains("ç¿»è¨³") || chunk.contains("Chinese") {
            if out.translator.is_none() {
                out.translator = Some(chunk);
            }
        } else if chunk.contains("DL") || chunk.contains("ã«ã©ã¼") {
            if out.version_tag.is_none() {
                out.version_tag = Some(chunk);
            }
        } else if out.circle.is_none() {
            out.circle = Some(chunk);
        }
    }
    if let Some(s) = series_caps.into_iter().next() {
        out.series = Some(s);
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
        // "C" alone is not a recognized translator keyword - the parser
        // only flags translator for chunks containing 翻訳 / Chinese.
        assert_eq!(p.translator, None);
        assert_eq!(p.version_tag.as_deref(), Some("DL"));
    }

    #[test]
    fn falls_back_to_full_filename() {
        let p = parse("random_file");
        assert_eq!(p.title, "random_file");
        assert_eq!(p.circle, None);
    }

    #[test]
    fn series_does_not_eat_paren_inside_bracket() {
        // Regression: the first implementation captured (B) from inside
        // [A (B)] as the series, masking the real (series) later.
        let p = parse("[Circle (Author)] Some Title (RealSeries) [Tag]");
        assert_eq!(p.series.as_deref(), Some("RealSeries"));
    }

    #[test]
    fn chinese_translator_via_chinese_keyword() {
        let p = parse("[SomeCircle] My Title [Chinese] [DL]");
        assert_eq!(p.circle.as_deref(), Some("SomeCircle"));
        assert_eq!(p.translator.as_deref(), Some("Chinese"));
        assert_eq!(p.version_tag.as_deref(), Some("DL"));
        assert_eq!(p.title, "My Title");
    }
}
