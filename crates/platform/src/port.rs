//! The platform's side of `PlatformPort` (the trait is
//! `arugula-platform-api`'s, #314, so the hub needn't build the platform):
//! each call, in process, answered by the feature it's about.

use futures_util::future::BoxFuture;

use arugula_control_wire::{
    forge::Standing,
    hosted::{SetupPlan, SetupReport},
    run_logs::{Upload, UploadGiven, Uploaded},
    transcripts::TranscriptPost,
    work::{
        ArtifactGiven, PushAsk, PushGiven, Report, Reported, SecretsAsk, SecretsGiven, SessionIssuesAsk,
        SessionIssuesGiven, SessionReport, Work,
    },
};

use crate::{
    Platform, billing,
    boxes::{BoxMember, BoxPost, BoxPosted},
    db,
    forge::{
        self, GithubUser, TokenAnswer,
        send::{Outcome, UserToken},
    },
    work::HostedMachine,
};
pub use arugula_platform_api::port::{Archived, Deleted, PlatformPort, PublicPreview, TurnAnswer};

/// In process. Billing (M139) answers `active_plans`, `roster_changed`
/// and its part of `account_deleted`; `delete_blockers` has nothing to say
/// since Cloud's Managoat side went (M189, M190); chat (M138) `chat_on` and its part of
/// `account_deleted`; projects and grants, the forge mirror (M162) and
/// native issues (M194), their part of `account_deleted`; the GitHub App
/// (M161) `forge_standing` and `github_token`; sends (M168) `send_draft`;
/// runs (M210), deploys (M217) and what starts runs (M214), their part of
/// `account_deleted`; hosted workspaces (M182) `hosted_wake`; hosted
/// inference (M185) `hosted_turn` and `hosted_turn_end`; hosted machines'
/// runs (M212) `hosted_work` and `hosted_report`, and their logs (M213)
/// `hosted_upload` and `hosted_uploaded`; hosted workspaces' setup (M183)
/// `hosted_plan` and `hosted_setup`; hosted workspaces' agents (M184)
/// `machine_drives` and `hosted_transcript`; public previews (M187)
/// `public_preview` and `public_ports`; deploys' runs (M219)
/// `hosted_secrets`; hud boxes (M237) `box_members` and `box_post`.
impl PlatformPort for Platform {
    fn delete_blockers<'a>(&'a self, account: &'a str, start: bool) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        let _ = (account, start);
        Box::pin(async { Ok(vec![]) })
    }

    fn account_deleted<'a>(&'a self, deleted: &'a Deleted) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            // The connection never lives across the bucket's await.
            let (rooms, run_files) = {
                let mut c = self.db.c();
                let tx = c.transaction()?;
                db::billing::erase_account(&tx, deleted)?;
                // Before the projects go: their hosted workspaces are deleted.
                db::hosted::erase_account(&tx, deleted, arugula_e2e::now_ms())?;
                db::inference::erase_account(&tx, deleted)?;
                db::clients::erase_account(&tx, deleted)?;
                db::hosted_transcripts::erase_account(&tx, deleted)?;
                db::projects::erase_account(&tx, deleted)?;
                db::forge::erase_account(&tx, deleted, arugula_e2e::now_ms())?;
                db::issues::erase_account(&tx, deleted)?;
                db::boards::erase_account(&tx, deleted)?;
                db::project_docs::erase_account(&tx, deleted)?;
                // Before the runs go: their files in the run-log bucket.
                let run_files = db::run_logs::erase_account(&tx)?;
                db::runs::erase_account(&tx, deleted)?;
                db::sessions::erase_account(&tx, deleted, arugula_e2e::now_ms())?;
                db::deploys::erase_account(&tx, deleted)?;
                db::redone::erase_account(&tx, deleted)?;
                db::triggers::erase_account(&tx, deleted)?;
                let rooms = crate::chat::erase_account(&tx, deleted)?;
                tx.commit()?;
                (rooms, run_files)
            };
            crate::chat::account_gone(self, &rooms);
            crate::run_logs::delete_files(self, &run_files, arugula_e2e::now_ms()).await;
            Ok(())
        })
    }

    fn active_plans<'a>(&'a self, owners: &'a [String]) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(async move { self.db.active_plans(owners) })
    }

    fn chat_on<'a>(&'a self, account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(crate::chat::chat_on(self, account))
    }

    fn roster_changed<'a>(&'a self, team: &'a str) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            // What's kept of the team goes now, not at the next feed tick:
            // the roster's route answers after this, so its caller reads
            // the new roster.
            if self.cache.team_changed(team) {
                self.chat.changed();
            }
            billing::sync_seats(self, team).await;
            Ok(())
        })
    }

    fn forge_standing<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repos: &'a [String],
        fresh: bool,
    ) -> BoxFuture<'a, anyhow::Result<Vec<Standing>>> {
        Box::pin(async move { Ok(forge::standing(self, account, github, repos, fresh).await) })
    }

    fn github_token<'a>(
        &'a self,
        account: &'a str,
        github: Option<&'a GithubUser>,
        repo: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<TokenAnswer>> {
        Box::pin(forge::token(self, account, github, repo))
    }

    fn send_draft<'a>(
        &'a self,
        account: &'a str,
        draft: &'a str,
        token: Option<UserToken>,
    ) -> BoxFuture<'a, anyhow::Result<Outcome>> {
        Box::pin(async move {
            let how = match token {
                Some(t) => forge::send::How::Token(t),
                None => forge::send::How::Machine,
            };
            // A session's PR (M259) comes back through the same callback.
            if draft.starts_with(crate::sessions::pr::PREFIX) {
                return Ok(match crate::sessions::pr::send(self, account, draft, how).await {
                    Ok(s) => forge::send::Outcome { pr: s.pr.url, ..Default::default() },
                    Err(e) => forge::send::Outcome { refused: Some(e.1), ..Default::default() },
                });
            }
            Ok(forge::send::outcome(forge::send::send(self, account, draft, how).await))
        })
    }

    fn hosted_wake<'a>(
        &'a self,
        project: &'a str,
        workspace: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<arugula_control_wire::hosted::WakeAnswer>> {
        Box::pin(crate::hosted::wake::wake(self, project, workspace))
    }

    fn hosted_turn<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        agent: &'a str,
        session: Option<&'a str>,
    ) -> BoxFuture<'a, anyhow::Result<TurnAnswer>> {
        Box::pin(crate::hosted::inference::turn(self, workspace, machine, agent, session))
    }

    fn hosted_turn_end<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        turn: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(crate::hosted::inference::turn_end(self, workspace, machine, turn))
    }

    fn hosted_work<'a>(
        &'a self,
        machine: &'a HostedMachine,
        takes: &'a [String],
    ) -> BoxFuture<'a, anyhow::Result<Work>> {
        Box::pin(crate::work::work(self, machine, takes))
    }

    fn hosted_report<'a>(
        &'a self,
        machine: &'a HostedMachine,
        report: &'a Report,
    ) -> BoxFuture<'a, anyhow::Result<Reported>> {
        Box::pin(crate::work::report(self, machine, report))
    }

    fn hosted_session_report<'a>(
        &'a self,
        machine: &'a HostedMachine,
        report: &'a SessionReport,
    ) -> BoxFuture<'a, anyhow::Result<Reported>> {
        Box::pin(crate::session_live::report(self, machine, report))
    }

    fn hosted_session_push<'a>(
        &'a self,
        machine: &'a HostedMachine,
        ask: &'a PushAsk,
    ) -> BoxFuture<'a, anyhow::Result<PushGiven>> {
        Box::pin(crate::sessions::push::token(self, machine, ask))
    }

    fn hosted_session_issues<'a>(
        &'a self,
        machine: &'a HostedMachine,
        ask: &'a SessionIssuesAsk,
    ) -> BoxFuture<'a, anyhow::Result<SessionIssuesGiven>> {
        Box::pin(crate::sessions::issues::answer(self, machine, ask))
    }

    fn hosted_secrets<'a>(
        &'a self,
        machine: &'a HostedMachine,
        ask: &'a SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<SecretsGiven>> {
        crate::features::deploys().secrets(self, machine, ask)
    }

    fn hosted_artifact<'a>(
        &'a self,
        machine: &'a HostedMachine,
        ask: &'a SecretsAsk,
    ) -> BoxFuture<'a, anyhow::Result<ArtifactGiven>> {
        crate::features::deploys().artifact(self, machine, ask)
    }

    fn hosted_plan<'a>(
        &'a self,
        project: &'a str,
        workspace: &'a str,
        machine: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Option<SetupPlan>>> {
        Box::pin(crate::hosted::setup::plan(self, project, workspace, machine))
    }

    fn hosted_setup<'a>(
        &'a self,
        project: &'a str,
        workspace: &'a str,
        machine: &'a str,
        report: &'a SetupReport,
    ) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(async move { crate::hosted::setup::report(self, project, workspace, machine, report) })
    }

    fn machine_drives<'a>(&'a self, daemon: &'a str, account: &'a str) -> BoxFuture<'a, anyhow::Result<bool>> {
        Box::pin(crate::grants::can_drive_cloud(self, daemon, account))
    }

    fn hosted_transcript<'a>(
        &'a self,
        workspace: &'a str,
        machine: &'a str,
        transcript: &'a TranscriptPost,
    ) -> BoxFuture<'a, anyhow::Result<Archived>> {
        Box::pin(crate::hosted::transcripts::archive(self, workspace, machine, transcript.clone()))
    }

    fn public_preview<'a>(&'a self, id: &'a str) -> BoxFuture<'a, anyhow::Result<Option<PublicPreview>>> {
        Box::pin(async move { crate::hosted::previews::lookup(self, id) })
    }

    fn public_ports<'a>(&'a self, workspace: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<(u16, String)>>> {
        Box::pin(async move { crate::hosted::previews::public_ports(self, workspace) })
    }

    fn hosted_upload<'a>(
        &'a self,
        machine: &'a HostedMachine,
        upload: &'a Upload,
    ) -> BoxFuture<'a, anyhow::Result<UploadGiven>> {
        Box::pin(crate::run_logs::upload(self, machine, upload))
    }

    fn hosted_uploaded<'a>(
        &'a self,
        machine: &'a HostedMachine,
        upload: &'a Upload,
    ) -> BoxFuture<'a, anyhow::Result<Uploaded>> {
        Box::pin(crate::run_logs::uploaded(self, machine, upload))
    }

    fn box_members<'a>(&'a self, project: &'a str) -> BoxFuture<'a, anyhow::Result<Option<Vec<BoxMember>>>> {
        Box::pin(crate::boxes::members(self, project))
    }

    fn box_post<'a>(&'a self, post: &'a BoxPost) -> BoxFuture<'a, anyhow::Result<BoxPosted>> {
        Box::pin(crate::boxes::post(self, post))
    }
}
