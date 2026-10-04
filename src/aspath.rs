/// hop順にASNを並べ、連続する同じASNを畳む。RIPEstatの値が無いhopは入力側のASNで埋める
/// 同じhop番号に複数の応答元がある時は、ASNが分かる最初の応答元だけを使う
pub fn as_path(hops: &[crate::model::Hop]) -> Vec<String> {
    let mut path: Vec<u32> = Vec::new();
    let mut taken_hop = None;
    for h in hops {
        if taken_hop == Some(h.hop) {
            continue;
        }
        let Some(asn) = h.asn.or(h.asn_reported) else {
            continue;
        };
        taken_hop = Some(h.hop);
        if path.last() != Some(&asn) {
            path.push(asn);
        }
    }
    path.into_iter().map(|a| format!("AS{a}")).collect()
}

#[cfg(test)]
mod tests {
    fn hop(hop: u32, asn: Option<u32>, asn_reported: Option<u32>) -> crate::model::Hop {
        crate::model::Hop {
            hop,
            asn,
            asn_reported,
            ..Default::default()
        }
    }

    #[test]
    fn collapses_consecutive() {
        let hops = [
            hop(1, None, None),
            hop(2, Some(2516), None),
            hop(3, None, None),
            hop(4, Some(2516), None),
            hop(5, Some(2518), None),
            hop(6, None, Some(2518)),
            hop(7, None, Some(59105)),
            hop(8, Some(59128), Some(1)),
        ];
        assert_eq!(
            crate::aspath::as_path(&hops),
            vec!["AS2516", "AS2518", "AS59105", "AS59128"]
        );
    }

    #[test]
    fn ecmp_uses_first_responder_with_asn() {
        let hops = [
            hop(1, Some(10), None),
            hop(2, None, None),
            hop(2, Some(20), None),
            hop(2, Some(30), None),
            hop(3, Some(40), None),
        ];
        assert_eq!(crate::aspath::as_path(&hops), vec!["AS10", "AS20", "AS40"]);
    }

    #[test]
    fn empty() {
        assert!(crate::aspath::as_path(&[hop(1, None, None)]).is_empty());
    }
}
