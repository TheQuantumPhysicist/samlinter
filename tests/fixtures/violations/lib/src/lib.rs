#[cfg_attr(samlinter, expect(syntax_error, reason = "fixture: syntax_error cannot be exempted"))]
pub fn answer() -> u8 {
    42
}

/// Installs logging — fixture: the exemption below covers this doc comment.
#[cfg_attr(samlinter, expect(em_dash, reason = "fixture: a fulfilled comment-rule exemption"))]
pub fn install() {
    tracing_subscriber::registry().init();
    let _ = format!("fixture");
}

#[cfg(test)]
mod support {
    pub fn quiet() {
        tracing_subscriber::fmt().init();
    }
}

#[cfg(test)]
mod tests;
