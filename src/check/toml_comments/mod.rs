use super::prose::{self, Piece};
use crate::config::ForbiddenWord;
use crate::finding::Finding;
use crate::workspace::TextFile;
use toml_parser::lexer::{Token, TokenKind};
use toml_parser::{Source, Span};

/// - Prose findings in the comments of a TOML file, at their line and column.
/// - Lexed the way the TOML parser lexes, so a `#` inside a string or a quoted key is no
///   comment, and string values are never read.
/// - Em-dashes and emoji are found in each comment, `#` included.
/// - Forbidden words are found in each run of comments on consecutive lines read as one
///   text, `#` markers dropped, so a phrase wrapped onto the next comment line still matches.
pub fn check(file: &TextFile, forbidden_words: &[ForbiddenWord]) -> Vec<Finding> {
    let text = &file.text;
    let comments: Vec<Span> = Source::new(text)
        .lex()
        .filter(is_comment)
        .map(|token| token.span())
        .collect();

    let marks = comments.iter().flat_map(|&span| {
        prose::marks(Piece {
            offset: span.start(),
            text: comment(text, span),
        })
    });
    let words = comments
        .chunk_by(|above, below| {
            text.get(above.end()..below.start())
                .is_some_and(prose::on_next_line)
        })
        .flat_map(|run| {
            let pieces: Vec<Piece> = run.iter().map(|&span| unmarked(text, span)).collect();
            prose::forbidden(&pieces, forbidden_words)
        });
    marks
        .chain(words)
        .map(|hit| hit.into_finding(&file.shown, text))
        .collect()
}

fn is_comment(token: &Token) -> bool {
    match token.kind() {
        TokenKind::Comment => true,
        TokenKind::Dot
        | TokenKind::Equals
        | TokenKind::Comma
        | TokenKind::LeftSquareBracket
        | TokenKind::RightSquareBracket
        | TokenKind::LeftCurlyBracket
        | TokenKind::RightCurlyBracket
        | TokenKind::Whitespace
        | TokenKind::Newline
        | TokenKind::LiteralString
        | TokenKind::BasicString
        | TokenKind::MlLiteralString
        | TokenKind::MlBasicString
        | TokenKind::Atom
        | TokenKind::Eof => false,
    }
}

/// A comment's text after its leading `#` markers, where it sits in the file.
fn unmarked(text: &str, span: Span) -> Piece<'_> {
    let comment = comment(text, span);
    let body = comment.trim_start_matches('#');
    Piece {
        offset: span.start() + (comment.len() - body.len()),
        text: body,
    }
}

fn comment(text: &str, span: Span) -> &str {
    // - Invariant: the lexer cuts `text` itself, at `#` and before a line break, both ASCII,
    //   so every span lies inside it on character boundaries.
    // - A span that does not is a lexer bug, not bad input, so there is nothing to report.
    text.get(span.start()..span.end())
        .expect("the TOML lexer's spans lie on character boundaries of its input")
}

#[cfg(test)]
mod tests;
