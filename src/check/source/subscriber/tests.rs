use super::super::super::text_file;
use super::super::{hits, render};
use super::{Gate, Installs, check};
use crate::config::{Config, words};
use crate::workspace::{Dependency, Package, PackageKind, Readme, Section};
use ra_ap_syntax::{AstNode, Edition};
use rstest::rstest;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn flagged(text: &str) -> Vec<String> {
    render(text, |file| check(file.syntax(), Installs::Flagged))
}

#[rstest]
#[case::init_method("fn f(s: S) {\n    s.init();\n}\n", &["2:7 library_subscriber"])]
#[case::try_init_method("fn f(s: S) {\n    let _ = s.try_init();\n}\n", &["2:15 library_subscriber"])]
#[case::chained_init(
    "fn f(l: L) {\n    Registry::default().with(l).init();\n}\n",
    &["2:33 library_subscriber"]
)]
#[case::init_function("fn f() {\n    fmt::init();\n}\n", &["2:10 library_subscriber"])]
#[case::try_init_function(
    "fn f() {\n    tracing_subscriber::fmt::try_init();\n}\n",
    &["2:30 library_subscriber"]
)]
#[case::set_global_default(
    "fn f(s: S) {\n    tracing::subscriber::set_global_default(s);\n}\n",
    &["2:26 library_subscriber"]
)]
#[case::set_default_function_is_no_install("fn f(s: S) {\n    let _g = set_default(s);\n}\n", &[])]
#[case::set_default_via_path_is_no_install(
    "fn f(s: S) {\n    tracing::subscriber::set_default(s);\n}\n",
    &[]
)]
#[case::generic_function(
    "fn f(s: S) {\n    set_global_default::<S>(s);\n}\n",
    &["2:5 library_subscriber"]
)]
#[case::in_a_constant("const C: () = fmt::init();\n", &["1:20 library_subscriber"])]
#[case::set_default_method_is_no_install("fn f(c: C) {\n    c.set_default();\n}\n", &[])]
#[case::other_call("fn f(s: S) {\n    s.start();\n    start();\n}\n", &[])]
#[case::closure_call("fn f(init: F) {\n    (init)();\n}\n", &[])]
#[case::macro_body_is_not_seen("fn f(s: S) {\n    m!(s.init());\n}\n", &[])]
#[case::reference_without_a_call("fn f() {\n    let g = fmt::init;\n}\n", &[])]
#[case::cfg_test_module(
    "#[cfg(test)]\nmod t {\n    fn f(s: S) {\n        s.init();\n    }\n}\n",
    &[]
)]
#[case::cfg_test_function("#[cfg(test)]\nfn f(s: S) {\n    s.init();\n}\n", &[])]
#[case::test_function("#[test]\nfn f() {\n    fmt::init();\n}\n", &[])]
#[case::test_function_in_an_impl(
    "impl T {\n    #[test]\n    fn f() {\n        fmt::init();\n    }\n}\n",
    &[]
)]
#[case::cfg_all_test("#[cfg(all(test, unix))]\nfn f(s: S) {\n    s.init();\n}\n", &[])]
#[case::cfg_any_test(
    "#[cfg(any(test, unix))]\nfn f(s: S) {\n    s.init();\n}\n",
    &["3:7 library_subscriber"]
)]
#[case::cfg_test_statement("fn f(s: S) {\n    #[cfg(test)]\n    s.init();\n}\n", &[])]
#[case::test_only_file("#![cfg(test)]\n\nfn f(s: S) {\n    s.init();\n}\n", &[])]
#[case::inner_cfg_test_module(
    "mod t {\n    #![cfg(test)]\n\n    fn f(s: S) {\n        s.init();\n    }\n}\n",
    &[]
)]
#[case::other_test_attribute("#[tokio::test]\nfn f(s: S) {\n    s.init();\n}\n", &[])]
#[case::test_attribute_after_another("#[ignore]\n#[test]\nfn f(s: S) {\n    s.init();\n}\n", &[])]
#[case::test_function_in_a_test_function(
    "#[test]\nfn f() {\n    fn g(s: S) {\n        s.init();\n    }\n}\n",
    &[]
)]
#[case::code_after_a_test_function(
    "#[test]\nfn t() {}\n\nfn f(s: S) {\n    s.init();\n}\n",
    &["5:7 library_subscriber"]
)]
#[case::test_gated_block("fn f(s: S) {\n    #![cfg(test)]\n    s.init();\n}\n", &[])]
#[case::chained_installs(
    "fn f(s: S) {\n    s.init().init();\n}\n",
    &["2:14 library_subscriber", "2:7 library_subscriber"]
)]
#[case::nested_calls(
    "fn f(s: S) {\n    g(fmt::init());\n}\n",
    &["2:12 library_subscriber"]
)]
fn flags_calls_that_install_a_subscriber(#[case] text: &str, #[case] expected: &[&str]) {
    assert_eq!(flagged(text), expected);
}

#[rstest]
#[case::method(
    "fn f(s: S) {\n    s.init();\n}\n",
    "library installs a log subscriber: `.init()`"
)]
#[case::function(
    "fn f(s: S) {\n    tracing::subscriber::set_global_default(s);\n}\n",
    "library installs a log subscriber: `tracing::subscriber::set_global_default()`"
)]
#[case::path_over_two_lines(
    "fn f(s: S) {\n    tracing::subscriber::\n        set_global_default(s);\n}\n",
    "library installs a log subscriber: `tracing::subscriber::set_global_default()`"
)]
#[case::path_with_a_comment(
    "fn f(s: S) {\n    tracing::subscriber:: /* c */ set_global_default(s);\n}\n",
    "library installs a log subscriber: `tracing::subscriber::set_global_default()`"
)]
#[case::generic_arguments(
    "fn f(s: S) {\n    subscriber::set_global_default::<S>(s);\n}\n",
    "library installs a log subscriber: `subscriber::set_global_default()`"
)]
#[case::qualified_path(
    "fn f() {\n    <S as T>::init();\n}\n",
    "library installs a log subscriber: `init()`"
)]
fn message_names_the_call(#[case] text: &str, #[case] expected: &str) {
    let messages: Vec<String> = hits(text, |file| check(file.syntax(), Installs::Flagged))
        .into_iter()
        .map(|hit| hit.message)
        .collect();
    assert_eq!(messages, [expected]);
}

#[test]
fn allowed_file_gets_no_finding() {
    assert_eq!(
        render("fn f(s: S) {\n    s.init();\n}\n", |file| check(
            file.syntax(),
            Installs::Allowed
        )),
        Vec::<String>::new()
    );
}

/// The library `lib/src/lib.rs` under `/workspace`, as `Package.library_code` holds it.
const LIB: &str = "/workspace/lib/src/lib.rs";

/// A package whose library code is `LIB`.
fn package(kind: PackageKind, dependencies: Vec<Dependency>) -> Package {
    Package {
        manifest: text_file("lib/Cargo.toml", ""),
        kind,
        readme: Readme::Missing {
            expected: PathBuf::from("lib/README.md"),
        },
        edition: Edition::Edition2024,
        dependencies,
        sources: Vec::new(),
        library_code: BTreeSet::from([PathBuf::from(LIB)]),
    }
}

fn dependency(crate_name: &str, section: Section) -> Dependency {
    Dependency {
        crate_name: crate_name.to_owned(),
        section,
    }
}

fn subscribers(configured: &str) -> Config {
    Config {
        forbidden_words: Vec::new(),
        subscriber_crates: words(&[configured]),
    }
}

/// What `Gate` makes of `LIB` and of a file outside every package's library code.
fn gate(packages: &[Package], configured: &str) -> (Installs, Installs) {
    let gate = Gate::of(packages, &subscribers(configured));
    (
        gate.installs(Path::new(LIB)),
        gate.installs(Path::new("/workspace/lib/src/tests.rs")),
    )
}

#[rstest]
#[case::library(PackageKind::Library, Section::Normal, Installs::Flagged)]
#[case::dev_dependency(PackageKind::Library, Section::Development, Installs::Allowed)]
#[case::build_dependency(PackageKind::Library, Section::Build, Installs::Allowed)]
#[case::binary_package(PackageKind::Binary, Section::Normal, Installs::Allowed)]
fn regular_subscriber_dependency_flags_library_code(
    #[case] kind: PackageKind,
    #[case] section: Section,
    #[case] expected: Installs,
) {
    let package = package(kind, vec![dependency("tracing-subscriber", section)]);
    assert_eq!(
        gate(&[package], "tracing-subscriber"),
        (expected, Installs::Allowed)
    );
}

#[rstest]
#[case::other_crate("tracing-subscriber", "tracing", Installs::Allowed)]
#[case::underscore_in_config("tracing_subscriber", "tracing-subscriber", Installs::Flagged)]
#[case::underscore_in_crate_name("tracing-subscriber", "tracing_subscriber", Installs::Flagged)]
#[case::both_underscored("tracing_subscriber", "tracing_subscriber", Installs::Flagged)]
#[case::separator_dropped("tracing_subscriber", "tracingsubscriber", Installs::Allowed)]
fn subscriber_names_match_with_dash_and_underscore_alike(
    #[case] configured: &str,
    #[case] crate_name: &str,
    #[case] expected: Installs,
) {
    let package = package(
        PackageKind::Library,
        vec![dependency(crate_name, Section::Normal)],
    );
    assert_eq!(gate(&[package], configured).0, expected);
}

/// A file is flagged when any package with the dependency reaches it as library code.
#[test]
fn any_library_reaching_a_file_flags_it() {
    let without = package(PackageKind::Library, Vec::new());
    let with = Package {
        library_code: BTreeSet::new(),
        ..package(
            PackageKind::Library,
            vec![dependency("tracing-subscriber", Section::Normal)],
        )
    };
    let reaching = package(
        PackageKind::Library,
        vec![dependency("tracing-subscriber", Section::Normal)],
    );
    assert_eq!(
        (
            gate(&[without, with], "tracing-subscriber").0,
            gate(
                &[package(PackageKind::Library, Vec::new()), reaching],
                "tracing-subscriber"
            )
            .0,
        ),
        (Installs::Allowed, Installs::Flagged)
    );
}
