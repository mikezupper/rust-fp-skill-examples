# rust-fp-skill-examples

[![CI](https://github.com/mikezupper/rust-fp-skill-examples/actions/workflows/ci.yml/badge.svg)](https://github.com/mikezupper/rust-fp-skill-examples/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-CC_BY_4.0-lightgrey)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.88%2B_edition_2024-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![axum](https://img.shields.io/badge/axum-0.8-000000)](https://github.com/tokio-rs/axum)
[![sqlx](https://img.shields.io/badge/sqlx-0.9-4169E1)](https://github.com/transact-rs/sqlx)
[![ROP](https://img.shields.io/badge/errors-railway--oriented-orange)](https://fsharpforfunandprofit.com/rop/)
[![Tests](https://img.shields.io/badge/tests-58_passing-success)](#the-testing-story)
[![Testing](https://img.shields.io/badge/testing-property--based-blueviolet)](#the-testing-story)
[![unwrap](https://img.shields.io/badge/unwrap-denied-red)](#the-enforcement-story)
[![unsafe](https://img.shields.io/badge/unsafe-forbidden-red)](#the-enforcement-story)
[![Skill](https://img.shields.io/badge/Claude_Code-rust--fp--skill_battle--tested-d97757?logo=anthropic&logoColor=white)](https://github.com/mikezupper/rust-fp-skill)

A working application that battle-tests the [rust-fp-skill](https://github.com/mikezupper/rust-fp-skill) — a [Claude Code skill](https://code.claude.com/docs/en/skills) that steers an AI coding agent toward functional-programming rigor in Rust: railway-oriented error handling, illegal-states-unrepresentable domain modeling, parse-don't-validate boundaries, and property-based testing. This is the **proof repo**: every rule the skill states is exercised here by code that compiles, tests that pass, and lints that fail the build when violated. Every bug found building it was fed back into the skill as new guidance.

| App | What it is | Stack | Proves |
|---|---|---|---|
| [`ecommerce/`](ecommerce/) | Full commerce API — catalog, hierarchical category navigation, auth, cart, **atomic checkout**, order history | axum 0.8, sqlx 0.9 + SQLite, tokio, tracing | Domain modeling at scale, the transaction boundary on the error track, architecture enforced by cargo |

---

## Table of contents

- [Goals & motivation](#goals--motivation)
- [The skill under test](#the-skill-under-test)
- [Quick start](#quick-start)
- [The app](#the-app)
- [Configuration](#configuration)
- [The enforcement story](#the-enforcement-story)
- [The testing story](#the-testing-story)
- [Interop: it serves an existing storefront unmodified](#interop-it-serves-an-existing-storefront-unmodified)
- [What building this taught the skill](#what-building-this-taught-the-skill)
- [Repo layout](#repo-layout)

## Goals & motivation

AI coding agents write plausible Rust by default. It compiles, it passes the demo, and it is quietly wrong in the same places every time: `unwrap()` on the happy path, `Box<dyn Error>` that callers cannot match on, `anyhow` in a library, `String` errors, `f64` money, `Utc::now()` buried in business logic, validation scattered through the call graph instead of parsing once at the boundary.

A skill fixes this by encoding an opinionated engineering philosophy the agent must follow — but a skill that has never built anything is just an essay.

This repo closes that loop. Its goals:

1. **Prove the skill produces working software** — not snippets: a domain model, persistence, auth, a real transaction boundary, an HTTP surface, and a test suite that runs in CI.
2. **Battle-test the guidance.** The app was built strictly under the skill's rules. Where reality diverged from the docs — a lint that fires in a place the docs didn't anticipate, a code sample that would not compile, two reference files contradicting each other — the finding went back into the skill ([see below](#what-building-this-taught-the-skill)).
3. **Serve as a reference implementation.** If you want to see what "railway-oriented programming in Rust" actually looks like end to end — including where it stops being worth it — clone and run.

The philosophy comes from Scott Wlaschin's [F# for Fun and Profit](https://fsharpforfunandprofit.com) ([railway-oriented programming](https://fsharpforfunandprofit.com/rop/), [designing with types](https://fsharpforfunandprofit.com/series/designing-with-types/), [property-based testing](https://fsharpforfunandprofit.com/series/property-based-testing/)) and Alexis King's [*Parse, don't validate*](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/). Rust's `Result`, enums, ownership, trait system — and, crucially, **cargo itself** — are the realization.

## The skill under test

| Skill | Repo | What it mandates |
|---|---|---|
| **rust-fp-skill** | [mikezupper/rust-fp-skill](https://github.com/mikezupper/rust-fp-skill) | Typed error tracks with `thiserror` (never `Box<dyn Error>`/`anyhow` in a library); newtypes for every domain primitive; enums instead of flag soup; `TryFrom` at every boundary; effects behind traits with one wiring site; property tests for the pure core; a lint tier that denies `unwrap`/`expect`/`panic`/`indexing_slicing`/wildcard match arms; and a mandatory self-review pass |

## Quick start

Requires **Rust 1.88+** (edition 2024). No database to install — SQLite is embedded and the schema self-migrates.

```bash
git clone https://github.com/mikezupper/rust-fp-skill-examples
cd rust-fp-skill-examples/ecommerce

cargo run                                               # http://localhost:3000
cargo test --workspace                                  # 58 tests
cargo clippy --workspace --all-targets -- -D warnings   # the full deny tier
cargo tree -p domain --edges normal --depth 1           # the architecture, asserted
```

Drive it:

```bash
TOKEN=$(curl -sX POST localhost:3000/auth/register \
  -H 'content-type: application/json' \
  -d '{"email":"shopper@example.com","password":"correct horse battery"}' | jq -r .token)

curl -sX PUT localhost:3000/cart/items -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"productId":"p-headphones","quantity":50}'         # seeded stock is 3

curl -sX POST localhost:3000/orders -H "authorization: Bearer $TOKEN"
# 409 {"_tag":"InsufficientStock","available":3,"productId":"p-headphones","requested":50}
```

## The app

### `ecommerce` — the skill at production scale

Catalog, search, hierarchical category navigation, register/login, cart, checkout, order history. Real persistence (sqlx + SQLite), real auth (scrypt + bearer sessions), real transactional integrity.

| Endpoint | Auth | Errors |
|---|---|---|
| `POST /auth/register`, `POST /auth/login` | — | `EmailTaken` 409, `InvalidCredentials` 401 |
| `GET /catalog/categories` (tree), `GET /catalog/products?search=&category=`, `GET /catalog/products/{id}` | — | `ProductNotFound` 404 |
| `GET /cart`, `PUT /cart/items`, `DELETE /cart/items/{product_id}` | bearer | `Unauthorized` 401, `ProductNotFound` 404 |
| `POST /orders` (checkout), `GET /orders`, `GET /orders/{id}` | bearer | `CartEmpty` 409, `InsufficientStock` 409, `OrderNotFound` 404 |
| `GET /health` | — | — |

The centerpiece is **checkout as a workflow-owned transaction** (`crates/app/src/workflows/orders.rs`):

```
db.begin()
  read cart → reserve stock line-by-line → snapshot prices → write order → clear cart
tx.commit()
```

**Nothing in that function rolls anything back.** Every `?` returns early, which drops the boxed transaction, and sqlx's `Drop` issues a `ROLLBACK` — and because `commit` takes `Box<Self>`, a transaction that is still alive is *provably uncommitted*. Ownership and railway-oriented early exit turn out to be the same mechanism, so the failure path is correct by construction rather than by remembering to write it. This is the one thing Rust does here that a garbage-collected functional language cannot.

The `app` crate owns that transaction **without knowing what a database is**: `Database::begin` returns a `Box<dyn CheckoutTx>`, so `sqlx::Error` never reaches a workflow signature and swapping SQLite for Postgres is a new adapter plus an edit to `main.rs`.

Other highlights:

- **Illegal states don't compile.** `Order` holds a `NonEmpty<OrderLine>`, so an order with zero lines is unrepresentable — including over the wire, since `Deserialize` routes through `TryFrom<Vec<T>>`. `Quantity` is bounded 1..=99; `Cents` is a non-negative integer newtype (never `f64`); `Email` normalises on construction, so `Shopper@Example.COM ` and `shopper@example.com` cannot become two accounts.
- **Parsing happens once, at the edge.** Handlers contain no validation because nothing is left to validate — `Quantity` rejects `0` and `100` inside `Deserialize`, before any workflow runs, and a test asserts it. Database rows get the same treatment: `crates/infra/src/rows.rs` is a flat `*Row` per query plus a `TryFrom`, so schema drift breaks one obvious place instead of producing a domain value that violates an invariant.
- **Authorization is a function parameter.** `CurrentUser` is a `FromRequestParts` extractor; a protected handler takes one, and there is no way to write that handler without it. `get_order` is scoped by user id, so another user's order is `OrderNotFound`, not `Forbidden` — no information leak, and no check to forget.
- **Time and randomness are injected.** `clippy.toml` bans `chrono::Utc::now`, `Uuid::new_v4` and `rand::random` **by path**; the two adapters that legitimately need them carry a one-function `#[allow]` with a `reason`. Tests substitute a `FixedClock` and a sequential id generator, so every run is deterministic.
- **Anti-enumeration auth**: `InvalidCredentials` is returned identically for an unknown email and a wrong password. Passwords are a `Password` newtype with a hand-written redacting `Debug` and no `Serialize` impl at all — it can arrive in a request body and can never leave in a response.
- **Order lines snapshot name and price at purchase time**, so a later catalog edit cannot rewrite a customer's receipt.
- **scrypt runs on `spawn_blocking`** — hashing is deliberately CPU- and memory-hard, which is exactly what must never occupy an async executor thread.
- **Errors cross the wire as a tagged union**: `{"_tag":"InsufficientStock","productId":…,"requested":50,"available":3}`. A client switches on the tag and reads typed fields; the structured `available` count travels from the SQL statement that refused to reserve stock, through two error enums, into the JSON body without ever being flattened into prose.

## Configuration

Everything configurable is an environment variable, read **once** in `main.rs`. Nothing deeper in the codebase touches the environment. Defaults work out of the box.

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `3000` | HTTP port |
| `DATABASE_URL` | `sqlite://ecommerce.db` | SQLite file; `sqlite::memory:` for ephemeral |
| `RUST_LOG` | `info` | `tracing-subscriber` `EnvFilter` directive |

The database self-migrates and self-seeds idempotently on boot. Delete the file to reset. Seeded catalog: categories `electronics/laptops/audio/books`, five products with fixed ids (`p-laptop-pro` stock 5, `p-headphones` stock 3, …) so the docs and the tests agree.

## The enforcement story

The skill's central claim is that in Rust, architecture is not a convention you lint for — it is a property of the dependency graph that cargo resolves. This repo is where that claim is falsifiable:

```console
$ cargo tree -p domain --edges normal --depth 1
domain v0.1.0
├── chrono v0.4.45      # WITHOUT the "clock" feature — Utc::now() does not exist here
├── nutype v0.7.0
├── serde v1.0.229
└── thiserror v2.0.19
```

No tokio. No sqlx. No axum. A workflow cannot reach into infrastructure because the crate it would import is not a dependency — `use sqlx::…` in `domain` is a *resolution error*, not a review comment. CI asserts it on every push.

Each of these is a build failure here, not a guideline:

| Rule | Enforced by |
|---|---|
| The domain is pure | cargo — the dependency is not in `Cargo.toml` |
| No `unwrap`/`expect`/`panic!`/slice indexing | `[workspace.lints]` deny tier |
| No `Utc::now()`, `Uuid::new_v4()`, `rand::random()` outside their adapters | `clippy.toml` `disallowed-methods` (ban by full path) |
| Adding an enum variant breaks every match on it | `clippy::wildcard_enum_match_arm` |
| An invalid domain value cannot be deserialized | `nutype` validators inside `Deserialize` |
| An order with zero lines cannot exist | `NonEmpty<T>` with a fallible `TryFrom` |
| A failed checkout leaves no partial state | `Drop` on the transaction + `?`'s early exit |
| Money is never a float | `Cents(u32)` newtype + `clippy::float_arithmetic` |
| `unsafe` cannot be reintroduced downstream | `unsafe_code = "forbid"` (not `deny`) |

## The testing story

58 tests. Each tier uses the cheapest tool that gives real confidence.

| Tier | Count | Tool | What it catches |
|---|---|---|---|
| Pure domain properties | 30 | `proptest` | Invariants over *all* inputs — pricing, money bounds, tree totality, JSON round-trip, email idempotence |
| Adapter units | 3 | `#[tokio::test]` | scrypt hash/verify, distinct salts per hash |
| Workflows | 15 | `#[tokio::test]` + in-memory SQLite | Auth, catalog, cart, **checkout atomicity**, oversell prevention, cross-user isolation |
| HTTP | 10 | axum on port 0 + `reqwest` | Status-code mapping, the error envelope, boundary rejection, the full purchase journey |

Two are worth calling out.

**The atomicity proof.** `insufficient_stock_rolls_back_the_entire_checkout` puts two lines in a cart: the first reserves successfully, the second cannot. It asserts not merely that an error came back, but that the *successful* first reservation was undone, the cart is untouched, and no order was written. That is the assertion that proves the `Drop`-based rollback actually fires rather than merely being described in a comment.

**The totality property.** `build_category_tree` must be total: every input category appears in the output exactly once, for *every* input. The obvious implementation silently drops categories caught in an `a → b → a` cycle — it passes every example test and fails this property in seconds. Self-parents, dangling parents, and cycles are all generated by a strategy over a deliberately small id alphabet so collisions are likely.

No mocking library appears anywhere. Test doubles are hand-written trait impls (`FixedClock`, `SeqIdGen`) and a different `DATABASE_URL` — the workflows under test are byte-for-byte the ones that ship.

## Interop: it serves an existing storefront unmodified

The [Lit SSR storefront](https://github.com/mikezupper/effect-fp-skill-examples/tree/main/ecommerce-ui) from the sibling proof repo — written against a completely different backend, in a different language — runs against this Rust API **with zero changes to the UI**. Point it at this server and the catalog renders, auth works, the cart drawer prices correctly, and the out-of-stock path shows the right message:

```bash
# terminal 1
cd ecommerce && PORT=3001 cargo run

# terminal 2 — the storefront from the sibling repo, unmodified
cd ../effect-fp-skill-examples/ecommerce-ui && npm run dev   # :5173
```

Getting there was itself a lesson in what "wire contract" means, and it is recorded below.

## What building this taught the skill

The feedback loop is the point of this repo. Findings that became skill guidance:

| Finding | Where it went |
|---|---|
| `panic = "abort"` in the release profile makes `tower_http::CatchPanicLayer` **inert** — two reference files were recommending mutually incompatible things | `references/scaffold.md` no longer sets `panic`, and both files now explain the unwind-vs-abort trade |
| `clippy::expect_used` fires inside `tests/*.rs` integration files, because each is its own crate and `cfg(test)` is **not** set for them — `#![cfg_attr(test, allow(…))]` silently does nothing there | `references/scaffold.md`: relaxations must be a crate-root `#![allow]` in integration tests |
| `i32::from(u32)` does not exist — a persistence sample would not have compiled | `references/database.md`: `i32::try_from(…)?` |
| A newtype's private field cannot be reached with `.0` from another crate; newtypes need explicit borrowing accessors | `references/database.md` |
| A `_ =>` arm had slipped into a reference file whose own rule bans them over domain enums | `references/database.md`: named or-pattern, so a new variant breaks the build |
| `nutype`'s generated `Deserialize` routes through the validating constructor — the one legitimate exception to "never derive `Deserialize` on a domain type" | `references/domain-types.md`, `references/boundaries.md`, `references/code-review.md` |
| Deriving `Deserialize` directly onto a `Config` struct contradicted the parse-don't-validate rule the same skill states elsewhere | `references/di-context.md` now defers to `references/boundaries.md` |
| A recursive tree walk cannot hold an immutable borrow of a `visited` set in a closure while the recursive call needs it mutably (E0500) — the candidate set must be materialised first | shaped the `build_category_tree` reference implementation |
| Serialization casing **is** part of the wire contract: an integration test failed the moment the request payload shape changed, which is exactly what should happen | the HTTP suite pins request and response shapes, not just status codes |

Each row was a real, reproduced failure — a compiler error, a failing test, or a lint — not a hypothetical. The wildcard-match-arm rule caught the author of this repo twice while writing it.

## Repo layout

```
rust-fp-skill-examples/
├── ecommerce/                  # the app — see its README
│   ├── Cargo.toml              # [workspace.lints] — the deny tier
│   ├── clippy.toml             # disallowed-methods: the path-level bans
│   ├── deny.toml               # cargo-deny: licenses, advisories, banned crates
│   ├── rust-toolchain.toml
│   └── crates/
│       ├── domain/             # pure: newtypes, enums, pricing, the tree builder
│       ├── app/                # workflows + ports (traits); owns the transaction boundary
│       ├── infra/              # adapters: sqlx repos, scrypt, clock, id generator
│       └── api/                # axum router, DTOs, error→HTTP mapping, the one wiring site
└── .github/workflows/ci.yml    # fmt, clippy -D warnings, tests, cargo-deny, purity assertion
```

| CI step | Does |
|---|---|
| Format | `cargo fmt --all -- --check` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Test | `cargo test --workspace` |
| Supply chain | `cargo deny check` — licenses, advisories, banned crates |
| Purity assertion | fails the build if `domain` gains a dependency on tokio, sqlx, axum, reqwest, or hyper |

## License

Text, markup, and code licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) © 2026 Mike Zupper. The engineering philosophy credits [Scott Wlaschin](https://fsharpforfunandprofit.com) and [Alexis King](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/); neither endorses this repo.
