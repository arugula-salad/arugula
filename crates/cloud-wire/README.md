# arugula-cloud-wire

What Arugula control's `/api/cloud/…` routes take and answer with: Cloud
workspaces and their settings, tabs, past tabs, agents, the models a tab may
run on, and past cloud runs. The platform (`arugula-platform`) serves them,
and the web client's copy is generated from them (`web/src/proto.gen.ts`,
`just proto-ts`).

Depends on serde only. Not `arugula-control-wire`: some of these types are
strict on purpose (a Cloud workspace's settings or an agent with a field
control doesn't know is refused), which that crate's rules don't allow. See
`src/lib.rs` for this crate's two rules.
