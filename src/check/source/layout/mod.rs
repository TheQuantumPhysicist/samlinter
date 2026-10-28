use super::{is_bare_atom, is_bare_cfg_atom, start};
use crate::finding::{Hit, Rule};
use ra_ap_syntax::ast::{self, HasAttrs, HasModuleItem, HasName};
use ra_ap_syntax::{AstNode, AstToken, SyntaxNode, SyntaxToken};
use std::cmp::Ordering;

/// Where an item belongs in a module; declaration order is the required order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, strum::IntoStaticStr)]
enum Slot {
    #[strum(serialize = "mod declaration")]
    ModDeclaration,
    #[strum(serialize = "use item")]
    Use,
    #[strum(serialize = "#[cfg(test)] use item")]
    TestUse,
    #[strum(serialize = "code")]
    Code,
    #[strum(serialize = "`mod tests;`")]
    TestsModule,
}

impl Slot {
    fn name(self) -> &'static str {
        self.into()
    }

    /// Spacing between two consecutive items of the slot.
    const fn within(self) -> Spacing {
        match self {
            Self::Use | Self::TestUse => Spacing::Contiguous,
            Self::ModDeclaration | Self::Code | Self::TestsModule => Spacing::Free,
        }
    }

    /// Spacing between the slot's last item and the first item of a later slot.
    const fn before_next(self) -> Spacing {
        match self {
            Self::ModDeclaration | Self::Use | Self::TestUse => Spacing::Separated,
            Self::Code | Self::TestsModule => Spacing::Free,
        }
    }
}

enum Spacing {
    Contiguous,
    /// A blank line, between slot groups.
    Separated,
    /// A blank line, between two functions with bodies.
    BetweenFunctions,
    Free,
}

/// - What the layout rules read from an item.
/// - A function with a body is code that needs a blank line from the next such function;
///   bodyless trait and extern declarations may touch each other.
#[derive(Clone, Copy)]
enum Kind {
    FnWithBody,
    Slotted(Slot),
}

impl Kind {
    const fn slot(self) -> Slot {
        match self {
            Self::FnWithBody => Slot::Code,
            Self::Slotted(slot) => slot,
        }
    }

    /// Where there is no group order, as in impl, trait and block bodies, every slot is code.
    const fn as_code(self) -> Self {
        match self {
            Self::FnWithBody => Self::FnWithBody,
            Self::Slotted(_) => Self::Slotted(Slot::Code),
        }
    }
}

struct Placed<'a> {
    node: &'a SyntaxNode,
    kind: Kind,
}

pub fn check(file: &ast::SourceFile) -> Vec<Hit> {
    let nested = file
        .syntax()
        .descendants()
        .filter_map(ast::ItemList::cast)
        .map(|list| list.items().collect::<Vec<_>>());
    let modules = std::iter::once(file.items().collect::<Vec<_>>()).chain(nested);
    let module_hits = modules.flat_map(|items| module_layout(&items));
    let impl_hits = file
        .syntax()
        .descendants()
        .filter_map(ast::AssocItemList::cast)
        .flat_map(|list| associated_layout(&list.assoc_items().collect::<Vec<_>>()));
    let block_hits = file
        .syntax()
        .descendants()
        .filter_map(ast::StmtList::cast)
        .flat_map(|list| block_layout(&list.statements().collect::<Vec<_>>()));
    module_hits.chain(impl_hits).chain(block_hits).collect()
}

fn module_layout(items: &[ast::Item]) -> Vec<Hit> {
    let placed: Vec<Placed> = items
        .iter()
        .map(|item| Placed {
            node: item.syntax(),
            kind: kind(item),
        })
        .collect();
    let gates = items
        .iter()
        .zip(&placed)
        .filter_map(|(item, placed)| gate_hit(item, placed.kind.slot()));
    order(&placed)
        .into_iter()
        .chain(spacing(&placed))
        .chain(gates)
        .collect()
}

/// Impl and trait bodies have no group order, so every item there is code.
fn associated_layout(items: &[ast::AssocItem]) -> Vec<Hit> {
    let placed: Vec<Placed> = items
        .iter()
        .map(|item| Placed {
            node: item.syntax(),
            kind: associated_kind(item),
        })
        .collect();
    spacing(&placed)
}

/// - Items in a block have no group order either, so every statement there is code.
/// - Other statements stay in the list, so two functions they separate are not neighbors.
fn block_layout(statements: &[ast::Stmt]) -> Vec<Hit> {
    let placed: Vec<Placed> = statements
        .iter()
        .map(|statement| Placed {
            node: statement.syntax(),
            kind: statement_kind(statement),
        })
        .collect();
    spacing(&placed)
}

fn associated_kind(item: &ast::AssocItem) -> Kind {
    match item {
        ast::AssocItem::Fn(function) => function_kind(function),
        ast::AssocItem::Const(_) | ast::AssocItem::MacroCall(_) | ast::AssocItem::TypeAlias(_) => {
            Kind::Slotted(Slot::Code)
        }
    }
}

fn statement_kind(statement: &ast::Stmt) -> Kind {
    match statement {
        ast::Stmt::Item(item) => kind(item).as_code(),
        ast::Stmt::ExprStmt(_) | ast::Stmt::LetStmt(_) => Kind::Slotted(Slot::Code),
    }
}

fn function_kind(function: &ast::Fn) -> Kind {
    match function.body() {
        Some(_) => Kind::FnWithBody,
        None => Kind::Slotted(Slot::Code),
    }
}

fn kind(item: &ast::Item) -> Kind {
    match item {
        ast::Item::Fn(function) => function_kind(function),
        ast::Item::Module(module) => Kind::Slotted(module_slot(module)),
        // `extern crate` is a use item, under the same cfg rule.
        ast::Item::ExternCrate(_) | ast::Item::Use(_) => {
            if is_test_only(item) {
                Kind::Slotted(Slot::TestUse)
            } else {
                Kind::Slotted(Slot::Use)
            }
        }
        ast::Item::AsmExpr(_)
        | ast::Item::Const(_)
        | ast::Item::Enum(_)
        | ast::Item::ExternBlock(_)
        | ast::Item::Impl(_)
        | ast::Item::MacroCall(_)
        | ast::Item::MacroDef(_)
        | ast::Item::MacroRules(_)
        | ast::Item::Static(_)
        | ast::Item::Struct(_)
        | ast::Item::Trait(_)
        | ast::Item::TypeAlias(_)
        | ast::Item::Union(_) => Kind::Slotted(Slot::Code),
    }
}

fn module_slot(module: &ast::Module) -> Slot {
    match module.item_list() {
        Some(_) => Slot::Code,
        None => match module.name() {
            Some(name) if name.text() == "tests" => Slot::TestsModule,
            Some(_) | None => Slot::ModDeclaration,
        },
    }
}

fn cfg_attributes(item: &impl HasAttrs) -> impl Iterator<Item = ast::CfgMeta> {
    item.attrs()
        .filter_map(|attr| attr.meta())
        .filter_map(cfg_meta)
}

fn cfg_meta(meta: ast::Meta) -> Option<ast::CfgMeta> {
    match meta {
        ast::Meta::CfgMeta(cfg) => Some(cfg),
        ast::Meta::CfgAttrMeta(_)
        | ast::Meta::KeyValueMeta(_)
        | ast::Meta::PathMeta(_)
        | ast::Meta::TokenTreeMeta(_)
        | ast::Meta::UnsafeMeta(_) => None,
    }
}

/// Stacked cfg attributes all apply, so one that requires `test` makes the item test-only.
fn is_test_only(item: &impl HasAttrs) -> bool {
    cfg_attributes(item)
        .filter_map(|cfg| cfg.cfg_predicate())
        .any(|predicate| requires_test(&predicate))
}

/// - Whether `predicate` holds only when a bare `test` does.
/// - Every member of `all(..)` must hold; `any(..)` and `not(..)` require no single member.
fn requires_test(predicate: &ast::CfgPredicate) -> bool {
    match predicate {
        ast::CfgPredicate::CfgAtom(atom) => is_bare_atom(atom, "test"),
        ast::CfgPredicate::CfgComposite(composite) => match composite.keyword() {
            Some(keyword) if keyword.text() == "all" => composite
                .cfg_predicates()
                .any(|member| requires_test(&member)),
            // `any`, `not`, and keywords rustc rejects.
            Some(_) | None => false,
        },
    }
}

/// Stacked cfg attributes combine like `cfg(all(..))`, so a second one is a further condition.
fn is_only_cfg_test(item: &impl HasAttrs) -> bool {
    let cfgs: Vec<ast::CfgMeta> = cfg_attributes(item).collect();
    match cfgs.as_slice() {
        [only] => only
            .cfg_predicate()
            .is_some_and(|predicate| is_bare_cfg_atom(&predicate, "test")),
        [] | [_, _, ..] => false,
    }
}

fn gate_hit(item: &ast::Item, slot: Slot) -> Option<Hit> {
    match slot {
        Slot::TestsModule => (!is_only_cfg_test(item)).then(|| {
            Hit::new(
                start(item.syntax()),
                Rule::ItemOrder,
                "`mod tests;` needs exactly one cfg attribute, #[cfg(test)]; further conditions belong inside tests.rs",
            )
        }),
        Slot::ModDeclaration | Slot::Use | Slot::TestUse | Slot::Code => None,
    }
}

/// Flags each item that comes after an item of a later slot.
fn order(items: &[Placed]) -> Vec<Hit> {
    items
        .iter()
        .scan(None, |latest: &mut Option<Slot>, item| {
            let slot = item.kind.slot();
            let previous = *latest;
            *latest = previous.max(Some(slot));
            Some(
                previous
                    .filter(|previous| slot < *previous)
                    .map(|previous| {
                        Hit::new(
                            start(item.node),
                            Rule::ItemOrder,
                            format!("{} after {}", slot.name(), previous.name()),
                        )
                    }),
            )
        })
        .flatten()
        .collect()
}

/// Checks each pair of neighbors against the one spacing their kinds require.
fn spacing(items: &[Placed]) -> Vec<Hit> {
    items
        .iter()
        .zip(items.iter().skip(1))
        .filter_map(|(first, second)| {
            let blank = blank_line_between(first.node, second.node);
            let at = start(second.node);
            match (required_spacing(first, second), blank) {
                (Spacing::Contiguous, true) => Some(Hit::new(
                    at,
                    Rule::ItemSpacing,
                    format!(
                        "blank line between consecutive {}s",
                        second.kind.slot().name()
                    ),
                )),
                (Spacing::Separated, false) => Some(Hit::new(
                    at,
                    Rule::ItemSpacing,
                    format!(
                        "no blank line between the last {} and this {}",
                        first.kind.slot().name(),
                        second.kind.slot().name()
                    ),
                )),
                (Spacing::BetweenFunctions, false) => Some(Hit::new(
                    at,
                    Rule::FnSpacing,
                    "no blank line between this function and the previous one",
                )),
                (Spacing::Contiguous, false)
                | (Spacing::Separated | Spacing::BetweenFunctions, true)
                | (Spacing::Free, true | false) => None,
            }
        })
        .collect()
}

fn required_spacing(first: &Placed, second: &Placed) -> Spacing {
    match (first.kind, second.kind) {
        (Kind::FnWithBody, Kind::FnWithBody) => Spacing::BetweenFunctions,
        (Kind::FnWithBody, Kind::Slotted(_))
        | (Kind::Slotted(_), Kind::FnWithBody | Kind::Slotted(_)) => {
            slot_spacing(first.kind.slot(), second.kind.slot())
        }
    }
}

fn slot_spacing(first: Slot, second: Slot) -> Spacing {
    match first.cmp(&second) {
        Ordering::Equal => first.within(),
        Ordering::Less => first.before_next(),
        // An out-of-order pair, which item_order reports.
        Ordering::Greater => Spacing::Free,
    }
}

/// - True when a whitespace token from the end of `first` to `start(second)` holds two line
///   breaks.
/// - The scan reaches into `second`, where the parser files the comments above it, so a
///   whitespace-only line under such a comment counts; `second`'s doc comment ends the scan.
/// - Comments split whitespace tokens, so a comment line never counts as blank.
fn blank_line_between(first: &SyntaxNode, second: &SyntaxNode) -> bool {
    let end = start(second);
    std::iter::successors(
        first.last_token().and_then(|token| token.next_token()),
        SyntaxToken::next_token,
    )
    .take_while(|token| usize::from(token.text_range().start()) < end)
    .filter_map(ast::Whitespace::cast)
    .any(|whitespace| whitespace.text().matches('\n').count() >= 2)
}

#[cfg(test)]
mod tests;
