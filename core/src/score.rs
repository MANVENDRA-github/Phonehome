//! Privacy scorecard (ARCHITECTURE §2.4).
//!
//! A pure, explainable 0–100 privacy-risk score per device (higher = more
//! concerning), blended from four normalized components. The [`Scorecard`]
//! ALWAYS carries its component values, the raw inputs, and the weights — there
//! is no unexplained number (SPEC M3). `blocked` is reported as context but is
//! deliberately NOT a risk component (a blocked query means the filter protected
//! you, which is ambiguous to score).
//!
//! Weights are PROVISIONAL (D-012): defensible defaults, sanity-checked against
//! the synthetic fixture's ranking, pending a real-household tuning pass. They
//! live in one struct so they are trivially adjustable.

/// Saturation caps — a device talking to this many distinct tracker entities /
/// countries scores the component at 100. Chosen for household scale.
const ENTITY_CAP: f64 = 8.0;
const COUNTRY_CAP: f64 = 6.0;
/// Query volume (log-scaled) that saturates the chattiness component.
const CHATTINESS_CAP: f64 = 5000.0;

/// Blend weights for the four components (need not sum to 1; the score divides
/// by their sum). Provisional — see D-012.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ScoreWeights {
    pub tracker_share: f64,
    pub entity_spread: f64,
    pub country_spread: f64,
    pub chattiness: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            tracker_share: 0.45,  // fraction of traffic going to trackers — the headline
            entity_spread: 0.25,  // how many distinct tracker companies it feeds
            country_spread: 0.15, // across how many jurisdictions
            chattiness: 0.15,     // sheer phone-home volume (log-scaled)
        }
    }
}

impl ScoreWeights {
    fn sum(&self) -> f64 {
        self.tracker_share + self.entity_spread + self.country_spread + self.chattiness
    }
}

/// Per-device aggregates over a window, fed to [`score`].
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ScoreInputs {
    pub total_queries: i64,
    pub blocked_queries: i64,
    pub tracker_queries: i64,
    pub distinct_tracker_entities: i64,
    pub distinct_countries: i64,
}

/// Each component normalized to 0–100 (what the weights blend).
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ScoreComponents {
    pub tracker_share: u32,
    pub entity_spread: u32,
    pub country_spread: u32,
    pub chattiness: u32,
}

/// The full, self-explaining scorecard.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Scorecard {
    pub score: u32,
    pub components: ScoreComponents,
    pub inputs: ScoreInputs,
    pub weights: ScoreWeights,
}

fn pct(x: f64) -> u32 {
    (x * 100.0).round().clamp(0.0, 100.0) as u32
}

/// Computes a device's privacy score from its aggregated inputs. Total of 0
/// yields a score of 0 with no division by zero.
pub fn score(inputs: ScoreInputs, weights: ScoreWeights) -> Scorecard {
    let total = inputs.total_queries.max(0) as f64;

    let tracker_share = if total > 0.0 {
        (inputs.tracker_queries.max(0) as f64 / total).min(1.0)
    } else {
        0.0
    };
    // Every component is gated on `total`, so a device with no traffic scores 0
    // even if a caller hands us nonzero spreads (`aggregate` never does, but
    // `score` is public and its contract above promises this).
    let entity_spread = if total > 0.0 {
        (inputs.distinct_tracker_entities.max(0) as f64 / ENTITY_CAP).min(1.0)
    } else {
        0.0
    };
    let country_spread = if total > 0.0 {
        (inputs.distinct_countries.max(0) as f64 / COUNTRY_CAP).min(1.0)
    } else {
        0.0
    };
    let chattiness = if total > 0.0 {
        ((total + 1.0).log10() / (CHATTINESS_CAP + 1.0).log10()).min(1.0)
    } else {
        0.0
    };

    let denom = weights.sum();
    let blended = if denom > 0.0 {
        (weights.tracker_share * tracker_share
            + weights.entity_spread * entity_spread
            + weights.country_spread * country_spread
            + weights.chattiness * chattiness)
            / denom
    } else {
        0.0
    };

    Scorecard {
        score: pct(blended),
        components: ScoreComponents {
            tracker_share: pct(tracker_share),
            entity_spread: pct(entity_spread),
            country_spread: pct(country_spread),
            chattiness: pct(chattiness),
        },
        inputs,
        weights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(total: i64, tracker: i64, entities: i64, countries: i64) -> ScoreInputs {
        ScoreInputs {
            total_queries: total,
            blocked_queries: 0,
            tracker_queries: tracker,
            distinct_tracker_entities: entities,
            distinct_countries: countries,
        }
    }

    #[test]
    fn empty_inputs_score_zero_no_panic() {
        let s = score(inputs(0, 0, 0, 0), ScoreWeights::default());
        assert_eq!(s.score, 0);
        assert_eq!(s.components.tracker_share, 0);
    }

    /// The spread components used to bypass the `total > 0` guard, so a device
    /// with zero traffic but nonzero spreads scored above 0 — contradicting the
    /// "total of 0 yields a score of 0" contract on `score`.
    #[test]
    fn zero_traffic_scores_zero_even_with_nonzero_spreads() {
        let s = score(inputs(0, 0, 8, 6), ScoreWeights::default());
        assert_eq!(s.score, 0);
        assert_eq!(s.components.entity_spread, 0);
        assert_eq!(s.components.country_spread, 0);
    }

    #[test]
    fn quiet_first_party_device_scores_low() {
        // 200 queries, none to trackers, one country.
        let s = score(inputs(200, 0, 0, 1), ScoreWeights::default());
        assert!(s.score < 25, "quiet device scored {}", s.score);
    }

    #[test]
    fn tracker_magnet_scores_high() {
        // Chatty, mostly trackers, many entities and countries.
        let s = score(inputs(2000, 1600, 8, 6), ScoreWeights::default());
        assert!(s.score > 70, "tracker magnet scored {}", s.score);
    }

    #[test]
    fn score_is_monotonic_in_tracker_share() {
        let w = ScoreWeights::default();
        let low = score(inputs(1000, 100, 2, 2), w).score;
        let mid = score(inputs(1000, 500, 2, 2), w).score;
        let high = score(inputs(1000, 900, 2, 2), w).score;
        assert!(low < mid && mid < high, "{low} < {mid} < {high}");
    }

    #[test]
    fn components_and_inputs_are_reported() {
        let s = score(inputs(1000, 250, 4, 3), ScoreWeights::default());
        // tracker_share input 250/1000 = 25%.
        assert_eq!(s.components.tracker_share, 25);
        // entity_spread 4/8 = 50%, country_spread 3/6 = 50%.
        assert_eq!(s.components.entity_spread, 50);
        assert_eq!(s.components.country_spread, 50);
        assert_eq!(s.inputs.tracker_queries, 250);
    }

    #[test]
    fn spreads_saturate_at_cap() {
        let s = score(inputs(1000, 100, 50, 40), ScoreWeights::default());
        assert_eq!(s.components.entity_spread, 100);
        assert_eq!(s.components.country_spread, 100);
    }
}
