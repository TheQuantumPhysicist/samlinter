use ra_ap_syntax::{AstNode, Edition, SyntaxError, SyntaxNode, WalkEvent, ast};

/// - Tree depth up to which `ra_ap_syntax`'s own drop thread frees a tree safely.
/// - That thread has a default 2 MiB stack and frees recursively; it overflowed near 10000
///   levels (5000 nested inline modules), so this keeps a tenfold margin.
const DROP_THREAD_DEPTH: usize = 1000;

/// How deep a tree is, against what the parser's drop thread frees safely.
#[derive(Debug, PartialEq, Eq)]
enum Depth {
    Shallow,
    Deep,
}

impl Depth {
    /// Walked iteratively, so the measure itself does not recurse.
    fn of(root: &SyntaxNode) -> Self {
        let (_, deepest) = root
            .preorder()
            .fold((0_usize, 0_usize), |(depth, deepest), event| match event {
                WalkEvent::Enter(_) => {
                    let entered = depth.saturating_add(1);
                    (entered, deepest.max(entered))
                }
                WalkEvent::Leave(_) => (depth.saturating_sub(1), deepest),
            });
        if deepest > DROP_THREAD_DEPTH {
            Self::Deep
        } else {
            Self::Shallow
        }
    }
}

/// - `text` parsed as a Rust file: its tree and its syntax errors.
/// - `ra_ap_syntax` hands a dropped parse to its own thread, whose 2 MiB stack overflows freeing
///   a deep tree and aborts the process.
/// - So a deep tree's parse is never dropped: it stays in memory until samlinter exits. A
///   shallow one, every ordinary file, is dropped and freed as usual.
pub fn parse(text: &str, edition: Edition) -> (ast::SourceFile, Vec<SyntaxError>) {
    let parse = ast::SourceFile::parse(text, edition);
    let parsed = (parse.tree(), parse.errors());
    match Depth::of(parsed.0.syntax()) {
        Depth::Shallow => drop(parse),
        Depth::Deep => std::mem::forget(parse),
    }
    parsed
}

#[cfg(test)]
mod tests;
