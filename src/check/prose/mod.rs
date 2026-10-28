mod emoji;

use crate::config::Word;
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
pub fn check(text: &str, forbidden_words: &[Word]) -> Vec<Hit> {
    let piece = Piece { offset: 0, text };
    marks(piece)
        .chain(forbidden(&[piece], forbidden_words))
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
pub fn forbidden(pieces: &[Piece], forbidden_words: &[Word]) -> Vec<Hit> {
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

    forbidden_words
        .iter()
        .map(Word::as_str)
        .flat_map(|word| {
            positions
                .iter()
                .filter(|&&(_, at)| is_whole_phrase_at(&joined, at, word))
                .map(move |&(offset, _)| {
                    Hit::new(
                        offset,
                        Rule::ForbiddenWord,
                        format!("`{word}` is listed in forbidden-words in samlinter.toml"),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// - Whether two line comments with `between` separating them read as one text.
/// - Only whitespace holding one line break may part them: a blank line or code ends the run.
pub fn on_next_line(between: &str) -> bool {
    between.chars().all(char::is_whitespace) && between.matches('\n').count() == 1
}

/// Whether `phrase` starts at byte `at` of `text` with neither neighbor a word character.
fn is_whole_phrase_at(text: &str, at: usize, phrase: &str) -> bool {
    let length = text.get(at..).and_then(|tail| phrase_length(tail, phrase));
    length.is_some_and(|length| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let after = text.get(at + length..).and_then(|tail| tail.chars().next());
        !before.is_some_and(is_word_character) && !after.is_some_and(is_word_character)
    })
}

/// - Byte length of `phrase` matched at the start of `text`, if it matches there.
/// - Each whitespace run between the phrase's words matches any whitespace run in `text`,
///   line breaks included, so a phrase wrapped across lines still matches.
fn phrase_length(text: &str, phrase: &str) -> Option<usize> {
    let mut words = phrase.split_whitespace();
    let first = words.next()?;
    let first_end = text.starts_with(first).then_some(first.len())?;
    words.try_fold(first_end, |end, word| {
        let rest = text.get(end..)?;
        let body = rest.trim_start();
        let gap = rest.len() - body.len();
        (gap > 0 && body.starts_with(word)).then_some(end + gap + word.len())
    })
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests;
