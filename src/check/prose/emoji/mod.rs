use unicode_properties::emoji::{
    EmojiStatus, UnicodeEmoji, is_emoji_presentation_selector, is_regional_indicator, is_zwj,
};

/// What a character can do in an emoji sequence.
enum Glyph {
    RegionalIndicator,
    /// A skin-tone modifier.
    Modifier,
    /// VS16, which asks for the emoji form of the character before it.
    PresentationSelector,
    Joiner,
    /// Renders as emoji on its own and takes no skin tone.
    Presentation,
    /// Renders as emoji on its own; a skin tone right after it is part of it.
    PresentationModifierBase,
    /// Renders as text on its own; a VS16 or skin tone turns it into an emoji.
    ModifierBase,
    /// Renders as text on its own; only a VS16 turns it into an emoji.
    Component,
    /// Renders as emoji on its own, but the VS15 right after it asks for its text form.
    TextPresentation,
    Text,
}

/// VS15, which asks for the text form of the character before it.
const TEXT_PRESENTATION_SELECTOR: char = '\u{fe0e}';

impl Glyph {
    /// The glyph of `character`, which `following` comes right after, if anything does.
    fn of(character: char, following: Option<char>) -> Self {
        let glyph = if is_regional_indicator(character) {
            Self::RegionalIndicator
        } else if is_emoji_presentation_selector(character) {
            Self::PresentationSelector
        } else if is_zwj(character) {
            Self::Joiner
        } else {
            Self::of_status(character.emoji_status())
        };
        match following {
            Some(TEXT_PRESENTATION_SELECTOR) => glyph.in_text_form(),
            // Any other character, or the end of the text, leaves the default presentation.
            Some(_) | None => glyph,
        }
    }

    /// - The glyph a VS15 right after it leaves.
    /// - A character that renders as emoji on its own takes its text form (UTS #51).
    /// - A skin tone or regional indicator has no text form: it only builds a sequence.
    /// - The VS15 itself is text, so it ends any sequence it follows.
    const fn in_text_form(self) -> Self {
        match self {
            Self::Presentation | Self::PresentationModifierBase => Self::TextPresentation,
            Self::RegionalIndicator
            | Self::Modifier
            | Self::PresentationSelector
            | Self::Joiner
            | Self::ModifierBase
            | Self::Component
            | Self::TextPresentation
            | Self::Text => self,
        }
    }

    const fn of_status(status: EmojiStatus) -> Self {
        match status {
            EmojiStatus::EmojiPresentationAndModifierAndEmojiComponent => Self::Modifier,
            EmojiStatus::EmojiPresentation | EmojiStatus::EmojiPresentationAndEmojiComponent => {
                Self::Presentation
            }
            EmojiStatus::EmojiPresentationAndModifierBase => Self::PresentationModifierBase,
            EmojiStatus::EmojiModifierBase => Self::ModifierBase,
            EmojiStatus::NonEmojiButEmojiComponent
            | EmojiStatus::EmojiOther
            | EmojiStatus::EmojiOtherAndEmojiComponent => Self::Component,
            // `EmojiStatus` is `#[non_exhaustive]`: statuses added by later `unicode-properties`
            // releases count as text until listed here.
            EmojiStatus::NonEmoji | _ => Self::Text,
        }
    }
}

/// A character's byte offset and the character itself.
type Start = (usize, char);

/// Where the scan stands after a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sequence {
    Outside,
    /// After a text symbol: a VS16 makes it an emoji, reported at the symbol.
    AfterSymbol(Start),
    /// After a text modifier base: a VS16 or skin tone makes it an emoji, reported at the base.
    AfterModifierBase(Start),
    /// Right after a modifier base in a flagged emoji: a skin tone, VS16 or joiner continues it.
    Open,
    /// Elsewhere in a flagged emoji: a VS16 or joiner continues it; a skin tone is a new emoji.
    Closed,
    /// After a joiner in a flagged emoji: the next emoji character continues it.
    Joined,
    /// After a flagged regional indicator still waiting for its partner.
    HalfFlag,
}

/// One character's effect on the scan.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// Starts a new visible emoji at the given character.
    Starts(Start, Sequence),
    /// Text, or part of the emoji already flagged.
    Passes(Sequence),
}

impl Sequence {
    /// The step for `glyph`, the glyph of the character at `here`.
    const fn step(self, glyph: Glyph, here: Start) -> Step {
        match (self, glyph) {
            (
                Self::Outside
                | Self::AfterSymbol(_)
                | Self::AfterModifierBase(_)
                | Self::Open
                | Self::Closed
                | Self::Joined
                | Self::HalfFlag,
                Glyph::TextPresentation | Glyph::Text,
            )
            | (Self::Outside | Self::AfterSymbol(_) | Self::AfterModifierBase(_), Glyph::Joiner)
            // A VS16 with nothing before it to change shows nothing.
            | (Self::Outside, Glyph::PresentationSelector) => Step::Passes(Self::Outside),
            (
                Self::Outside
                | Self::AfterSymbol(_)
                | Self::AfterModifierBase(_)
                | Self::Open
                | Self::Closed
                | Self::HalfFlag,
                Glyph::Component,
            ) => Step::Passes(Self::AfterSymbol(here)),
            (
                Self::Outside
                | Self::AfterSymbol(_)
                | Self::AfterModifierBase(_)
                | Self::Open
                | Self::Closed
                | Self::HalfFlag,
                Glyph::ModifierBase,
            ) => Step::Passes(Self::AfterModifierBase(here)),
            (Self::Open | Self::Closed | Self::Joined | Self::HalfFlag, Glyph::Joiner) => {
                Step::Passes(Self::Joined)
            }
            // A joined modifier base takes a skin tone like one standing alone.
            (Self::Joined, Glyph::ModifierBase | Glyph::PresentationModifierBase) => {
                Step::Passes(Self::Open)
            }
            // After a joiner any emoji character, a skin tone included, is joined on.
            (
                Self::Joined,
                Glyph::RegionalIndicator
                | Glyph::Modifier
                | Glyph::PresentationSelector
                | Glyph::Presentation
                | Glyph::Component,
            )
            | (Self::HalfFlag, Glyph::RegionalIndicator)
            | (Self::Open | Self::Closed | Self::HalfFlag, Glyph::PresentationSelector)
            | (Self::Open, Glyph::Modifier) => Step::Passes(Self::Closed),
            (Self::AfterSymbol(symbol), Glyph::PresentationSelector)
            | (Self::AfterModifierBase(symbol), Glyph::PresentationSelector | Glyph::Modifier) => {
                Step::Starts(symbol, Self::Closed)
            }
            (
                Self::Outside
                | Self::AfterSymbol(_)
                | Self::AfterModifierBase(_)
                | Self::Open
                | Self::Closed,
                Glyph::RegionalIndicator,
            ) => Step::Starts(here, Self::HalfFlag),
            // A skin tone not right after a modifier base shows as a swatch of its own.
            (Self::Outside | Self::AfterSymbol(_) | Self::Closed | Self::HalfFlag, Glyph::Modifier)
            | (
                Self::Outside
                | Self::AfterSymbol(_)
                | Self::AfterModifierBase(_)
                | Self::Open
                | Self::Closed
                | Self::HalfFlag,
                Glyph::Presentation,
            ) => Step::Starts(here, Self::Closed),
            (
                Self::Outside
                | Self::AfterSymbol(_)
                | Self::AfterModifierBase(_)
                | Self::Open
                | Self::Closed
                | Self::HalfFlag,
                Glyph::PresentationModifierBase,
            ) => Step::Starts(here, Self::Open),
        }
    }
}

/// - The byte offset and character starting each visible emoji: one that renders as emoji
///   on its own, a text symbol a VS16 turns into one, or a text modifier base a skin tone
///   turns into one.
/// - Characters continuing it (VS16, joined emoji, a flag's second regional indicator, a
///   skin tone right after a modifier base) are part of the same emoji, so one emoji is one
///   finding.
/// - A skin tone anywhere else, an already toned emoji included, is a swatch of its own.
/// - An emoji a VS15 right after it turns to text is no emoji.
pub fn starts(text: &str) -> impl Iterator<Item = Start> {
    let followers = text.chars().skip(1).map(Some).chain(std::iter::once(None));
    text.char_indices()
        .zip(followers)
        .scan(Sequence::Outside, |sequence, (here, following)| {
            let (next, start) = match sequence.step(Glyph::of(here.1, following), here) {
                Step::Starts(start, next) => (next, Some(start)),
                Step::Passes(next) => (next, None),
            };
            *sequence = next;
            Some(start)
        })
        .flatten()
}

#[cfg(test)]
mod tests;
