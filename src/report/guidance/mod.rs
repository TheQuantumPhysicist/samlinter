use crate::finding::Rule;
use std::fmt;

/// - The bad fix every rule shares: `exclude` silences every rule for the files it covers.
/// - Rendered after each rule's own bad fixes.
const EXCLUDE_IS_FOR_GENERATED_CODE: &str = "Do not add the file to `exclude` in samlinter.toml to silence a finding; `exclude` is for generated code.";

/// What a reader needs to fix a rule's findings properly.
pub struct Guidance {
    pub why: &'static [&'static str],
    pub fix: &'static [&'static str],
    /// Tempting edits that silence a finding but keep the problem, this rule's own.
    pub bad_fixes: &'static [&'static str],
}

impl Guidance {
    /// This rule's own bad fixes, then the one every rule shares.
    fn all_bad_fixes(&self) -> impl Iterator<Item = &'static str> {
        self.bad_fixes
            .iter()
            .copied()
            .chain([EXCLUDE_IS_FOR_GENERATED_CODE])
    }

    /// Exhaustive on purpose: a new rule does not compile until it explains itself.
    pub const fn of(rule: Rule) -> Self {
        match rule {
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
            Rule::PanicStrategy => PANIC_STRATEGY,
            Rule::RestPattern => REST_PATTERN,
            Rule::RestrictedVisibility => RESTRICTED_VISIBILITY,
            Rule::SyntaxError => SYNTAX_ERROR,
            Rule::UnfulfilledExemption => UNFULFILLED_EXEMPTION,
            Rule::WorkspaceDependencyForm => WORKSPACE_DEPENDENCY_FORM,
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

const BINARY_README: Guidance = Guidance {
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

const EM_DASH: Guidance = Guidance {
    why: &[
        "Comments and documentation should read like an engineer's notes. An em-dash usually joins two thoughts that belong in separate sentences or points.",
    ],
    fix: &[
        "Rewrite the sentence: use a colon when the second part explains the first, otherwise split it into two sentences or two dash-prefixed points.",
    ],
    bad_fixes: &[
        "Do not swap in a look-alike character such as an en-dash or a horizontal bar; the sentence still needs rewriting.",
    ],
};

const EMOJI: Guidance = Guidance {
    why: &[
        "Comments and documentation should read like an engineer's notes. Emoji carry no technical information and render differently across terminals and editors.",
    ],
    fix: &[
        "Delete the emoji, or replace it with the word it stands for, such as `done`, `fails` or `warning`.",
    ],
    bad_fixes: &[
        "Do not substitute a text symbol that plays the same role, such as a check mark or a cross; write the word.",
        "Do not append a VS15 (U+FE0E) to force the text form; the symbol still stands in for a word.",
    ],
};

const FEATURE_FLAGS: Guidance = Guidance {
    why: &[
        "Every feature flag doubles the set of build configurations. Most combinations never get built or tested, so bugs hide in them.",
        "An optional dependency is a feature flag too: cargo creates a feature named after it.",
    ],
    fix: &[
        "Delete the `[features]` table and every `optional = true`, and pick one behavior: always compile the code, or move the optional part into its own crate that users depend on explicitly.",
        "Enabling a feature of a dependency, as in `dep = { workspace = true, features = [\"x\"] }`, is fine: it is not a feature flag of this crate.",
    ],
    bad_fixes: &[
        "Do not rebuild the switch from custom `cfg` flags, environment variables or build-script logic; that is the same extra dimension under another name.",
    ],
};

const FN_SPACING: Guidance = Guidance {
    why: &[
        "A blank line between functions shows each one as a unit at a glance. rustfmt never adds it, so nothing else enforces it.",
    ],
    fix: &[
        "Insert one blank line above this function. Its doc comments and attributes belong to it, so the blank line goes above them.",
    ],
    bad_fixes: &["Do not use a comment line as the separator; it has to be a blank line."],
};

const FORBIDDEN_WORD: Guidance = Guidance {
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
    ],
};

const ITEM_ORDER: Guidance = Guidance {
    why: &[
        "Every file follows one layout, so a reader finds its submodules, imports, code and tests in the same place everywhere.",
        "`mod tests;` carries one cfg attribute, exactly `#[cfg(test)]`, so test code never compiles into the product and every tests module carries the same gate.",
    ],
    fix: &[
        "Move the item to its slot: `mod` declarations, `use` items, `#[cfg(test)]` `use` items, code, and `#[cfg(test)] mod tests;` as the last item, with one blank line between groups. `extern crate` counts as a `use` item. A `use` item whose cfg attributes require `test`, such as `#[cfg(all(test, unix))]`, is a `#[cfg(test)]` `use` item.",
        "Gate `mod tests;` with one cfg attribute, exactly `#[cfg(test)]`. Move any further condition, including a second stacked `#[cfg(..)]`, into `tests.rs`, for example as `#![cfg(unix)]` at its top.",
        "A `macro_rules!` macro is visible only to code after it, so a `mod` declaration cannot move above a macro its module uses. Move such macros into their own module and declare it first, as `#[macro_use] mod macros;`.",
    ],
    bad_fixes: &[
        "Do not move a misplaced `use` into a function body or spell out full paths instead; keep every import in the one block at the top.",
        "Do not wrap items in an inline `mod` block to escape the order.",
    ],
};

const ITEM_SPACING: Guidance = Guidance {
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

const LIBRARY_SUBSCRIBER: Guidance = Guidance {
    why: &[
        "Only the final program knows where logs go and at what level. A library that installs a subscriber fights the program's own setup, and a process can have only one global subscriber.",
    ],
    fix: &[
        "In the library, only emit log events, for example with `tracing` or `log` macros, and install the subscriber in each binary's `main`.",
        "Tests that want log output may initialize a subscriber: keep the crate under `[dev-dependencies]`.",
    ],
    bad_fixes: &[
        "Do not hide the initialization in a library helper that binaries call.",
        "Do not put the dependency behind a feature flag.",
    ],
};

const MALFORMED_EXEMPTION: Guidance = Guidance {
    why: &[
        "An exemption must name exactly which rule it lifts and why, so a reviewer can judge it. A malformed exemption exempts nothing.",
        "Only `rest_pattern` and `member_version` accept exemptions, because only they have a stated exception: types whose fields cannot be named, and dependencies that by definition serve one crate.",
    ],
    fix: &[
        "Exempt `rest_pattern` in Rust only: write `#[cfg_attr(samlinter, expect(rest_pattern, reason = \"<why>\"))]` on the item, field, match arm, statement or parameter that holds the violation. An exemption is an outer `#[...]` on the node it covers; an inner `#![...]` is malformed wherever it sits.",
        "Rust accepts no attribute on an assignment: exempt a destructuring assignment such as `(a, ..) = t;` on a block around it or on the enclosing function.",
        "Exempt `member_version` in the member's Cargo.toml only: write `[package.metadata.samlinter.expect.member_version]` with `<dependency> = \"<why>\"`.",
        "If the rule accepts no exemption, delete the exemption and fix the code.",
    ],
    bad_fixes: &[
        "Do not add or keep an exemption without the project owner's approval.",
        "Do not write a vague reason such as `needed`, `temporary` or `legacy`; name the concrete case.",
    ],
};

const MEMBER_VERSION: Guidance = Guidance {
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

const PANIC_STRATEGY: Guidance = Guidance {
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

const REST_PATTERN: Guidance = Guidance {
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

const RESTRICTED_VISIBILITY: Guidance = Guidance {
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

const SYNTAX_ERROR: Guidance = Guidance {
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

const UNFULFILLED_EXEMPTION: Guidance = Guidance {
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

const WORKSPACE_DEPENDENCY_FORM: Guidance = Guidance {
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
