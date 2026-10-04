// `-r`の出力では番号の後に`|--`か`` `-- ``が付く
static HOP_LINE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^\s*([0-9]+)\.(?:[|`]--)?\s+(\S.*)$").unwrap());

// ECMPで同じhopに応答した他のアドレスは、hop番号の無い字下げ行に`|-- <addr>`や`<addr>`の形で続く
static CONTINUATION_LINE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^\s+[|`+ -]*(\S.*)$").unwrap());

pub fn is_hop_line(line: &str) -> bool {
    HOP_LINE.is_match(line)
}

pub fn parse(body: &str) -> Result<crate::model::Parsed, crate::parse::Error> {
    let mut hops: Vec<crate::model::Hop> = Vec::new();
    for line in body.lines() {
        if let Some(c) = HOP_LINE.captures(line) {
            let hop = crate::parse::parse_hop_number(&c[1])
                .and_then(|n| parse_hop(n, &c[2]))
                .ok_or_else(|| crate::parse::Error::BadLine(line.to_owned()))?;
            hops.push(hop);
        } else if let Some(prev) = hops.last()
            && let Some(c) = CONTINUATION_LINE.captures(line)
            && let Some(extra) = parse_continuation(prev.hop, &c[1])
        {
            // mtrは同じ応答元を、ASN無しの行とASN付きの行で2回出すことがある
            match hops
                .iter_mut()
                .find(|h| h.hop == extra.hop && h.ip == extra.ip && h.host == extra.host)
            {
                Some(existing) => {
                    existing.asn_reported = existing.asn_reported.or(extra.asn_reported)
                }
                None => hops.push(extra),
            }
        }
    }
    Ok(crate::model::Parsed {
        format: crate::model::Format::MtrText,
        target: None,
        hops,
    })
}

fn parse_continuation(hop: u32, rest: &str) -> Option<crate::model::Hop> {
    if rest.starts_with('[') {
        return None; // MPLSラベル
    }
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let (asn_reported, tokens) = split_asn(&tokens);
    let (ip, host) = crate::parse::parse_responder(tokens)?;
    if ip.is_none() && host.is_none() {
        return None;
    }
    Some(crate::model::Hop {
        hop,
        ip,
        host,
        asn_reported,
        ..Default::default()
    })
}

fn split_asn<'a, 'b>(tokens: &'a [&'b str]) -> (Option<u32>, &'a [&'b str]) {
    match tokens.split_first() {
        Some((first, rest)) if first.starts_with("AS") => (crate::parse::parse_asn(first), rest),
        _ => (None, tokens),
    }
}

fn parse_hop(hop: u32, rest: &str) -> Option<crate::model::Hop> {
    if rest.contains("(waiting for reply)") {
        return Some(crate::model::Hop {
            hop,
            ..Default::default()
        });
    }
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() < 8 {
        return None;
    }
    let (head, nums) = tokens.split_at(tokens.len() - 7);
    let num = |i: usize| nums[i].parse::<f64>().ok();
    let stats = crate::model::Stats {
        loss: nums[0].trim_end_matches('%').parse().ok()?,
        snt: nums[1].parse().ok()?,
        last: num(2)?,
        avg: num(3)?,
        best: num(4)?,
        wrst: num(5)?,
        stdev: num(6)?,
    };
    let (asn_reported, head) = split_asn(head);
    let (ip, host) = crate::parse::parse_responder(head)?;
    Some(crate::model::Hop {
        hop,
        ip,
        host,
        asn_reported,
        stats: Some(stats),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample() {
        let p = crate::parse::mtr_text::parse(include_str!("../../tests/fixtures/mtr-text.txt"))
            .unwrap();
        let hops = &p.hops;
        assert_eq!(
            hops.iter().map(|h| h.hop).collect::<Vec<_>>(),
            vec![1, 5, 9, 13, 20]
        );
        assert_eq!(
            hops[0].ip,
            Some("2001:268:9964:65d:7d91:c314:9263:f1be".parse().unwrap())
        );
        assert_eq!(hops[0].asn_reported, Some(2516));
        assert_eq!(hops[0].stats.as_ref().unwrap().wrst, 787.7);
        assert_eq!(
            hops[1],
            crate::model::Hop {
                hop: 5,
                ..Default::default()
            }
        );
        assert_eq!(hops[2].stats.as_ref().unwrap().loss, 30.8);
        assert_eq!(hops[3].ip, None);
        assert_eq!(
            hops[3].host.as_deref(),
            Some("6otejin301.int-gw.kddi.ne.jp")
        );
        assert_eq!(hops[3].stats.as_ref().unwrap().wrst, 1220.0);
        assert_eq!(hops[4].asn_reported, Some(59128));
        assert_eq!(
            hops[4].host.as_deref(),
            Some("hoshino.compute.kitashirakawa.dark-kuins.net")
        );
    }

    #[test]
    fn report_mode_with_header() {
        let body = "Start: 2026-10-04T19:48:00+0900\nHOST: 29-er.local                Loss%   Snt   Last   Avg  Best  Wrst StDev\n  1.|-- 172.20.10.1                0.0%     3   17.0  14.2   5.6  20.1   7.7\n  2.|-- ???                       100.0     3    0.0   0.0   0.0   0.0   0.0\n 16.|-- one.one.one.one (1.1.1.1)  0.0%     3   39.1  45.3  36.7  60.0  12.8\n";
        let p = crate::parse::mtr_text::parse(body).unwrap();
        assert_eq!(p.hops.len(), 3);
        assert_eq!(p.hops[1].ip, None);
        assert_eq!(p.hops[1].stats.as_ref().unwrap().loss, 100.0);
        assert_eq!(p.hops[0].ip, Some("172.20.10.1".parse().unwrap()));
        assert_eq!(p.hops[2].host.as_deref(), Some("one.one.one.one"));
    }

    #[test]
    fn ecmp_continuation_lines() {
        let body = "  1.|-- 10.0.0.1   0.0%     3    1.0   1.0   1.0   1.0   0.0\n        10.0.0.2\n    |  `|-- 10.0.0.2\n    |   |-- 10.0.0.3\n    |  |+-- [MPLS: Lbl 1 TC 0 S 1 TTL 1]\n  2.|-- 10.0.1.1   0.0%     3    1.0   1.0   1.0   1.0   0.0\n";
        let p = crate::parse::mtr_text::parse(body).unwrap();
        let got: Vec<(u32, String)> = p
            .hops
            .iter()
            .map(|h| (h.hop, h.ip.unwrap().to_string()))
            .collect();
        assert_eq!(
            got,
            vec![
                (1, "10.0.0.1".into()),
                (1, "10.0.0.2".into()),
                (1, "10.0.0.3".into()),
                (2, "10.0.1.1".into()),
            ]
        );
        assert!(p.hops[1].stats.is_none());
    }

    #[test]
    fn duplicate_continuation_fills_asn() {
        let body = " 1. AS2516 gateway.example 0.0% 3 1 1 1 1 0\n        other.example\n     AS13335 other.example\n";
        let p = crate::parse::mtr_text::parse(body).unwrap();
        assert_eq!(p.hops.len(), 2);
        assert_eq!(p.hops[1].host.as_deref(), Some("other.example"));
        assert_eq!(p.hops[1].asn_reported, Some(13335));
    }

    #[test]
    fn ipinfo_other_than_asn_is_error() {
        assert!(crate::parse::mtr_text::parse(" 1. US 1.1.1.1 0.0% 3 1 1 1 1 0\n").is_err());
    }

    #[test]
    fn indented_header_after_hop_is_ignored() {
        let body = " 1. 1.1.1.1 0.0% 3 1 1 1 1 0\n  Start: 2026-10-04T19:48:00+0900\n  HOST: 29-er.local Loss% Snt Last Avg Best Wrst StDev\n";
        assert_eq!(crate::parse::mtr_text::parse(body).unwrap().hops.len(), 1);
    }

    #[test]
    fn out_of_range_hop_is_error() {
        assert!(crate::parse::mtr_text::parse("4294967296. (waiting for reply)\n").is_err());
    }

    #[test]
    fn header_line_is_not_hop() {
        assert!(!crate::parse::mtr_text::is_hop_line(
            "29-er.local (2001:db8::1) -> hoshino.c.k.dark-kuins.net (2001:db8::2)"
        ));
    }
}
