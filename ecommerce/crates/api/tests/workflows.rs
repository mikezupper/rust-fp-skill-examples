//! Workflow tests against real (in-memory) infrastructure.
//!
//! No mocking library appears anywhere in this file. The substitutions are a
//! different database URL and two hand-written adapters — fakes as values, not
//! expectations about call sequences. A test that asserts "the repository's `insert`
//! was called once" is testing the implementation; these assert what the system
//! *did*, which is what a user can observe.

// Integration tests are their own crates, so `cfg(test)` is not set for them and the
// workspace deny tier applies in full. Relaxed at the crate root with a reason: in a
// test, `expect` and `panic!` are assertions — a fixture that fails to build means the
// test is wrong, and failing loudly is the correct report.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test assertions: a failed precondition should abort the test loudly"
)]

mod common;

use app::workflows::{auth, cart, catalog, orders};
use app::{CartError, CheckoutError, LoginError, OrderLookupError, RegisterError};
use common::{email, password, product_id, qty, test_ctx};
use domain::OrderId;

// Seeded catalog: `p-laptop-pro` has stock 5, `p-headphones` has stock 3.
const LAPTOP_PRO: &str = "p-laptop-pro";
const HEADPHONES: &str = "p-headphones";
const LAPTOP_PRO_CENTS: u32 = 199_900;
const HEADPHONES_CENTS: u32 = 34_900;

// ---------------------------------------------------------------------------
// auth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn register_then_login_round_trips() {
    let ctx = test_ctx().await;
    let (mail, pass) = (
        email("shopper@example.com"),
        password("correct horse battery"),
    );

    auth::register(&ctx, mail.clone(), &pass)
        .await
        .expect("registration succeeds");
    let session = auth::login(&ctx, &mail, &pass)
        .await
        .expect("login succeeds");

    assert_eq!(session.email, mail);
}

#[tokio::test]
async fn duplicate_registration_fails_with_email_taken() {
    let ctx = test_ctx().await;
    let (mail, pass) = (
        email("shopper@example.com"),
        password("correct horse battery"),
    );

    auth::register(&ctx, mail.clone(), &pass)
        .await
        .expect("first registration succeeds");
    let failure = auth::register(&ctx, mail.clone(), &pass)
        .await
        .expect_err("second registration is rejected");

    // `matches!` rather than `assert_eq!`: these error enums wrap non-comparable
    // sources, so they neither derive nor should derive `PartialEq`.
    assert!(matches!(failure, RegisterError::EmailTaken { .. }));
}

#[tokio::test]
async fn email_normalisation_prevents_duplicate_accounts() {
    // Registering "Shopper@Example.COM " after "shopper@example.com" must collide.
    // The check lives in the *type*, not in the workflow — this asserts the type is
    // doing its job end to end.
    let ctx = test_ctx().await;
    let pass = password("correct horse battery");

    auth::register(&ctx, email("shopper@example.com"), &pass)
        .await
        .expect("first succeeds");
    let failure = auth::register(&ctx, email("  Shopper@Example.COM "), &pass)
        .await
        .expect_err("the same address in different clothes is still taken");

    assert!(matches!(failure, RegisterError::EmailTaken { .. }));
}

#[tokio::test]
async fn wrong_password_and_unknown_email_are_indistinguishable() {
    let ctx = test_ctx().await;
    let (mail, pass) = (
        email("shopper@example.com"),
        password("correct horse battery"),
    );
    auth::register(&ctx, mail.clone(), &pass)
        .await
        .expect("registration succeeds");

    let wrong_password = auth::login(&ctx, &mail, &password("wrong password entirely"))
        .await
        .expect_err("bad password is rejected");
    let unknown_email = auth::login(&ctx, &email("nobody@example.com"), &pass)
        .await
        .expect_err("unknown account is rejected");

    // Identical variants, by design: distinguishing them would let an attacker
    // enumerate which addresses have accounts.
    assert!(matches!(wrong_password, LoginError::InvalidCredentials));
    assert!(matches!(unknown_email, LoginError::InvalidCredentials));
}

// ---------------------------------------------------------------------------
// catalog
// ---------------------------------------------------------------------------

#[tokio::test]
async fn category_navigation_nests_children_under_parents() {
    let ctx = test_ctx().await;
    let tree = catalog::category_navigation(&ctx)
        .await
        .expect("navigation succeeds");

    let electronics = tree
        .iter()
        .find(|node| node.slug == "electronics")
        .expect("electronics is a root category");
    let mut children: Vec<&str> = electronics
        .children
        .iter()
        .map(|c| c.slug.as_str())
        .collect();
    children.sort_unstable();

    assert_eq!(children, vec!["audio", "laptops"]);
}

#[tokio::test]
async fn search_and_category_filters_compose() {
    let ctx = test_ctx().await;

    let laptops = catalog::browse_products(
        &ctx,
        &catalog::BrowseQuery {
            search: Some("Pro".to_owned()),
            category_slug: Some("laptops".to_owned()),
        },
    )
    .await
    .expect("browse succeeds");

    assert_eq!(laptops.len(), 1);
    assert_eq!(laptops[0].id.as_ref(), LAPTOP_PRO);
}

#[tokio::test]
async fn an_unknown_category_yields_an_empty_list_not_an_error() {
    let ctx = test_ctx().await;

    let results = catalog::browse_products(
        &ctx,
        &catalog::BrowseQuery {
            search: None,
            category_slug: Some("no-such-category".to_owned()),
        },
    )
    .await
    .expect("browsing an unknown category is not a failure");

    assert!(results.is_empty());
}

// ---------------------------------------------------------------------------
// cart
// ---------------------------------------------------------------------------

#[tokio::test]
async fn adding_an_unknown_product_fails_with_product_not_found() {
    let ctx = test_ctx().await;
    let session = auth::register(
        &ctx,
        email("a@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("registration succeeds");

    let failure = cart::set_cart_item(&ctx, &session.user_id, &product_id("p-nope"), qty(1))
        .await
        .expect_err("unknown products are rejected");

    assert!(matches!(failure, CartError::ProductNotFound { .. }));
}

#[tokio::test]
async fn setting_a_cart_item_is_idempotent() {
    let ctx = test_ctx().await;
    let session = auth::register(
        &ctx,
        email("a@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("registration succeeds");

    cart::set_cart_item(&ctx, &session.user_id, &product_id(LAPTOP_PRO), qty(2))
        .await
        .expect("first set succeeds");
    let view = cart::set_cart_item(&ctx, &session.user_id, &product_id(LAPTOP_PRO), qty(2))
        .await
        .expect("second set succeeds");

    // PUT semantics: the quantity is replaced, not accumulated.
    assert_eq!(view.lines.len(), 1);
    assert_eq!(view.total_cents.get(), 2 * LAPTOP_PRO_CENTS);
}

// ---------------------------------------------------------------------------
// checkout — the transaction boundary
// ---------------------------------------------------------------------------

#[tokio::test]
async fn checkout_converts_the_cart_into_an_order_and_clears_it() {
    let ctx = test_ctx().await;
    let session = auth::register(
        &ctx,
        email("a@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("registration succeeds");
    let user = &session.user_id;

    cart::set_cart_item(&ctx, user, &product_id(LAPTOP_PRO), qty(2))
        .await
        .expect("add laptop");
    cart::set_cart_item(&ctx, user, &product_id(HEADPHONES), qty(1))
        .await
        .expect("add headphones");

    let order = orders::checkout(&ctx, user)
        .await
        .expect("checkout succeeds");

    assert_eq!(
        order.total_cents.get(),
        2 * LAPTOP_PRO_CENTS + HEADPHONES_CENTS
    );
    assert_eq!(order.lines.len(), 2);

    // Everything the transaction promised: cart cleared, stock decremented, order
    // readable from history and by id.
    assert!(
        cart::get_cart(&ctx, user)
            .await
            .expect("cart readable")
            .lines
            .is_empty()
    );

    let laptop = ctx
        .products
        .find_by_id(&product_id(LAPTOP_PRO))
        .await
        .expect("product readable")
        .expect("seeded product exists");
    assert_eq!(laptop.stock, 3, "stock should have gone 5 -> 3");

    assert_eq!(
        orders::order_history(&ctx, user)
            .await
            .expect("history readable")
            .len(),
        1
    );
    assert_eq!(
        orders::get_order(&ctx, user, &order.id)
            .await
            .expect("order readable")
            .id,
        order.id
    );
}

#[tokio::test]
async fn checkout_of_an_empty_cart_fails_with_cart_empty() {
    let ctx = test_ctx().await;
    let session = auth::register(
        &ctx,
        email("a@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("registration succeeds");

    let failure = orders::checkout(&ctx, &session.user_id)
        .await
        .expect_err("an empty cart cannot be checked out");

    assert!(matches!(failure, CheckoutError::CartEmpty));
}

/// **The atomicity proof.**
///
/// The cart holds two lines. The first reserves successfully; the second cannot. The
/// workflow returns early — and the assertion is not just that it returned an error,
/// but that the *successful* first reservation was undone.
///
/// Nothing in the workflow rolls that back explicitly. The `?` returned, the boxed
/// transaction dropped, and `Drop` issued a `ROLLBACK`. This test is what proves the
/// mechanism actually fires rather than merely being described in a comment.
#[tokio::test]
async fn insufficient_stock_rolls_back_the_entire_checkout() {
    let ctx = test_ctx().await;
    let session = auth::register(
        &ctx,
        email("a@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("registration succeeds");
    let user = &session.user_id;

    cart::set_cart_item(&ctx, user, &product_id(LAPTOP_PRO), qty(2))
        .await
        .expect("in stock (5)");
    cart::set_cart_item(&ctx, user, &product_id(HEADPHONES), qty(50))
        .await
        .expect("NOT in stock (3)");

    let failure = orders::checkout(&ctx, user)
        .await
        .expect_err("checkout must fail");

    match failure {
        CheckoutError::InsufficientStock {
            requested,
            available,
            ..
        } => {
            assert_eq!(requested, 50);
            // The available count is read inside the failed transaction, so it is
            // consistent with the attempt rather than a later guess.
            assert_eq!(available, 3);
        }
        // Spelled out rather than `_ =>`. `clippy::wildcard_enum_match_arm` is denied
        // workspace-wide, including in tests: adding a variant to `CheckoutError` must
        // break this match, because a test that silently accepts a new failure mode is
        // worse than no test.
        other @ (CheckoutError::CartEmpty | CheckoutError::Repo(_)) => {
            panic!("expected InsufficientStock, got {other:?}")
        }
    }

    // THE ASSERTIONS THAT MATTER: the failure left no partial state anywhere.
    let laptop = ctx
        .products
        .find_by_id(&product_id(LAPTOP_PRO))
        .await
        .expect("product readable")
        .expect("seeded product exists");
    assert_eq!(
        laptop.stock, 5,
        "the successful reservation must have been rolled back"
    );

    assert_eq!(
        cart::get_cart(&ctx, user)
            .await
            .expect("cart readable")
            .lines
            .len(),
        2,
        "the cart must be untouched"
    );
    assert!(
        orders::order_history(&ctx, user)
            .await
            .expect("history readable")
            .is_empty(),
        "no order may have been written"
    );
}

#[tokio::test]
async fn stock_cannot_be_oversold_by_sequential_checkouts() {
    // Headphones have stock 3. Two customers each want 2. Exactly one may succeed.
    let ctx = test_ctx().await;
    let first = auth::register(
        &ctx,
        email("a@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("first registers");
    let second = auth::register(
        &ctx,
        email("b@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("second registers");

    cart::set_cart_item(&ctx, &first.user_id, &product_id(HEADPHONES), qty(2))
        .await
        .expect("add");
    cart::set_cart_item(&ctx, &second.user_id, &product_id(HEADPHONES), qty(2))
        .await
        .expect("add");

    orders::checkout(&ctx, &first.user_id)
        .await
        .expect("the first checkout wins");
    let failure = orders::checkout(&ctx, &second.user_id)
        .await
        .expect_err("the second must not oversell");

    assert!(matches!(
        failure,
        CheckoutError::InsufficientStock {
            available: 1,
            requested: 2,
            ..
        }
    ));
}

#[tokio::test]
async fn one_user_cannot_read_another_users_order() {
    let ctx = test_ctx().await;
    let owner = auth::register(
        &ctx,
        email("owner@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("owner registers");
    let stranger = auth::register(
        &ctx,
        email("stranger@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("stranger registers");

    cart::set_cart_item(&ctx, &owner.user_id, &product_id(LAPTOP_PRO), qty(1))
        .await
        .expect("add");
    let order = orders::checkout(&ctx, &owner.user_id)
        .await
        .expect("checkout succeeds");

    // NotFound rather than Forbidden: a stranger learns nothing about whether the id
    // exists. Authorization is expressed as part of the query, so it cannot be
    // bypassed by forgetting to call a check.
    let failure = orders::get_order(&ctx, &stranger.user_id, &order.id)
        .await
        .expect_err("another user's order is not visible");

    assert!(matches!(failure, OrderLookupError::NotFound { .. }));
}

#[tokio::test]
async fn an_unknown_order_id_is_not_found() {
    let ctx = test_ctx().await;
    let session = auth::register(
        &ctx,
        email("a@example.com"),
        &password("correct horse battery"),
    )
    .await
    .expect("registration succeeds");

    let failure = orders::get_order(
        &ctx,
        &session.user_id,
        &OrderId::try_new("o-does-not-exist").expect("valid id shape"),
    )
    .await
    .expect_err("unknown orders are not found");

    assert!(matches!(failure, OrderLookupError::NotFound { .. }));
}
