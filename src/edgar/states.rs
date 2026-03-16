use std::collections::BTreeMap;
use std::sync::LazyLock;

static REGION_TO_STATE: LazyLock<Vec<(String, String)>> = LazyLock::new(|| {
    let toml_str = include_str!("../../regions.toml");
    let table: BTreeMap<String, BTreeMap<String, String>> =
        toml::from_str(toml_str).expect("failed to parse regions.toml");
    let regions = table.get("regions").expect("missing [regions] table");
    // Sort by city name length descending so longer matches take priority
    let mut entries: Vec<(String, String)> = regions
        .iter()
        .map(|(city, state)| (city.clone(), state.clone()))
        .collect();
    entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    entries
});

pub fn guess_state_from_name(name: &str) -> String {
    let upper = name.to_uppercase();

    // Check for explicit "STATE OF X" patterns first
    if let Some(rest) = upper.strip_prefix("STATE OF ")
        && let Some(state) = match_state_name(rest)
    {
        return state;
    }

    // Check for state abbreviations at the end: "... TX", "... CA"
    let words: Vec<&str> = upper.split_whitespace().collect();
    if words.len() >= 2
        && let Some(state) = abbrev_to_state(words[words.len() - 1])
    {
        return state;
    }

    // Check for state names anywhere in the name
    for (state_name, state) in STATE_NAMES {
        if upper.contains(state_name) {
            return state.to_string();
        }
    }

    // Check for well-known region patterns from regions.toml
    for (region, state) in REGION_TO_STATE.iter() {
        if upper.contains(region.as_str()) {
            return state.clone();
        }
    }

    "Unknown".to_string()
}

fn match_state_name(text: &str) -> Option<String> {
    for (name, abbrev) in STATE_NAMES {
        if text.starts_with(name) {
            return Some(abbrev.to_string());
        }
    }
    None
}

fn abbrev_to_state(abbrev: &str) -> Option<String> {
    for (_, st) in STATE_NAMES {
        if abbrev == *st {
            return Some(st.to_string());
        }
    }
    match abbrev {
        "DC" | "PR" | "GU" | "VI" | "AS" | "MP" => Some(abbrev.to_string()),
        _ => None,
    }
}

const STATE_NAMES: &[(&str, &str)] = &[
    ("ALABAMA", "AL"),
    ("ALASKA", "AK"),
    ("ARIZONA", "AZ"),
    ("ARKANSAS", "AR"),
    ("CALIFORNIA", "CA"),
    ("COLORADO", "CO"),
    ("CONNECTICUT", "CT"),
    ("DELAWARE", "DE"),
    ("FLORIDA", "FL"),
    ("GEORGIA", "GA"),
    ("HAWAII", "HI"),
    ("IDAHO", "ID"),
    ("ILLINOIS", "IL"),
    ("INDIANA", "IN"),
    ("IOWA", "IA"),
    ("KANSAS", "KS"),
    ("KENTUCKY", "KY"),
    ("LOUISIANA", "LA"),
    ("MAINE", "ME"),
    ("MARYLAND", "MD"),
    ("MASSACHUSETTS", "MA"),
    ("MICHIGAN", "MI"),
    ("MINNESOTA", "MN"),
    ("MISSISSIPPI", "MS"),
    ("MISSOURI", "MO"),
    ("MONTANA", "MT"),
    ("NEBRASKA", "NE"),
    ("NEVADA", "NV"),
    ("NEW HAMPSHIRE", "NH"),
    ("NEW JERSEY", "NJ"),
    ("NEW MEXICO", "NM"),
    ("NEW YORK", "NY"),
    ("NORTH CAROLINA", "NC"),
    ("NORTH DAKOTA", "ND"),
    ("OHIO", "OH"),
    ("OKLAHOMA", "OK"),
    ("OREGON", "OR"),
    ("PENNSYLVANIA", "PA"),
    ("RHODE ISLAND", "RI"),
    ("SOUTH CAROLINA", "SC"),
    ("SOUTH DAKOTA", "SD"),
    ("TENNESSEE", "TN"),
    ("TEXAS", "TX"),
    ("UTAH", "UT"),
    ("VERMONT", "VT"),
    ("VIRGINIA", "VA"),
    ("WASHINGTON", "WA"),
    ("WEST VIRGINIA", "WV"),
    ("WISCONSIN", "WI"),
    ("WYOMING", "WY"),
    ("DISTRICT OF COLUMBIA", "DC"),
    ("PUERTO RICO", "PR"),
    ("GUAM", "GU"),
    ("VIRGIN ISLANDS", "VI"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explicit_prefix() {
        assert_eq!(guess_state_from_name("State of California"), "CA");
        assert_eq!(guess_state_from_name("State of New York"), "NY");
        assert_eq!(
            guess_state_from_name("State of Texas General Obligation"),
            "TX"
        );
    }

    #[test]
    fn test_abbreviation_suffix() {
        assert_eq!(guess_state_from_name("Some Issuer CA"), "CA");
        assert_eq!(guess_state_from_name("County Revenue NY"), "NY");
        assert_eq!(guess_state_from_name("Harris County TX"), "TX");
    }

    #[test]
    fn test_name_in_text() {
        assert_eq!(guess_state_from_name("Oregon Health Authority"), "OR");
        assert_eq!(guess_state_from_name("Connecticut Housing Finance"), "CT");
        assert_eq!(
            guess_state_from_name("Massachusetts Bay Transportation"),
            "MA"
        );
    }

    #[test]
    fn test_city() {
        assert_eq!(
            guess_state_from_name("New York City Transitional Finance"),
            "NY"
        );
        assert_eq!(guess_state_from_name("Chicago Board of Education"), "IL");
        assert_eq!(
            guess_state_from_name("Los Angeles Unified School District"),
            "CA"
        );
    }

    #[test]
    fn test_unknown() {
        assert_eq!(
            guess_state_from_name("Acalanes Union High School District"),
            "Unknown"
        );
        assert_eq!(
            guess_state_from_name("Generic Municipal Authority"),
            "Unknown"
        );
    }

    #[test]
    fn test_district_of_columbia() {
        assert_eq!(guess_state_from_name("District of Columbia Water"), "DC");
        assert_eq!(guess_state_from_name("Some Bond DC"), "DC");
    }

    #[test]
    fn test_regions_toml_loaded() {
        // Verify the TOML file loads and has entries
        assert!(!REGION_TO_STATE.is_empty());
        // Check a known entry
        assert!(
            REGION_TO_STATE
                .iter()
                .any(|(r, s)| r == "PORTLAND" && s == "OR")
        );
    }
}
