use crate::finding::{Hit, Rule};
use crate::syntax::shown_path;
use ra_ap_syntax::{AstNode, Direction, NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken, T, ast};

const MATCHES_MESSAGE: &str = "`matches!` absorbs every variant the expression does not name";

const ENUM_IS_MESSAGE: &str = "`EnumIs` generates `.is_variant()` checks that absorb new variants";

/// The clock types a direct `now()` read is flagged on, by the path's final segments.
const CLOCKS: [&str; 4] = ["SystemTime", "Instant", "Utc", "Local"];

/// A regex constructor call, by the path's final segments.
const REGEX_CONSTRUCTORS: [&str; 2] = ["Regex", "RegexBuilder"];

/// The RNG types a direct construction is flagged on, wherever the name appears in a path.
const RNG_TYPES: [&str; 4] = ["OsRng", "StdRng", "SmallRng", "ChaChaRng"];

/// The RNG free functions flagged as calls, at the name.
const RNG_FUNCTIONS: [&str; 4] = ["thread_rng", "random", "from_entropy", "seed_from_u64"];

/// The RNG setup methods flagged by name.
const RNG_METHODS: [&str; 2] = ["from_entropy", "seed_from_u64"];

/// - The banned-symbol tripwires, one finding per symbol use, at the token that names it:
///   the `matches!` macro (`matches_macro`), `EnumIs` in a derive list (`enum_is`), a direct
///   clock read (`system_time_now`), the regex crate (`regex_use`) and direct RNG
///   construction (`direct_rng`).
/// - One pass over the tree; matched by name with no type information, macro bodies unseen.
pub fn check(root: &SyntaxNode) -> Vec<Hit> {
    root.descendants()
        .filter_map(|node| {
            macro_matches(&node)
                .or_else(|| enum_is(&node))
                .or_else(|| path_hit(&node))
                .or_else(|| rng_method(&node))
        })
        .collect()
}

/// The `matches!` macro by any route (`matches!`, `core::matches!`, `std::matches!`), in any
/// position a macro call sits, at the name.
fn macro_matches(node: &SyntaxNode) -> Option<Hit> {
    let call = ast::MacroCall::cast(node.clone())?;
    let name = call.path()?.segments().last()?.name_ref()?;
    (name.text() == "matches").then(|| {
        Hit::new(
            usize::from(name.syntax().text_range().start()),
            Rule::MatchesMacro,
            MATCHES_MESSAGE,
        )
    })
}

/// `EnumIs` among the comma-separated derives of a `#[derive(..)]` list, at the `EnumIs`
/// token. A derive list is a token tree the `derive` name opens, so one nested in another
/// attribute's token tree, as in `cfg_attr(.., derive(..))`, is found the same way as a
/// direct `#[derive(..)]`.
fn enum_is(node: &SyntaxNode) -> Option<Hit> {
    let attr = ast::Attr::cast(node.clone())?;
    let name = attr
        .syntax()
        .descendants()
        .filter_map(ast::TokenTree::cast)
        .filter(|tree| opens_after_derive(tree.syntax()))
        .flat_map(|tree| derive_names(&tree).into_iter())
        .find(|name| name.text() == "EnumIs")?;
    Some(Hit::new(
        usize::from(name.text_range().start()),
        Rule::EnumIs,
        ENUM_IS_MESSAGE,
    ))
}

/// Whether the text right before this node, trivia skipped, is the bare `derive` name, so
/// the node opens a derive list. The direct `#[derive(..)]` carries the name as its path,
/// and one nested in a `cfg_attr(..)` token tree as a plain token; both sit right before
/// the list, and any other name or group before it means the tree is no derive list.
fn opens_after_derive(tree: &SyntaxNode) -> bool {
    tree.siblings_with_tokens(Direction::Prev)
        .skip(1)
        .find_map(|element| match element {
            NodeOrToken::Node(node) => Some(node.text() == "derive"),
            NodeOrToken::Token(token) if !token.kind().is_trivia() => {
                Some(token.text() == "derive")
            }
            NodeOrToken::Token(_) => None,
        })
        .unwrap_or(false)
}

/// Each comma-separated derive name, at the token that ends it. A piece may end at the
/// closing paren, whose text names no derive.
fn derive_names(tree: &ast::TokenTree) -> Vec<SyntaxToken> {
    let tokens: Vec<SyntaxToken> = tree
        .syntax()
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| !token.kind().is_trivia())
        .collect();
    tokens
        .split_inclusive(|token| token.kind() == T![,])
        .filter_map(|piece| {
            piece
                .iter()
                .rev()
                .find(|token| token.kind() != T![,] && token.kind() != T![')'])
                .cloned()
        })
        .collect()
}

/// The five path shapes, at most one hit per path, at the segment that names the symbol:
/// the regex crate by its leading segment, a regex constructor by its final segments, a
/// direct clock read by its final segments as a call, and direct RNG by type name or free
/// function.
fn path_hit(node: &SyntaxNode) -> Option<Hit> {
    let path = ast::Path::cast(node.clone())?;
    if !outermost(&path) {
        return None;
    }
    regex_crate(&path)
        .or_else(|| regex_constructor(&path))
        .or_else(|| clock(&path))
        .or_else(|| rng_type(&path))
        .or_else(|| rng_free(&path))
}

/// Whether the path is not a qualifier inside a longer one, which reports for it.
fn outermost(path: &ast::Path) -> bool {
    path.syntax()
        .parent()
        .is_none_or(|parent| parent.kind() != SyntaxKind::PATH)
}

/// Whether the path is a call's callee, so `random` counts only with parentheses or a
/// turbofish before them.
fn called(path: &ast::Path) -> bool {
    path.syntax()
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::PATH_EXPR)
        .is_some_and(|callee| {
            callee
                .parent()
                .is_some_and(|call| call.kind() == SyntaxKind::CALL_EXPR)
        })
}

/// The regex crate by any route, in any position: a use tree (`use regex::..`), a type, an
/// expression, at the leading segment.
fn regex_crate(path: &ast::Path) -> Option<Hit> {
    let name = path.first_segment()?.name_ref()?;
    (name.text() == "regex").then(|| {
        Hit::new(
            usize::from(name.syntax().text_range().start()),
            Rule::RegexUse,
            format!("regex crate use: `{}`", shown_path(path)),
        )
    })
}

/// A `Regex::new` / `RegexBuilder::new` constructor by any route, at the `new`; a path whose
/// leading segment is already `regex` is reported by `regex_crate` instead.
fn regex_constructor(path: &ast::Path) -> Option<Hit> {
    let names: Vec<ast::NameRef> = path
        .segments()
        .filter_map(|segment| segment.name_ref())
        .collect();
    match names.as_slice() {
        [.., kind, constructor]
            if REGEX_CONSTRUCTORS.contains(&kind.text()) && constructor.text() == "new" =>
        {
            Some(Hit::new(
                usize::from(constructor.syntax().text_range().start()),
                Rule::RegexUse,
                format!("regex crate use: `{}()`", shown_path(path)),
            ))
        }
        _ => None,
    }
}

/// `SystemTime::now`, `Instant::now` (std or tokio), `Utc::now` or `Local::now`, by any
/// route, as a call, at the `now`.
fn clock(path: &ast::Path) -> Option<Hit> {
    let names: Vec<ast::NameRef> = path
        .segments()
        .filter_map(|segment| segment.name_ref())
        .collect();
    match names.as_slice() {
        [.., receiver, now]
            if now.text() == "now" && CLOCKS.contains(&receiver.text()) && called(path) =>
        {
            Some(Hit::new(
                usize::from(now.syntax().text_range().start()),
                Rule::SystemTimeNow,
                format!("direct clock read: `{}()`", shown_path(path)),
            ))
        }
        _ => None,
    }
}

/// Construction of one of the direct RNG types, wherever the name appears in a path: a bare
/// value, a type annotation, a use tree, or a constructor called on it, at the type name.
fn rng_type(path: &ast::Path) -> Option<Hit> {
    let name = path
        .segments()
        .filter_map(|segment| segment.name_ref())
        .find(|name| RNG_TYPES.contains(&name.text()))?;
    Some(Hit::new(
        usize::from(name.syntax().text_range().start()),
        Rule::DirectRng,
        format!("direct RNG: `{}`", shown_path(path)),
    ))
}

/// A direct RNG free function as a call, by any route: `thread_rng()`, `rand::random()` or
/// bare `random()` (with parentheses or a turbofish), `from_entropy`, `seed_from_u64`, at
/// the name.
fn rng_free(path: &ast::Path) -> Option<Hit> {
    let name = path.segments().last()?.name_ref()?;
    (RNG_FUNCTIONS.contains(&name.text()) && called(path)).then(|| {
        Hit::new(
            usize::from(name.syntax().text_range().start()),
            Rule::DirectRng,
            format!("direct RNG: `{}()`", shown_path(path)),
        )
    })
}

/// Direct RNG setup by method-call name, `rng.seed_from_u64(..)` and the like, at the name.
fn rng_method(node: &SyntaxNode) -> Option<Hit> {
    let name = ast::MethodCallExpr::cast(node.clone())?.name_ref()?;
    RNG_METHODS.contains(&name.text()).then(|| {
        Hit::new(
            usize::from(name.syntax().text_range().start()),
            Rule::DirectRng,
            format!("direct RNG: `.{}()`", name.text()),
        )
    })
}

#[cfg(test)]
mod tests;
