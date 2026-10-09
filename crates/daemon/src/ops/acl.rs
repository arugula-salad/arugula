//! `acl` and `links`: who may do what in each session, and a read-only link
//! to one (`arugula access`, the People page). All are the owner's (`serve`
//! checks that before the handler runs); the logic stays in `acl::api`.

use arugula_proto::{
    api::{AclSetRequest, Empty, LinkRequest},
    op::ops::{AclGet, AclSet, LinkMint},
};
use serde_json::Value;

use super::{Cx, Handle, OpError};

impl Handle for AclGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Value, OpError> {
        Ok(crate::acl::api::list(cx.app))
    }
}

impl Handle for AclSet {
    async fn handle(cx: &Cx<'_>, _: (), req: AclSetRequest) -> Result<Value, OpError> {
        Ok(crate::acl::api::set(cx.app, req).await?)
    }
}

impl Handle for LinkMint {
    async fn handle(cx: &Cx<'_>, _: (), req: LinkRequest) -> Result<Value, OpError> {
        Ok(crate::acl::api::link(cx.app, req).await?)
    }
}
