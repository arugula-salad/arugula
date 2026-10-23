//! The platform page's "Linked products" (M229): the products whose own
//! users are linked to the reader's account by numeric GitHub id, and the
//! button that ends one. Links are identity, kept in the hub beside its
//! issuer's clients (`crates/control/src/links.rs`), so both calls go
//! through [`HubPort`](crate::hub::HubPort). The hub audits an unlink as the
//! person's, and doesn't call the product: the product sees it on its next
//! call to the link API.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::{Value, json};

use crate::{Platform, auth::Person};

/// `GET`: the reader's linked products.
pub const LINKED_PRODUCTS: &str = "/api/linked-products";
/// `POST`: end one.
pub const UNLINK_PRODUCT: &str = "/api/linked-products/{client}/{user}/unlink";

type R = Result<Json<Value>, ApiError>;

/// `GET /api/linked-products`: `{ links: [{ client, product, user, name?, linked }] }`.
pub async fn list(State(app): State<Arc<Platform>>, s: Person) -> R {
    let links = app.hub.product_links(&s.account).await?;
    Ok(Json(json!({ "links": links })))
}

/// `POST /api/linked-products/{client}/{user}/unlink`: end one of the
/// reader's links. 404 when it isn't one of theirs.
pub async fn unlink(State(app): State<Arc<Platform>>, s: Person, Path((client, user)): Path<(String, String)>) -> R {
    if !app.hub.unlink_product(&s.account, &client, &user).await? {
        return Err(err(StatusCode::NOT_FOUND, "no such linked product"));
    }
    Ok(Json(json!({ "unlinked": true })))
}
