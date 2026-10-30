use crate::finding::{Exemptability, Rule};
use std::fmt;

/// - The bad fix every rule shares: `exclude` silences every rule for the files it covers.
/// - Rendered last.
const EXCLUDE_IS_FOR_GENERATED_CODE: &str = "Do not add the file to `exclude` in samlinter.toml to silence a finding; `exclude` is for generated code.";

/// - The bad fix every exemptable rule shares.
/// - Rendered after the rule's own bad fixes.
const EXEMPTING_NEEDS_APPROVAL: &str =
    "Do not exempt the finding without the project owner's approval.";

/// A rule's own text.
struct Text {
    why: &'static [&'static str],
    fix: &'static [&'static str],
    /// Tempting edits that silence a finding but keep the problem, this rule's own.
    bad_fixes: &'static [&'static str],
}

/// What a reader needs to fix a rule's findings properly.
pub struct Guidance {
    pub why: &'static [&'static str],
    pub fix: &'static [&'static str],
    /// Tempting edits that silence a finding but keep the problem, this rule's own.
    pub bad_fixes: &'static [&'static str],
    /// Whether the shared exemption bad fix applies.
    pub exemptability: Exemptability,
}

impl Guidance {
    /// This rule's own bad fixes, then the ones it shares with other rules.
    fn all_bad_fixes(&self) -> impl Iterator<Item = &'static str> {
        let exempting: &[&'static str] = match self.exemptability {
            Exemptability::Never => &[],
            Exemptability::Source(_) | Exemptability::Manifest(_) => &[EXEMPTING_NEEDS_APPROVAL],
        };
        self.bad_fixes
            .iter()
            .chain(exempting)
            .copied()
            .chain([EXCLUDE_IS_FOR_GENERATED_CODE])
    }

    /// Exhaustive on purpose: a new rule does not compile until it explains itself.
    pub const fn of(rule: Rule) -> Self {
        let text = match rule {
            Rule::BinaryReadme => BINARY_README,
            Rule::EmDash => EM_DASH,
            Rule::Emoji => EMOJI,
            Rule::FeatureFlags => FEATURE_FLAGS,
            Rule::FnSpacing => FN_SPACING,
            Rule::ForbiddenWord => FORBIDDEN_WORD,
            Rule::ItemOrder => ITEM_ORDER,
            Rule::ItemSpacing => ITEM_SPACING,
            Rule::LibrarySubscriber => LIBRARY_SUBSCRIBER,
            Rule::MalformedExemption => MALFORMED_EXEMPTION,
            Rule::MemberVersion => MEMBER_VERSION,
            Rule::ModuleShape => MODULE_SHAPE,
            Rule::PanicStrategy => PANIC_STRATEGY,
            Rule::PathAttr => PATH_ATTR,
            Rule::RestPattern => REST_PATTERN,
            Rule::RestrictedVisibility => RESTRICTED_VISIBILITY,
            Rule::SyntaxError => SYNTAX_ERROR,
            Rule::UnfulfilledExemption => UNFULFILLED_EXEMPTION,
            Rule::WorkspaceDependencyForm => WORKSPACE_DEPENDENCY_FORM,
        };
        Self {
            why: text.why,
            fix: text.fix,
            bad_fixes: text.bad_fixes,
            exemptability: rule.exemptability(),
        }
    }
}

impl fmt::Display for Guidance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bad_fixes: Vec<&str> = self.all_bad_fixes().collect();
        [
            ("why", self.why),
            ("fix", self.fix),
            ("bad fixes", bad_fixes.as_slice()),
        ]
        .iter()
        .try_for_each(|(label, points)| {
            writeln!(formatter, "  {label}:")?;
            points
                .iter()
                .try_for_each(|point| writeln!(formatter, "    - {point}"))
        })
    }
}

const BINARY_README: Text = Text {
    why: &[
        "Someone who finds a binary needs to know what it is for and how to run it without reading its source.",
    ],
    fix: &[
        "Add the README.md the finding names, in the package directory, stating what the binary does and how it relates to the rest of the system, the exact run command, the configuration or environment it requires, and the output to expect.",
    ],
    bad_fixes: &[
        "Do not add an empty or placeholder README just to pass.",
        "Do not write a tutorial or an architecture essay; keep it to those four facts.",
    ],
};

const EM_DASH: Text = Text {
    why: &[
        "Comments and documentation should read like an engineer's notes. An em-dash usually joins two thoughts that belong in separate sentences or points.",
    ],
    fix: &[
        "Rewrite the sentence: use a colon when the second part explains the first, otherwise split it into two sentences or two dash-prefixed points.",
    ],
    bad_fixes: &[
        "Do not swap in a look-alike character such as an en-dash or a horizontal bar; the sentence still needs rewriting.",
        "Do not exempt the comment instead of rewriting it; an exemption is no substitute for fixing prose.",
    ],
};

const EMOJI: Text = Text {
    why: &[
        "Comments and documentation should read like an engineer's notes. Emoji carry no technical information and render differently across terminals and editors.",
    ],
    fix: &[
        "Delete the emoji, or replace it with the word it stands for, such as `done`, `fails` or `warning`.",
    ],
    bad_fixes: &[
        "Do not substitute a text symbol that plays the same role, such as a check mark or a cross; write the word.",
        "Do not append a VS15 (U+FE0E) to force the text form; the symbol still stands in for a word.",
        "Do not exempt the comment instead of rewriting it; an exemption is no substitute for fixing prose.",
    ],
};

const FEATURE_FLAGS: Text = Text {
    why: &[
        "Every feature flag doubles the set of build configurations. Most combinations never get built or tested, so bugs hide in them.",
        "An optional dependency is a feature flag too: cargo creates a feature named after it.",
    ],
    fix: &[
        "Delete the `[features]` table and every `optional = true`, and pick one behavior: always compile the code, or move the optional part into its own crate that users depend on explicitly.",
        "Enabling a feature of a dependency, as in `dep = { workspace = true, features = [\"x\"] }`, is fine: it is not a feature flag of this crate.",
        "The sanctioned escape for a feature that only exposes this crate's test helpers to sibling test crates, gated with `#[cfg(any(test, feature = \"<name>\"))]` at every use, is an exemption in Cargo.toml: `[package.metadata.samlinter.expect.feature_flags]` with `features = \"<why>\"`.",
    ],
    bad_fixes: &[
        "Do not rebuild the switch from custom `cfg` flags, environment variables or build-script logic; that is the same extra dimension under another name.",
    ],
};

const FN_SPACING: Text = Text {
    why: &[
        "A blank line between functions shows each one as a unit at a glance. rustfmt never adds it, so nothing else enforces it.",
    ],
    fix: &[
        "Insert one blank line above this function. Its doc comments and attributes belong to it, so the blank line goes above them.",
    ],
    bad_fixes: &["Do not use a comment line as the separator; it has to be a blank line."],
};

const FORBIDDEN_WORD: Text = Text {
    why: &[
        "The project lists this word or phrase under `forbidden-words` in samlinter.toml because it must not appear in comments or README files.",
        "Typical entries name documents that readers of the code do not have, refer to the tools used to write the code, or are filler phrases. A comment that points at a document nobody else can open explains nothing.",
    ],
    fix: &[
        "State the reasoning itself: the constraint, decision or invariant the reference stood for.",
        "If the comment only said where to look, delete it or replace it with the one fact the reader needs.",
    ],
    bad_fixes: &[
        "Do not misspell, abbreviate, hyphenate or paraphrase the word to slip past the whole-word match.",
        "Do not move the reference into a string literal or an identifier.",
        "Do not exempt the comment instead of rewriting it; an exemption is no substitute for fixing a forbidden word in prose.",
    ],
};

const ITEM_ORDER: Text = Text {
    why: &[
        "Every file follows one layout, so a reader finds its submodules, imports, code and tests in the same place everywhere.",
        "`mod tests;` carries one cfg attribute, exactly `#[cfg(test)]`, so test code never compiles into the product and every tests module carries the same gate.",
    ],
    fix: &[
        "Move the item to its slot: `mod` declarations, `use` items, `#[cfg(test)]` `use` items, code, other `#[cfg(test)]` modules, and `#[cfg(test)] mod tests;` as the last item, with one blank line between groups. A test-only module, such as `#[cfg(test)] mod test_support;`, goes at the end, not among the `mod` declarations. `extern crate` counts as a `use` item. A `use` item whose cfg attributes require `test`, such as `#[cfg(all(test, unix))]`, is a `#[cfg(test)]` `use` item.",
        "Gate `mod tests;` with one cfg attribute, exactly `#[cfg(test)]`. Move any further condition, including a second stacked `#[cfg(..)]`, into `tests.rs`, for example as `#![cfg(unix)]` at its top.",
        "A `macro_rules!` macro is visible only to code after it, so a `mod` declaration cannot move above a macro its module uses. Move such macros into their own module and declare it first, as `#[macro_use] mod macros;`.",
    ],
    bad_fixes: &[
        "Do not move a misplaced `use` into a function body or spell out full paths instead; keep every import in the one block at the top.",
        "Do not wrap items in an inline `mod` block to escape the order.",
    ],
};

const ITEM_SPACING: Text = Text {
    why: &[
        "Imports form one contiguous block, so rustfmt sorts them as one group and a reader sees every dependency in one place.",
        "One blank line marks each boundary: after the `mod` declarations, after the regular `use` items, and after the `#[cfg(test)]` `use` items.",
    ],
    fix: &[
        "Delete blank lines inside a group of `use` items, and put exactly one blank line between groups.",
    ],
    bad_fixes: &[
        "Do not separate `use` items with comment lines to keep visual groups; keep one block.",
    ],
};

const LIBRARY_SUBSCRIBER: Text = Text {
    why: &[
        "Only the final program knows where logs go and at what level. A library that installs a subscriber fights the program's own setup, and a process can have only one global subscriber.",
    ],
    fix: &[
        "In the library, only emit log events, for example with `tracing` or `log` macros. Building layers or subscribers for a binary to install is fine.",
        "Install the subscriber (`init`, `try_init`, `set_global_default`) only in each binary's `main`.",
        "Tests may install one: in a test function (an attribute whose path ends in `test`, such as `#[test]` or `#[tokio::test]`), under a cfg that requires `test`, in a file only test-only `mod` declarations or `cfg_attr(test, path = ..)` paths reach, or in a test, bench or example target or the build script.",
        "samlinter has no type information, so in such a library any `.init()` or `.try_init()` method call counts, and any function call whose last path segment is `init`, `try_init` or `set_global_default`, such as the library's own `init()`. If one is not a subscriber install, rename it or exempt the site with the project owner's approval.",
    ],
    bad_fixes: &[
        "Do not hide the install call in a macro or behind a function pointer to dodge the name match.",
        "Do not move the install into a library helper that the binary calls; that call is in the library and is flagged.",
        "Do not put the dependency behind a feature flag.",
    ],
};

const MALFORMED_EXEMPTION: Text = Text {
    why: &[
        "An exemption must name exactly which rule it lifts and why, so a reviewer can judge it. A malformed exemption exempts nothing.",
        "Every rule with findings in Rust source accepts an exemption on the node that holds the finding: `rest_pattern`, `restricted_visibility`, `item_order`, `item_spacing`, `fn_spacing`, `em_dash`, `emoji`, `forbidden_word`, `library_subscriber`, `path_attr` and `module_shape`. Of the manifest rules, `member_version` (keyed by dependency) and `feature_flags` (keyed by the `[features]` table or an optional dependency's name) do. Syntax errors and the exemption rules never do, so an exemption cannot hide itself.",
        "Comment rule findings in Cargo.toml, samlinter.toml and README files cannot be exempted: no attribute can sit there. Neither can a Rust comment that belongs to no node.",
    ],
    fix: &[
        "Exempt a Rust source rule in Rust only: write `#[cfg_attr(samlinter, expect(<rule>, reason = \"<why>\"))]` on the item, field, match arm, statement or parameter that holds the violation. It covers that node's own text; an item or a field also owns its doc comments and the plain comments directly above it. A plain comment above a `let` statement, a match arm or a parameter is outside the node. A file's `//!` docs, a comment a blank line apart from the next item and comments after the last item belong to no node and cannot be exempted: rewrite them. An exemption is an outer `#[...]` on the node it covers, or an inner `#![...]` at the top of a file or inline module; in a function body it covers nothing and is malformed.",
        "Rust accepts no attribute on an assignment: exempt a destructuring assignment such as `(a, ..) = t;` on a block around it or on the enclosing function.",
        "Exempt `member_version` in the member's Cargo.toml only: write `[package.metadata.samlinter.expect.member_version]` with `<dependency> = \"<why>\"`.",
        "Exempt `feature_flags` in any package manifest, the root package's included (a virtual root has no `[package]` table): write `[package.metadata.samlinter.expect.feature_flags]` with `features = \"<why>\"` for the `[features]` table, or with the optional dependency's name for a dependency with `optional = true`.",
        "If the rule accepts no exemption, delete the exemption and fix the code.",
    ],
    bad_fixes: &[
        "Do not add or keep an exemption without the project owner's approval.",
        "Do not write a vague reason such as `needed`, `temporary` or `legacy`; name the concrete case.",
    ],
};

const MEMBER_VERSION: Text = Text {
    why: &[
        "Each dependency has one version for the whole workspace, declared in the root manifest, so an upgrade happens in one place and crates never drift onto different versions.",
    ],
    fix: &[
        "Declare the dependency with its version in the root `[workspace.dependencies]`, then write `<name> = { workspace = true }` here, adding `features = [...]` only when this crate needs more than the root enables.",
        "Exception: a dependency that by definition serves only this crate's purpose, such as `rand` inside the one crate that owns all randomness, may keep its version here. Exempt it with the project owner's approval: `[package.metadata.samlinter.expect.member_version]` with `<name> = \"<why it belongs only here>\"`.",
        "The test for the exception is the dependency's purpose, not how many crates use it today.",
    ],
    bad_fixes: &[
        "Do not exempt a dependency just because only one crate uses it right now.",
        "Do not switch to a git or path source to avoid declaring a version.",
    ],
};

const PANIC_STRATEGY: Text = Text {
    why: &[
        "A panic means an invariant broke. With unwinding, only the panicking thread or task dies while the rest of the process keeps running on state that may be half-updated.",
        "Aborting ends the whole process, so broken state cannot spread.",
    ],
    fix: &[
        "In the root Cargo.toml, set `panic = \"abort\"` in both `[profile.dev]` and `[profile.release]`.",
        "Tests and benchmarks ignore this setting and keep unwinding, so `#[should_panic]` still works.",
    ],
    bad_fixes: &[
        "Do not set it for release only.",
        "Do not wrap production code in `catch_unwind` to keep running after a panic.",
    ],
};

const PATH_ATTR: Text = Text {
    why: &[
        "`#[path]` overrides Rust's file-name-to-module mapping, so a module can live anywhere and the module tree stops mirroring the file system.",
        "A reader can no longer find a module from its path, or tell from a file's place which module it is.",
    ],
    fix: &[
        "Move the file to where its module path says, `a/b.rs` or `a/b/mod.rs` for the module `a::b`, and delete the attribute.",
        "If the file belongs where it is, restructure the modules so the tree matches the directories.",
        "For helpers shared by integration tests, put them in `tests/common/mod.rs` and write `mod common;` in each test file that uses them.",
        "Every `path` counts, also inside `cfg_attr(..)`, nested or beside other attributes. A valid exception is platform-file selection, such as `#[cfg_attr(unix, path = \"sys_unix.rs\")]`: exempt it with the project owner's approval, `#[cfg_attr(samlinter, expect(path_attr, reason = \"<why>\"))]`, on the item that carries the attribute. An inner `#![path]` is exempted by an expect on the node it sits on: an outer expect on the enclosing inline module, or an inner expect at the top of that module or of the file.",
    ],
    bad_fixes: &[
        "Do not replace it with `include!`; samlinter does not see included files, and the code still lives apart from its module path.",
        "Do not symlink the file into place; samlinter refuses symlinks, and the link hides where the code lives.",
    ],
};

const REST_PATTERN: Text = Text {
    why: &[
        "`..` matches every field the pattern does not name, including fields added later.",
        "When a field is added, this code keeps compiling and silently ignores it, so the compiler can no longer list every place that must decide what the new field means.",
    ],
    fix: &[
        "Name every field: bind the ones you use and write `name: _` for each one you deliberately ignore. In tuple structs and tuples, write one `_` per position.",
        "Adding a field then breaks the build here, which forces a decision at this site.",
        "Only for a type whose fields cannot be named, such as a `#[non_exhaustive]` struct from another crate, keep `..` and exempt the site with the project owner's approval: `#[cfg_attr(samlinter, expect(rest_pattern, reason = \"<type> is #[non_exhaustive]; the .. absorbs <what>\"))]`, and cover the absorbed cases with tests. A destructuring assignment takes no attribute; put the exemption on a block around it.",
    ],
    bad_fixes: &[
        "Do not replace the pattern with a catch-all `_ =>` arm, a `matches!` check, or an `if let` that skips the other cases; they absorb new fields and variants the same way.",
        "Do not exempt a type defined in this workspace; its fields are known, so name them.",
    ],
};

const RESTRICTED_VISIBILITY: Text = Text {
    why: &[
        "`pub(crate)`, `pub(super)` and `pub(in path)` exist to let code outside an item's own module subtree reach into it.",
        "Needing them means the module tree does not match the dependency graph: visibility is widened by decree instead of by moving code to where it is used.",
        "The tree then stops showing what depends on what, and a later split into crates runs into hidden dependencies.",
    ],
    fix: &[
        "Find every user of the item.",
        "If every user sits inside the parent of the item's module, plain `pub` is the fix: in a private module, `pub` reaches only that parent's subtree, so the tree itself defines the reach.",
        "If a user sits elsewhere, move code so dependencies follow the tree: a module used by exactly one sibling becomes a private submodule of that sibling, and an item used by several modules moves to their nearest common ancestor.",
        "When a parent module should expose a child's item, re-export it explicitly with `pub use` at each level, so every widening is visible in the tree.",
    ],
    bad_fixes: &[
        "Do not swap in `pub` and then mark modules on the path `pub mod` until it compiles; that flattens the hierarchy until everything reaches everything.",
        "Do not merge modules or pile code into one file to dodge visibility; the module tree should mirror the dependency graph.",
    ],
};

const SYNTAX_ERROR: Text = Text {
    why: &[
        "samlinter lints only files that parse completely, because findings on a half-parsed file would be wrong.",
    ],
    fix: &[
        "Fix the syntax error; `cargo check` shows it with more context.",
        "If the compiler accepts the code but samlinter does not, samlinter's parser is older than that syntax: upgrade samlinter.",
    ],
    bad_fixes: &[
        "Do not delete the file unless nothing uses it.",
        "Do not hide the code inside a macro body, where samlinter cannot see it.",
    ],
};

const UNFULFILLED_EXEMPTION: Text = Text {
    why: &[
        "The exemption covers no finding anymore: the code it excused changed or moved, or an exemption nested inside it, or an earlier one for the same rule on the same node, already covers that finding. Left in place, it would silently excuse a future violation at that spot.",
    ],
    fix: &[
        "Delete the exemption. If the violation moved, samlinter reports the new site on its own; judge that case afresh.",
    ],
    bad_fixes: &[
        "Do not keep it just in case.",
        "Do not widen it onto an enclosing item so it matches something else.",
    ],
};

const MODULE_SHAPE: Text = Text {
    why: &[
        "The module tree must mirror the file system: a module with submodules is a directory, so a reader finds every module from its path.",
    ],
    fix: &[
        "Move the file into the directory shape: `git mv something.rs something/mod.rs`, keeping the submodules where they are.",
    ],
    bad_fixes: &[
        "Do not fake the directory with `#[path = \"something/mod.rs\"]`; `path_attr` reports every `path` attribute.",
        "Do not symlink the file into place; samlinter refuses symlinks.",
    ],
};

const WORKSPACE_DEPENDENCY_FORM: Text = Text {
    why: &[
        "Every workspace dependency uses one spelling, an inline table, so manifests stay uniform and the same line extends naturally when a crate needs extra features.",
    ],
    fix: &[
        "Write `<name> = { workspace = true }` under the dependency section, with any `features = [...]` inside the same braces.",
    ],
    bad_fixes: &[
        "Do not replace the workspace reference with a direct version; versions belong in the root manifest.",
    ],
};

#[cfg(test)]
mod tests;
