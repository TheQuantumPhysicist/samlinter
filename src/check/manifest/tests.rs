use super::super::{PROFILES, render, text_file};
use super::{FixedRule, Role, Violation, check};
use crate::config::{Config, words};
use crate::finding::{Exemptability, Rule};
use crate::workspace::{Dependency, Package, PackageKind, Readme, Section};
use ra_ap_syntax::Edition;
use rstest::rstest;
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
    }
}

/// A dependency as `cargo metadata` reports it.
fn dependency(key: &str, crate_name: &str, section: Section, target: Option<&str>) -> Dependency {
    Dependency {
        key: key.to_owned(),
        crate_name: crate_name.to_owned(),
        section,
        target: target.map(|platform| platform.parse().unwrap()),
    }
}

fn subscriber(section: Section) -> Dependency {
    dependency("tracing-subscriber", "tracing-subscriber", section, None)
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
    &["2:29 feature_flags"]
)]
#[case::optional_dependency_table(
    "[dependencies.serde]\nworkspace = true\noptional = true\n",
    &["3:1 feature_flags", "1:15 workspace_dependency_form"]
)]
#[case::optional_target_build_dependency(
    "[target.'cfg(unix)'.build-dependencies]\ncc = { workspace = true, optional = true }\n",
    &["2:26 feature_flags"]
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

#[rstest]
#[case::direct(
    "[dependencies]\ntracing-subscriber = { workspace = true }\n",
    subscriber(Section::Normal),
    &["2:1 library_subscriber"]
)]
#[case::renamed_here(
    "[dependencies]\nlogs = { package = \"tracing-subscriber\", workspace = true }\n",
    dependency("logs", "tracing-subscriber", Section::Normal, None),
    &["2:1 library_subscriber"]
)]
#[case::renamed_in_workspace_dependencies(
    "[dependencies]\nlogs = { workspace = true }\n",
    dependency("logs", "tracing-subscriber", Section::Normal, None),
    &["2:1 library_subscriber"]
)]
#[case::target_specific(
    "[target.'cfg(unix)'.dependencies]\ntracing-subscriber = { workspace = true }\n",
    dependency("tracing-subscriber", "tracing-subscriber", Section::Normal, Some("cfg(unix)")),
    &["2:1 library_subscriber"]
)]
#[case::target_written_with_spaces(
    "[target.'cfg( unix )'.dependencies]\ntracing-subscriber = { workspace = true }\n",
    dependency("tracing-subscriber", "tracing-subscriber", Section::Normal, Some("cfg(unix)")),
    &["2:1 library_subscriber"]
)]
#[case::target_triple(
    "[target.x86_64-unknown-linux-gnu.dependencies]\ntracing-subscriber = { workspace = true }\n",
    dependency(
        "tracing-subscriber",
        "tracing-subscriber",
        Section::Normal,
        Some("x86_64-unknown-linux-gnu")
    ),
    &["2:1 library_subscriber"]
)]
#[case::second_target(
    "[target.'cfg(unix)'.dependencies]\nlogs = { workspace = true }\n\n[target.'cfg(windows)'.dependencies]\nlogs = { workspace = true }\n",
    dependency("logs", "tracing-subscriber", Section::Normal, Some("cfg(windows)")),
    &["5:1 library_subscriber"]
)]
#[case::in_tests(
    "[dev-dependencies]\ntracing-subscriber = { workspace = true }\n",
    subscriber(Section::Development),
    &[]
)]
#[case::in_build_script(
    "[build-dependencies]\ntracing-subscriber = { workspace = true }\n",
    subscriber(Section::Build),
    &[]
)]
#[case::other_crate(
    "[dependencies]\ntracing = { workspace = true }\n",
    dependency("tracing", "tracing", Section::Normal, None),
    &[]
)]
#[case::same_key_in_two_sections(
    "[dev-dependencies]\ntracing-subscriber = { workspace = true }\n\n[dependencies]\ntracing-subscriber = { workspace = true }\n",
    subscriber(Section::Normal),
    &["5:1 library_subscriber"]
)]
fn libraries_do_not_depend_on_subscribers(
    #[case] text: &str,
    #[case] dependency: Dependency,
    #[case] expected: &[&str],
) {
    let package = package(text, PackageKind::Library, None, vec![dependency]);
    assert_eq!(
        render(&check(&Role::Member(&package), &config()).unwrap()),
        expected
    );
}

/// crates.io treats `-` and `_` in a crate name as one character.
#[rstest]
#[case::underscore_in_config("tracing_subscriber", "tracing-subscriber", &["2:1 library_subscriber"])]
#[case::underscore_in_crate_name("tracing-subscriber", "tracing_subscriber", &["2:1 library_subscriber"])]
#[case::both_underscored("tracing_subscriber", "tracing_subscriber", &["2:1 library_subscriber"])]
#[case::separator_dropped("tracing_subscriber", "tracingsubscriber", &[])]
fn subscriber_names_match_with_dash_and_underscore_alike(
    #[case] configured: &str,
    #[case] crate_name: &str,
    #[case] expected: &[&str],
) {
    let text = format!("[dependencies]\n{crate_name} = {{ workspace = true }}\n");
    let package = package(
        &text,
        PackageKind::Library,
        None,
        vec![dependency(crate_name, crate_name, Section::Normal, None)],
    );
    let config = Config {
        subscriber_crates: words(&[configured]),
        ..config()
    };
    assert_eq!(
        render(&check(&Role::Member(&package), &config).unwrap()),
        expected
    );
}

/// - Cargo reads the same text, so none of these reaches a real run.
/// - Each would otherwise drop or misplace a `library_subscriber` finding.
#[rstest]
#[case::no_entry_with_the_key("[dependencies]\ntracing = { workspace = true }\n", None)]
#[case::entry_in_another_section(
    "[dev-dependencies]\ntracing-subscriber = { workspace = true }\n",
    None
)]
#[case::top_level_entry_for_a_target_dependency(
    "[dependencies]\ntracing-subscriber = { workspace = true }\n",
    Some("cfg(unix)")
)]
#[case::target_entry_for_a_top_level_dependency(
    "[target.'cfg(unix)'.dependencies]\ntracing-subscriber = { workspace = true }\n",
    None
)]
#[case::unparsable_platform(
    "[target.'<garbage>'.dependencies]\ntracing-subscriber = { workspace = true }\n",
    Some("cfg(unix)")
)]
fn subscriber_without_its_entry_is_an_error(#[case] text: &str, #[case] target: Option<&str>) {
    let dependency = dependency(
        "tracing-subscriber",
        "tracing-subscriber",
        Section::Normal,
        target,
    );
    let package = package(text, PackageKind::Library, None, vec![dependency]);
    let error = check(&Role::Member(&package), &config()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Cargo.toml: `cargo metadata` lists dependency `tracing-subscriber`, but samlinter finds no entry for it in this file; please report this samlinter bug"
    );
}

#[test]
fn dependency_that_is_no_subscriber_needs_no_entry() {
    let package = package(
        "[dependencies]\n",
        PackageKind::Library,
        None,
        vec![dependency("tracing", "tracing", Section::Normal, None)],
    );
    assert_eq!(
        render(&check(&Role::Member(&package), &config()).unwrap()),
        Vec::<String>::new()
    );
}

#[test]
fn root_package_library_does_not_depend_on_subscribers() {
    let text = format!("{PROFILES}\n[dependencies]\ntracing-subscriber = \"0.3\"\n");
    let package = package(
        &text,
        PackageKind::Library,
        None,
        vec![subscriber(Section::Normal)],
    );
    assert_eq!(
        render(&check(&Role::RootPackage(&package), &config()).unwrap()),
        ["8:1 library_subscriber"]
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
    &["8:29 feature_flags"]
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

/// `toml_edit` gives every parsed key a position, dotted and header keys included, so only a key
/// built by hand reaches this error.
#[test]
fn key_without_a_position_stops_the_run() {
    let key = Key::new("serde");
    let error = Violation::at_key(&key, FixedRule::FeatureFlags, "message")
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
