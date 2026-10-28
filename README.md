# samlinter

Fails the build on Rust workspace conventions that the compiler and clippy cannot check.
It parses the Rust sources, `Cargo.toml` and `README.md` of every package in a Cargo
workspace and prints each violation.
There are no warnings and no rule switches: any finding fails the run.

## Run

```sh
cargo install --path . --locked             # from a local checkout
cargo install samlinter --locked            # from crates.io, once published
samlinter                                   # anywhere inside the workspace
samlinter --manifest-path path/to/Cargo.toml
```

- Needs `cargo` on `PATH`: the workspace layout and each package's dependencies come from
  `cargo metadata`.
- Needs `samlinter.toml` at the workspace root (see Configuration).
- Always checks the whole workspace with the root's `samlinter.toml`, also when run from a
  member directory or given a member's `Cargo.toml`.
- Exit status: 0 clean, 1 findings, 2 error, such as bad arguments, an invalid or missing
  `samlinter.toml`, a failed `cargo metadata`, an unreadable file, a declared module with no
  file, or output that cannot be written.

## Output

Findings are grouped by rule, one per line as `path:line:column: rule: message`; whole-file
findings omit the position. Paths are relative to the workspace root, absolute outside it.
Columns count characters; a byte order mark starting the file
takes none. The finding count goes to stderr. Each group ends with three
indented sections that tell whoever fixes the finding what to do:

- `why`: the design reason behind the rule.
- `fix`: how to resolve the finding properly.
- `bad fixes`: tempting edits that silence the finding but keep the problem, each starting
  with "Do not".

```text
app/src/main.rs:11:1: restricted_visibility: `pub(crate)` bypasses the module hierarchy
  why:
    - `pub(crate)`, `pub(super)` and `pub(in path)` exist to let code outside an item's own module subtree reach into it.
    ...
  fix:
    - Find every user of the item.
    ...
  bad fixes:
    - Do not swap in `pub` and then mark modules on the path `pub mod` until it compiles; that flattens the hierarchy until everything reaches everything.
    ...
```

## Rules

Rust sources are found the way the compiler finds them, not by listing directories:

- Each target root `cargo metadata` reports (library, binaries, tests, examples, benches,
  build script) is a source.
- From each source, every `mod name;` declaration is followed to `name.rs` or `name/mod.rs`
  by the Rust reference's module path rules, inline `mod m { .. }` blocks and `#[path]`
  included; an inline module's inner `#![path]` counts after its outer attributes.
- cfg is not evaluated: every declared module is checked, test modules included, and each
  `#[cfg_attr(.., path = "..")]` file or inline module directory too. Like the compiler,
  only the first `path` attribute counts, so one after an unconditional `#[path]` is
  skipped.
- A declared module with no file on disk is an error that names the declaration and the
  paths tried.
- Each file is checked once, even when `#[path]` or a symlink reaches it twice, and is
  reported by its real path, with `..` and symlinks resolved. Its `mod` declarations are
  followed from every route that reaches it: as `a.rs` they resolve under `a/`, through
  `#[path]` beside the file.
- A file several packages reach belongs to the one whose directory holds it, the innermost
  when packages nest; a file outside them all goes to the lowest manifest path. A package's
  directory is where cargo finds its `Cargo.toml`, also when that is a symlink. The file is
  parsed with that package's edition.
- Files outside the module tree, such as build output or a stray `.rs` file nothing
  declares, are never read.
- Not seen: files pulled in with `include!`, modules declared inside macro bodies or
  function bodies.

| Rule | Flags |
| --- | --- |
| `rest_pattern` | `..` inside struct, tuple-struct and tuple patterns, which silently absorbs a field added later |
| `restricted_visibility` | `pub(crate)`, `pub(super)`, `pub(in path)` |
| `item_order` | module items out of order: mod declarations, use items, `#[cfg(test)]` use items, code, `#[cfg(test)] mod tests;` |
| `item_spacing` | a blank line inside a group of use items, or no blank line after the mod declarations, the use items or the `#[cfg(test)]` use items |
| `fn_spacing` | consecutive functions with bodies, in a module, an impl or trait body, or a block, with no blank line between them |
| `em_dash` | U+2014 in a comment |
| `emoji` | an emoji in a comment, one finding per visible emoji, at its first character |
| `forbidden_word` | a configured word or phrase in a comment |

- `rest_pattern`: also on the left of a destructuring assignment, such as `(a, ..) = t;` or
  `(..) = t;`.
- `rest_pattern`: slice rests stay allowed.
- `item_order`: a use item is a `#[cfg(test)]` use item when its cfg attributes together
  require `test`: a stacked `#[cfg(test)]`, or `test` inside `all(..)`.
- `item_order`: `any(..)` and `not(..)` do not count.
- `item_order`: `extern crate` counts as a use item, by the same cfg rule.
- `item_order`: an inline `mod tests { .. }` counts as code.
- `item_order`: also a `mod tests;` whose cfg attributes are not exactly one `#[cfg(test)]`;
  further conditions, a second stacked `#[cfg(..)]` included, go inside `tests.rs`.
- `emoji`: a VS16, an emoji joined on with U+200D (a skin tone included) and a flag's second
  letter are part of the emoji before them.
- `emoji`: so a text symbol such as U+2714 followed by a VS16 is reported at U+2714.
- `emoji`: a skin tone is part of a modifier base right before it, such as U+261D or U+1F44D.
- `emoji`: a skin tone anywhere else, after an emoji that takes no skin tone or one that
  already has one included, is reported at the skin tone itself.
- `emoji`: a VS16 with no symbol or emoji before it shows nothing and is not reported.
- `emoji`: a VS15 right after a character that is an emoji on its own, such as U+2614 or
  U+1F44D, asks for its text form, so that character is not reported.
- `emoji`: a skin tone or a flag letter has no text form, so a VS15 after one changes nothing.

The comment rules read every comment, doc comments included, also inside macro bodies.
They read the `#` comments of every `Cargo.toml` samlinter checks and of `samlinter.toml`
too, never their string values; a manifest that does not parse gets only its `syntax_error`.

Cargo manifests:

| Rule | Flags |
| --- | --- |
| `member_version` | a version requirement outside the root manifest |
| `workspace_dependency_form` | `x.workspace = true` or a `[dependencies.x]` table with `workspace = true`, instead of `x = { workspace = true }` |
| `feature_flags` | a `[features]` table, or a dependency with `optional = true`, which cargo turns into a feature |
| `panic_strategy` | a root manifest whose `[profile.dev]` or `[profile.release]` is missing or does not set `panic = "abort"` |
| `binary_readme` | a package with a binary target and no `README.md` in its package directory |
| `library_subscriber` | a package without a binary target with a configured subscriber crate as a regular dependency, renamed or not; dev and build dependencies may use one |

- `binary_readme`: the package directory is where cargo finds its `Cargo.toml`, also when
  that is a symlink.
- `binary_readme`: a `README.md` symlink that leads nowhere counts as no README.
- `binary_readme`: the finding names the path the README belongs at.

`README.md` in each package's directory, and at the root of a workspace with no root
package: `em_dash`, `emoji`, `forbidden_word`. A README two packages share, through a
symlink say, is reported once.

Always on:

- `syntax_error`: a file that does not parse is reported, never half-linted.
- `malformed_exemption` and `unfulfilled_exemption`: see Exemptions.

Limits: no type information. Macro bodies are token trees: only the comment rules look inside
them.

## Configuration

`samlinter.toml` sits at the workspace root. Every key is required; unknown keys are an error.

```toml
forbidden-words = ["NOTES.md", "it is worth noting"]
subscriber-crates = ["tracing-subscriber"]
```

- `forbidden-words`: banned in comments and README files, matched case-sensitively as whole words.
  The words of a phrase match across any whitespace run, line breaks included. Line comments
  of one kind on consecutive lines (`//`, `///` or `//!` in Rust, `#` in TOML) read as one
  text with their markers dropped, so a phrase wrapped onto the next comment line matches; a
  blank line or code between them ends the run. A finding points at the phrase's first
  character.
- Entries in both lists must be non-empty, with no surrounding whitespace.
- `subscriber-crates`: crates that install a log subscriber; a package without a binary target
  may list them only as dev or build dependencies. As on crates.io, `-` and `_` in a name
  match each other.

The file holds facts, not policy: nothing in it disables a rule or excludes a path.

## Exemptions

Only `rest_pattern` and `member_version` accept exemptions, and each one needs a reason.
Each is exempted in the kind of file its findings appear in: `rest_pattern` in Rust,
`member_version` in a member `Cargo.toml`. An exemption naming a rule in the other kind of
file, or a rule that accepts none, is a `malformed_exemption`.

In Rust, put the attribute on the item, field, match arm, statement or parameter that holds
the violation:

```rust
#[cfg_attr(samlinter, expect(rest_pattern, reason = "vendor::Config is #[non_exhaustive]"))]
let vendor::Config { timeout, .. } = config;
```

- Nothing ever sets the `samlinter` cfg, so the compiler drops the attribute. It is valid on
  stable Rust in every attribute position.
- Stable Rust accepts no attribute on an assignment, so a destructuring assignment such as
  `(a, ..) = t;` cannot carry one. Put the exemption on a block around it,
  `#[cfg_attr(..)] { (a, ..) = t; }`, or on the enclosing function.
- An exemption is an outer `#[...]` on the node it covers. An inner attribute,
  `#![cfg_attr(samlinter, ...)]`, is a `malformed_exemption` wherever it sits.
- Declare the cfg once so the `unexpected_cfgs` lint accepts it, in `[lints.rust]` or
  `[workspace.lints.rust]`:

```toml
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(samlinter)"] }
```

In a member `Cargo.toml`, an exemption names the dependency:

```toml
[package.metadata.samlinter.expect.member_version]
rand = "rand serves only this crate's business case"
```

An exemption that matches nothing is itself a finding, so stale exemptions cannot pile up.
When Rust exemptions nest, a finding counts only for the innermost one, so an outer exemption
that repeats an inner one is unfulfilled, as is the second naming of a rule on one node,
in one attribute or in two.

## License

AGPL-3.0-only, see `LICENSE`.
