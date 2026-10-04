/// `<YYYY-MM>/<id>`で記録を指す。URLから受け取るので、S3キーに埋める前に形式を検証する
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TraceRef {
    month: String,
    id: String,
}

static REF_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^(\d{4}-(?:0[1-9]|1[0-2]))/(\d{8}T\d{6}Z-[a-z0-9-]+?(?:-v[46])?)$").unwrap()
});

impl TraceRef {
    pub fn parse(s: &str) -> Option<Self> {
        let c = REF_RE.captures(s)?;
        Some(Self {
            month: c[1].to_owned(),
            id: c[2].to_owned(),
        })
    }

    /// `traces/<YYYY-MM>/<id>.json`から作る
    pub fn from_trace_key(key: &str) -> Option<Self> {
        Self::parse(key.strip_prefix("traces/")?.strip_suffix(".json")?)
    }

    /// `public/<YYYY-MM>/<id>`から作る
    pub fn from_public_key(key: &str) -> Option<Self> {
        Self::parse(key.strip_prefix("public/")?)
    }

    pub fn trace_key(&self) -> String {
        format!("traces/{self}.json")
    }

    pub fn public_key(&self) -> String {
        format!("public/{self}")
    }
}

impl std::fmt::Display for TraceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.month, self.id)
    }
}

impl serde::Serialize for TraceRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// 閲覧のAPIで、誰として読んでいるか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// 閲覧用トークンを持っていて、全件を読める
    Viewer,
    /// トークン無しで、公開した記録だけを読める
    Anonymous,
}

/// トークンが送られてきて閲覧用として通らない時は、公開分に落とさず`None`を返す
pub fn access(viewers: &crate::auth::Tokens, token: Option<&str>) -> Option<Access> {
    match token {
        None => Some(Access::Anonymous),
        Some(t) => viewers.authenticate(t).map(|_| Access::Viewer),
    }
}

/// CloudFrontがオリジンへのリクエストに付ける秘密のヘッダを照合する。Function URLを直接叩いた閲覧はキャッシュを素通りするので拒む
pub fn verify_origin_secret(secrets: &crate::auth::Tokens, header: Option<&str>) -> bool {
    header.is_some_and(|h| secrets.authenticate(h).is_some())
}

/// CloudFrontに1日残す。閲覧者への応答はCloudFrontがno-storeに書き換える
pub const SHARED_CACHE: &str = "public, s-maxage=86400";

/// 閲覧用トークンで読んだ応答は、非公開の記録を含むのでどこにも残さない
pub fn cache_control(access: Access) -> &'static str {
    match access {
        Access::Viewer => "no-store",
        Access::Anonymous => SHARED_CACHE,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Summary {
    pub key: TraceRef,
    pub ts: String,
    pub client: String,
    pub label: Option<String>,
    pub af: Option<u8>,
    pub target: Option<String>,
    pub source: Option<crate::model::AsInfo>,
    pub as_path: Vec<String>,
    pub hop_count: usize,
    pub public: bool,
}

impl Summary {
    pub fn new(key: TraceRef, trace: crate::model::Trace, public: bool) -> Self {
        Self {
            key,
            hop_count: hop_count(&trace.hops),
            ts: trace.ts,
            client: trace.client,
            label: trace.label,
            af: trace.af,
            target: trace.target,
            source: trace.source,
            as_path: trace.as_path,
            public,
        }
    }
}

/// 同じhop番号に複数の応答元がある時も1つと数える
pub fn hop_count(hops: &[crate::model::Hop]) -> usize {
    hops.iter()
        .map(|h| h.hop)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_ref() {
        let r = crate::view::TraceRef::parse("2026-10/20261004T100827Z-mbp-v6").unwrap();
        assert_eq!(r.to_string(), "2026-10/20261004T100827Z-mbp-v6");
        assert_eq!(r.trace_key(), "traces/2026-10/20261004T100827Z-mbp-v6.json");
        assert_eq!(r.public_key(), "public/2026-10/20261004T100827Z-mbp-v6");
        assert!(crate::view::TraceRef::parse("2026-10/20261004T100827Z-iphone").is_some());
    }

    #[test]
    fn reject_malformed_ref() {
        for s in [
            "",
            "2026-13/20261004T100827Z-mbp-v6",
            "2026-10/../20261004T100827Z-mbp-v6",
            "2026-10/20261004T100827Z-mbp-v6/",
            "/2026-10/20261004T100827Z-mbp-v6",
            "2026-10/20261004T100827Z-MBP",
            "2026-10/20261004T100827Z-mbp.json",
            "2026-10/2026-10/20261004T100827Z-mbp",
        ] {
            assert_eq!(crate::view::TraceRef::parse(s), None, "{s}");
        }
    }

    #[test]
    fn ref_from_keys() {
        let r = crate::view::TraceRef::parse("2026-10/20261004T100827Z-mbp-v6").unwrap();
        assert_eq!(
            crate::view::TraceRef::from_trace_key(&r.trace_key()),
            Some(r.clone())
        );
        assert_eq!(
            crate::view::TraceRef::from_public_key(&r.public_key()),
            Some(r)
        );
        assert_eq!(
            crate::view::TraceRef::from_trace_key("raw/2026-10/20261004T100827Z-mbp-v6.txt"),
            None
        );
    }

    #[test]
    fn access_by_token() {
        // sha256("secret")
        let viewers = crate::auth::Tokens::from_json(
            r#"{"me":"2bb80d537b1da3e38bd30361aa855686bde0eacd7162fef6a25fe97bf527a25b"}"#,
        )
        .unwrap();
        assert_eq!(
            crate::view::access(&viewers, None),
            Some(crate::view::Access::Anonymous)
        );
        assert_eq!(
            crate::view::access(&viewers, Some("secret")),
            Some(crate::view::Access::Viewer)
        );
        assert_eq!(crate::view::access(&viewers, Some("wrong")), None);
        assert_eq!(crate::view::access(&viewers, Some("")), None);
    }

    #[test]
    fn origin_secret() {
        // sha256("secret")
        let secrets = crate::auth::Tokens::from_json(
            r#"{"cloudfront":"2bb80d537b1da3e38bd30361aa855686bde0eacd7162fef6a25fe97bf527a25b"}"#,
        )
        .unwrap();
        assert!(crate::view::verify_origin_secret(&secrets, Some("secret")));
        assert!(!crate::view::verify_origin_secret(&secrets, Some("wrong")));
        assert!(!crate::view::verify_origin_secret(&secrets, Some("")));
        assert!(!crate::view::verify_origin_secret(&secrets, None));
    }

    #[test]
    fn only_anonymous_api_responses_are_shared() {
        assert_eq!(
            crate::view::cache_control(crate::view::Access::Viewer),
            "no-store"
        );
        assert_eq!(
            crate::view::cache_control(crate::view::Access::Anonymous),
            crate::view::SHARED_CACHE
        );
    }

    #[test]
    fn summary_counts_distinct_hops() {
        let trace: crate::model::Trace = serde_json::from_value(serde_json::json!({
            "ts": "2026-10-04T10:08:27Z",
            "client": "mbp",
            "label": "povo",
            "af": 6,
            "as_path": ["AS2516"],
            "format": "traceroute-text",
            "hops": [
                {"hop": 1, "ip": null},
                {"hop": 2, "ip": "2001:db8::1"},
                {"hop": 2, "ip": "2001:db8::2"},
            ],
        }))
        .unwrap();
        let key = crate::view::TraceRef::parse("2026-10/20261004T100827Z-mbp-v6").unwrap();
        let s = crate::view::Summary::new(key, trace, true);
        assert_eq!(s.hop_count, 2);
        assert_eq!(s.label.as_deref(), Some("povo"));
        assert!(s.public);
        assert_eq!(
            serde_json::to_value(&s).unwrap()["key"],
            "2026-10/20261004T100827Z-mbp-v6"
        );
    }
}
