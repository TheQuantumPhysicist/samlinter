use super::{EXCLUDE_IS_FOR_GENERATED_CODE, EXEMPTING_NEEDS_APPROVAL, Guidance};
use crate::finding::{Exemptability, Rule};
use rstest::rstest;
use std::collections::BTreeSet;
use strum::IntoEnumIterator;

const NONE: [&str; 0] = [];

fn points(guidance: &Guidance) -> impl Iterator<Item = &'static str> {
    guidance
        .why
        .iter()
        .chain(guidance.fix)
        .copied()
        .chain(guidance.all_bad_fixes())
}

#[test]
fn every_rule_explains_why_how_and_what_not_to_do() {
    for rule in Rule::iter() {
        let guidance = Guidance::of(rule);
        assert_ne!(guidance.why, NONE, "{} lacks why", rule.name());
        assert_ne!(guidance.fix, NONE, "{} lacks fix", rule.name());
        assert_ne!(guidance.bad_fixes, NONE, "{} lacks bad fixes", rule.name());
    }
}

#[rstest]
#[case::binary_readme(
    Rule::BinaryReadme,
    "Someone who finds a binary needs to know what it is for and how to run it without reading its source."
)]
#[case::discarded_result(
    Rule::DiscardedResult,
    "A bare `let _ =` throws the result away: a call that can fail hides its error, and a reader cannot tell a deliberate discard from an accidental one."
)]
#[case::direct_rng(
    Rule::DirectRng,
    "Directly constructed randomness is unauditable: a failure or an attack cannot be replayed, and a test cannot pin what the code draws."
)]
#[case::em_dash(
    Rule::EmDash,
    "Comments and documentation should read like an engineer's notes. An em-dash usually joins two thoughts that belong in separate sentences or points."
)]
#[case::emoji(
    Rule::Emoji,
    "Comments and documentation should read like an engineer's notes. Emoji carry no technical information and render differently across terminals and editors."
)]
#[case::enum_is(
    Rule::EnumIs,
    "strum's `EnumIs` generates `.is_variant()` shortcuts, the same absorption hazard as `matches!`: when a variant is added, every check keeps compiling and silently answers false."
)]
#[case::feature_flags(
    Rule::FeatureFlags,
    "Every feature flag doubles the set of build configurations. Most combinations never get built or tested, so bugs hide in them."
)]
#[case::fn_spacing(
    Rule::FnSpacing,
    "A blank line between functions shows each one as a unit at a glance. rustfmt never adds it, so nothing else enforces it."
)]
#[case::forbidden_word(
    Rule::ForbiddenWord,
    "The project lists this word or phrase under `forbidden-words` or `forbidden-words-insensitive` in samlinter.toml because it must not appear in comments or README files."
)]
#[case::item_order(
    Rule::ItemOrder,
    "Every file follows one layout, so a reader finds its submodules, imports, code and tests in the same place everywhere."
)]
#[case::item_spacing(
    Rule::ItemSpacing,
    "Imports form one contiguous block, so rustfmt sorts them as one group and a reader sees every dependency in one place."
)]
#[case::json_macro(
    Rule::JsonMacro,
    "`json!` builds a `serde_json::Value` with no definition behind it, so the compiler cannot check keys, value types or required fields; a shape that drifts surfaces only when the JSON is read."
)]
#[case::library_subscriber(
    Rule::LibrarySubscriber,
    "Only the final program knows where logs go and at what level. A library that installs a subscriber fights the program's own setup, and a process can have only one global subscriber."
)]
#[case::malformed_exemption(
    Rule::MalformedExemption,
    "An exemption must name exactly which rule it lifts and why, so a reviewer can judge it. A malformed exemption exempts nothing."
)]
#[case::matches_macro(
    Rule::MatchesMacro,
    "On a compiler-exhaustive enum, `matches!` answers one question and silently absorbs every variant it does not name: adding a variant keeps this code compiling, now wrongly answering false."
)]
#[case::member_version(
    Rule::MemberVersion,
    "Each dependency has one version for the whole workspace, declared in the root manifest, so an upgrade happens in one place and crates never drift onto different versions."
)]
#[case::module_shape(
    Rule::ModuleShape,
    "The module tree must mirror the file system: a module with submodules is a directory, so a reader finds every module from its path."
)]
#[case::panic_strategy(
    Rule::PanicStrategy,
    "A panic means an invariant broke. With unwinding, only the panicking thread or task dies while the rest of the process keeps running on state that may be half-updated."
)]
#[case::path_attr(
    Rule::PathAttr,
    "`#[path]` overrides Rust's file-name-to-module mapping, so a module can live anywhere and the module tree stops mirroring the file system."
)]
#[case::regex_use(
    Rule::RegexUse,
    "A regular expression is an open-ended security boundary: alternation, mode flags, repetition and character classes accept input shapes the author did not prove out, and defenses against overpermissive patterns are a treadmill."
)]
#[case::rest_pattern(
    Rule::RestPattern,
    "`..` matches every field the pattern does not name, including fields added later."
)]
#[case::restricted_visibility(
    Rule::RestrictedVisibility,
    "`pub(crate)`, `pub(super)` and `pub(in path)` exist to let code outside an item's own module subtree reach into it."
)]
#[case::system_time_now(
    Rule::SystemTimeNow,
    "A direct `now()` makes the code non-deterministic: a test cannot control the clock, so behavior depending on time cannot be tested or replayed."
)]
#[case::syntax_error(
    Rule::SyntaxError,
    "samlinter lints only files that parse completely, because findings on a half-parsed file would be wrong."
)]
#[case::unfulfilled_exemption(
    Rule::UnfulfilledExemption,
    "The exemption covers no finding anymore: the code it excused changed or moved, or an exemption nested inside it, or an earlier one for the same rule on the same node, already covers that finding. Left in place, it would silently excuse a future violation at that spot."
)]
#[case::workspace_dependency_form(
    Rule::WorkspaceDependencyForm,
    "Every workspace dependency uses one spelling, an inline table, so manifests stay uniform and the same line extends naturally when a crate needs extra features."
)]
fn each_rule_gets_its_own_guidance(#[case] rule: Rule, #[case] first_why: &str) {
    assert_eq!(Guidance::of(rule).why.first(), Some(&first_why));
}

#[test]
fn every_rule_has_its_own_why() {
    let whys: BTreeSet<&[&str]> = Rule::iter().map(|rule| Guidance::of(rule).why).collect();
    assert_eq!(whys.len(), Rule::iter().count());
}

#[test]
fn bad_fixes_read_as_prohibitions() {
    for rule in Rule::iter() {
        for point in Guidance::of(rule).all_bad_fixes() {
            assert!(point.starts_with("Do not "), "{}: {point}", rule.name());
        }
    }
}

/// - `exclude` silences every rule, so every rule warns against using it for that.
/// - Every exemptable rule warns against exempting without approval, before that.
#[test]
fn every_rule_ends_its_bad_fixes_with_the_shared_ones() {
    for rule in Rule::iter() {
        let guidance = Guidance::of(rule);
        let own = guidance
            .bad_fixes
            .iter()
            .fold(String::new(), |text, point| text + "    - " + point + "\n");
        let exempting = match rule.exemptability() {
            Exemptability::Never => String::new(),
            Exemptability::Source(_) | Exemptability::Manifest(_) => {
                format!("    - {EXEMPTING_NEEDS_APPROVAL}\n")
            }
        };
        assert!(
            guidance.to_string().ends_with(&format!(
                "  bad fixes:\n{own}{exempting}    - {EXCLUDE_IS_FOR_GENERATED_CODE}\n"
            )),
            "{}",
            rule.name()
        );
    }
}

#[test]
fn points_are_single_ascii_lines() {
    for rule in Rule::iter() {
        for point in points(&Guidance::of(rule)) {
            assert!(
                point.is_ascii() && !point.contains('\n'),
                "{}: {point}",
                rule.name()
            );
        }
    }
}

#[test]
fn exemption_guidance_shows_where_each_exemptable_rule_is_exempted() {
    let guidance = Guidance::of(Rule::MalformedExemption);
    let why = guidance.why.join(" ");
    let fix = guidance.fix.join(" ");
    for rule in Rule::iter() {
        let name = rule.name();
        let named = why.contains(&format!("`{name}`"));
        match rule.exemptability() {
            Exemptability::Never => assert!(!named, "{name}"),
            Exemptability::Source(_) => {
                assert!(named, "{name}");
                assert!(
                    fix.contains("#[cfg_attr(samlinter, expect(<rule>, "),
                    "{name}"
                );
            }
            Exemptability::Manifest(_) => {
                assert!(named, "{name}");
                assert!(
                    fix.contains(&format!("[package.metadata.samlinter.expect.{name}]")),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn renders_labeled_bullet_sections() {
    let guidance = Guidance {
        why: &["a"],
        fix: &["b", "c"],
        bad_fixes: &["Do not d"],
        exemptability: Exemptability::Never,
    };
    assert_eq!(
        guidance.to_string(),
        format!(
            "  why:\n    - a\n  fix:\n    - b\n    - c\n  bad fixes:\n    - Do not d\n    - {EXCLUDE_IS_FOR_GENERATED_CODE}\n"
        )
    );
}
