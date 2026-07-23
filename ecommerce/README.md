# ecommerce

A full commerce API — catalog, category navigation, register/login, cart, **atomic checkout**, order history — built strictly with the [rust-fp-skill](https://github.com/mikezupper/rust-fp-skill). Axum 0.8 + sqlx 0.9/SQLite, four crates, 58 tests.

## Run

```bash
cargo run                       # http://localhost:3000
cargo test --workspace          # 58 tests, incl. the checkout-atomicity proof
cargo clippy --workspace --all-targets -- -D warnings
```

Config: `PORT` (default 3000), `DATABASE_URL` (default `sqlite://ecommerce.db`; use `sqlite::memory:` for ephemeral). The schema is created and seeded idempotently at boot — delete the file to reset.

## API surface

| Endpoint | Auth | Errors |
|---|---|---|
| `POST /auth/register`, `POST /auth/login` | — | `EmailTaken` 409, `InvalidCredentials` 401 |
| `GET /catalog/categories` (tree), `GET /catalog/products?search=&category=`, `GET /catalog/products/{id}` | — | `ProductNotFound` 404 |
| `GET /cart`, `PUT /cart/items`, `DELETE /cart/items/{product_id}` | bearer | `Unauthorized` 401, `ProductNotFound` 404 |
| `POST /orders` (checkout), `GET /orders`, `GET /orders/{id}` | bearer | `CartEmpty` 409, `InsufficientStock` 409, `OrderNotFound` 404 |
| `GET /health` | — | — |

```bash
TOKEN=$(curl -sX POST localhost:3000/auth/register \
  -H 'content-type: application/json' \
  -d '{"email":"shopper@example.com","password":"correct horse battery"}' | jq -r .token)

curl -sX PUT localhost:3000/cart/items -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"productId":"p-headphones","quantity":50}'           # stock is 3

curl -sX POST localhost:3000/orders -H "authorization: Bearer $TOKEN"
# 409 {"_tag":"InsufficientStock","available":3,
#       "productId":"p-headphones","requested":50}
```

## Architecture — enforced by cargo, not by convention

```
crates/domain   pure: newtypes, enums, pricing, the category-tree builder
crates/app      workflows + ports (traits). Owns the transaction boundary.
crates/infra    adapters: sqlx repositories, scrypt, the clock, the id generator
crates/api      axum router, DTOs, error→HTTP mapping, and the one wiring site
```

The dependency direction is not a diagram, it is a build failure:

```console
$ cargo tree -p domain --edges normal --depth 1
domain v0.1.0
├── chrono v0.4.45      # WITHOUT the "clock" feature — Utc::now() does not exist here
├── nutype v0.7.0
├── serde v1.0.229
└── thiserror v2.0.19
```

No tokio. No sqlx. No axum. A workflow cannot reach into infrastructure because the crate it would import is not a dependency, and adding it means editing a `Cargo.toml` in a pull request where someone will see it.

## What it demonstrates

**The transaction boundary lives in the workflow** (`crates/app/src/workflows/orders.rs`). Checkout reads the cart, reserves stock line by line, writes the order and clears the cart. It opens the transaction because it is the only layer that knows those four operations are one unit of work.

**Rollback is structural, not remembered.** Nothing in `checkout` rolls anything back. Every `?` returns early, which drops the boxed transaction, and `sqlx`'s `Drop` issues a `ROLLBACK` — and `commit` takes `Box<Self>`, so a live transaction is provably uncommitted. Ownership and railway-oriented early exit turn out to be the same mechanism. `insufficient_stock_rolls_back_the_entire_checkout` asserts the successful first reservation was undone, the cart is untouched, and no order was written.

**`app` owns a transaction without knowing what a database is.** `Database::begin` returns `Box<dyn CheckoutTx>`; the SQLite type never appears in a workflow signature.

**Illegal states don't compile.** `Order` holds a `NonEmpty<OrderLine>`, so an order with no lines is unrepresentable — including over the wire, since `Deserialize` goes through `TryFrom<Vec<T>>`. `Quantity` is bounded 1..=99, `Cents` is a non-negative integer newtype (never `f64`), and `Email` normalises on construction so `Shopper@Example.COM ` and `shopper@example.com` cannot become two accounts.

**Parsing happens once, at the edge.** Handlers contain no validation because there is nothing left to validate: `Quantity` rejects 0 and 100 inside `Deserialize`, before any workflow runs. `an_out_of_range_quantity_never_reaches_a_workflow` proves it. Database rows get the same treatment — `crates/infra/src/rows.rs` is a flat `*Row` per query plus a `TryFrom`, so a schema drift breaks one obvious place instead of producing a domain value that violates an invariant.

**Authorization is a function parameter.** `CurrentUser` is a `FromRequestParts` extractor; a protected handler takes one, and there is no way to write that handler without it. `get_order` is scoped by user id, so another user's order is `not_found` rather than `forbidden` — no information leak, and no check to forget.

**Time and randomness are injected.** `clippy.toml` bans `chrono::Utc::now`, `Uuid::new_v4` and `rand::random` by path; the two adapters that need them carry a one-function `#[allow]` with a reason. Tests substitute a `FixedClock` and a sequential id generator, so every run is deterministic.

**Property tests, not just examples.** `build_category_tree` is total: every input category appears in the output exactly once, for every input — including self-parents, dangling parents and `a → b → a` cycles, which the obvious implementation silently drops. Cart pricing is tested for sum-equals-parts, order-independence and line-count preservation; `Order` round-trips through JSON; `Email` normalisation is idempotent.

**No mocking library.** Test doubles are hand-written trait impls and a different `DATABASE_URL`. The workflows under test are the ones that ship.

## Test suite

| Suite | Count | What it covers |
|---|---|---|
| `crates/domain` unit + property | 30 | pricing invariants, tree totality, money bounds, email normalisation, JSON round-trip |
| `crates/infra` | 3 | scrypt hash/verify, distinct salts |
| `crates/api/tests/workflows.rs` | 15 | auth, catalog, cart, checkout, **atomicity**, oversell prevention, cross-user isolation |
| `crates/api/tests/http.rs` | 10 | status-code mapping, the error envelope, boundary rejection, the full purchase journey |

## Deliberate simplifications

Called out so they are not mistaken for recommendations:

- **Runtime-checked SQL** (`sqlx::query_as`) rather than the `query_as!` macro, so the example builds from a clean checkout with no database present. Production should use the macros plus a committed `.sqlx` offline directory.
- **Idempotent DDL at boot** rather than `sqlx::migrate!` with numbered, checked-in migration files.
- **Sessions are opaque rows with no expiry.** Real sessions need a TTL and revocation.
- **SQLite**, for a single-file example. The `Database`/`CheckoutTx` ports exist precisely so swapping in Postgres is a new adapter and an edit to `main.rs`.
