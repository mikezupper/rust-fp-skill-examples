//! Catalog browsing. Read-only, so no transaction and no error track beyond
//! "that product does not exist".

use domain::{CategoryTree, Product, ProductId, build_category_tree};

use crate::ctx::Ctx;
use crate::errors::{ProductError, RepoError};
use crate::ports::ProductFilter;

#[derive(Debug, Clone, Default)]
pub struct BrowseQuery {
    pub search: Option<String>,
    pub category_slug: Option<String>,
}

#[tracing::instrument(skip_all, err)]
pub async fn category_navigation(ctx: &Ctx) -> Result<Vec<CategoryTree>, RepoError> {
    let categories = ctx.products.list_categories().await?;
    // The interesting work is a pure function over the fetched rows. It is total —
    // cycles and dangling parents included — and property-tested in `domain`.
    Ok(build_category_tree(&categories))
}

#[tracing::instrument(skip_all, err)]
pub async fn browse_products(ctx: &Ctx, query: &BrowseQuery) -> Result<Vec<Product>, RepoError> {
    // An unknown category slug yields an empty list rather than a 404: browsing is
    // forgiving, and a stale bookmark should not look like a broken site. That is a
    // product decision, and it belongs here rather than in the repository.
    let category_id = match &query.category_slug {
        None => None,
        Some(slug) => match ctx.products.find_category_by_slug(slug).await? {
            Some(category) => Some(category.id),
            None => return Ok(Vec::new()),
        },
    };

    ctx.products
        .list(&ProductFilter {
            search: query.search.clone(),
            category_id,
        })
        .await
}

#[tracing::instrument(skip(ctx), fields(product_id = %id), err)]
pub async fn get_product(ctx: &Ctx, id: &ProductId) -> Result<Product, ProductError> {
    // Here absence *is* an error — the caller asked for a specific product by id.
    // Same repository method, different decision, made where the context exists.
    ctx.products
        .find_by_id(id)
        .await?
        .ok_or_else(|| ProductError::NotFound {
            product_id: id.clone(),
        })
}
