#[derive(serde::Deserialize)]
struct Root {
    report: Report,
}

#[derive(serde::Deserialize)]
struct Report {
    mtr: Meta,
    hubs: Vec<Hub>,
}

#[derive(serde::Deserialize)]
struct Meta {
    dst: Option<String>,
}

#[derive(serde::Deserialize)]
struct Hub {
    count: u32,
    host: String,
    /// `-z`付きの時だけ出る。`AS2516`や`AS???`の形
    #[serde(rename = "ASN")]
    asn: Option<String>,
    #[serde(rename = "Loss%")]
    loss: f64,
    #[serde(rename = "Snt")]
    snt: u32,
    #[serde(rename = "Last")]
    last: f64,
    #[serde(rename = "Avg")]
    avg: f64,
    #[serde(rename = "Best")]
    best: f64,
    #[serde(rename = "Wrst")]
    wrst: f64,
    #[serde(rename = "StDev")]
    stdev: f64,
}

pub fn parse(body: &str) -> Result<crate::model::Parsed, crate::parse::Error> {
    let root: Root = serde_json::from_str(body)?;
    let hops = root
        .report
        .hubs
        .into_iter()
        .map(|h| {
            let (ip, host) = crate::parse::split_host(&h.host);
            crate::model::Hop {
                hop: h.count,
                ip,
                host,
                asn_reported: h.asn.as_deref().and_then(parse_asn),
                stats: Some(crate::model::Stats {
                    loss: h.loss,
                    snt: h.snt,
                    last: h.last,
                    avg: h.avg,
                    best: h.best,
                    wrst: h.wrst,
                    stdev: h.stdev,
                }),
                ..Default::default()
            }
        })
        .collect();
    Ok(crate::model::Parsed {
        format: crate::model::Format::MtrJson,
        target: root.report.mtr.dst,
        hops,
    })
}

pub(crate) fn parse_asn(s: &str) -> Option<u32> {
    s.strip_prefix("AS")?.parse().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn numeric_only() {
        let p = crate::parse::mtr_json::parse(include_str!("../../tests/fixtures/mtr-json-n.json"))
            .unwrap();
        assert_eq!(p.target.as_deref(), Some("1.1.1.1"));
        assert_eq!(p.hops.len(), 16);
        assert_eq!(p.hops[0].ip, Some("172.20.10.1".parse().unwrap()));
        assert_eq!(p.hops[1].hop, 2);
        assert_eq!(p.hops[1].ip, None);
        assert_eq!(p.hops[1].host, None);
        assert_eq!(p.hops[6].ip, None);
        let s = p.hops[13].stats.as_ref().unwrap();
        assert_eq!(s.loss, 66.666);
        assert_eq!(s.snt, 3);
        assert_eq!(p.hops[15].host, None);
    }

    #[test]
    fn with_reverse_names() {
        let p = crate::parse::mtr_json::parse(include_str!("../../tests/fixtures/mtr-json-b.json"))
            .unwrap();
        let last = p.hops.last().unwrap();
        assert_eq!(last.hop, 16);
        assert_eq!(last.ip, Some("1.1.1.1".parse().unwrap()));
        assert_eq!(last.host.as_deref(), Some("one.one.one.one"));
    }

    #[test]
    fn reported_asn() {
        let body = r#"{"report":{"mtr":{"dst":"x"},"hubs":[
            {"count":1,"host":"1.1.1.1","ASN":"AS13335","Loss%":0.0,"Snt":1,"Last":1.0,"Avg":1.0,"Best":1.0,"Wrst":1.0,"StDev":0.0},
            {"count":2,"host":"???","ASN":"AS???","Loss%":100.0,"Snt":1,"Last":0.0,"Avg":0.0,"Best":0.0,"Wrst":0.0,"StDev":0.0}]}}"#;
        let p = crate::parse::mtr_json::parse(body).unwrap();
        assert_eq!(p.hops[0].asn_reported, Some(13335));
        assert_eq!(p.hops[1].asn_reported, None);
    }
}
