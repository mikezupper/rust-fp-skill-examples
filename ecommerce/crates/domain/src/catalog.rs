//! Catalog: products, categories, and the pure category-tree builder.

use std::collections::BTreeSet;

use nutype::nutype;
use serde::{Deserialize, Serialize};

use crate::money::Cents;

// ---------------------------------------------------------------------------
// Branded primitives.
//
// `nutype` makes the validating constructor the ONLY way to build the value: the
// inner field is private and no `From<String>` is generated. A `ProductId` in a
// function signature is therefore a proof that the string was checked, once,
// somewhere at a boundary — not a request for the callee to check it again.
// ---------------------------------------------------------------------------

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 64),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct ProductId(String);

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 64),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct CategoryId(String);

#[nutype(
    sanitize(trim, uppercase),
    validate(not_empty, len_char_max = 32),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct Sku(String);

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 200),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct DisplayName(String);

#[nutype(
    sanitize(trim, lowercase),
    validate(not_empty, len_char_max = 64),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct Slug(String);

// ---------------------------------------------------------------------------
// Domain records.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: CategoryId,
    pub name: DisplayName,
    pub slug: Slug,
    /// Absence is `None`, never an empty string or a magic "root" sentinel.
    pub parent_id: Option<CategoryId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: ProductId,
    pub sku: Sku,
    pub name: DisplayName,
    pub description: String,
    pub price_cents: Cents,
    pub category_id: CategoryId,
    pub stock: u32,
}

/// The response shape for category navigation. This is a wire type, so it uses
/// plain strings: it has already been through the domain and is on its way out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryTree {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub children: Vec<CategoryTree>,
}

// ---------------------------------------------------------------------------
// Pure category navigation.
// ---------------------------------------------------------------------------

/// Flat category list -> forest. Pure and **total**.
///
/// Totality is the hard part and the reason this function has a property test:
/// every input category must appear in the output exactly once, for *every*
/// input — including inputs the seed data never produces.
///
/// - A category whose parent is missing from the input becomes a root.
/// - A category that is its own parent becomes a root.
/// - Categories in a cycle (a -> b -> a) are unreachable from any root, so they
///   are grafted on as roots rather than silently dropped.
///
/// The naive implementation drops cycle participants on the floor. That is a
/// data-loss bug that only shows up on malformed data, which is exactly the
/// class of bug example-based tests never find.
#[must_use]
pub fn build_category_tree(categories: &[Category]) -> Vec<CategoryTree> {
    let ids: BTreeSet<&CategoryId> = categories.iter().map(|c| &c.id).collect();
    let mut visited: BTreeSet<CategoryId> = BTreeSet::new();

    let mut roots: Vec<CategoryTree> = categories
        .iter()
        .filter(|c| is_root(c, &ids))
        .map(|c| build_node(c, categories, &mut visited))
        .collect();

    // Anything still unvisited is part of a cycle. Graft it on rather than lose it.
    let orphans: Vec<&Category> = categories
        .iter()
        .filter(|c| !visited.contains(&c.id))
        .collect();
    for orphan in orphans {
        if !visited.contains(&orphan.id) {
            roots.push(build_node(orphan, categories, &mut visited));
        }
    }

    roots
}

fn is_root(category: &Category, ids: &BTreeSet<&CategoryId>) -> bool {
    match &category.parent_id {
        None => true,
        // Self-parenting and dangling parents both mean "no usable parent".
        Some(parent) => *parent == category.id || !ids.contains(parent),
    }
}

/// `visited` is contained local mutation: it never escapes `build_category_tree`,
/// so the public function stays pure and referentially transparent. Contained
/// mutation inside a pure signature is idiomatic Rust, not a rule violation.
fn build_node(
    category: &Category,
    all: &[Category],
    visited: &mut BTreeSet<CategoryId>,
) -> CategoryTree {
    visited.insert(category.id.clone());

    // Two passes rather than one chained iterator: the child lookup borrows `visited`
    // immutably while the recursive call needs it mutably, so the candidate set is
    // materialised first. Each child is re-checked against `visited` inside the loop,
    // because an earlier sibling's subtree may have already claimed it.
    let candidates: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.parent_id.as_ref() == Some(&category.id))
        .map(|(index, _)| index)
        .collect();

    let mut children = Vec::with_capacity(candidates.len());
    for index in candidates {
        let Some(child) = all.get(index) else {
            continue;
        };
        if visited.contains(&child.id) {
            continue;
        }
        children.push(build_node(child, all, visited));
    }

    CategoryTree {
        id: category.id.as_ref().to_owned(),
        name: category.name.as_ref().to_owned(),
        slug: category.slug.as_ref().to_owned(),
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn category(id: &str, parent: Option<&str>) -> Category {
        Category {
            id: CategoryId::try_new(id).expect("valid test id"),
            name: DisplayName::try_new(id).expect("valid test name"),
            slug: Slug::try_new(id).expect("valid test slug"),
            parent_id: parent.map(|p| CategoryId::try_new(p).expect("valid test parent")),
        }
    }

    fn count_nodes(nodes: &[CategoryTree]) -> usize {
        nodes.iter().map(|n| 1 + count_nodes(&n.children)).sum()
    }

    fn collect_ids(nodes: &[CategoryTree], out: &mut Vec<String>) {
        for node in nodes {
            out.push(node.id.clone());
            collect_ids(&node.children, out);
        }
    }

    #[test]
    fn nests_children_under_parents() {
        let cats = vec![
            category("electronics", None),
            category("laptops", Some("electronics")),
            category("audio", Some("electronics")),
            category("books", None),
        ];
        let tree = build_category_tree(&cats);

        assert_eq!(tree.len(), 2);
        let electronics = tree
            .iter()
            .find(|n| n.slug == "electronics")
            .expect("electronics is a root");
        let mut child_slugs: Vec<&str> = electronics
            .children
            .iter()
            .map(|c| c.slug.as_str())
            .collect();
        child_slugs.sort_unstable();
        assert_eq!(child_slugs, vec!["audio", "laptops"]);
    }

    #[test]
    fn a_missing_parent_makes_a_root() {
        let cats = vec![category("laptops", Some("nonexistent"))];
        assert_eq!(count_nodes(&build_category_tree(&cats)), 1);
    }

    #[test]
    fn a_two_node_cycle_is_not_dropped() {
        // The regression that motivates the totality property below.
        let cats = vec![category("a", Some("b")), category("b", Some("a"))];
        assert_eq!(count_nodes(&build_category_tree(&cats)), 2);
    }

    #[test]
    fn a_self_parenting_category_becomes_a_root() {
        let cats = vec![category("a", Some("a"))];
        assert_eq!(count_nodes(&build_category_tree(&cats)), 1);
    }

    prop_compose! {
        /// Generates categories over a small id alphabet so that parent references
        /// collide often — which is what makes cycles and dangling parents likely.
        fn arb_categories()(
            raw in prop::collection::vec(
                (0usize..8, prop::option::of(0usize..8)),
                0..8,
            )
        ) -> Vec<Category> {
            let mut seen = BTreeSet::new();
            raw.into_iter()
                .filter(|(id, _)| seen.insert(*id))       // ids are primary keys
                .map(|(id, parent)| {
                    category(&format!("c{id}"), parent.map(|p| format!("c{p}")).as_deref())
                })
                .collect()
        }
    }

    proptest! {
        /// TOTALITY: every input category appears in the output exactly once,
        /// for every input — cycles, dangling parents, self-parents and all.
        #[test]
        fn tree_contains_every_category_exactly_once(cats in arb_categories()) {
            let tree = build_category_tree(&cats);
            prop_assert_eq!(count_nodes(&tree), cats.len());

            let mut ids = Vec::new();
            collect_ids(&tree, &mut ids);
            let unique: BTreeSet<&String> = ids.iter().collect();
            prop_assert_eq!(unique.len(), ids.len(), "a category appeared twice");
        }

        /// The output is a forest, not a graph: recursion terminates.
        #[test]
        fn tree_is_finite(cats in arb_categories()) {
            let tree = build_category_tree(&cats);
            prop_assert!(count_nodes(&tree) <= cats.len());
        }
    }
}
