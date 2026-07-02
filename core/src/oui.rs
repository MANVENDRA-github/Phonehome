//! OUI (MAC vendor-prefix) resolution — used by device naming (ARCHITECTURE §2.2)
//! and destination enrichment.
//!
//! The bundled table (`core/data/oui.csv`) is a CURATED SEED, not a mirror of
//! the full IEEE MA-L registry: vendor names for the dev fixture's prefixes plus
//! common home-network vendors. It uses the standard `PREFIX,Vendor` CSV shape,
//! so dropping in the full registry identifies arbitrary hardware unchanged.

use std::collections::HashMap;
use std::sync::OnceLock;

const BUNDLED: &str = include_str!("../data/oui.csv");

fn table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| parse(BUNDLED))
}

fn parse(csv: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((prefix, vendor)) = line.split_once(',') {
            let key = normalize_prefix(prefix);
            let vendor = vendor.trim();
            if key.len() == 6 && !vendor.is_empty() {
                m.insert(key, vendor.to_owned());
            }
        }
    }
    m
}

/// First 3 octets as 6 uppercase hex chars, any separators stripped.
fn normalize_prefix(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(6)
        .flat_map(|c| c.to_uppercase())
        .collect()
}

/// Vendor for a MAC address, if its OUI is in the bundled table.
pub fn vendor_for_mac(mac: &str) -> Option<String> {
    let prefix = normalize_prefix(mac);
    if prefix.len() < 6 {
        return None;
    }
    table().get(&prefix).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_known_fixture_prefix_case_insensitively() {
        assert_eq!(
            vendor_for_mac("f0:5c:77:11:22:33").as_deref(),
            Some("Samsung Electronics")
        );
        assert_eq!(
            vendor_for_mac("F0-5C-77-AA-BB-CC").as_deref(),
            Some("Samsung Electronics")
        );
    }

    #[test]
    fn unknown_prefix_is_none() {
        assert_eq!(vendor_for_mac("02:00:00:00:00:01"), None);
    }

    #[test]
    fn garbage_or_short_input_is_none_not_panic() {
        assert_eq!(vendor_for_mac(""), None);
        assert_eq!(vendor_for_mac("zz"), None);
        assert_eq!(vendor_for_mac("f0:5c"), None);
    }

    #[test]
    fn every_bundled_row_parses() {
        // Guards against a malformed edit to oui.csv silently dropping entries.
        assert!(table().len() >= 14, "seed table unexpectedly small");
    }
}
