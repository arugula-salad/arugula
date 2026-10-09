//! `studio` (M35, Labs): the person's studio token, its apps and the hud
//! follower links kept for them. A way into a studio box is the owner's (it
//! signs in as them), so all are `Owner`. The work is `labs/apps/routes.rs`;
//! a build without Labs has no studio routes, so its twins are never reached.

use arugula_proto::{
    api::{Empty, FollowerLinkRequest, StudioApps, StudioLoggedIn, StudioLoginRequest, StudioStatus},
    op::ops::{StudioAppsList, StudioFollow, StudioGet, StudioLogin, StudioLogout, StudioUnfollow},
};

use super::{Cx, Handle, OpError};

impl Handle for StudioGet {
    async fn handle(_: &Cx<'_>, _: (), _: Empty) -> Result<StudioStatus, OpError> {
        Ok(crate::labs::studio_status().await?)
    }
}

impl Handle for StudioLogin {
    async fn handle(_: &Cx<'_>, _: (), req: StudioLoginRequest) -> Result<StudioLoggedIn, OpError> {
        Ok(crate::labs::studio_login(req).await?)
    }
}

impl Handle for StudioLogout {
    async fn handle(_: &Cx<'_>, _: (), _: Empty) -> Result<Empty, OpError> {
        Ok(crate::labs::studio_logout().await?)
    }
}

impl Handle for StudioAppsList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<StudioApps, OpError> {
        Ok(crate::labs::studio_apps(cx.app).await?)
    }
}

impl Handle for StudioFollow {
    async fn handle(_: &Cx<'_>, app: String, req: FollowerLinkRequest) -> Result<Empty, OpError> {
        Ok(crate::labs::studio_follow(&app, Some(&req.link)).await?)
    }
}

impl Handle for StudioUnfollow {
    async fn handle(_: &Cx<'_>, app: String, _: Empty) -> Result<Empty, OpError> {
        Ok(crate::labs::studio_follow(&app, None).await?)
    }
}
