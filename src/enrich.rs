const RIPESTAT_URL: &str = "https://stat.ripe.net/data/prefix-overview/data.json";
// RIPEstatは同じ送信元からの同時リクエストを8件までに制限している
const MAX_CONCURRENT_LOOKUPS: usize = 4;
// Lambdaのtimeout(30秒)内にS3への保存を終えられるよう、補完全体を打ち切る
const LOOKUP_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

// IANA Special-Purpose Address RegistryでGlobally Reachableでない範囲。192.0.0.9等の例外は無視して範囲ごと除く
const NON_GLOBAL_V4: &[(u32, u8)] = &[
    (0x0000_0000, 8),  // 0.0.0.0/8
    (0x0a00_0000, 8),  // 10.0.0.0/8
    (0x6440_0000, 10), // 100.64.0.0/10
    (0x7f00_0000, 8),  // 127.0.0.0/8
    (0xa9fe_0000, 16), // 169.254.0.0/16
    (0xac10_0000, 12), // 172.16.0.0/12
    (0xc000_0000, 24), // 192.0.0.0/24
    (0xc000_0200, 24), // 192.0.2.0/24
    (0xc058_6302, 32), // 192.88.99.2/32
    (0xc0a8_0000, 16), // 192.168.0.0/16
    (0xc612_0000, 15), // 198.18.0.0/15
    (0xc633_6400, 24), // 198.51.100.0/24
    (0xcb00_7100, 24), // 203.0.113.0/24
    (0xe000_0000, 3),  // 224.0.0.0/3 (224.0.0.0/4と240.0.0.0/4)
];

const NON_GLOBAL_V6: &[(u128, u8)] = &[
    (0, 127),                          // ::/128, ::1/128
    (0x0064_ff9b_0001 << 80, 48),      // 64:ff9b:1::/48
    (0x0100 << 112, 64),               // 100::/64
    ((0x0100 << 112) | (1 << 64), 64), // 100:0:0:1::/64
    (0x2001 << 112, 23),               // 2001::/23
    (0x2001_0db8 << 96, 32),           // 2001:db8::/32
    (0x3fff << 112, 20),               // 3fff::/20
    (0x5f00 << 112, 16),               // 5f00::/16
    (0xfc00 << 112, 7),                // fc00::/7
    (0xfe80 << 112, 10),               // fe80::/10
    (0xff00 << 112, 8),                // ff00::/8
];

fn in_prefixes<T>(addr: T, prefixes: &[(T, u8)]) -> bool
where
    T: Copy + PartialEq + std::ops::Shr<u32, Output = T>,
{
    let bits = (std::mem::size_of::<T>() * 8) as u32;
    prefixes
        .iter()
        .any(|&(net, len)| addr >> (bits - len as u32) == net >> (bits - len as u32))
}

/// 経路広告されうるアドレスか。RIPEstatに問い合わせる対象を絞るのに使う
pub fn is_global(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => !in_prefixes(v4.to_bits(), NON_GLOBAL_V4),
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_global(v4.into()),
            None => !in_prefixes(v6.to_bits(), NON_GLOBAL_V6),
        },
    }
}

#[derive(serde::Deserialize)]
struct PrefixOverview {
    data: PrefixOverviewData,
}

#[derive(serde::Deserialize)]
struct PrefixOverviewData {
    #[serde(default)]
    asns: Vec<PrefixOverviewAsn>,
    resource: String,
}

#[derive(serde::Deserialize)]
struct PrefixOverviewAsn {
    asn: u32,
    holder: String,
}

/// 広告されていないアドレスでは`asns`が空になり、`None`を返す
pub fn parse_prefix_overview(
    body: &[u8],
) -> Result<Option<crate::model::AsInfo>, serde_json::Error> {
    let r: PrefixOverview = serde_json::from_slice(body)?;
    Ok(r.data
        .asns
        .into_iter()
        .next()
        .map(|a| crate::model::AsInfo {
            asn: a.asn,
            holder: a.holder,
            prefix: r.data.resource,
        }))
}

pub struct Client {
    http: reqwest::Client,
}

impl Client {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client");
        Self { http }
    }

    async fn lookup(
        &self,
        ip: std::net::IpAddr,
    ) -> Result<Option<crate::model::AsInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let body = self
            .http
            .get(RIPESTAT_URL)
            .query(&[
                ("resource", ip.to_string().as_str()),
                ("sourceapp", "upstream-save"),
            ])
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(parse_prefix_overview(&body)?)
    }

    /// グローバルなアドレスだけを並列に引く。失敗したアドレスと期限までに終わらなかったアドレスは結果から外す
    pub async fn lookup_all(
        &self,
        ips: impl IntoIterator<Item = std::net::IpAddr>,
    ) -> std::collections::HashMap<std::net::IpAddr, crate::model::AsInfo> {
        use futures::StreamExt as _;
        let unique: std::collections::HashSet<std::net::IpAddr> =
            ips.into_iter().filter(|ip| is_global(*ip)).collect();
        let deadline = tokio::time::Instant::now() + LOOKUP_DEADLINE;
        let mut stream = futures::stream::iter(unique)
            .map(|ip| async move { (ip, self.lookup(ip).await) })
            .buffer_unordered(MAX_CONCURRENT_LOOKUPS);
        let mut results = Vec::new();
        loop {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(r)) => results.push(r),
                Ok(None) => break,
                Err(_) => {
                    tracing::warn!("RIPEstat lookups hit the deadline");
                    break;
                }
            }
        }
        results
            .into_iter()
            .filter_map(|(ip, r)| match r {
                Ok(info) => info.map(|i| (ip, i)),
                Err(e) => {
                    tracing::warn!(%ip, error = %e, "RIPEstat lookup failed");
                    None
                }
            })
            .collect()
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

pub fn apply(
    hops: &mut [crate::model::Hop],
    infos: &std::collections::HashMap<std::net::IpAddr, crate::model::AsInfo>,
) {
    for hop in hops {
        if let Some(info) = hop.ip.and_then(|ip| infos.get(&ip)) {
            hop.asn = Some(info.asn);
            hop.holder = Some(info.holder.clone());
            hop.prefix = Some(info.prefix.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    fn global(s: &str) -> bool {
        crate::enrich::is_global(s.parse().unwrap())
    }

    #[test]
    fn non_global_addresses() {
        for s in [
            "172.20.10.1",
            "10.60.84.249",
            "172.25.208.5",
            "192.168.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "169.254.1.1",
            "198.18.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "fe80::1",
            "fd00::1",
            "::1",
            "::ffff:10.0.0.1",
            "2001:db8::1",
            "192.0.0.170",
            "224.0.0.1",
            "100::1",
            "2001:2::1",
            "64:ff9b:1::a00:1",
            "3fff::1",
            "ff02::1",
        ] {
            assert!(!global(s), "{s}");
        }
    }

    #[test]
    fn global_addresses() {
        for s in [
            "1.1.1.1",
            "192.0.1.1",
            "198.20.0.1",
            "64:ff9b::808:808",
            "2001:200::1",
            "27.86.110.166",
            "100.128.0.1",
            "2001:268:f309:d001::111",
            "::ffff:1.1.1.1",
        ] {
            assert!(global(s), "{s}");
        }
    }

    #[test]
    fn prefix_overview_announced() {
        let info = crate::enrich::parse_prefix_overview(include_bytes!(
            "../tests/fixtures/ripestat-prefix-overview.json"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(info.asn, 2516);
        assert_eq!(info.holder, "KDDI - KDDI CORPORATION");
        assert_eq!(info.prefix, "27.86.0.0/16");
    }

    #[test]
    fn prefix_overview_not_announced() {
        let body = br#"{"data":{"asns":[],"resource":"10.60.84.249","announced":false}}"#;
        assert_eq!(crate::enrich::parse_prefix_overview(body).unwrap(), None);
    }

    #[test]
    fn apply_fills_hops() {
        let ip: std::net::IpAddr = "27.86.110.166".parse().unwrap();
        let info = crate::model::AsInfo {
            asn: 2516,
            holder: "KDDI".into(),
            prefix: "27.86.0.0/16".into(),
        };
        let infos = std::collections::HashMap::from([(ip, info)]);
        let mut hops = vec![
            crate::model::Hop {
                hop: 1,
                ip: Some(ip),
                ..Default::default()
            },
            crate::model::Hop {
                hop: 2,
                ..Default::default()
            },
        ];
        crate::enrich::apply(&mut hops, &infos);
        assert_eq!(hops[0].asn, Some(2516));
        assert_eq!(hops[0].prefix.as_deref(), Some("27.86.0.0/16"));
        assert_eq!(hops[1].asn, None);
    }
}
