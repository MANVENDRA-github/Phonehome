//! Destination enrichment (ARCHITECTURE §2.3): domain → owning entity, category,
//! country, and tracker-ness.
//!
//! Enrichment is PURE and offline — it reads two bundled curated seeds
//! (`entities.toml`, `trackers.txt`) with no network calls, so it runs inside the
//! ingestion transaction (D-005 stays intact). Country comes from the entity map,
//! not GeoIP (D-011). Both seeds share the shape of the real datasets they stand
//! in for, so the full `entities` map / oisd blocklist drop in unchanged.

use serde::Deserialize;
use std::collections::HashSet;
use std::sync::OnceLock;

const ENTITIES_TOML: &str = include_str!("../data/entities.toml");
const TRACKERS_TXT: &str = include_str!("../data/trackers.txt");

/// Destination category, coarsest privacy-relevant grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// The service the user is deliberately using (Netflix, GitHub).
    FirstParty,
    /// Necessary infrastructure (NTP, push, cert checks, config).
    Functional,
    /// Content-delivery / media.
    Cdn,
    /// Device/OS phone-home metrics.
    Telemetry,
    /// Usage analytics / measurement SDKs.
    Analytics,
    /// Ad networks.
    Advertising,
    /// No entity match.
    Unknown,
}

impl Category {
    /// Whether this category, on its own, marks a destination as a tracker.
    pub fn is_tracking(self) -> bool {
        matches!(
            self,
            Category::Advertising | Category::Analytics | Category::Telemetry
        )
    }
}

/// The enrichment attached to a destination domain.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Enrichment {
    pub entity: Option<String>,
    pub category: Category,
    pub country: Option<String>,
    /// `true` when the category is tracking OR the domain is on the blocklist.
    pub is_tracker: bool,
    pub on_blocklist: bool,
}

#[derive(Deserialize)]
struct EntitiesFile {
    entity: Vec<EntityEntry>,
}

#[derive(Deserialize)]
struct EntityEntry {
    suffix: String,
    name: String,
    category: Category,
    country: Option<String>,
}

fn entities() -> &'static [EntityEntry] {
    static ENTITIES: OnceLock<Vec<EntityEntry>> = OnceLock::new();
    ENTITIES.get_or_init(|| {
        toml::from_str::<EntitiesFile>(ENTITIES_TOML)
            .expect("bundled entities.toml must parse")
            .entity
    })
}

fn trackers() -> &'static HashSet<String> {
    static TRACKERS: OnceLock<HashSet<String>> = OnceLock::new();
    TRACKERS.get_or_init(|| {
        TRACKERS_TXT
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_ascii_lowercase)
            .collect()
    })
}

/// True when `domain` equals `suffix` or is a dot-subdomain of it.
fn suffix_matches(domain: &str, suffix: &str) -> bool {
    domain == suffix || domain.ends_with(&format!(".{suffix}"))
}

/// Whether `domain` (or a parent) is on the bundled blocklist.
fn on_blocklist(domain: &str) -> bool {
    let table = trackers();
    // Walk parent domains: a.b.c.com -> b.c.com -> c.com …
    let mut d = domain;
    loop {
        if table.contains(d) {
            return true;
        }
        match d.split_once('.') {
            Some((_, rest)) if rest.contains('.') => d = rest,
            _ => return false,
        }
    }
}

/// Enriches a domain. Always returns a value — an unmapped domain yields
/// `Category::Unknown` with `entity: None` (explicit, per SPEC M3 acceptance).
pub fn enrich(domain: &str) -> Enrichment {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();

    // Longest matching suffix wins, so specific hostnames beat base domains.
    let best = entities()
        .iter()
        .filter(|e| suffix_matches(&domain, &e.suffix.to_ascii_lowercase()))
        .max_by_key(|e| e.suffix.len());

    let blocked = on_blocklist(&domain);
    match best {
        Some(e) => Enrichment {
            entity: Some(e.name.clone()),
            category: e.category,
            country: e.country.clone(),
            is_tracker: e.category.is_tracking() || blocked,
            on_blocklist: blocked,
        },
        None => Enrichment {
            entity: None,
            category: Category::Unknown,
            country: None,
            is_tracker: blocked,
            on_blocklist: blocked,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_seeds_parse() {
        assert!(entities().len() >= 75, "entity seed unexpectedly small");
        assert!(!trackers().is_empty());
    }

    #[test]
    fn maps_entity_category_and_country() {
        let e = enrich("samsungads.com");
        assert_eq!(e.entity.as_deref(), Some("Samsung Ads"));
        assert_eq!(e.category, Category::Advertising);
        assert_eq!(e.country.as_deref(), Some("KR"));
        assert!(e.is_tracker);
    }

    #[test]
    fn longest_suffix_wins() {
        // googleads.g.doubleclick.net has its own entry (Google Ads); the base
        // doubleclick.net entry must not shadow it.
        let specific = enrich("googleads.g.doubleclick.net");
        assert_eq!(specific.entity.as_deref(), Some("Google Ads"));
        // A different subdomain of the base falls back to the base entry.
        let base = enrich("static.doubleclick.net");
        assert_eq!(base.entity.as_deref(), Some("Google DoubleClick"));
    }

    #[test]
    fn subdomain_of_mapped_host_inherits() {
        let e = enrich("edge.device-metrics-us.amazon.com");
        assert_eq!(e.category, Category::Telemetry);
        assert!(e.is_tracker);
    }

    #[test]
    fn functional_and_first_party_are_not_trackers() {
        assert!(!enrich("api.github.com").is_tracker);
        assert!(!enrich("netflix.com").is_tracker);
        assert_eq!(enrich("crates.io").category, Category::FirstParty);
    }

    #[test]
    fn unknown_domain_is_explicit() {
        let e = enrich("totally-unknown-domain-xyz.example");
        assert_eq!(e.entity, None);
        assert_eq!(e.category, Category::Unknown);
        assert_eq!(e.country, None);
        assert!(!e.is_tracker);
    }

    #[test]
    fn blocklist_flags_tracker_without_entity() {
        // On trackers.txt, and (for this domain) also has no analytics entity —
        // blocklist membership alone marks it a tracker.
        let e = enrich("scorecardresearch.com");
        assert!(e.on_blocklist);
        assert!(e.is_tracker);
    }

    #[test]
    fn trailing_dot_and_case_normalized() {
        assert_eq!(
            enrich("SAMSUNGADS.COM.").entity.as_deref(),
            Some("Samsung Ads")
        );
    }
}
