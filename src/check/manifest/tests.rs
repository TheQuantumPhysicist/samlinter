use super::super::{PROFILES, render, text_file};
use super::{FixedRule, Role, Violation, check};
use crate::config::{Config, words};
use crate::finding::{Exemptability, Rule};
use crate::workspace::{Dependency, Package, PackageKind, Readme, Section};
use ra_ap_syntax::Edition;
use rstest::rstest;
use std::collections::BTreeSet;
use std::path::PathBuf;
use strum::IntoEnumIterator;
use toml_edit::Key;

const README: Option<&str> = Some("# app\n");

fn package(
    text: &str,
    kind: PackageKind,
    readme: Option<&str>,
    dependencies: Vec<Dependency>,
) -> Package {
    Package {
        manifest: text_file("Cargo.toml", text),
        kind,
        readme: readme.map_or_else(
            || Readme::Missing {
                expected: PathBuf::from("README.md"),
            },
            |text| Readme::Found(text_file("README.md", text)),
        ),
        edition: Edition::Edition2024,
        dependencies,
        sources: Vec::new(),
        library_code: BTreeSet::new(),
    }
}

fn subscriber(section: Section) -> Dependency {
    Dependency {
        crate_name: "tracing-subscriber".to_owned(),
        section,
    }
}

fn config() -> Config {
    Config {
        forbidden_words: Vec::new(),
        subscriber_crates: words(&["tracing-subscriber"]),
    }
}

fn library_member(text: &str) -> Vec<String> {
    let package = package(text, PackageKind::Library, None, Vec::new());
    render(&check(&Role::Member(&package), &config()).unwrap())
}

fn virtual_root(text: &str) -> Vec<String> {
    render(
        &check(
            &Role::VirtualRoot(&text_file("Cargo.toml", text)),
            &config(),
        )
        .unwrap(),
    )
}

#[rstest]
#[case::feature_table("[package]\nname = \"a\"\n\n[features]\nfast = []\n", &["4:2 feature_flags"])]
#[case::optional_dependency(
    "[dependencies]\nserde = { workspace = true, optional = true }\n",
    &["2:1 feature_flags"]
)]
#[case::optional_dependency_table(
    "[dependencies.serde]\nworkspace = true\noptional = true\n",
    &["1:15 feature_flags", "1:15 workspace_dependency_form"]
)]
#[case::optional_target_build_dependency(
    "[target.'cfg(unix)'.build-dependencies]\ncc = { workspace = true, optional = true }\n",
    &["2:1 feature_flags"]
)]
#[case::required_dependency(
    "[dependencies]\nserde = { workspace = true, optional = false }\n",
    &[]
)]
#[case::dotted_workspace_key("[dependencies]\nstrum.workspace = true\n", &["2:1 workspace_dependency_form"])]
#[case::workspace_sub_table("[dependencies.strum]\nworkspace = true\n", &["1:15 workspace_dependency_form"])]
#[case::inline_workspace_table(
    "[dependencies]\nstrum = { workspace = true, features = [\"derive\"] }\n",
    &[]
)]
#[case::version_string("[dependencies]\nrand = \"0.10\"\n", &["2:1 member_version"])]
#[case::version_key("[dev-dependencies]\nrstest = { version = \"0.28\" }\n", &["2:1 member_version"])]
#[case::build_dependency_version("[build-dependencies]\ncc = \"1\"\n", &["2:1 member_version"])]
#[case::underscore_dev_section("[dev_dependencies]\nrstest = \"0.28\"\n", &["2:1 member_version"])]
#[case::underscore_build_section("[build_dependencies]\ncc = \"1\"\n", &["2:1 member_version"])]
#[case::underscore_target_section(
    "[target.'cfg(unix)'.dev_dependencies]\nlibc = \"0.2\"\n",
    &["2:1 member_version"]
)]
#[case::target_specific_version(
    "[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n",
    &["2:1 member_version"]
)]
#[case::path_dependency("[dependencies]\nlocal = { path = \"../local\" }\n", &[])]
#[case::version_in_dependency_table("[dependencies.serde]\nversion = \"1\"\n", &["1:15 member_version"])]
#[case::optional_not_a_boolean(
    "[dependencies]\nserde = { workspace = true, optional = \"true\" }\n",
    &[]
)]
#[case::value_cargo_rejects("[dependencies]\nserde = 5\n", &[])]
#[case::target_workspace_sub_table(
    "[target.'cfg(unix)'.dependencies.cc]\nworkspace = true\n",
    &["1:34 workspace_dependency_form"]
)]
#[case::not_toml("[dependencies\n", &["1:14 syntax_error"])]
#[case::not_toml_after_a_byte_order_mark("\u{feff}[dependencies\n", &["1:14 syntax_error"])]
#[case::comment_prose("[package]\nname = \"a\" # note \u{2014} x\n", &["2:19 em_dash"])]
#[case::string_value_is_not_a_comment("[package]\ndescription = \"a \u{2014} b\"\n", &[])]
#[case::comments_of_a_file_that_does_not_parse(
    "# \u{2014}\n[dependencies\n",
    &["2:14 syntax_error"]
)]
fn checks_library_member_manifests(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(library_member(text), expected);
}

#[rstest]
#[case::optional_dependency(
    "[dependencies]\nserde = { workspace = true, optional = true }\n",
    &["optional dependency `serde` is a feature flag"]
)]
#[case::not_toml("[dependencies\n", &["unclosed table, expected `]`"])]
fn library_member_messages(#[case] text: &str, #[case] expected: &[&str]) {
    let package = package(text, PackageKind::Library, None, Vec::new());
    let messages: Vec<String> = check(&Role::Member(&package), &config())
        .unwrap()
        .into_iter()
        .map(|finding| finding.message)
        .collect();
    assert_eq!(messages, expected);
}

/// `toml_edit` reports a dotted key 80 or more segments deep with no position.
#[test]
fn syntax_error_without_a_position_is_a_whole_file_finding() {
    let text = format!("{} = 1\n", ["a"; 81].join("."));
    let package = package(&text, PackageKind::Library, None, Vec::new());
    let findings = check(&Role::Member(&package), &config()).unwrap();
    let messages: Vec<&str> = findings
        .iter()
        .map(|finding| finding.message.as_str())
        .collect();
    assert_eq!(
        (render(&findings), messages),
        (
            vec!["file syntax_error".to_owned()],
            vec!["recursion limit"]
        )
    );
}

/// The `library_subscriber` check reads a library's sources; the dependency alone is fine.
#[rstest]
#[case::direct("[dependencies]\ntracing-subscriber = { workspace = true }\n")]
#[case::renamed_here(
    "[dependencies]\nlogs = { package = \"tracing-subscriber\", workspace = true }\n"
)]
#[case::target_specific(
    "[target.'cfg(unix)'.dependencies]\ntracing-subscriber = { workspace = true }\n"
)]
fn library_may_depend_on_a_subscriber(#[case] text: &str) {
    let package = package(
        text,
        PackageKind::Library,
        None,
        vec![subscriber(Section::Normal)],
    );
    assert_eq!(
        render(&check(&Role::Member(&package), &config()).unwrap()),
        Vec::<String>::new()
    );
}

#[rstest]
#[case::without_readme(None, &["file binary_readme"])]
#[case::with_readme(README, &[])]
fn binary_needs_readme_and_may_install_a_subscriber(
    #[case] readme: Option<&str>,
    #[case] expected: &[&str],
) {
    let package = package(
        "[dependencies]\ntracing-subscriber = { workspace = true }\n",
        PackageKind::Binary,
        readme,
        vec![subscriber(Section::Normal)],
    );
    assert_eq!(
        render(&check(&Role::Member(&package), &config()).unwrap()),
        expected
    );
}

#[rstest]
#[case::missing_profiles("[workspace]\n", &["file panic_strategy", "file panic_strategy"])]
#[case::unwinding_dev(
    "[profile.dev]\npanic = \"unwind\"\n\n[profile.release]\npanic = \"abort\"\n",
    &["1:10 panic_strategy"]
)]
#[case::unwinding_release(
    "[profile.dev]\npanic = \"abort\"\n\n[profile.release]\npanic = \"unwind\"\n",
    &["4:10 panic_strategy"]
)]
#[case::aborting_profiles(PROFILES, &[])]
#[case::comment_prose(
    "# \u{2014}\n[profile.dev]\npanic = \"abort\"\n\n[profile.release]\npanic = \"abort\"\n",
    &["1:3 em_dash"]
)]
#[case::profile_not_a_table("profile = 1\n", &["file panic_strategy", "file panic_strategy"])]
#[case::dev_profile_not_a_table(
    "[profile]\ndev = 1\n\n[profile.release]\npanic = \"abort\"\n",
    &["2:1 panic_strategy"]
)]
#[case::dotted_keys(
    "profile.dev.panic = \"abort\"\nprofile.release.panic = \"abort\"\n",
    &[]
)]
#[case::inline_tables(
    "profile = { dev = { panic = \"unwind\" }, release = { panic = \"abort\" } }\n",
    &["1:13 panic_strategy"]
)]
#[case::panic_not_a_string(
    "[profile.dev]\npanic = true\n\n[profile.release]\npanic = \"abort\"\n",
    &["1:10 panic_strategy"]
)]
fn root_requires_abort_on_panic(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(virtual_root(text), expected);
}

#[test]
fn root_package_requires_abort_on_panic() {
    let package = package(
        "[package]\nname = \"p\"\n",
        PackageKind::Binary,
        README,
        Vec::new(),
    );
    assert_eq!(
        render(&check(&Role::RootPackage(&package), &config()).unwrap()),
        ["file panic_strategy", "file panic_strategy"]
    );
}

#[test]
fn root_package_may_hold_versions() {
    let text = format!("{PROFILES}\n[dependencies]\nanyhow = \"1\"\n");
    let package = package(&text, PackageKind::Binary, README, Vec::new());
    assert_eq!(
        render(&check(&Role::RootPackage(&package), &config()).unwrap()),
        Vec::<String>::new()
    );
}

#[rstest]
#[case::feature_table("\n[features]\nfast = []\n", &["7:2 feature_flags"])]
#[case::dotted_workspace_key(
    "\n[dependencies]\nstrum.workspace = true\n",
    &["8:1 workspace_dependency_form"]
)]
#[case::optional_dependency(
    "\n[dependencies]\nserde = { workspace = true, optional = true }\n",
    &["8:1 feature_flags"]
)]
fn root_package_gets_the_rules_of_every_manifest(#[case] tail: &str, #[case] expected: &[&str]) {
    let text = format!("{PROFILES}{tail}");
    let package = package(&text, PackageKind::Binary, README, Vec::new());
    assert_eq!(
        render(&check(&Role::RootPackage(&package), &config()).unwrap()),
        expected
    );
}

#[rstest]
#[case::exempt_version(
    "[package.metadata.samlinter.expect.member_version]\nrand = \"specialized to this crate by definition\"\n\n[dependencies]\nrand = \"0.10\"\n",
    &[]
)]
#[case::unfulfilled(
    "[package.metadata.samlinter.expect.member_version]\nserde = \"why\"\n\n[dependencies]\nserde = { workspace = true }\n",
    &["2:1 unfulfilled_exemption"]
)]
#[case::one_exemption_covers_every_section(
    "[package.metadata.samlinter.expect.member_version]\nrand = \"why\"\n\n[dependencies]\nrand = \"0.10\"\n\n[dev-dependencies]\nrand = \"0.10\"\n",
    &[]
)]
fn member_version_exemption_names_the_dependency(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(library_member(text), expected);
}

#[rstest]
#[case::the_features_table(
    "[features]\nfast = []\n\n[package.metadata.samlinter.expect.feature_flags]\nfeatures = \"why\"\n",
    &[]
)]
#[case::an_optional_dependency(
    "[dependencies]\nserde = { workspace = true, optional = true }\n\n[package.metadata.samlinter.expect.feature_flags]\nserde = \"why\"\n",
    &[]
)]
#[case::other_name(
    "[features]\nfast = []\n\n[package.metadata.samlinter.expect.feature_flags]\nx = \"why\"\n",
    &["1:2 feature_flags", "5:1 unfulfilled_exemption"]
)]
fn feature_flags_exemption_names_the_table_or_the_optional_dependency(
    #[case] text: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(library_member(text), expected);
}

/// No attribute can sit in a manifest comment, so its comment findings stay.
#[test]
fn comment_rule_exemption_in_a_manifest_is_malformed_and_lifts_nothing() {
    assert_eq!(
        library_member("[package.metadata.samlinter.expect.em_dash]\nx = \"why\" # a \u{2014} b\n"),
        ["1:36 malformed_exemption", "2:15 em_dash"]
    );
}

#[test]
fn member_version_exemption_on_the_root_package_is_unfulfilled() {
    let text = format!(
        "{PROFILES}\n[package.metadata.samlinter.expect.member_version]\nanyhow = \"why\"\n\n[dependencies]\nanyhow = \"1\"\n"
    );
    let package = package(&text, PackageKind::Binary, README, Vec::new());
    assert_eq!(
        render(&check(&Role::RootPackage(&package), &config()).unwrap()),
        ["8:1 unfulfilled_exemption"]
    );
}

/// `feature_flags` runs on the root package too, so its exemption lifts there as well; a
/// virtual root has no `[package]` table, so it has nothing to write the exemption in.
#[rstest]
#[case::the_features_table(
    "\n[features]\nfast = []\n\n[package.metadata.samlinter.expect.feature_flags]\nfeatures = \"why\"\n"
)]
#[case::an_optional_dependency(
    "\n[dependencies]\nserde = { workspace = true, optional = true }\n\n[package.metadata.samlinter.expect.feature_flags]\nserde = \"why\"\n"
)]
fn feature_flags_exemption_on_the_root_package_lifts(#[case] tail: &str) {
    let text = format!("{PROFILES}{tail}");
    let package = package(&text, PackageKind::Binary, README, Vec::new());
    assert_eq!(
        render(&check(&Role::RootPackage(&package), &config()).unwrap()),
        Vec::<String>::new()
    );
}

/// The README and the config template claim it: a virtual root's `[features]` table flags, and
/// with no `[package]` table no exemption can be written for it.
#[test]
fn virtual_root_features_table_flags() {
    let text = format!("{PROFILES}\n[features]\nfast = []\n");
    assert_eq!(virtual_root(&text), ["7:2 feature_flags"]);
}

/// `toml_edit` gives every parsed key a position, dotted and header keys included, so only a key
/// built by hand reaches this error.
#[test]
fn key_without_a_position_stops_the_run() {
    let key = Key::new("serde");
    let error = Violation::at_key(&key, FixedRule::PanicStrategy, "message")
        .into_finding(&text_file("Cargo.toml", ""))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Cargo.toml: the parsed key `serde` has no position in this file; please report this samlinter bug"
    );
}

#[test]
fn fixed_rules_accept_no_exemption() {
    for fixed in FixedRule::iter() {
        let rule = Rule::from(fixed);
        assert_eq!(
            rule.exemptability(),
            Exemptability::Never,
            "{}",
            rule.name()
        );
    }
}
