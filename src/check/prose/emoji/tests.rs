// - TODO: randomized variant: random ASCII filler with random emoji sequences (single,
//   skin-toned, flags, joined, VS16-forced) inserted at random points: exactly one start
//   per inserted sequence, at its first character: the text symbol when a VS16 forces
//   one, the modifier base when a skin tone does; an emoji a VS15 turns to text gives
//   none.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::{Glyph, Sequence, Start, Step, starts};
use rstest::rstest;

/// The character a pending state waits on.
const BEFORE: Start = (0, '\u{261D}');
/// The character being stepped over.
const HERE: Start = (3, '\u{1F600}');

#[rstest]
#[case::emoji("ok \u{1F600}", &["3 U+1F600"])]
#[case::modifier_base("\u{1F44D}", &["0 U+1F44D"])]
#[case::text_symbols_are_not_emoji("\u{a9} \u{2122} \u{2192} \u{2713}", &[])]
#[case::variation_selector_forces_emoji("\u{2714}\u{fe0f}", &["0 U+2714"])]
#[case::keycap("1\u{fe0f}\u{20e3}", &["0 U+0031"])]
#[case::skin_tone_on_a_text_modifier_base("a \u{261D}\u{1F3FB}", &["2 U+261D"])]
#[case::selector_on_a_text_modifier_base("\u{261D}\u{fe0f}", &["0 U+261D"])]
#[case::skin_tone_on_a_symbol_it_cannot_tone("\u{a9}\u{1F3FB}", &["2 U+1F3FB"])]
#[case::skin_tone_on_a_digit("1\u{1F3FB}", &["1 U+1F3FB"])]
#[case::second_modifier_base_takes_the_skin_tone(
    "\u{261D}\u{261D}\u{1F3FB}",
    &["3 U+261D"]
)]
#[case::text_symbol_after_a_modifier_base("\u{261D}\u{a9}\u{fe0f}", &["3 U+00A9"])]
#[case::joiner_after_a_modifier_base("\u{261D}\u{200d}\u{1F600}", &["6 U+1F600"])]
#[case::text_between_a_modifier_base_and_a_skin_tone("\u{261D}a\u{1F3FB}", &["4 U+1F3FB"])]
#[case::emoji_after_a_modifier_base("\u{261D}\u{1F600}", &["3 U+1F600"])]
#[case::flag_after_a_modifier_base("\u{261D}\u{1F1FA}\u{1F1F8}", &["3 U+1F1FA"])]
#[case::modifier_base_after_a_text_symbol("\u{a9}\u{261D}\u{1F3FB}", &["2 U+261D"])]
#[case::modifier_base_after_an_emoji(
    "\u{1F600}\u{261D}\u{1F3FB}",
    &["0 U+1F600", "4 U+261D"]
)]
#[case::modifier_base_after_a_half_flag(
    "\u{1F1FA}\u{261D}\u{1F3FB}",
    &["0 U+1F1FA", "4 U+261D"]
)]
#[case::skin_tone_on_a_joined_text_modifier_base(
    "\u{1F600}\u{200d}\u{261D}\u{1F3FB}",
    &["0 U+1F600"]
)]
#[case::skin_tone_on_a_joined_emoji_modifier_base(
    "\u{1F600}\u{200d}\u{1F44D}\u{1F3FD}",
    &["0 U+1F600"]
)]
#[case::skin_tone_after_a_selector_on_a_modifier_base(
    "\u{261D}\u{fe0f}\u{1F3FB}",
    &["0 U+261D", "6 U+1F3FB"]
)]
#[case::text_symbol_then_text("\u{2714}a\u{fe0f}", &[])]
#[case::stray_selector_after_text("a\u{fe0f}", &[])]
#[case::stray_selector_alone("\u{fe0f}", &[])]
#[case::stray_selector_after_a_joiner("a\u{200d}\u{fe0f}", &[])]
#[case::digit_then_emoji("1\u{1F600}", &["1 U+1F600"])]
#[case::text_symbol_then_emoji("\u{a9}\u{1F600}", &["2 U+1F600"])]
#[case::text_symbol_then_flag("\u{a9}\u{1F1FA}\u{1F1F8}", &["2 U+1F1FA"])]
#[case::joiner_after_a_text_symbol("\u{a9}\u{200d}\u{1F600}", &["5 U+1F600"])]
#[case::second_text_symbol_takes_the_selector("\u{a9}\u{2714}\u{fe0f}", &["2 U+2714"])]
#[case::flag("\u{1F1FA}\u{1F1F8}", &["0 U+1F1FA"])]
#[case::two_flags(
    "\u{1F1FA}\u{1F1F8}\u{1F1EB}\u{1F1F7}",
    &["0 U+1F1FA", "8 U+1F1EB"]
)]
#[case::unpaired_regional_indicator_after_a_flag(
    "\u{1F1FA}\u{1F1F8}\u{1F1EB}",
    &["0 U+1F1FA", "8 U+1F1EB"]
)]
#[case::lone_skin_tone_modifier("\u{1F3FB}", &["0 U+1F3FB"])]
#[case::skin_tone_after_an_emoji_it_cannot_tone(
    "\u{1F600}\u{1F3FB}",
    &["0 U+1F600", "4 U+1F3FB"]
)]
#[case::skin_tone_after_a_flag(
    "\u{1F1FA}\u{1F1F8}\u{1F3FB}",
    &["0 U+1F1FA", "8 U+1F3FB"]
)]
#[case::second_skin_tone_on_a_toned_emoji(
    "\u{1F44D}\u{1F3FD}\u{1F3FD}",
    &["0 U+1F44D", "8 U+1F3FD"]
)]
#[case::skin_tone_on_its_base("a \u{1F44D}\u{1F3FD} b", &["2 U+1F44D"])]
#[case::presentation_selector_on_an_emoji("\u{1F600}\u{fe0f}", &["0 U+1F600"])]
#[case::joined_sequence(
    "\u{1F468}\u{200d}\u{1F469}\u{200d}\u{1F467}",
    &["0 U+1F468"]
)]
#[case::joined_sequence_from_a_text_symbol(
    "\u{2764}\u{fe0f}\u{200d}\u{1F525}",
    &["0 U+2764"]
)]
#[case::joined_text_symbols("\u{1F3F3}\u{fe0f}\u{200d}\u{26A7}\u{fe0f}", &["0 U+1F3F3"])]
#[case::adjacent_emoji("\u{1F600}\u{1F600}", &["0 U+1F600", "4 U+1F600"])]
#[case::joiner_after_text("a\u{200d}\u{1F600}", &["4 U+1F600"])]
#[case::text_symbol_after_an_emoji(
    "\u{1F600}\u{2714}\u{fe0f}",
    &["0 U+1F600", "4 U+2714"]
)]
#[case::emoji_after_a_half_flag("\u{1F1FA}\u{1F600}", &["0 U+1F1FA", "4 U+1F600"])]
#[case::skin_tone_after_a_half_flag("\u{1F1FA}\u{1F3FB}", &["0 U+1F1FA", "4 U+1F3FB"])]
#[case::text_between_regional_indicators(
    "\u{1F1FA}a\u{1F1F8}",
    &["0 U+1F1FA", "5 U+1F1F8"]
)]
#[case::text_symbol_between_regional_indicators(
    "\u{1F1FA}\u{a9}\u{1F1F8}",
    &["0 U+1F1FA", "6 U+1F1F8"]
)]
#[case::selector_between_regional_indicators(
    "\u{1F1FA}\u{fe0f}\u{1F1F8}",
    &["0 U+1F1FA", "7 U+1F1F8"]
)]
#[case::emoji_joined_to_a_half_flag("\u{1F1FA}\u{200d}\u{1F600}", &["0 U+1F1FA"])]
#[case::two_joiners("\u{1F600}\u{200d}\u{200d}\u{1F600}", &["0 U+1F600"])]
#[case::regional_indicator_joined("\u{1F600}\u{200d}\u{1F1FA}", &["0 U+1F600"])]
#[case::skin_tone_joined("\u{1F600}\u{200d}\u{1F3FB}", &["0 U+1F600"])]
#[case::selector_joined("\u{1F600}\u{200d}\u{fe0f}", &["0 U+1F600"])]
#[case::text_after_a_joiner("\u{1F600}\u{200d}a\u{1F600}", &["0 U+1F600", "8 U+1F600"])]
#[case::hair_component("\u{1F9B0}", &["0 U+1F9B0"])]
#[case::tag_sequence_flag(
    "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
    &["0 U+1F3F4"]
)]
#[case::text_presentation_selector_on_an_emoji("\u{2614}\u{fe0e}", &[])]
#[case::text_presentation_selector_on_an_emoji_modifier_base("\u{1F44D}\u{fe0e}", &[])]
#[case::text_presentation_selector_on_a_text_symbol("\u{2714}\u{fe0e}", &[])]
#[case::text_presentation_selector_after_text("a\u{fe0e}", &[])]
#[case::text_presentation_selector_changes_only_the_character_before_it(
    "\u{2614}\u{2614}\u{fe0e}",
    &["0 U+2614"]
)]
#[case::text_presentation_selector_with_text_between("\u{2614}a\u{fe0e}", &["0 U+2614"])]
#[case::emoji_after_a_text_form("\u{2614}\u{fe0e}\u{1F600}", &["6 U+1F600"])]
#[case::skin_tone_after_a_text_form_modifier_base(
    "\u{1F44D}\u{fe0e}\u{1F3FD}",
    &["7 U+1F3FD"]
)]
#[case::text_form_joined_on("\u{1F600}\u{200d}\u{2614}\u{fe0e}", &["0 U+1F600"])]
#[case::text_presentation_selector_on_a_skin_tone("\u{1F3FB}\u{fe0e}", &["0 U+1F3FB"])]
#[case::text_presentation_selector_on_a_regional_indicator(
    "\u{1F1FA}\u{fe0e}",
    &["0 U+1F1FA"]
)]
#[case::empty_text("", &[])]
fn one_start_per_visible_emoji(#[case] text: &str, #[case] expected: &[&str]) {
    let rendered: Vec<String> = starts(text)
        .map(|(index, character)| format!("{index} U+{:04X}", u32::from(character)))
        .collect();
    assert_eq!(rendered, expected);
}

#[rstest]
#[case::outside_regional_indicator(
    Sequence::Outside,
    Glyph::RegionalIndicator,
    Step::Starts(HERE, Sequence::HalfFlag)
)]
#[case::outside_modifier(
    Sequence::Outside,
    Glyph::Modifier,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::outside_presentation_selector(
    Sequence::Outside,
    Glyph::PresentationSelector,
    Step::Passes(Sequence::Outside)
)]
#[case::outside_joiner(Sequence::Outside, Glyph::Joiner, Step::Passes(Sequence::Outside))]
#[case::outside_presentation(
    Sequence::Outside,
    Glyph::Presentation,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::outside_presentation_modifier_base(
    Sequence::Outside,
    Glyph::PresentationModifierBase,
    Step::Starts(HERE, Sequence::Open)
)]
#[case::outside_modifier_base(
    Sequence::Outside,
    Glyph::ModifierBase,
    Step::Passes(Sequence::AfterModifierBase(HERE))
)]
#[case::outside_component(
    Sequence::Outside,
    Glyph::Component,
    Step::Passes(Sequence::AfterSymbol(HERE))
)]
#[case::outside_text_presentation(
    Sequence::Outside,
    Glyph::TextPresentation,
    Step::Passes(Sequence::Outside)
)]
#[case::outside_text(Sequence::Outside, Glyph::Text, Step::Passes(Sequence::Outside))]
#[case::after_symbol_regional_indicator(
    Sequence::AfterSymbol(BEFORE),
    Glyph::RegionalIndicator,
    Step::Starts(HERE, Sequence::HalfFlag)
)]
#[case::after_symbol_modifier(
    Sequence::AfterSymbol(BEFORE),
    Glyph::Modifier,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::after_symbol_presentation_selector(
    Sequence::AfterSymbol(BEFORE),
    Glyph::PresentationSelector,
    Step::Starts(BEFORE, Sequence::Closed)
)]
#[case::after_symbol_joiner(
    Sequence::AfterSymbol(BEFORE),
    Glyph::Joiner,
    Step::Passes(Sequence::Outside)
)]
#[case::after_symbol_presentation(
    Sequence::AfterSymbol(BEFORE),
    Glyph::Presentation,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::after_symbol_presentation_modifier_base(
    Sequence::AfterSymbol(BEFORE),
    Glyph::PresentationModifierBase,
    Step::Starts(HERE, Sequence::Open)
)]
#[case::after_symbol_modifier_base(
    Sequence::AfterSymbol(BEFORE),
    Glyph::ModifierBase,
    Step::Passes(Sequence::AfterModifierBase(HERE))
)]
#[case::after_symbol_component(
    Sequence::AfterSymbol(BEFORE),
    Glyph::Component,
    Step::Passes(Sequence::AfterSymbol(HERE))
)]
#[case::after_symbol_text_presentation(
    Sequence::AfterSymbol(BEFORE),
    Glyph::TextPresentation,
    Step::Passes(Sequence::Outside)
)]
#[case::after_symbol_text(
    Sequence::AfterSymbol(BEFORE),
    Glyph::Text,
    Step::Passes(Sequence::Outside)
)]
#[case::after_modifier_base_regional_indicator(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::RegionalIndicator,
    Step::Starts(HERE, Sequence::HalfFlag)
)]
#[case::after_modifier_base_modifier(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::Modifier,
    Step::Starts(BEFORE, Sequence::Closed)
)]
#[case::after_modifier_base_presentation_selector(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::PresentationSelector,
    Step::Starts(BEFORE, Sequence::Closed)
)]
#[case::after_modifier_base_joiner(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::Joiner,
    Step::Passes(Sequence::Outside)
)]
#[case::after_modifier_base_presentation(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::Presentation,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::after_modifier_base_presentation_modifier_base(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::PresentationModifierBase,
    Step::Starts(HERE, Sequence::Open)
)]
#[case::after_modifier_base_modifier_base(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::ModifierBase,
    Step::Passes(Sequence::AfterModifierBase(HERE))
)]
#[case::after_modifier_base_component(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::Component,
    Step::Passes(Sequence::AfterSymbol(HERE))
)]
#[case::after_modifier_base_text_presentation(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::TextPresentation,
    Step::Passes(Sequence::Outside)
)]
#[case::after_modifier_base_text(
    Sequence::AfterModifierBase(BEFORE),
    Glyph::Text,
    Step::Passes(Sequence::Outside)
)]
#[case::open_regional_indicator(
    Sequence::Open,
    Glyph::RegionalIndicator,
    Step::Starts(HERE, Sequence::HalfFlag)
)]
#[case::open_modifier(Sequence::Open, Glyph::Modifier, Step::Passes(Sequence::Closed))]
#[case::open_presentation_selector(
    Sequence::Open,
    Glyph::PresentationSelector,
    Step::Passes(Sequence::Closed)
)]
#[case::open_joiner(Sequence::Open, Glyph::Joiner, Step::Passes(Sequence::Joined))]
#[case::open_presentation(
    Sequence::Open,
    Glyph::Presentation,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::open_presentation_modifier_base(
    Sequence::Open,
    Glyph::PresentationModifierBase,
    Step::Starts(HERE, Sequence::Open)
)]
#[case::open_modifier_base(
    Sequence::Open,
    Glyph::ModifierBase,
    Step::Passes(Sequence::AfterModifierBase(HERE))
)]
#[case::open_component(
    Sequence::Open,
    Glyph::Component,
    Step::Passes(Sequence::AfterSymbol(HERE))
)]
#[case::open_text_presentation(
    Sequence::Open,
    Glyph::TextPresentation,
    Step::Passes(Sequence::Outside)
)]
#[case::open_text(Sequence::Open, Glyph::Text, Step::Passes(Sequence::Outside))]
#[case::closed_regional_indicator(
    Sequence::Closed,
    Glyph::RegionalIndicator,
    Step::Starts(HERE, Sequence::HalfFlag)
)]
#[case::closed_modifier(
    Sequence::Closed,
    Glyph::Modifier,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::closed_presentation_selector(
    Sequence::Closed,
    Glyph::PresentationSelector,
    Step::Passes(Sequence::Closed)
)]
#[case::closed_joiner(Sequence::Closed, Glyph::Joiner, Step::Passes(Sequence::Joined))]
#[case::closed_presentation(
    Sequence::Closed,
    Glyph::Presentation,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::closed_presentation_modifier_base(
    Sequence::Closed,
    Glyph::PresentationModifierBase,
    Step::Starts(HERE, Sequence::Open)
)]
#[case::closed_modifier_base(
    Sequence::Closed,
    Glyph::ModifierBase,
    Step::Passes(Sequence::AfterModifierBase(HERE))
)]
#[case::closed_component(
    Sequence::Closed,
    Glyph::Component,
    Step::Passes(Sequence::AfterSymbol(HERE))
)]
#[case::closed_text_presentation(
    Sequence::Closed,
    Glyph::TextPresentation,
    Step::Passes(Sequence::Outside)
)]
#[case::closed_text(Sequence::Closed, Glyph::Text, Step::Passes(Sequence::Outside))]
#[case::joined_regional_indicator(
    Sequence::Joined,
    Glyph::RegionalIndicator,
    Step::Passes(Sequence::Closed)
)]
#[case::joined_modifier(Sequence::Joined, Glyph::Modifier, Step::Passes(Sequence::Closed))]
#[case::joined_presentation_selector(
    Sequence::Joined,
    Glyph::PresentationSelector,
    Step::Passes(Sequence::Closed)
)]
#[case::joined_joiner(Sequence::Joined, Glyph::Joiner, Step::Passes(Sequence::Joined))]
#[case::joined_presentation(Sequence::Joined, Glyph::Presentation, Step::Passes(Sequence::Closed))]
#[case::joined_presentation_modifier_base(
    Sequence::Joined,
    Glyph::PresentationModifierBase,
    Step::Passes(Sequence::Open)
)]
#[case::joined_modifier_base(Sequence::Joined, Glyph::ModifierBase, Step::Passes(Sequence::Open))]
#[case::joined_component(Sequence::Joined, Glyph::Component, Step::Passes(Sequence::Closed))]
#[case::joined_text_presentation(
    Sequence::Joined,
    Glyph::TextPresentation,
    Step::Passes(Sequence::Outside)
)]
#[case::joined_text(Sequence::Joined, Glyph::Text, Step::Passes(Sequence::Outside))]
#[case::half_flag_regional_indicator(
    Sequence::HalfFlag,
    Glyph::RegionalIndicator,
    Step::Passes(Sequence::Closed)
)]
#[case::half_flag_modifier(
    Sequence::HalfFlag,
    Glyph::Modifier,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::half_flag_presentation_selector(
    Sequence::HalfFlag,
    Glyph::PresentationSelector,
    Step::Passes(Sequence::Closed)
)]
#[case::half_flag_joiner(Sequence::HalfFlag, Glyph::Joiner, Step::Passes(Sequence::Joined))]
#[case::half_flag_presentation(
    Sequence::HalfFlag,
    Glyph::Presentation,
    Step::Starts(HERE, Sequence::Closed)
)]
#[case::half_flag_presentation_modifier_base(
    Sequence::HalfFlag,
    Glyph::PresentationModifierBase,
    Step::Starts(HERE, Sequence::Open)
)]
#[case::half_flag_modifier_base(
    Sequence::HalfFlag,
    Glyph::ModifierBase,
    Step::Passes(Sequence::AfterModifierBase(HERE))
)]
#[case::half_flag_component(
    Sequence::HalfFlag,
    Glyph::Component,
    Step::Passes(Sequence::AfterSymbol(HERE))
)]
#[case::half_flag_text_presentation(
    Sequence::HalfFlag,
    Glyph::TextPresentation,
    Step::Passes(Sequence::Outside)
)]
#[case::half_flag_text(Sequence::HalfFlag, Glyph::Text, Step::Passes(Sequence::Outside))]
fn each_state_steps_each_glyph(
    #[case] sequence: Sequence,
    #[case] glyph: Glyph,
    #[case] expected: Step,
) {
    assert_eq!(sequence.step(glyph, HERE), expected);
}
