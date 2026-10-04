#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    MtrJson,
    MtrText,
    TracerouteText,
}

impl Format {
    pub fn from_param(s: &str) -> Option<Self> {
        match s {
            "mtr-json" => Some(Self::MtrJson),
            "mtr-text" => Some(Self::MtrText),
            "traceroute-text" => Some(Self::TracerouteText),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stats {
    pub loss: f64,
    pub snt: u32,
    pub last: f64,
    pub avg: f64,
    pub best: f64,
    pub wrst: f64,
    pub stdev: f64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hop {
    pub hop: u32,
    pub ip: Option<std::net::IpAddr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn_reported: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asn: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<Stats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtts: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeouts: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AsInfo {
    pub asn: u32,
    pub holder: String,
    pub prefix: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Parsed {
    pub format: Format,
    pub target: Option<String>,
    pub hops: Vec<Hop>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Trace {
    pub ts: String,
    pub client: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub af: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<std::net::IpAddr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AsInfo>,
    pub as_path: Vec<String>,
    pub format: Format,
    pub hops: Vec<Hop>,
}
