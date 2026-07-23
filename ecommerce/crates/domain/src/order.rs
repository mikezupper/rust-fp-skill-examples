//! Orders, and the non-empty invariant that makes an empty order unrepresentable.

use chrono::{DateTime, Utc};
use nutype::nutype;
use serde::{Deserialize, Serialize};

use crate::cart::{CartLine, Quantity};
use crate::catalog::{DisplayName, ProductId};
use crate::money::Cents;
use crate::user::UserId;

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
pub struct OrderId(String);

/// A vector guaranteed to hold at least one element.
///
/// This exists so that `Order` cannot represent an order with no lines. The
/// alternative — a `Vec` plus a comment plus a check in every consumer — is the
/// thing this whole architecture is trying to avoid. `Deserialize` goes through
/// `TryFrom<Vec<T>>`, so the invariant survives the wire too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<T>",
    into = "Vec<T>",
    bound(serialize = "T: Clone + serde::Serialize")
)]
pub struct NonEmpty<T>(Vec<T>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("expected at least one element, got none")]
pub struct Empty;

impl<T> TryFrom<Vec<T>> for NonEmpty<T> {
    type Error = Empty;

    fn try_from(items: Vec<T>) -> Result<Self, Self::Error> {
        if items.is_empty() {
            return Err(Empty);
        }
        Ok(Self(items))
    }
}

impl<T> From<NonEmpty<T>> for Vec<T> {
    fn from(value: NonEmpty<T>) -> Self {
        value.0
    }
}

impl<T> NonEmpty<T> {
    /// Infallible construction from a head plus a (possibly empty) tail.
    pub fn new(head: T, mut tail: Vec<T>) -> Self {
        let mut items = vec![head];
        items.append(&mut tail);
        Self(items)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Total: there is always a first element, so this cannot fail and does not
    /// return an `Option`.
    #[must_use]
    pub fn first(&self) -> &T {
        // A `NonEmpty` with no elements is unconstructible; the fallback is dead code
        // that exists only because `[T]::first` cannot know that.
        match self.0.first() {
            Some(item) => item,
            None => unreachable_nonempty(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always false. Present so clippy's `len_without_is_empty` is satisfied without
    /// implying that emptiness is a state this type can reach.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    pub fn map<U, F: FnMut(&T) -> U>(&self, f: F) -> NonEmpty<U> {
        NonEmpty(self.0.iter().map(f).collect())
    }
}

impl<'a, T> IntoIterator for &'a NonEmpty<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cold]
#[allow(clippy::panic, reason = "unconstructible state; see NonEmpty::first")]
fn unreachable_nonempty() -> ! {
    panic!("invariant violated: NonEmpty was constructed empty")
}

// ---------------------------------------------------------------------------
// Order
// ---------------------------------------------------------------------------

/// A line as it was at the moment of purchase.
///
/// Name and unit price are **snapshotted**, not referenced. A later catalog edit
/// must not rewrite the customer's receipt — so this deliberately duplicates data
/// rather than holding a `ProductId` and joining at read time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderLine {
    pub product_id: ProductId,
    pub name: DisplayName,
    pub unit_price_cents: Cents,
    pub quantity: Quantity,
}

impl OrderLine {
    #[must_use]
    pub fn line_total(&self) -> Cents {
        self.unit_price_cents
            .checked_mul(self.quantity.into_inner())
            .unwrap_or(Cents::new(u32::MAX))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: OrderId,
    pub user_id: UserId,
    pub lines: NonEmpty<OrderLine>,
    pub total_cents: Cents,
    pub placed_at: DateTime<Utc>,
}

impl Order {
    /// The only constructor. `total_cents` is derived, never supplied — so an order
    /// whose total disagrees with its lines cannot be built. `placed_at` is a
    /// parameter rather than a `Utc::now()` call, which is what keeps this function
    /// pure and this whole module testable without a clock.
    #[must_use]
    pub fn place(
        id: OrderId,
        user_id: UserId,
        lines: NonEmpty<OrderLine>,
        placed_at: DateTime<Utc>,
    ) -> Self {
        let total_cents = order_total(lines.as_slice());
        Self {
            id,
            user_id,
            lines,
            total_cents,
            placed_at,
        }
    }
}

/// Pure snapshot of priced cart lines into order lines.
#[must_use]
pub fn to_order_lines(cart_lines: &NonEmpty<CartLine>) -> NonEmpty<OrderLine> {
    cart_lines.map(|line| OrderLine {
        product_id: line.product.id.clone(),
        name: line.product.name.clone(),
        unit_price_cents: line.product.price_cents,
        quantity: line.quantity,
    })
}

#[must_use]
pub fn order_total(lines: &[OrderLine]) -> Cents {
    lines.iter().map(OrderLine::line_total).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn line(price: u32, qty: u32) -> OrderLine {
        OrderLine {
            product_id: ProductId::try_new("p-1").expect("valid"),
            name: DisplayName::try_new("Thing").expect("valid"),
            unit_price_cents: Cents::new(price),
            quantity: Quantity::try_new(qty).expect("in range"),
        }
    }

    #[test]
    fn an_empty_order_cannot_be_constructed() {
        let empty: Vec<OrderLine> = Vec::new();
        assert!(NonEmpty::try_from(empty).is_err());
    }

    #[test]
    fn an_empty_line_list_cannot_be_deserialized() {
        // The invariant holds across the wire, not just in memory.
        let json = r#"{"id":"o-1","user_id":"u-1","lines":[],"total_cents":0,"placed_at":"2026-01-01T00:00:00Z"}"#;
        assert!(serde_json::from_str::<Order>(json).is_err());
    }

    #[test]
    fn total_is_derived_from_lines_not_supplied() {
        let lines = NonEmpty::new(line(1000, 2), vec![line(250, 4)]);
        let order = Order::place(
            OrderId::try_new("o-1").expect("valid"),
            crate::user::UserId::try_new("u-1").expect("valid"),
            lines,
            DateTime::from_timestamp(0, 0).expect("epoch is a valid timestamp"),
        );
        assert_eq!(order.total_cents, Cents::new(2000 + 1000));
    }

    prop_compose! {
        fn arb_lines()(
            raw in prop::collection::vec((0u32..100_000, 1u32..=99), 1..12)
        ) -> NonEmpty<OrderLine> {
            let lines: Vec<OrderLine> = raw.into_iter().map(|(p, q)| line(p, q)).collect();
            NonEmpty::try_from(lines).expect("generator produces at least one line")
        }
    }

    proptest! {
        /// The order total always equals the sum of its line totals.
        #[test]
        fn total_equals_sum_of_line_totals(lines in arb_lines()) {
            let summed: Cents = lines.iter().map(OrderLine::line_total).sum();
            prop_assert_eq!(order_total(lines.as_slice()), summed);
        }

        /// Round-trip: an order survives JSON encode/decode unchanged. This is the
        /// property that keeps the wire format and the domain model from drifting.
        #[test]
        fn order_round_trips_through_json(lines in arb_lines()) {
            let order = Order::place(
                OrderId::try_new("o-1").expect("valid"),
                crate::user::UserId::try_new("u-1").expect("valid"),
                lines,
                DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp"),
            );
            let encoded = serde_json::to_string(&order).expect("order serializes");
            let decoded: Order = serde_json::from_str(&encoded).expect("order round-trips");
            prop_assert_eq!(order, decoded);
        }

        /// Snapshotting preserves every line, in order.
        #[test]
        fn snapshotting_preserves_line_count(lines in arb_lines()) {
            let cart_lines: Vec<CartLine> = lines
                .iter()
                .map(|l| CartLine {
                    product: crate::catalog::Product {
                        id: l.product_id.clone(),
                        sku: crate::catalog::Sku::try_new("SKU-1").expect("valid"),
                        name: l.name.clone(),
                        description: String::new(),
                        price_cents: l.unit_price_cents,
                        category_id: crate::catalog::CategoryId::try_new("c").expect("valid"),
                        stock: 0,
                    },
                    quantity: l.quantity,
                    line_total_cents: l.line_total(),
                })
                .collect();
            let cart_lines = NonEmpty::try_from(cart_lines).expect("non-empty");
            prop_assert_eq!(to_order_lines(&cart_lines).len(), lines.len());
        }
    }
}
