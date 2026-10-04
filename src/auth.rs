/// クライアント名とトークンのSHA-256の対応表。平文のトークンは持たない
pub struct Tokens {
    name_by_hash: std::collections::HashMap<String, String>,
}

impl Tokens {
    /// `{"<name>": "<sha256 hex>", ...}`の形のJSONを読む
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        let by_name: std::collections::HashMap<String, String> = serde_json::from_str(s)?;
        let name_by_hash = by_name
            .into_iter()
            .map(|(name, hash)| (hash.to_ascii_lowercase(), name))
            .collect();
        Ok(Self { name_by_hash })
    }

    pub fn authenticate(&self, token: &str) -> Option<&str> {
        use sha2::Digest as _;
        let hash = hex::encode(sha2::Sha256::digest(token.as_bytes()));
        self.name_by_hash.get(&hash).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lookup_by_hash() {
        // sha256("secret")
        let t = crate::auth::Tokens::from_json(
            r#"{"mbp":"2BB80D537B1DA3E38BD30361AA855686BDE0EACD7162FEF6A25FE97BF527A25B"}"#,
        )
        .unwrap();
        assert_eq!(t.authenticate("secret"), Some("mbp"));
        assert_eq!(t.authenticate("wrong"), None);
        assert_eq!(t.authenticate(""), None);
    }
}
