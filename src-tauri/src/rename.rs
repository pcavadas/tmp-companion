//! Bulk rename: compute new preset display names by find-replace, template, or
//! sequential numbering, with name-length validation.
//!
//! The LIVE write is `proto::rename_current_preset` (+ save), golden-tested in
//! `proto.rs`. This module owns the name *computation* + validation (the testable
//! core). The firmware name-length cap is a parameter the app supplies.

/// How to derive a preset's new name. `index` (0-based, supplied at apply) feeds the
/// `{n}` token and the `Number` start offset.
#[derive(Debug, Clone, PartialEq)]
pub enum RenameSpec {
    /// Replace the first occurrence of `from` with `to` (case-sensitive).
    FindReplace { from: String, to: String },
    /// A template with `{name}` (original name) and `{n}` (1-based position) tokens.
    Template { pattern: String },
    /// Prefix a zero-padded sequence number: `"{NN}. {name}"`, width `width`, from `start`.
    Number { width: usize, start: u32 },
}

/// Compute the new name for `name` at 0-based `index` under `spec`.
pub fn apply_rename(name: &str, index: usize, spec: &RenameSpec) -> String {
    match spec {
        RenameSpec::FindReplace { from, to } => {
            if from.is_empty() {
                name.to_string()
            } else {
                name.replacen(from, to, 1)
            }
        }
        RenameSpec::Template { pattern } => pattern
            .replace("{name}", name)
            .replace("{n}", &(index + 1).to_string()),
        RenameSpec::Number { width, start } => {
            let num = *start + index as u32;
            format!("{num:0width$}. {name}", width = *width)
        }
    }
}

/// Validate a computed name: non-empty after trimming, and within `max` chars.
pub fn validate_name(name: &str, max: usize) -> Result<(), String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("name is empty".into());
    }
    if t.chars().count() > max {
        return Err(format!(
            "name '{t}' is {} chars, exceeds the {max}-char limit",
            t.chars().count()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // AC1 — template / find-replace / number compute the expected names.
    #[test]
    fn rename_template_findreplace_number() {
        // find-replace (first occurrence)
        assert_eq!(
            apply_rename(
                "Clean Lead Boost",
                0,
                &RenameSpec::FindReplace {
                    from: "Lead".into(),
                    to: "Rhythm".into()
                }
            ),
            "Clean Rhythm Boost"
        );
        // template with {name} + {n} (1-based)
        assert_eq!(
            apply_rename(
                "Twin",
                2,
                &RenameSpec::Template {
                    pattern: "{n}. {name}".into()
                }
            ),
            "3. Twin"
        );
        // zero-padded numbering from a start offset
        assert_eq!(
            apply_rename("Twin", 0, &RenameSpec::Number { width: 2, start: 1 }),
            "01. Twin"
        );
        assert_eq!(
            apply_rename("Orange", 9, &RenameSpec::Number { width: 2, start: 1 }),
            "10. Orange"
        );
    }

    // AC — name-length validation (empty + over-limit).
    #[test]
    fn name_length_validation() {
        assert!(validate_name("", 24).is_err());
        assert!(validate_name("   ", 24).is_err());
        assert!(validate_name(&"x".repeat(25), 24).is_err());
        assert!(validate_name(&"x".repeat(24), 24).is_ok());
        assert!(validate_name("Clean Twin", 24).is_ok());
    }
}
