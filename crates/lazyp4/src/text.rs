//! String handling for what the panels show: trimming a depot path, taking the
//! first line of a description, and the substring test behind `/`.
//!
//! Nothing here knows about the UI or about Perforce, so each piece can be
//! tested on its own.

/// Depot paths are long and share a prefix; the tail is what identifies them.
pub fn short_path(depot_path: &str) -> &str {
    depot_path.trim_start_matches('/')
}

/// First line of a description, for a one-line entry.
pub fn first_line(description: &str) -> &str {
    description.lines().next().unwrap_or_default().trim_end()
}

/// Whether a description says anything.
///
/// Perforce writes `<saved by Perforce>` itself when it shelves work into a
/// changelist you never described, so that placeholder counts as empty.
pub fn has_description(description: &str) -> bool {
    let text = description.trim();
    !text.is_empty() && text != "<saved by Perforce>"
}

/// Case-insensitive substring match, which is what `/` is for: narrowing a
/// long list quickly, not writing a pattern.
pub fn matches(filter: &str, haystack: &str) -> bool {
    filter.is_empty() || haystack.to_lowercase().contains(&filter.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placeholder_description_says_nothing() {
        // Perforce writes `<saved by Perforce>` itself; it is not a message.
        assert!(!has_description("<saved by Perforce>"));
        assert!(!has_description("   "));
        assert!(has_description("# Do not submit"));
    }

    #[test]
    fn a_description_is_shown_by_its_first_line() {
        assert_eq!(first_line("Fix the door\n\nDetail follows"), "Fix the door");
        assert_eq!(first_line("Trailing space  \nnext"), "Trailing space");
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn a_depot_path_loses_its_leading_slashes() {
        assert_eq!(short_path("//depot/main/Door.cpp"), "depot/main/Door.cpp");
    }

    #[test]
    fn a_filter_ignores_case() {
        assert!(matches("door", "//depot/main/Door.cpp"));
        assert!(matches("DOOR", "//depot/main/door.cpp"));
        assert!(!matches("window", "//depot/main/Door.cpp"));
    }

    #[test]
    fn an_empty_filter_matches_everything() {
        assert!(matches("", "//depot/main/Door.cpp"));
        assert!(matches("", ""));
    }
}
