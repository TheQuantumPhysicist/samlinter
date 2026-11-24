mod emoji;

use crate::config::{Case, ForbiddenPattern, ForbiddenWord};
use crate::finding::{Hit, Rule};

const EM_DASH: char = '\u{2014}';

/// A stretch of prose inside a file.
#[derive(Clone, Copy)]
pub struct Piece<'a> {
    /// Byte offset of `text` within its file.
    pub offset: usize,
    pub text: &'a str,
}

/// Checks a whole documentation file.
pub fn check(
    text: &str,
    forbidden_words: &[ForbiddenWord],
    forbidden_patterns: &[ForbiddenPattern],
) -> Vec<Hit> {
    let piece = Piece { offset: 0, text };
    marks(piece)
        .chain(forbidden(&[piece], forbidden_words, forbidden_patterns))
        .collect()
}

/// Em-dash and emoji hits in `piece`.
pub fn marks(piece: Piece) -> impl Iterator<Item = Hit> {
    let em_dashes = piece
        .text
        .char_indices()
        .filter(|&(_, character)| character == EM_DASH)
        .map(move |(index, _)| Hit::new(piece.offset + index, Rule::EmDash, "em-dash (U+2014)"));
    let emoji = emoji::starts(piece.text).map(move |(index, character)| {
        Hit::new(
            piece.offset + index,
            Rule::Emoji,
            format!("emoji U+{:04X}", u32::from(character)),
        )
    });
    em_dashes.chain(emoji)
}

/// - Forbidden-word hits in `pieces` read as one text, a line break joining each piece to
///   the next.
/// - Each hit sits at its first character's byte offset in the file.
/// - Forbidden-pattern hits: every non-empty regex match, wherever it matches; word
///   boundaries are the pattern's own job, since a pattern exists exactly when the
///   whole-word rule cannot express the shape.
pub fn forbidden(
    pieces: &[Piece],
    forbidden_words: &[ForbiddenWord],
    forbidden_patterns: &[ForbiddenPattern],
) -> Vec<Hit> {
    let joined = pieces
        .iter()
        .map(|piece| piece.text)
        .collect::<Vec<_>>()
        .join("\n");
    let starts = pieces.iter().scan(0, |next, piece| {
        let start = *next;
        *next = start + piece.text.len() + 1;
        Some(start)
    });
    // Each character of each piece as (offset in the file, offset in `joined`).
    let positions: Vec<(usize, usize)> = pieces
        .iter()
        .zip(starts)
        .flat_map(|(piece, start)| {
            piece
                .text
                .char_indices()
                .map(move |(index, _)| (piece.offset + index, start + index))
        })
        .collect();

    // Every char boundary of `joined` as (offset in `joined`, offset in the file): the
    // pieces' own, plus each piece's end, where the joining line break sits and a regex
    // match may start.
    let mut boundaries: Vec<(usize, usize)> = Vec::new();
    let mut joined_at = 0;
    for piece in pieces {
        boundaries.extend(
            piece
                .text
                .char_indices()
                .map(|(at, _)| (joined_at + at, piece.offset + at)),
        );
        boundaries.push((
            joined_at + piece.text.len(),
            piece.offset + piece.text.len(),
        ));
        joined_at += piece.text.len() + 1;
    }
    let file_at = |at: usize| {
        let index = boundaries.partition_point(|&(boundary, _)| boundary < at);
        boundaries
            .get(index)
            .expect("a regex match starts at a char boundary of the joined text")
            .1
    };

    forbidden_words
        .iter()
        .flat_map(|listed| {
            let (word, case) = (listed.as_str(), listed.case());
            positions
                .iter()
                .filter(|&&(_, at)| is_whole_phrase_at(&joined, at, word, case))
                .map(move |&(offset, _)| {
                    Hit::new(
                        offset,
                        Rule::ForbiddenWord,
                        format!("`{word}` is listed in {} in samlinter.toml", case.key()),
                    )
                })
                .collect::<Vec<_>>()
        })
        .chain(forbidden_patterns.iter().flat_map(|listed| {
            let pattern = listed.as_str();
            listed
                .regex()
                .find_iter(&joined)
                // A zero-width match has no span to report and no position to point at.
                .filter(|found| !found.is_empty())
                .map(move |found| {
                    Hit::new(
                        file_at(found.start()),
                        Rule::ForbiddenPattern,
                        format!(
                            "`{pattern}` is listed in {} in samlinter.toml",
                            ForbiddenPattern::KEY
                        ),
                    )
                })
                .collect::<Vec<_>>()
        }))
        .collect()
}

/// - Whether two line comments with `between` separating them read as one text.
/// - Only whitespace holding one line break may part them: a blank line or code ends the run.
pub fn on_next_line(between: &str) -> bool {
    between.chars().all(char::is_whitespace) && between.matches('\n').count() == 1
}

/// Whether `phrase` starts at byte `at` of `text` with case `case` and neither neighbor a
/// word character.
fn is_whole_phrase_at(text: &str, at: usize, phrase: &str, case: Case) -> bool {
    let length = text
        .get(at..)
        .and_then(|tail| phrase_length(tail, phrase, case));
    length.is_some_and(|length| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let after = text.get(at + length..).and_then(|tail| tail.chars().next());
        !before.is_some_and(is_word_character) && !after.is_some_and(is_word_character)
    })
}

/// - Byte length of `phrase` matched at the start of `text` with case `case`, if it matches
///   there.
/// - Each whitespace run between the phrase's words matches any whitespace run in `text`,
///   line breaks included, so a phrase wrapped across lines still matches.
fn phrase_length(text: &str, phrase: &str, case: Case) -> Option<usize> {
    let mut words = phrase.split_whitespace();
    let first = words.next()?;
    let first_end = word_length(text, first, case)?;
    words.try_fold(first_end, |end, word| {
        let rest = text.get(end..)?;
        let body = rest.trim_start();
        let gap = rest.len() - body.len();
        let word_end = word_length(body, word, case)?;
        (gap > 0).then_some(end + gap + word_end)
    })
}

/// Byte length of `word` matched at the start of `text` with case `case`.
fn word_length(text: &str, word: &str, case: Case) -> Option<usize> {
    match case {
        Case::Exact => text.starts_with(word).then_some(word.len()),
        Case::Insensitive => folded_length(text, word),
    }
}

/// - Byte length of `word` matched at the start of `text` whatever the case of each letter.
/// - Character by character, so the matched length stays a byte length in `text`.
fn folded_length(text: &str, word: &str) -> Option<usize> {
    let mut tail = text;
    for word_char in word.chars() {
        let text_char = tail.chars().next()?;
        if !text_char.to_lowercase().eq(word_char.to_lowercase()) {
            return None;
        }
        tail = tail.get(text_char.len_utf8()..)?;
    }
    Some(text.len() - tail.len())
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests;
