// - TODO: randomized variant: random finding offsets and random nested exemption scopes: each
//   finding inside some scope is dropped and credited to the narrowest, every scope credited
//   with none comes back unfulfilled.
// - Postponed: test randomness must come from the `randomness` crate, which is not
//   available to this crate yet.
// - Plan: add it as a dev-dependency, then write this as an rstest `#[trace]` case over a
//   seeded ChaCha20 RNG with a fresh seed per run.
use super::apply_exemptions;
use rstest::rstest;
use std::ops::Range;

/// What comes out: a finding at an offset, or a finding about an exemption.
#[derive(Debug, PartialEq, Eq)]
enum Out {
    Finding(usize),
    Unfulfilled(Range<usize>),
    Malformed(&'static str),
}

fn covers(scope: &Range<usize>, out: &Out) -> bool {
    match out {
        Out::Finding(offset) => scope.contains(offset),
        Out::Unfulfilled(_) | Out::Malformed(_) => false,
    }
}

fn apply(findings: &[usize], exemptions: Vec<Result<Range<usize>, Out>>) -> Vec<Out> {
    apply_exemptions(
        findings.iter().copied().map(Out::Finding).collect(),
        exemptions,
        covers,
        ExactSizeIterator::len,
        |scope| Out::Unfulfilled(scope.clone()),
    )
}

#[rstest]
#[case::nothing_in_nothing_out(&[], vec![], vec![])]
#[case::exemptions_without_findings_are_unfulfilled(
    &[],
    vec![Ok(0..2), Ok(5..9)],
    vec![Out::Unfulfilled(0..2), Out::Unfulfilled(5..9)]
)]
#[case::uncovered_finding_is_kept(&[1], vec![Ok(5..9)], vec![Out::Finding(1), Out::Unfulfilled(5..9)])]
#[case::covered_finding_is_dropped(&[6], vec![Ok(5..9)], vec![])]
#[case::one_exemption_covers_several_findings(&[5, 6, 8], vec![Ok(5..9)], vec![])]
#[case::narrowest_gets_the_credit(&[6], vec![Ok(0..20), Ok(5..9)], vec![Out::Unfulfilled(0..20)])]
#[case::equal_width_credits_the_first_listed(
    &[6],
    vec![Ok(4..8), Ok(5..9)],
    vec![Out::Unfulfilled(5..9)]
)]
#[case::outer_keeps_a_finding_the_inner_misses(&[2, 6], vec![Ok(0..20), Ok(5..9)], vec![])]
#[case::credit_indexes_count_malformed_entries(
    &[11],
    vec![Err(Out::Malformed("a")), Ok(10..12)],
    vec![Out::Malformed("a")]
)]
#[case::kept_then_unfulfilled_then_malformed_each_in_input_order(
    &[30, 20],
    vec![Err(Out::Malformed("a")), Ok(10..12), Err(Out::Malformed("b")), Ok(0..2)],
    vec![
        Out::Finding(30),
        Out::Finding(20),
        Out::Unfulfilled(10..12),
        Out::Unfulfilled(0..2),
        Out::Malformed("a"),
        Out::Malformed("b"),
    ]
)]
fn credits_each_finding_to_the_narrowest_covering_exemption(
    #[case] findings: &[usize],
    #[case] exemptions: Vec<Result<Range<usize>, Out>>,
    #[case] expected: Vec<Out>,
) {
    assert_eq!(apply(findings, exemptions), expected);
}
