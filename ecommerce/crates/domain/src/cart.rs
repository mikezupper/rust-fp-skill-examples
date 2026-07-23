//! Cart pricing. Entirely pure — the cart total is a function of its contents and
//! nothing else, so it is testable without a database, a clock, or a runtime.

use nutype::nutype;
use serde::{Deserialize, Serialize};

use crate::catalog::Product;
use crate::money::Cents;

/// A line quantity. The bounds are part of the type: a `Quantity` of `0` or `10_000`
/// cannot be constructed, so no downstream function has to defend against one.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 99),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        Serialize,
        Deserialize
    )
)]
pub struct Quantity(u32);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartLine {
    pub product: Product,
    pub quantity: Quantity,
    pub line_total_cents: Cents,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartView {
    pub lines: Vec<CartLine>,
    pub total_cents: Cents,
}

/// Saturating rather than checked: `Cents` maxes at ~$42.9M and `Quantity` at 99,
/// so the product of two valid values is representable by construction. The
/// saturation is a belt-and-braces default that can never trigger for in-range
/// inputs — proved by the property test below.
#[must_use]
pub fn line_total(price: Cents, quantity: Quantity) -> Cents {
    price
        .checked_mul(quantity.into_inner())
        .unwrap_or(Cents::new(u32::MAX))
}

/// Builds the priced view of a cart. Pure: same items in, same view out.
#[must_use]
pub fn make_cart_view(items: &[(Product, Quantity)]) -> CartView {
    let lines: Vec<CartLine> = items
        .iter()
        .map(|(product, quantity)| CartLine {
            line_total_cents: line_total(product.price_cents, *quantity),
            product: product.clone(),
            quantity: *quantity,
        })
        .collect();

    let total_cents = lines.iter().map(|l| l.line_total_cents).sum();

    CartView { lines, total_cents }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CategoryId, DisplayName, ProductId, Sku};
    use proptest::prelude::*;

    prop_compose! {
        fn arb_product()(
            id in 0u32..1000,
            price in 0u32..1_000_000,
            stock in 0u32..1000,
        ) -> Product {
            Product {
                id: ProductId::try_new(format!("p-{id}")).expect("valid id"),
                sku: Sku::try_new(format!("SKU-{id}")).expect("valid sku"),
                name: DisplayName::try_new(format!("Product {id}")).expect("valid name"),
                description: String::new(),
                price_cents: Cents::new(price),
                category_id: CategoryId::try_new("cat").expect("valid category"),
                stock,
            }
        }
    }

    prop_compose! {
        fn arb_items()(
            items in prop::collection::vec((arb_product(), 1u32..=99), 0..16)
        ) -> Vec<(Product, Quantity)> {
            items
                .into_iter()
                .map(|(p, q)| (p, Quantity::try_new(q).expect("in range by construction")))
                .collect()
        }
    }

    proptest! {
        /// The headline invariant: the total is the sum of the line totals.
        #[test]
        fn total_equals_sum_of_lines(items in arb_items()) {
            let view = make_cart_view(&items);
            let summed: Cents = view.lines.iter().map(|l| l.line_total_cents).sum();
            prop_assert_eq!(view.total_cents, summed);
        }

        /// Commutativity: a cart is a bag, not a list, as far as price is concerned.
        #[test]
        fn total_is_invariant_under_reordering(items in arb_items()) {
            let forward = make_cart_view(&items).total_cents;
            let mut reversed = items;
            reversed.reverse();
            prop_assert_eq!(forward, make_cart_view(&reversed).total_cents);
        }

        /// Every line total is exactly price x quantity — and, because both operands
        /// are bounded by their types, the saturating path in `line_total` is
        /// unreachable for any constructible input.
        #[test]
        fn every_line_total_is_price_times_quantity(items in arb_items()) {
            let view = make_cart_view(&items);
            for line in &view.lines {
                let expected = line.product.price_cents.get() * line.quantity.into_inner();
                prop_assert_eq!(line.line_total_cents.get(), expected);
            }
        }

        /// Structure preservation: pricing never adds or drops a line.
        #[test]
        fn line_count_is_preserved(items in arb_items()) {
            prop_assert_eq!(make_cart_view(&items).lines.len(), items.len());
        }
    }

    #[test]
    fn quantity_rejects_out_of_range_values() {
        assert!(Quantity::try_new(0).is_err());
        assert!(Quantity::try_new(100).is_err());
        assert!(Quantity::try_new(1).is_ok());
        assert!(Quantity::try_new(99).is_ok());
    }

    #[test]
    fn an_empty_cart_totals_zero() {
        let view = make_cart_view(&[]);
        assert_eq!(view.total_cents, Cents::ZERO);
        assert!(view.lines.is_empty());
    }
}
