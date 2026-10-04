pub mod mtr_json;
pub mod mtr_text;
pub mod traceroute_text;

use crate::model::{Format, Parsed};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not detect the input format")]
    UnknownFormat,
    #[error("no hop found as {0:?}")]
    NoHops(Format),
    #[error("invalid mtr json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unparsable line: {0:?}")]
    BadLine(String),
}

/// `declared`が無い時は本文から形式を推定する
pub fn parse(body: &str, declared: Option<Format>) -> Result<Parsed, Error> {
    let format = match declared {
        Some(f) => f,
        None => detect(body).ok_or(Error::UnknownFormat)?,
    };
    let parsed = match format {
        Format::MtrJson => crate::parse::mtr_json::parse(body)?,
        Format::MtrText => crate::parse::mtr_text::parse(body)?,
        Format::TracerouteText => crate::parse::traceroute_text::parse(body)?,
    };
    if parsed.hops.is_empty() {
        return Err(Error::NoHops(format));
    }
    Ok(parsed)
}

pub fn detect(body: &str) -> Option<Format> {
    if serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .is_some_and(|v| v.pointer("/report/hubs").is_some())
    {
        return Some(Format::MtrJson);
    }
    if body.lines().any(crate::parse::mtr_text::is_hop_line) {
        return Some(Format::MtrText);
    }
    if body.lines().any(crate::parse::traceroute_text::is_hop_line) {
        return Some(Format::TracerouteText);
    }
    None
}

/// TTLの範囲(1〜255)に収まるhop番号だけを受け付ける
pub fn parse_hop_number(s: &str) -> Option<u32> {
    s.parse().ok().filter(|n| (1..=255).contains(n))
}

/// JSONに書けない`NaN`や`inf`は受け付けない
pub fn parse_finite(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// 空白で区切られた応答元の欄を読む。`ip`・`name`・`name (ip)`・`???`だけを受け付け、それ以外は`None`
pub fn parse_responder(tokens: &[&str]) -> Option<(Option<std::net::IpAddr>, Option<String>)> {
    let (ip, host) = match tokens {
        [one] => split_host(one),
        [_, paren] if paren.starts_with('(') => split_host(&tokens.join(" ")),
        _ => return None,
    };
    match (ip, host) {
        (None, Some(name)) if !looks_like_hostname(&name) => None,
        // 逆引きの無いhopを`2001:db8::1 (2001:db8::1)`と出すtracerouteがある
        (Some(ip), Some(name)) if name.parse::<std::net::IpAddr>().is_ok() => {
            Some((Some(ip), None))
        }
        (Some(_), Some(name)) if !looks_like_hostname(&name) => None,
        (ip, host) => Some((ip, host)),
    }
}

/// `AS2516`の形を読む。`AS???`等は`None`
pub fn parse_asn(s: &str) -> Option<u32> {
    s.strip_prefix("AS")?.parse().ok()
}

// ドットを含まない名前も逆引きとしてはありうるが、無関係な文章の単語と区別できないので受け付けない
fn looks_like_hostname(s: &str) -> bool {
    s.contains('.')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// mtrやtracerouteのホスト欄を`(ip, 逆引き名)`に分ける。`name (ip)`・IPのみ・名前のみ・`???`を受け付ける
pub fn split_host(s: &str) -> (Option<std::net::IpAddr>, Option<String>) {
    let s = s.trim();
    if s.is_empty() || s == "???" {
        return (None, None);
    }
    if let Some((name, rest)) = s.split_once(" (")
        && let Some(inner) = rest.strip_suffix(')')
        && let Ok(ip) = inner.parse()
    {
        return (Some(ip), Some(name.trim().to_owned()));
    }
    match s.parse() {
        Ok(ip) => (Some(ip), None),
        Err(_) => (None, Some(s.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_finite_rejects_non_finite() {
        assert_eq!(crate::parse::parse_finite("1.5"), Some(1.5));
        for s in ["NaN", "inf", "-inf", "infinity", "x"] {
            assert_eq!(crate::parse::parse_finite(s), None, "{s}");
        }
    }

    #[test]
    fn split_host_variants() {
        assert_eq!(crate::parse::split_host("???"), (None, None));
        assert_eq!(
            crate::parse::split_host("one.one.one.one (1.1.1.1)"),
            (
                Some("1.1.1.1".parse().unwrap()),
                Some("one.one.one.one".into())
            )
        );
        assert_eq!(
            crate::parse::split_host("2001:268:f309:d001::111"),
            (Some("2001:268:f309:d001::111".parse().unwrap()), None)
        );
        assert_eq!(
            crate::parse::split_host("6otejin301.int-gw.kddi.ne.jp"),
            (None, Some("6otejin301.int-gw.kddi.ne.jp".into()))
        );
    }

    #[test]
    fn parse_responder_rejects_non_host() {
        assert_eq!(crate::parse::parse_responder(&["???"]), Some((None, None)));
        assert!(crate::parse::parse_responder(&["US", "1.1.1.1"]).is_none());
        assert!(crate::parse::parse_responder(&["unrelated"]).is_none());
        assert!(crate::parse::parse_responder(&["Start:"]).is_none());
        assert!(crate::parse::parse_responder(&["a.example", "(1.1.1.1)"]).is_some());
        assert_eq!(
            crate::parse::parse_responder(&["2001:db8::1", "(2001:db8::1)"]),
            Some((Some("2001:db8::1".parse().unwrap()), None))
        );
        assert!(crate::parse::parse_responder(&["_gateway", "(10.0.0.1)"]).is_none());
    }

    #[test]
    fn detect_fixtures() {
        use crate::model::Format;
        assert_eq!(
            crate::parse::detect(include_str!("../../tests/fixtures/mtr-json-b.json")),
            Some(Format::MtrJson)
        );
        assert_eq!(
            crate::parse::detect(include_str!("../../tests/fixtures/mtr-text.txt")),
            Some(Format::MtrText)
        );
        assert_eq!(
            crate::parse::detect(include_str!("../../tests/fixtures/henet.txt")),
            Some(Format::TracerouteText)
        );
        assert_eq!(crate::parse::detect("hello"), None);
    }

    #[test]
    fn declared_format_mismatch_is_error() {
        let body = include_str!("../../tests/fixtures/henet.txt");
        assert!(crate::parse::parse(body, Some(crate::model::Format::MtrJson)).is_err());
        assert!(crate::parse::parse(body, Some(crate::model::Format::MtrText)).is_err());
    }
}
