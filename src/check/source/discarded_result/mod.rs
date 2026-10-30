use super::{comment_tokens, end, outside_test_code, start};
use crate::finding::{Hit, Rule, positions};
use crate::workspace::TestCode;
use ra_ap_syntax::{AstNode, AstToken, SyntaxNode, ast};

const MESSAGE: &str = "`let _ =` discards a result without a stated reason";

/// - Every `let _ = ..;` statement with no comment on its own line or on the line directly
///   above, at the statement's start.
/// - The adjacent comment is not an exemption: stating the reason there is how this rule is
///   satisfied, so an `expect` attribute is the wrong tool and none is accepted.
/// - Silent inside test code, like every rule sharing `outside_test_code`: a file the walk
///   reached as test code whole-file, a node whose own attributes require `test`, and test
///   functions. The stated reason is always available there, as anywhere.
/// - Macro bodies are token trees, so a discard inside one is not seen.
pub fn check(root: &SyntaxNode, text: &str, testness: TestCode) -> Vec<Hit> {
    let discards: Vec<Discard> = outside_test_code(root, testness, |node| {
        let stmt = ast::LetStmt::cast(node.clone())?;
        discards_everything(&stmt).then(|| Discard {
            first: start(stmt.syntax()),
            last: end(stmt.syntax()),
        })
    });
    let comment_starts: Vec<usize> = comment_tokens(root)
        .iter()
        .map(|comment| usize::from(comment.syntax().text_range().start()))
        .collect();
    // The lines of every edge this rule judges, one linear pass per group, each result
    // aligned with the collection it came from.
    let firsts: Vec<usize> = discards.iter().map(|discard| discard.first).collect();
    let lasts: Vec<usize> = discards.iter().map(|discard| discard.last).collect();
    let placed: Vec<(usize, usize, usize)> = discards
        .iter()
        .zip(positions(text, &firsts))
        .zip(positions(text, &lasts))
        .map(|((discard, first), last)| (discard.first, first.line, last.line))
        .collect();
    let comment_lines: Vec<usize> = positions(text, &comment_starts)
        .iter()
        .map(|position| position.line)
        .collect();
    placed
        .iter()
        .filter(|(_, first, last)| !explained(&comment_lines, *first, *last))
        .map(|(first, _, _)| Hit::new(*first, Rule::DiscardedResult, MESSAGE))
        .collect()
}

/// Whether a comment sits on the statement's first line, its last line, or on the line
/// directly above its first: those state the reason for the discard.
fn explained(comment_lines: &[usize], first: usize, last: usize) -> bool {
    comment_lines
        .iter()
        .any(|line| *line == first || *line == last || line + 1 == first)
}

/// Whether the statement discards its whole result: an initializer dropped under the
/// wildcard pattern alone.
fn discards_everything(stmt: &ast::LetStmt) -> bool {
    stmt.initializer().is_some()
        && match stmt.pat() {
            Some(ast::Pat::WildcardPat(_)) => true,
            Some(
                ast::Pat::BoxPat(_)
                | ast::Pat::CfgPredPat(_)
                | ast::Pat::ConstBlockPat(_)
                | ast::Pat::DerefPat(_)
                | ast::Pat::IdentPat(_)
                | ast::Pat::LiteralPat(_)
                | ast::Pat::MacroPat(_)
                | ast::Pat::NotNull(_)
                | ast::Pat::OrPat(_)
                | ast::Pat::ParenPat(_)
                | ast::Pat::PathPat(_)
                | ast::Pat::RangePat(_)
                | ast::Pat::RecordPat(_)
                | ast::Pat::RefPat(_)
                | ast::Pat::RestPat(_)
                | ast::Pat::SlicePat(_)
                | ast::Pat::TuplePat(_)
                | ast::Pat::TupleStructPat(_),
            )
            | None => false,
        }
}

/// A `let _ = ..;` statement, by the byte offsets its adjacency is judged from.
struct Discard {
    /// Where the statement's own text starts (its first attribute or its `let`): the
    /// finding points here, and this offset fixes the statement's first line.
    first: usize,
    /// Where its last token (the semicolon) ends, which fixes its last line.
    last: usize,
}

#[cfg(test)]
mod tests;
