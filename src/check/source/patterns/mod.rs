use super::start;
use crate::finding::{Hit, Rule};
use ra_ap_syntax::ast::{BinaryOp, HasArgList};
use ra_ap_syntax::{AstNode, SyntaxNode, SyntaxToken, ast};

const MESSAGE: &str = "`..` absorbs every field this pattern does not name";

/// - Rest patterns inside struct, tuple-struct and tuple patterns, and the same rests on the
///   left of a destructuring assignment such as `(a, ..) = t;`.
/// - Slice rests (`[first, ..]`) stay allowed: a slice's length is open-ended input.
pub fn check(root: &SyntaxNode) -> impl Iterator<Item = Hit> {
    root.descendants()
        .flat_map(|node| {
            let assigned = assignee(&node).into_iter();
            rest_patterns(&node)
                .into_iter()
                .chain(assigned.flat_map(|left| assignee_rests(&left, Position::Nested)))
        })
        .map(|offset| Hit::new(offset, Rule::RestPattern, MESSAGE))
}

/// The offsets of the rests in a pattern node's own fields.
fn rest_patterns(node: &SyntaxNode) -> Vec<usize> {
    let record = ast::RecordPatFieldList::cast(node.clone()).and_then(|fields| fields.rest_pat());
    let tuple_struct = ast::TupleStructPat::cast(node.clone())
        .into_iter()
        .flat_map(|pattern| pattern.fields());
    let tuple = ast::TuplePat::cast(node.clone())
        .into_iter()
        .flat_map(|pattern| pattern.fields());
    let positional = tuple_struct.chain(tuple).filter_map(rest_pattern);
    record
        .into_iter()
        .chain(positional)
        .map(|rest| start(rest.syntax()))
        .collect()
}

/// The left side of a plain `=`; a compound assignment such as `+=` does not destructure.
fn assignee(node: &SyntaxNode) -> Option<ast::Expr> {
    let assignment = ast::BinExpr::cast(node.clone())?;
    match assignment.op_kind()? {
        BinaryOp::Assignment { op: None } => assignment.lhs(),
        BinaryOp::Assignment { op: Some(_) }
        | BinaryOp::LogicOp(_)
        | BinaryOp::ArithOp(_)
        | BinaryOp::CmpOp(_) => None,
    }
}

/// Where an assignee expression sits, which decides what a bare `..` in it means.
#[derive(Clone, Copy)]
enum Position {
    /// - A tuple field, a call argument, or the inside of parentheses.
    /// - A bare `..` here is a positional rest; rustc reads `(..)` as the tuple pattern `(..)`.
    Positional,
    /// - The whole assignee, an array element or a record field.
    /// - A bare `..` here is a range, or a slice rest in an array, and is not flagged.
    Nested,
}

/// - The offsets of the rests in an assignee expression and the assignees nested in it.
/// - Its expression forms mirror patterns: a record expression's `..` without a base is a
///   record rest, a bare `..` in a positional place is a positional rest.
fn assignee_rests(expr: &ast::Expr, position: Position) -> Vec<usize> {
    match expr {
        ast::Expr::RangeExpr(range) => match position {
            Position::Positional => range
                .is_range_full()
                .then(|| start(range.syntax()))
                .into_iter()
                .collect(),
            Position::Nested => Vec::new(),
        },
        ast::Expr::ParenExpr(paren) => paren
            .expr()
            .iter()
            .flat_map(|inner| assignee_rests(inner, Position::Positional))
            .collect(),
        ast::Expr::TupleExpr(tuple) => tuple
            .fields()
            .flat_map(|element| assignee_rests(&element, Position::Positional))
            .collect(),
        ast::Expr::CallExpr(call) => call
            .arg_list()
            .into_iter()
            .flat_map(|arguments| arguments.args())
            .flat_map(|element| assignee_rests(&element, Position::Positional))
            .collect(),
        ast::Expr::ArrayExpr(array) => array
            .exprs()
            .flat_map(|element| assignee_rests(&element, Position::Nested))
            .collect(),
        ast::Expr::RecordExpr(record) => record
            .record_expr_field_list()
            .into_iter()
            .flat_map(|fields| {
                let rest = record_rest(&fields).map(|dotdot| offset(&dotdot));
                let nested = fields
                    .fields()
                    .filter_map(|field| field.expr())
                    .flat_map(|field| assignee_rests(&field, Position::Nested));
                rest.into_iter().chain(nested).collect::<Vec<_>>()
            })
            .collect(),
        ast::Expr::AsmExpr(_)
        | ast::Expr::AwaitExpr(_)
        | ast::Expr::BecomeExpr(_)
        | ast::Expr::BinExpr(_)
        | ast::Expr::BlockExpr(_)
        | ast::Expr::BreakExpr(_)
        | ast::Expr::CastExpr(_)
        | ast::Expr::CfgPredExpr(_)
        | ast::Expr::ClosureExpr(_)
        | ast::Expr::ContinueExpr(_)
        | ast::Expr::FieldExpr(_)
        | ast::Expr::ForExpr(_)
        | ast::Expr::FormatArgsExpr(_)
        | ast::Expr::IfExpr(_)
        | ast::Expr::IncludeBytesExpr(_)
        | ast::Expr::IndexExpr(_)
        | ast::Expr::LetExpr(_)
        | ast::Expr::Literal(_)
        | ast::Expr::LoopExpr(_)
        | ast::Expr::MacroExpr(_)
        | ast::Expr::MatchExpr(_)
        | ast::Expr::MethodCallExpr(_)
        | ast::Expr::OffsetOfExpr(_)
        | ast::Expr::PathExpr(_)
        | ast::Expr::PrefixExpr(_)
        | ast::Expr::RefExpr(_)
        | ast::Expr::ReturnExpr(_)
        | ast::Expr::TryExpr(_)
        | ast::Expr::UnderscoreExpr(_)
        | ast::Expr::WhileExpr(_)
        | ast::Expr::YeetExpr(_)
        | ast::Expr::YieldExpr(_) => Vec::new(),
    }
}

/// The `..` of a record expression with no base to copy the other fields from.
fn record_rest(fields: &ast::RecordExprFieldList) -> Option<SyntaxToken> {
    match fields.spread() {
        Some(_) => None,
        None => fields.dotdot_token(),
    }
}

fn offset(token: &SyntaxToken) -> usize {
    usize::from(token.text_range().start())
}

fn rest_pattern(pattern: ast::Pat) -> Option<ast::RestPat> {
    match pattern {
        ast::Pat::RestPat(rest) => Some(rest),
        ast::Pat::BoxPat(_)
        | ast::Pat::CfgPredPat(_)
        | ast::Pat::ConstBlockPat(_)
        | ast::Pat::DerefPat(_)
        | ast::Pat::IdentPat(_)
        | ast::Pat::LiteralPat(_)
        | ast::Pat::MacroPat(_)
        | ast::Pat::NotNull(_)
        | ast::Pat::OrPat(_)
        | ast::Pat::ParenPat(_)
        | ast::Pat::PathPat(_)
        | ast::Pat::RangePat(_)
        | ast::Pat::RecordPat(_)
        | ast::Pat::RefPat(_)
        | ast::Pat::SlicePat(_)
        | ast::Pat::TuplePat(_)
        | ast::Pat::TupleStructPat(_)
        | ast::Pat::WildcardPat(_) => None,
    }
}

#[cfg(test)]
mod tests;
