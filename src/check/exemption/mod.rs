/// What one finding becomes.
enum Credit<F> {
    /// No exemption covers it.
    Kept(F),
    /// Dropped, credited to the exemption at this index into the valid ones.
    CreditedTo(usize),
}

impl<F> Credit<F> {
    const fn goes_to(&self, exemption: usize) -> bool {
        match self {
            Self::Kept(_) => false,
            Self::CreditedTo(index) => *index == exemption,
        }
    }
}

/// - Credits each finding to the narrowest valid exemption covering it and drops the finding,
///   so an exemption that only repeats a narrower one counts as unfulfilled.
/// - Of equally narrow exemptions, the first listed gets the credit.
/// - Returns the findings no exemption covers, then a finding for each valid exemption credited
///   with none, then each malformed one, each group in input order.
pub fn apply_exemptions<F, E, W: Ord>(
    findings: Vec<F>,
    exemptions: Vec<Result<E, F>>,
    covers: impl Fn(&E, &F) -> bool,
    width: impl Fn(&E) -> W,
    unfulfilled: impl Fn(&E) -> F,
) -> Vec<F> {
    let valid: Vec<&E> = exemptions
        .iter()
        .filter_map(|result| result.as_ref().ok())
        .collect();

    let credits: Vec<Credit<F>> = findings
        .into_iter()
        .map(|finding| {
            let narrowest = valid
                .iter()
                .enumerate()
                .filter(|(_, exemption)| covers(exemption, &finding))
                .min_by_key(|(_, exemption)| width(exemption))
                .map(|(index, _)| index);
            match narrowest {
                None => Credit::Kept(finding),
                Some(index) => Credit::CreditedTo(index),
            }
        })
        .collect();

    let unmatched: Vec<F> = valid
        .iter()
        .enumerate()
        .filter(|(index, _)| !credits.iter().any(|credit| credit.goes_to(*index)))
        .map(|(_, exemption)| unfulfilled(exemption))
        .collect();
    let kept = credits.into_iter().filter_map(|credit| match credit {
        Credit::Kept(finding) => Some(finding),
        Credit::CreditedTo(_) => None,
    });
    let malformed = exemptions.into_iter().filter_map(Result::err);
    kept.chain(unmatched).chain(malformed).collect()
}

#[cfg(test)]
mod tests;
