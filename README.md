# samlinter

Fails the build on Rust workspace conventions that the compiler and clippy cannot check.
It parses the Rust sources, `Cargo.toml` and `README.md` of every package in a Cargo
workspace and prints each violation.
No finding is a mere warning and there are no rule switches: any finding fails the run.

## Run

```sh
cargo install --path . --locked             # from a local checkout
cargo install samlinter --locked            # from crates.io
samlinter                                   # anywhere inside the workspace
samlinter --manifest-path path/to/Cargo.toml
```

- Needs `cargo` on `PATH`: the workspace layout and each package's dependencies come from
  `cargo metadata`.
- Needs `samlinter.toml` at the workspace root (see Configuration).
- Always checks the whole workspace with the root's `samlinter.toml`, also when run from a
  member directory or given a member's `Cargo.toml`.
- Every run, `--help` and `--version` included, runs on a thread with a 256 MiB stack, so
  deeply nested files fit; if the OS refuses that stack (for example under a low
  `ulimit -v`), samlinter exits 2.
- Exit status: 0 clean, 1 findings, 2 error, such as bad arguments, an invalid or missing
  `samlinter.toml`, such as a bad `exclude` entry, a failed `cargo metadata`, an unreadable
  file, a declared module with no file, a module path that cannot be inspected (for example
  behind a directory that may not be searched), a symlink (samlinter does not follow
  symlinks), output that cannot be written, or a lint thread that cannot start.
- Warnings go to stderr as `samlinter: warning: ...` and leave the exit status alone.

## Output

Findings are grouped by rule, one per line as `path:line:column: rule: message`; whole-file
findings omit the position. Paths, in findings, warnings and errors alike, are relative to
the workspace root (`.` for the root itself), absolute outside it.
Every file (sources, manifests, READMEs, `samlinter.toml`) is shown by its path with `.`
and `..` resolved; `binary_readme` names where the missing README belongs.
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
  skipped. An inline module's `cfg_attr` `path` directory that does not exist is not
  entered, and each directory is entered once however it is spelled.
- Symlinks are not followed: a file or directory samlinter reads or enters that is a
  symlink stops the run (exit 2), naming it. That holds wherever it sits: module files and
  directories, `#[path]` targets, the manifests and target roots cargo reports, READMEs and
  `samlinter.toml`. The workspace root and the directories above it are taken as given.
- `.` and `..` resolve without reading the disk, each name checked for a symlink before a
  `..` leaves it.
- A `#[path]`, or a `cfg_attr` path, whose `..` leaves a workspace root or a directory above
  it that is a symlink would lead where the link points. samlinter does not follow that
  module: it prints `samlinter: warning: <file>:<line>: ...` on stderr, naming the
  declaration, and the exit status stays what the findings make it. Other candidate files
  of the same module are still followed.
- A target root or manifest cargo reports that climbs out of a symlinked root that way is
  not followed either, with a warning naming the path; for a manifest, the whole package.
- A `..` leaving the workspace root or a directory above it that is no directory anymore is
  an error naming it.
- A path ending in `/` or `/.` names a directory, so no module file is there, as for rustc.
- A declared module with no file on disk is an error that names the declaration and the
  paths tried, always including where the module sits with no cfg set. No file means
  nothing there: a missing path, a path below a file, a name too long to exist, or
  something that is neither a file nor a directory (a socket, FIFO or device). Unlike the
  compiler, which reads every failed lookup as absent, any other failure, a denied
  permission included, is an error.
- Each file is checked once, even when `#[path]` reaches it twice, and is reported by its
  path with `.` and `..` resolved. Its `mod` declarations are followed from every route that
  reaches it: as `a.rs` they resolve under `a/`, through `#[path]` beside the file.
- A file several packages reach belongs to the one whose directory holds it, the innermost
  when packages nest; a file outside them all goes to the lowest manifest path. The file is
  parsed with that package's edition.
- Files outside the module tree, such as build output or a stray `.rs` file nothing
  declares, are never read; neither are excluded files (see Configuration).
- Not seen: files pulled in with `include!`, modules declared inside macro bodies or
  function bodies.

| Rule | Flags |
| --- | --- |
| `rest_pattern` | `..` inside struct, tuple-struct and tuple patterns, which silently absorbs a field added later |
| `restricted_visibility` | `pub(crate)`, `pub(super)`, `pub(in path)` |
| `item_order` | module items out of order: mod declarations, use items, `#[cfg(test)]` use items, code, other `#[cfg(test)]` modules, `#[cfg(test)] mod tests;` |
| `item_spacing` | a blank line inside a group of use items, or no blank line after the mod declarations, the use items or the `#[cfg(test)]` use items |
| `fn_spacing` | consecutive functions with bodies, in a module, an impl or trait body, or a block, with no blank line between them |
| `em_dash` | U+2014 in a comment |
| `emoji` | an emoji in a comment, one finding per visible emoji, at its first character |
| `forbidden_word` | a configured word or phrase in a comment |
| `library_subscriber` | a call that installs a log subscriber in a library's code, see below |
| `path_attr` | each `path` attribute: `#[path = ".."]`, `#![path = ".."]` and a `path` inside `cfg_attr(..)`, see below |
| `module_shape` | a file the walk reaches as `something.rs` that declares an out-of-line module, see below |
| `matches_macro` | a `matches!(..)` invocation, by any path spelling |
| `enum_is` | `EnumIs` in a `#[derive(..)]` list |
| `system_time_now` | a `now()` call on `SystemTime`, `Instant` (std or tokio), `Utc` or `Local`, by any route |
| `regex_use` | the regex crate: any path whose leading segment is `regex`, plus a `Regex::new` / `RegexBuilder::new` constructor path, called or not; look-alike crates stay silent |
| `direct_rng` | direct randomness: `thread_rng()`, `random()`, an RNG type (`OsRng`, `StdRng`, `SmallRng`, `ChaChaRng`), `from_entropy` or `seed_from_u64` |
| `json_macro` | a `json!` macro invocation in non-test code, see below |
| `discarded_result` | a `let _ = ..;` with no comment stating why the discard is safe on its line or the one above |

- `rest_pattern`: also on the left of a destructuring assignment, such as `(a, ..) = t;` or
  `(..) = t;`.
- `rest_pattern`: slice rests stay allowed.
- `item_order`: a use item is a `#[cfg(test)]` use item when its cfg attributes together
  require `test`: a stacked `#[cfg(test)]`, or `test` inside `all(..)`.
- `item_order`: `any(..)` and `not(..)` do not count.
- `item_order`: `extern crate` counts as a use item, by the same cfg rule.
- `item_order`: a module other than `mod tests;` is a `#[cfg(test)]` module by the same cfg
  rule, declared or inline, gated outside (`#[cfg(test)] mod m { .. }`) or inside
  (`mod m { #![cfg(test)] .. }`); one with no such gate, an inline `mod tests { .. }`
  included, is a mod declaration or code. Other `#[cfg(test)]` items stay code.
- `item_order`: a `mod x;` whose file starts with `#![cfg(test)]` is not known as test-gated:
  the check reads one file at a time, so it stays a mod declaration.
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
- `library_subscriber`: flags, in the library code of a package without a binary target that
  lists a configured subscriber crate as a regular dependency, each `.init()` and
  `.try_init()` method call and each function call whose last path segment is `init`,
  `try_init` or `set_global_default`, at that name.
- `library_subscriber`: the dependency, renamed or not, target-specific or not, only turns the
  check on; dev and build dependencies do not. Building layers or subscribers is fine.
- `library_subscriber`: library code is what a library target reaches through `mod`
  declarations none of which requires `test` by the `item_order` cfg rule, inline modules and
  a file's own `#![cfg(test)]` included; a file only a `#[cfg_attr(test, path = "..")]` path
  reaches is not. A file any such route reaches is library code, whichever package owns it:
  a library that reaches another package's file through `#[path]` gets it checked too.
- `library_subscriber`: test code may install one: a call under a cfg that requires `test`
  or inside a test function, one with an attribute whose path ends in `test` (`#[test]`,
  `#[tokio::test]`, `#[test_log::test]`), a file only test-only declarations reach
  (`#[cfg(test)] mod tests;` leads to `tests.rs` and everything below it), and the test,
  bench, example and build script targets.
- `library_subscriber`: matched by name, with no type information, so any call so named
  counts, such as the library's own `init()` or `Config::init()`; a call inside a
  macro body is not seen.
- `path_attr`: `#[path]` overrides the file-name-to-module mapping, so the module tree stops
  mirroring the file system.
- `path_attr`: outer and inner, also inside `cfg_attr(..)`, nested
  (`cfg_attr(a, cfg_attr(b, path = ".."))`) or beside other attributes; whatever item it
  sits on, in every file samlinter reads, under `src/` or `tests/` alike. The key also
  counts in its raw-identifier spelling, `r#path`.
- `path_attr`: one finding per `path = ..`, at its `path` key, so two in one `cfg_attr` are
  two findings. A `path` attribute inside a macro body is not seen.
- `path_attr`: it only reports: the module walk still follows every `path` attribute as
  described above.
- `path_attr`: exempted on the item that carries the attribute. An inner `#![path]` is
  exempted by an expect on the node it sits on: an outer expect on the enclosing inline
  module, or an inner expect at the top of that module or of the file.
- `module_shape`: a module with submodules is a directory: the file is `something/mod.rs`,
  with the submodules under `something/` (`something/tests.rs`, `something/other.rs`). The
  rule flags each out-of-line `mod x;` in a file the walk reaches as `something.rs`, at the
  `mod` keyword, whether or not the child file exists (a missing child is already an error
  earlier), inside inline modules included.
- `module_shape`: no finding in a `something/mod.rs` file declaring submodules, in an
  inline module with no out-of-line declaration inside, in a file reached only through
  `#[path]` (`path_attr` reports those), or in any target root. Test-gated code is linted
  alike: a `tests.rs` module declaring helper files moves to `tests/mod.rs`. Modules
  declared inside function bodies or macro bodies are not seen.
- `module_shape`: exempted on the `mod x;` item, or by an inner expect at the top of the
  file, which covers every declaration in it.
- `matches_macro`: flagged at the macro's name, whatever path it is spelled by. Any other
  macro, and any call to a function or method named `matches`, is not a finding.
- `enum_is`: at most one finding per derive list, at the first `EnumIs` token; a list
  nested in another attribute's token tree, as in `cfg_attr(.., derive(..))`, counts the
  same as a direct `#[derive(..)]`.
- `system_time_now`: matched by name, so a `.now()` method call on a value of unknown type
  is not a finding. A bare `now` reference with no call parentheses, as in
  `let f = Utc::now;`, is not a call and stays silent. `Duration` and `TimeDelta`
  arithmetic stays allowed.
- `regex_use`: the leading-segment match covers use trees at any depth
  (`use regex::{Regex, internal::..}`) and every type or expression path; the constructor
  match covers any path ending in `Regex::new` / `RegexBuilder::new` whose leading segment
  is not `regex`, called or not.
- `direct_rng`: an RNG type name flags wherever it appears in a path: a use tree, a type
  annotation, a bare value, or a constructor called on it. `random` counts only with call
  parentheses or a turbofish.
- `direct_rng`: matched by name with no type information: a trait-method call such as
  `rng.gen()` is not flagged, so the rule is a tripwire for constructors and free
  functions, not a proof.
- `json_macro`: every path form counts: `json!(..)`, `serde_json::json!(..)`, any leading path
  ending in `::json!`, in expression or item position, reported at the macro's name. A
  `json!` inside a macro body is not seen.
- `json_macro`: silent inside test code: an item under a `#[cfg(test)]` module or function
  (an inline module's inner `#![cfg(test)]` counts), or inside a test function, one marked
  by an attribute whose path ends in `test` (`#[test]`, `#[tokio::test]`). Elsewhere a
  single justified use is exempted with an expect on the function or item that holds it.
- `discarded_result`: every `let _ = ..;` with an initializer. samlinter sees no types, so
  it cannot tell a fallible call from an infallible one: the workspace's code rules want a
  stated reason on every deliberate discard, and one line on a rare non-fallible discard
  is the accepted cost. A `let _ = &field;` existence assertion counts like any other.
- `discarded_result`: the comment is not an exemption attribute; it is the rule's own
  satisfied condition: a comment token on the statement's line or on the line directly
  above it. A blank line between the comment and the statement breaks adjacency, and a
  comment above a different statement explains that one, not this one. The statement's
  first line is its first attribute or the `let`; its last line is the semicolon's.
- `discarded_result`: accepts no expect attribute; an exemption naming it is a
  `malformed_exemption`.

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

- `binary_readme`: `README.md` follows the same nothing-there rule as module files; a
  `README.md` directory is an error.
- `binary_readme`: the finding names the path the README belongs at.

`README.md` in each package's directory, and at the root of a workspace with no root
package: `em_dash`, `emoji`, `forbidden_word`.

Always on:

- `syntax_error`: a file that does not parse is reported, never half-linted.
- `malformed_exemption` and `unfulfilled_exemption`: see Exemptions.

Limits: no type information, so `library_subscriber` flags any call named `init`, `try_init`
or `set_global_default` in such a library. Macro bodies are token trees: only
the comment rules look inside them. Nesting is tested to 5000 levels of parentheses and
inline modules, and to 20000 inline modules in a file with a syntax error; deeper or other
nesting may abort with a stack overflow.
A file whose syntax tree is more than 1000 levels deep keeps that tree in memory until
samlinter exits. The parser's time grows quadratically with nested parentheses: 8000 levels
take about 5 s in a debug build.

## Configuration

`samlinter.toml` sits at the workspace root. Every key but `exclude` is required; unknown keys
are an error.

```toml
forbidden-words = ["NOTES.md", "it is worth noting"]
forbidden-words-insensitive = ["organise", "learnt"]
subscriber-crates = ["tracing-subscriber"]
exclude = ["src/generated", "build/bindings.rs"]
```

- `forbidden-words`: banned in comments and README files, matched case-sensitively as whole words:
  an entry fires only on text of its own case. Proper nouns and file names go here.
- `forbidden-words-insensitive`: banned like `forbidden-words`, matched case-insensitively:
  any casing of an entry fires, a sentence start included. Spellings that must stay American
  go here.
- The words of a phrase match across any whitespace run, line breaks included. Line comments
  of one kind on consecutive lines (`//`, `///` or `//!` in Rust, `#` in TOML) read as one
  text with their markers dropped, so a phrase wrapped onto the next comment line matches; a
  blank line or code between them ends the run. A finding points at the phrase's first
  character.
- Entries in every list must be non-empty, with no surrounding whitespace.
- `subscriber-crates`: crates that install a log subscriber. A package without a binary target
  that lists one as a regular dependency gets its library code checked for install calls
  (`library_subscriber`); the dependency alone is no finding. As on crates.io, `-` and `_` in
  a name match each other.
- `exclude`: optional, default empty. Files and directories every rule is silenced for; it is
  for generated code, never for silencing a finding in code people write.
  - Each entry is a path relative to the workspace root, written with `/`, naming a file or
    a directory; a directory covers everything below it, `.` the whole root. No globs.
  - An entry that is absolute, climbs above the root with `..`, names nothing, or passes
    through a symlink is an error (exit 2) naming the entry, so a stale entry cannot
    silently hide nothing or the wrong thing.
  - A covered file gets no finding from any rule, whatever kind of file it is.
  - A module lookup stops at the first path an entry covers: a covered Rust file is never
    read, its `mod` declarations are not followed, and a symlink, a directory that may not
    be searched or an unreadable file below a covered path never stops the run.
  - A covered `Cargo.toml`, `README.md` or `samlinter.toml` is still read; only its
    findings are dropped.
  - Files outside the workspace root are never covered.

Nothing in the file disables a rule.

## Exemptions

Every exemption needs a reason, and is written in the kind of file its findings appear in:

- Rust: every rule with findings in Rust source, `rest_pattern`, `restricted_visibility`,
  `item_order`, `item_spacing`, `fn_spacing`, `em_dash`, `emoji`, `forbidden_word`,
  `json_macro`, `library_subscriber`, `path_attr`, `module_shape`, `matches_macro`, `enum_is`,
  `system_time_now`, `regex_use` and `direct_rng`, on the node that holds the finding. A comment that belongs to no node
  cannot be exempted (see below).
- A member `Cargo.toml`: `member_version`.
- A package `Cargo.toml`, a member's or the root package's: `feature_flags`. A virtual root
  has no `[package]` table to write the exemption in, so its `[features]` table, if any,
  cannot be exempted.
- Nowhere: `syntax_error`, `discarded_result`, `malformed_exemption`,
  `unfulfilled_exemption` and the other Cargo.toml rules. Comment findings in `Cargo.toml`,
  `samlinter.toml` and README files cannot be exempted either: no attribute can sit there.

An exemption naming a rule in the other kind of file, or a rule that accepts none, is a
`malformed_exemption`.

In Rust, put the attribute on the item, field, match arm, statement or parameter that holds
the violation:

```rust
#[cfg_attr(samlinter, expect(rest_pattern, reason = "vendor::Config is #[non_exhaustive]"))]
let vendor::Config { timeout, .. } = config;
```

- Nothing ever sets the `samlinter` cfg, so the compiler drops the attribute. It is valid on
  stable Rust in every attribute position.
- It covers the node's own text. An item or a field also owns its doc comments and the plain
  comments directly above it. A `let` statement, a match arm or a parameter does not own the
  plain comment above it, and a comment after a node on its last line is outside it.
- A file's `//!` docs, a comment a blank line apart from the next item and comments after the
  last item belong to no node, so no exemption reaches them: rewrite the comment.
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

In a member's `Cargo.toml`, a `member_version` exemption names the dependency; a
`feature_flags` exemption, in a member's or the root package's, names the `[features]` table
or the optional dependency:

```toml
[package.metadata.samlinter.expect.member_version]
rand = "rand serves only this crate's business case"

[package.metadata.samlinter.expect.feature_flags]
features = "the feature only exposes this crate's test helpers, gated as cfg(any(test, feature = ..))"
```

An exemption that matches nothing is itself a finding, so stale exemptions cannot pile up.
When Rust exemptions nest, a finding counts only for the innermost one, so an outer exemption
that repeats an inner one is unfulfilled, as is the second naming of a rule on one node,
in one attribute or in two.

## License

AGPL-3.0-only, see `LICENSE`.
