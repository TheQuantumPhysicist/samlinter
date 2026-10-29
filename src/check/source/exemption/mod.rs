use super::super::exemption::apply_exemptions;
use super::start;
use crate::finding::{Exemptability, Hit, Rule, SourceExemptable};
use crate::syntax::is_bare_cfg_atom;
use ra_ap_syntax::{AstNode, AstToken, NodeOrToken, SyntaxKind, SyntaxNode, SyntaxToken, T, ast};
use std::ops::Range;

const USAGE: &str =
    "expected exactly `expect(<rule>, reason = \"<why>\")` inside `cfg_attr(samlinter, ...)`";

/// - One rule named by a `#[cfg_attr(samlinter, expect(...))]` attribute.
/// - Its scope is the node's own text: an item or a field also holds its doc comments and the
///   plain comments directly above it, which the parser files under it; a `let`, match arm or
///   parameter does not. At the top of a file or an inline module the node is the file or the
///   module's item list, so the scope is the file's or the module body's text.
/// - A file's `//!` docs, a comment a blank line apart from the next item and comments after
///   the last item sit in no node's scope.
struct Exemption {
    rule: SourceExemptable,
    /// Byte range of the annotated node: item, field, match arm, statement or parameter.
    scope: Range<usize>,
    /// Start of the attribute, where findings about the exemption itself point.
    offset: usize,
}

impl Exemption {
    fn covers(&self, hit: &Hit) -> bool {
        hit.rule == Rule::from(self.rule) && self.scope.contains(&hit.offset)
    }

    /// Scopes nest, so of the exemptions covering a hit the narrowest is the innermost.
    fn width(&self) -> usize {
        self.scope.len()
    }

    fn unfulfilled(&self) -> Hit {
        Hit::new(
            self.offset,
            Rule::UnfulfilledExemption,
            format!(
                "exemption for `{}` matches nothing here",
                Rule::from(self.rule).name()
            ),
        )
    }
}

enum Argument {
    Rule(SourceExemptable),
    Reason,
}

impl Argument {
    fn rule(&self) -> Result<SourceExemptable, String> {
        match self {
            Self::Rule(rule) => Ok(*rule),
            Self::Reason => Err("`reason` must be the last argument".to_owned()),
        }
    }
}

/// - Drops hits covered by a `#[cfg_attr(samlinter, expect(...))]` on an enclosing node, each
///   credited to the innermost such exemption.
/// - Adds a hit for each malformed exemption and for each exempted rule credited with nothing.
pub fn apply(root: &SyntaxNode, hits: Vec<Hit>) -> Vec<Hit> {
    let exemptions = root
        .descendants()
        .filter_map(ast::Attr::cast)
        .filter_map(|attr| parse(&attr))
        .flatten()
        .collect();

    apply_exemptions(
        hits,
        exemptions,
        Exemption::covers,
        Exemption::width,
        Exemption::unfulfilled,
    )
}

/// - `None` when the attribute is not a samlinter exemption.
/// - Otherwise one exemption per rule it names, or the one hit saying why it is malformed.
fn parse(attr: &ast::Attr) -> Option<Vec<Result<Exemption, Hit>>> {
    let cfg_attr = attr.meta().and_then(cfg_attr_meta).filter(|meta| {
        meta.cfg_predicate()
            .is_some_and(|predicate| is_bare_cfg_atom(&predicate, "samlinter"))
    })?;

    // - Invariant: the parser places every attribute inside the node it is attached to.
    // - A parentless attribute is a parser bug, not bad input, so there is nothing to report.
    let range = attr
        .syntax()
        .parent()
        .expect("the parser nests every attribute in a node")
        .text_range();
    let scope = usize::from(range.start())..usize::from(range.end());
    let offset = start(attr.syntax());

    let exemptions = match placement(attr).and_then(|()| expected_rules(&cfg_attr)) {
        Ok(rules) => rules
            .into_iter()
            .map(|rule| {
                Ok(Exemption {
                    rule,
                    scope: scope.clone(),
                    offset,
                })
            })
            .collect(),
        Err(message) => vec![Err(Hit::new(offset, Rule::MalformedExemption, message))],
    };
    Some(exemptions)
}

fn cfg_attr_meta(meta: ast::Meta) -> Option<ast::CfgAttrMeta> {
    match meta {
        ast::Meta::CfgAttrMeta(cfg_attr) => Some(cfg_attr),
        ast::Meta::CfgMeta(_)
        | ast::Meta::KeyValueMeta(_)
        | ast::Meta::PathMeta(_)
        | ast::Meta::TokenTreeMeta(_)
        | ast::Meta::UnsafeMeta(_) => None,
    }
}

/// - An exemption is an outer attribute on the node it covers.
/// - An inner `#![...]` is accepted where the node it is attached to holds findings on
///   itself, where an inner `#![path]` can also sit: the top of a file, the body of an
///   inline module. In a function body it covers nothing, so it is malformed there.
fn placement(attr: &ast::Attr) -> Result<(), String> {
    let on_findings_node = attr.syntax().parent().is_some_and(|node| {
        let kind = node.kind();
        kind == SyntaxKind::SOURCE_FILE || kind == SyntaxKind::ITEM_LIST
    });
    match attr.kind() {
        ast::AttrKind::Outer => Ok(()),
        ast::AttrKind::Inner if on_findings_node => Ok(()),
        ast::AttrKind::Inner => Err("an exemption is an outer `#[...]` on the node it covers, or an inner `#![...]` at the top of a file or inline module; write it on the item, field, match arm, statement or parameter that holds the violation".to_owned()),
    }
}

fn expected_rules(cfg_attr: &ast::CfgAttrMeta) -> Result<Vec<SourceExemptable>, String> {
    let metas: Vec<ast::Meta> = cfg_attr.metas().collect();
    let [meta] = metas.as_slice() else {
        return Err(USAGE.to_owned());
    };
    let expect = token_tree_meta(meta)
        .filter(|meta| {
            meta.path()
                .is_some_and(|path| path.syntax().text() == "expect")
        })
        .ok_or(USAGE)?;
    let tree = expect.token_tree().ok_or(USAGE)?;
    rules_before_reason(&arguments(&tree)?)
}

/// `name(...)`, the shape of `expect(...)`.
const fn token_tree_meta(meta: &ast::Meta) -> Option<&ast::TokenTreeMeta> {
    match meta {
        ast::Meta::TokenTreeMeta(tree) => Some(tree),
        ast::Meta::CfgAttrMeta(_)
        | ast::Meta::CfgMeta(_)
        | ast::Meta::KeyValueMeta(_)
        | ast::Meta::PathMeta(_)
        | ast::Meta::UnsafeMeta(_) => None,
    }
}

fn rules_before_reason(arguments: &[Argument]) -> Result<Vec<SourceExemptable>, String> {
    match arguments.split_last() {
        Some((Argument::Reason, [])) => Err("name at least one rule before `reason`".to_owned()),
        Some((Argument::Reason, leading)) => leading.iter().map(Argument::rule).collect(),
        Some((Argument::Rule(_), _)) => {
            Err("missing `reason = \"<why>\"` at the end of the list".to_owned())
        }
        None => Err(USAGE.to_owned()),
    }
}

/// Comma-separated arguments inside `expect(...)`; a trailing comma is allowed.
fn arguments(tree: &ast::TokenTree) -> Result<Vec<Argument>, String> {
    let elements: Vec<_> = tree.syntax().children_with_tokens().collect();
    // The first and last elements are the parentheses.
    let inner = elements
        .get(1..elements.len().saturating_sub(1))
        .unwrap_or_default();
    let tokens = inner
        .iter()
        .filter(|element| !element.kind().is_trivia())
        .map(|element| match element {
            NodeOrToken::Token(token) => Ok(token.clone()),
            NodeOrToken::Node(node) => {
                Err(format!("unexpected `{}` inside `expect(...)`", node.text()))
            }
        })
        .collect::<Result<Vec<SyntaxToken>, String>>()?;

    // Each piece ends at its comma, so an empty list has no pieces and a trailing comma no
    // empty last piece.
    tokens
        .split_inclusive(|token| token.kind() == T![,])
        .map(|piece| {
            let comma = piece.split_last().filter(|(last, _)| last.kind() == T![,]);
            argument(comma.map_or(piece, |(_, leading)| leading))
        })
        .collect()
}

fn argument(tokens: &[SyntaxToken]) -> Result<Argument, String> {
    match tokens {
        [] => Err("empty argument inside `expect(...)`".to_owned()),
        [name] if name.kind() == SyntaxKind::IDENT => source_rule(name.text()).map(Argument::Rule),
        [key, equals, value] if key.text() == "reason" && equals.kind() == T![=] => {
            reason(value).map(|()| Argument::Reason)
        }
        // Any other token sequence inside `expect(...)` is malformed input.
        other => Err(format!(
            "unexpected `{}` inside `expect(...)`",
            other
                .iter()
                .map(SyntaxToken::text)
                .collect::<Vec<_>>()
                .join(" ")
        )),
    }
}

fn source_rule(name: &str) -> Result<SourceExemptable, String> {
    match Exemptability::named(name)? {
        Exemptability::Source(rule) => Ok(rule),
        Exemptability::Manifest(_) => Err(format!(
            "rule `{name}` is exempted in Cargo.toml, not in Rust source"
        )),
        Exemptability::Never => Err(format!("rule `{name}` cannot be exempted")),
    }
}

fn reason(token: &SyntaxToken) -> Result<(), String> {
    let literal = ast::String::cast(token.clone()).ok_or("`reason` must be a string literal")?;
    // - The lexer already rejects invalid escapes, so a file holding one never gets here.
    // - It lets through a bare carriage return and line continuations rustc only warns about.
    let value = literal.value().map_err(|_| {
        "`reason` holds a bare carriage return or a line continuation rustc warns about".to_owned()
    })?;

    if value.trim().is_empty() {
        Err("`reason` is empty; name the concrete case".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
