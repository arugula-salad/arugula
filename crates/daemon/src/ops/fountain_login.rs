//! `fountain.login*` (#942): this daemon's Managoat sign-in (`arugula
//! managoat login` and `logout`). Each is the owner's: the key it keeps
//! signs in as them. The work is `labs/fountain/account.rs`; a build without
//! Labs has no such routes, so its twins are never reached.

use arugula_proto::{
    api::{Empty, FountainLogin, FountainLoginRequest},
    op::ops::{FountainLoginGet, FountainLoginStart, FountainLogout},
};

use super::{Cx, Handle, OpError};

impl Handle for FountainLoginGet {
    async fn handle(_: &Cx<'_>, _: (), _: Empty) -> Result<FountainLogin, OpError> {
        Ok(crate::labs::fountain_login().await?)
    }
}

impl Handle for FountainLoginStart {
    async fn handle(_: &Cx<'_>, _: (), req: FountainLoginRequest) -> Result<FountainLogin, OpError> {
        Ok(crate::labs::fountain_login_start(req).await?)
    }
}

impl Handle for FountainLogout {
    async fn handle(_: &Cx<'_>, _: (), _: Empty) -> Result<FountainLogin, OpError> {
        Ok(crate::labs::fountain_logout().await?)
    }
}
