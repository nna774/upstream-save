/// hop順にASNを並べ、連続する同じASNを畳む。RIPEstatの値が無いhopは入力側のASNで埋める
pub fn as_path(hops: &[crate::model::Hop]) -> Vec<String> {
    let mut path: Vec<u32> = Vec::new();
    for asn in hops.iter().filter_map(|h| h.asn.or(h.asn_reported)) {
        if path.last() != Some(&asn) {
            path.push(asn);
        }
    }
    path.into_iter().map(|a| format!("AS{a}")).collect()
}

#[cfg(test)]
mod tests {
    fn hop(asn: Option<u32>, asn_reported: Option<u32>) -> crate::model::Hop {
        crate::model::Hop {
            asn,
            asn_reported,
            ..Default::default()
        }
    }

    #[test]
    fn collapses_consecutive() {
        let hops = [
            hop(None, None),
            hop(Some(2516), None),
            hop(None, None),
            hop(Some(2516), None),
            hop(Some(2518), None),
            hop(None, Some(2518)),
            hop(None, Some(59105)),
            hop(Some(59128), Some(1)),
        ];
        assert_eq!(
            crate::aspath::as_path(&hops),
            vec!["AS2516", "AS2518", "AS59105", "AS59128"]
        );
    }

    #[test]
    fn empty() {
        assert!(crate::aspath::as_path(&[hop(None, None)]).is_empty());
    }
}
