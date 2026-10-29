use super::{Outcome, run_on_stack};
use anyhow::anyhow;
use rstest::rstest;
use std::num::NonZeroUsize;

/// Enough for a test closure; the size is not under test.
const SMALL_STACK: usize = 1024 * 1024;

#[rstest]
#[case::clean(Outcome::Clean, Outcome::Clean)]
#[case::findings(
    Outcome::Findings(NonZeroUsize::new(3).unwrap()),
    Outcome::Findings(NonZeroUsize::new(3).unwrap())
)]
fn outcome_of_the_work_passes_through(#[case] outcome: Outcome, #[case] expected: Outcome) {
    assert_eq!(run_on_stack(SMALL_STACK, || Ok(outcome)).unwrap(), expected);
}

#[test]
fn error_of_the_work_passes_through() {
    let error = run_on_stack(SMALL_STACK, || Err(anyhow!("cannot read x"))).unwrap_err();
    assert_eq!(error.to_string(), "cannot read x");
}

/// No platform can reserve a stack the size of the address space.
#[test]
fn thread_that_cannot_start_is_an_error() {
    let error = run_on_stack(usize::MAX, || Ok(Outcome::Clean)).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "cannot start the lint thread with a {}-byte stack",
            usize::MAX
        )
    );
}

/// Test builds unwind, so the panic reaches `join`; the dev and release profiles abort first.
#[test]
fn thread_that_panics_is_an_error() {
    let error = run_on_stack(SMALL_STACK, || panic!("broken invariant")).unwrap_err();
    assert_eq!(error.to_string(), "the lint thread panicked");
}
