//! Device display-name resolution (ARCHITECTURE §2.2 precedence).
//!
//! Precedence, highest first:
//!   user-assigned > DHCP hostname > mDNS name > vendor + short MAC id > raw identity.

/// Computes a device's human label from its known name sources.
///
/// `identity_key` is the always-present fallback (a MAC or an IP). `mac` is used
/// only to append a short disambiguator to the vendor tier (e.g. two Samsung TVs
/// become "Samsung Electronics · 22:33" and "… · 44:55").
pub fn display_name(
    name_user: Option<&str>,
    name_dhcp: Option<&str>,
    name_mdns: Option<&str>,
    oui_vendor: Option<&str>,
    mac: Option<&str>,
    identity_key: &str,
) -> String {
    if let Some(n) = present(name_user) {
        return n.to_owned();
    }
    if let Some(n) = present(name_dhcp) {
        return n.to_owned();
    }
    if let Some(n) = present(name_mdns) {
        return n.to_owned();
    }
    if let Some(vendor) = present(oui_vendor) {
        return match mac.and_then(short_mac_suffix) {
            Some(suffix) => format!("{vendor} · {suffix}"),
            None => vendor.to_owned(),
        };
    }
    identity_key.to_owned()
}

/// A trimmed, non-empty view of an optional name, else `None`.
fn present(o: Option<&str>) -> Option<&str> {
    o.map(str::trim).filter(|s| !s.is_empty())
}

/// Last two octets of a MAC ("aa:bb:cc:dd:ee:ff" -> "ee:ff") for disambiguation.
fn short_mac_suffix(mac: &str) -> Option<String> {
    let octets: Vec<&str> = mac.split(':').collect();
    (octets.len() == 6).then(|| format!("{}:{}", octets[4], octets[5]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: &str = "f0:5c:77:11:22:33";

    #[test]
    fn user_name_wins_over_everything() {
        let n = display_name(
            Some("Living Room TV"),
            Some("samsung-tv"),
            Some("Samsung.local"),
            Some("Samsung Electronics"),
            Some(MAC),
            MAC,
        );
        assert_eq!(n, "Living Room TV");
    }

    #[test]
    fn dhcp_then_mdns_then_vendor() {
        assert_eq!(
            display_name(
                None,
                Some("samsung-tv"),
                Some("x.local"),
                Some("Samsung"),
                Some(MAC),
                MAC
            ),
            "samsung-tv"
        );
        assert_eq!(
            display_name(None, None, Some("x.local"), Some("Samsung"), Some(MAC), MAC),
            "x.local"
        );
        assert_eq!(
            display_name(
                None,
                None,
                None,
                Some("Samsung Electronics"),
                Some(MAC),
                MAC
            ),
            "Samsung Electronics · 22:33"
        );
    }

    #[test]
    fn vendor_without_mac_omits_suffix() {
        assert_eq!(
            display_name(None, None, None, Some("Samsung"), None, "192.168.1.9"),
            "Samsung"
        );
    }

    #[test]
    fn falls_back_to_identity_key() {
        assert_eq!(
            display_name(None, None, None, None, None, "192.168.1.50"),
            "192.168.1.50"
        );
    }

    #[test]
    fn blank_names_are_ignored() {
        assert_eq!(
            display_name(Some("  "), Some(""), None, None, None, "192.168.1.7"),
            "192.168.1.7"
        );
    }
}
