mod exemption;
mod layout;
mod module_shape;
mod path_attr;
mod patterns;
mod subscriber;
mod visibility;

use super::prose::{self, Piece};
use crate::config::Config;
use crate::finding::{BOM, Finding, Hit, Rule, positions};
use crate::workspace::{FileKind, TextFile};
use ra_ap_syntax::ast::CommentShape;
use ra_ap_syntax::{AstNode, AstToken, Edition, NodeOrToken, SyntaxError, SyntaxNode, ast};
use std::path::Path;
pub use subscriber::{Gate, Installs};

pub fn check(
    file: &TextFile,
    edition: Edition,
    config: &Config,
    installs: Installs,
    kind: FileKind,
) -> Vec<Finding> {
    let text = rustc_input(&file.text);

    let hits = match parse(&text, edition) {
        Ok(tree) => lint(&tree, &text, config, installs, kind, &file.shown),
        Err(errors) => errors
            .iter()
            .map(|error| {
                Hit::new(
                    usize::from(error.range().start()),
                    Rule::SyntaxError,
                    error.to_string(),
                )
            })
            .collect(),
    };

    let offsets: Vec<usize> = hits.iter().map(|hit| hit.offset).collect();
    hits.into_iter()
        .zip(positions(&text, &offsets))
        .map(|(hit, position)| hit.at(&file.shown, position))
        .collect()
}

/// - The text rustc lexes: a leading byte order mark dropped, CRLF line endings read as LF.
/// - Positions computed on it match rustc's lines and columns.
fn rustc_input(text: &str) -> String {
    text.strip_prefix(BOM).unwrap_or(text).replace("\r\n", "\n")
}

/// A file with any syntax error is reported, never linted half-parsed.
fn parse(text: &str, edition: Edition) -> Result<ast::SourceFile, Vec<SyntaxError>> {
    let (tree, errors) = crate::syntax::parse(text, edition);
    if errors.is_empty() {
        Ok(tree)
    } else {
        Err(errors)
    }
}

/// `text` is the source `tree` was parsed from; `shown` is where output shows the file.
fn lint(
    tree: &ast::SourceFile,
    text: &str,
    config: &Config,
    installs: Installs,
    kind: FileKind,
    shown: &Path,
) -> Vec<Hit> {
    let root = tree.syntax();
    let hits = patterns::check(root)
        .chain(visibility::check(root))
        .chain(path_attr::check(root))
        .chain(module_shape::check(tree, kind, shown))
        .chain(layout::check(tree))
        .chain(comment_hits(root, text, config))
        .chain(subscriber::check(root, installs))
        .collect();
    exemption::apply(root, hits)
}

/// - Plain and doc comment tokens alike, also inside macro bodies, where doc comments stay
///   bare tokens instead of `DOC_COMMENT` nodes.
/// - Em-dashes and emoji are found in each comment, markers included.
/// - Forbidden words are found in each run of line comments read as one text, markers
///   dropped, so a phrase wrapped onto the next comment line still matches.
fn comment_hits(root: &SyntaxNode, text: &str, config: &Config) -> Vec<Hit> {
    let comments: Vec<ast::AnyComment> = root
        .descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter_map(ast::AnyComment::cast)
        .collect();

    let marks = comments.iter().flat_map(|comment| {
        prose::marks(Piece {
            offset: usize::from(comment.syntax().text_range().start()),
            text: comment.text_with_markers(),
        })
    });
    let words = comments
        .chunk_by(|above, below| continues(text, above, below))
        .flat_map(|run| {
            let pieces: Vec<Piece> = run.iter().map(unmarked).collect();
            prose::forbidden(&pieces, &config.forbidden_words)
        });
    marks.chain(words).collect()
}

/// - Whether `below` continues the run of `above`: both line comments of one kind (plain,
///   outer doc or inner doc), on consecutive lines with nothing between them.
/// - A block comment is a run of its own.
fn continues(text: &str, above: &ast::AnyComment, below: &ast::AnyComment) -> bool {
    let between = text.get(
        usize::from(above.syntax().text_range().end())
            ..usize::from(below.syntax().text_range().start()),
    );
    match above.shape() {
        CommentShape::Line => {
            above.kind() == below.kind() && between.is_some_and(prose::on_next_line)
        }
        CommentShape::Block => false,
    }
}

/// A comment's text without its markers, where it sits in the file.
fn unmarked(comment: &ast::AnyComment) -> Piece<'_> {
    Piece {
        offset: usize::from(comment.syntax().text_range().start()) + comment.prefix().len(),
        text: comment.text(),
    }
}

/// - Where a node's own text starts: its first doc comment, attribute or token, whichever comes
///   first.
/// - Outer doc comments are `DOC_COMMENT` nodes, so they count; plain comments are trivia.
/// - The parser files the plain comments above an item, and a trailing comment ending the
///   line before it, under the item, so they are skipped.
fn start(node: &SyntaxNode) -> usize {
    let first = node
        .children_with_tokens()
        .find(|element| !element.kind().is_trivia());
    let range = first.map_or_else(|| node.text_range(), |element| element.text_range());
    usize::from(range.start())
}

/// Runs `rule` on edition-2024 `text`, which must parse.
#[cfg(test)]
fn hits(text: &str, rule: impl FnOnce(&ast::SourceFile) -> Vec<Hit>) -> Vec<Hit> {
    let parse = ast::SourceFile::parse(text, Edition::Edition2024);
    assert!(
        parse.errors().is_empty(),
        "test input must parse: {:?}",
        parse.errors()
    );
    rule(&parse.tree())
}

/// Each hit of `rule` on edition-2024 `text` as `line:column rule`.
#[cfg(test)]
fn render(text: &str, rule: impl FnOnce(&ast::SourceFile) -> Vec<Hit>) -> Vec<String> {
    hits(text, rule)
        .iter()
        .map(|hit| {
            format!(
                "{} {}",
                crate::finding::Position::of(text, hit.offset),
                hit.rule.name()
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;
