static HEADER: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^\s*traceroute6? to (\S+)").unwrap());
static HOP_LINE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^\s*([0-9]+)\s+(\S.*)$").unwrap());
// 同じhopで応答元が変わると、macOSのtracerouteはhop番号の無い字下げ行を続ける
static CONTINUATION_LINE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^\s+(\S.*)$").unwrap());

pub fn is_hop_line(line: &str) -> bool {
    HOP_LINE.is_match(line)
}

#[derive(Default)]
struct Responder {
    ip: Option<std::net::IpAddr>,
    host: Option<String>,
    rtts: Vec<f64>,
}

#[derive(Default)]
struct HopAcc {
    responders: Vec<Responder>,
    timeouts: u32,
}

impl HopAcc {
    fn responder(&mut self, ip: Option<std::net::IpAddr>, host: Option<String>) -> usize {
        let found = self.responders.iter().position(|r| {
            if ip.is_some() {
                r.ip == ip
            } else {
                r.ip.is_none() && r.host == host
            }
        });
        found.unwrap_or_else(|| {
            self.responders.push(Responder {
                ip,
                host,
                rtts: Vec::new(),
            });
            self.responders.len() - 1
        })
    }
}

/// he.netのNetwork Toolsアプリと標準tracerouteの出力を読む。同じhop番号の行は1つにまとめる
pub fn parse(body: &str) -> Result<crate::model::Parsed, crate::parse::Error> {
    let target = body
        .lines()
        .find_map(|l| HEADER.captures(l))
        .map(|c| c[1].to_owned());

    let mut acc: std::collections::BTreeMap<u32, HopAcc> = std::collections::BTreeMap::new();
    let mut last_hop = None;
    for line in body.lines() {
        let bad_line = || crate::parse::Error::BadLine(line.to_owned());
        let (hop, rest) = if let Some(c) = HOP_LINE.captures(line) {
            let hop = crate::parse::parse_hop_number(&c[1]).ok_or_else(bad_line)?;
            (hop, c.get(2).unwrap().as_str())
        } else if let Some(hop) = last_hop
            && let Some(c) = CONTINUATION_LINE.captures(line)
        {
            (hop, c.get(1).unwrap().as_str())
        } else {
            continue;
        };
        last_hop = Some(hop);
        parse_line(rest, acc.entry(hop).or_default()).ok_or_else(bad_line)?;
    }

    let mut hops = Vec::new();
    for (hop, a) in acc {
        let timeouts = (a.timeouts > 0).then_some(a.timeouts);
        if a.responders.is_empty() {
            hops.push(crate::model::Hop {
                hop,
                timeouts,
                ..Default::default()
            });
            continue;
        }
        for (i, r) in a.responders.into_iter().enumerate() {
            hops.push(crate::model::Hop {
                hop,
                ip: r.ip,
                host: r.host,
                rtts: (!r.rtts.is_empty()).then_some(r.rtts),
                timeouts: if i == 0 { timeouts } else { None },
                ..Default::default()
            });
        }
    }
    Ok(crate::model::Parsed {
        format: crate::model::Format::TracerouteText,
        target,
        hops,
    })
}

/// 行の残りは、応答元と、その後に続くRTT・`*`の並び。応答元は1行に複数並ぶことがある
fn parse_line(rest: &str, acc: &mut HopAcc) -> Option<()> {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let mut current: Option<usize> = None;
    let mut probes = 0;
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        if tok == "*" {
            acc.timeouts += 1;
            probes += 1;
        } else if tok == "-" {
            current = None;
        } else if tok.starts_with('!') {
            // `!H`等のICMPエラー注記
        } else if let Some(v) = tok.strip_suffix("ms").and_then(|n| n.parse::<f64>().ok()) {
            acc.responders[current?].rtts.push(v);
            probes += 1;
        } else if let Ok(v) = tok.parse::<f64>()
            && tokens.get(i + 1) == Some(&"ms")
        {
            acc.responders[current?].rtts.push(v);
            probes += 1;
            i += 1;
        } else {
            let mut responder = vec![tok];
            if let Some(next) = tokens.get(i + 1)
                && next.starts_with('(')
                && next.ends_with(')')
            {
                responder.push(next);
                i += 1;
            }
            let (ip, name) = crate::parse::parse_responder(&responder)?;
            current = Some(acc.responder(ip, name));
        }
        i += 1;
    }
    // RTTも`*`も無い行はtracerouteの出力ではない
    (probes > 0).then_some(())
}

#[cfg(test)]
mod tests {
    fn henet() -> crate::model::Parsed {
        crate::parse::traceroute_text::parse(include_str!("../../tests/fixtures/henet.txt"))
            .unwrap()
    }

    #[test]
    fn target_from_header() {
        assert_eq!(
            henet().target.as_deref(),
            Some("hoshino.c.k.dark-kuins.net")
        );
    }

    #[test]
    fn all_hops_present() {
        let p = henet();
        assert_eq!(
            p.hops.iter().map(|h| h.hop).collect::<Vec<_>>(),
            (1..=20).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_reply_hop() {
        let p = henet();
        assert_eq!(
            p.hops[0],
            crate::model::Hop {
                hop: 1,
                timeouts: Some(3),
                ..Default::default()
            }
        );
    }

    #[test]
    fn plain_ip_hop() {
        let p = henet();
        assert_eq!(p.hops[1].ip, Some("10.60.84.249".parse().unwrap()));
        assert_eq!(p.hops[1].rtts, Some(vec![69.38, 88.96, 193.53]));
        assert_eq!(p.hops[1].timeouts, None);
    }

    #[test]
    fn split_lines_of_same_hop_are_merged() {
        let p = henet();
        let h = &p.hops[13];
        assert_eq!(h.hop, 14);
        assert_eq!(h.ip, Some("125.29.26.14".parse().unwrap()));
        assert_eq!(h.rtts, Some(vec![21.55, 24.65]));
        assert_eq!(h.timeouts, Some(1));
    }

    #[test]
    fn name_and_ip() {
        let p = henet();
        let h = &p.hops[15];
        assert_eq!(h.hop, 16);
        assert_eq!(h.ip, Some("103.48.31.54".parse().unwrap()));
        assert_eq!(h.host.as_deref(), Some("ae0.pop03cr02.bb.homenoc.ad.jp"));
    }

    #[test]
    fn standard_traceroute_with_multiple_responders() {
        let body = "traceroute to example.com (192.0.2.1), 64 hops max, 52 byte packets\n 1  192.168.1.1  1.234 ms  1.100 ms  1.050 ms\n 2  10.0.0.1  5.0 ms  10.0.0.2  6.0 ms *\n 3  * * *\n";
        let p = crate::parse::traceroute_text::parse(body).unwrap();
        assert_eq!(p.target.as_deref(), Some("example.com"));
        assert_eq!(p.hops.len(), 4);
        assert_eq!(p.hops[0].rtts, Some(vec![1.234, 1.1, 1.05]));
        assert_eq!(p.hops[1].ip, Some("10.0.0.1".parse().unwrap()));
        assert_eq!(p.hops[1].timeouts, Some(1));
        assert_eq!(p.hops[2].hop, 2);
        assert_eq!(p.hops[2].ip, Some("10.0.0.2".parse().unwrap()));
        assert_eq!(p.hops[2].timeouts, None);
        assert_eq!(
            p.hops[3],
            crate::model::Hop {
                hop: 3,
                timeouts: Some(3),
                ..Default::default()
            }
        );
    }

    #[test]
    fn continuation_line_belongs_to_previous_hop() {
        let body = " 1  192.168.1.1  1.1 ms\n    192.168.1.2  2.2 ms *\n 2  10.0.0.1  3.3 ms\n";
        let p = crate::parse::traceroute_text::parse(body).unwrap();
        assert_eq!(p.hops.len(), 3);
        assert_eq!(p.hops[1].hop, 1);
        assert_eq!(p.hops[1].ip, Some("192.168.1.2".parse().unwrap()));
        assert_eq!(p.hops[1].rtts, Some(vec![2.2]));
        assert_eq!(p.hops[0].timeouts, Some(1));
    }

    #[test]
    fn unrelated_text_is_error() {
        assert!(crate::parse::traceroute_text::parse("2026 some unrelated text\n").is_err());
        assert!(
            crate::parse::traceroute_text::parse(" 1  10.0.0.1  1.0 ms\n    unrelated text *\n")
                .is_err()
        );
    }

    #[test]
    fn out_of_range_hop_is_error() {
        assert!(crate::parse::traceroute_text::parse("4294967296 * * *\n").is_err());
        assert!(crate::parse::traceroute_text::parse("256 * * *\n").is_err());
        assert!(
            crate::parse::parse(
                "\u{ff11} * * *\n",
                Some(crate::model::Format::TracerouteText)
            )
            .is_err()
        );
    }

    #[test]
    fn rtt_without_responder_is_error() {
        assert!(crate::parse::traceroute_text::parse(" 1  12.3ms\n").is_err());
    }
}
