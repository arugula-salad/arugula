//! The contract's JSON Schema (#219): every type here, as
//! `contract/schema/<file>.json`, checked in so that a change to a wire type
//! is a schema diff in its pull request. One file per module (`lib.json` for
//! the crate's root, `internal.json` for the hub and the platform's port),
//! each type and what it carries (`arugula-e2e`'s certificates, rosters and
//! grants) under `$defs`, by name.
//!
//! The schemas are what a reader accepts (schemars' deserialize contract):
//! a field read with `#[serde(default)]` isn't required, and none of them
//! refuses an unknown field. `just contract-schema` writes them; without
//! `write` it checks they're current, as CI does (`.github/actions/lint`).
//!
//! A new type with a serde derive gets
//! `#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]` beside it
//! and a place in [`files`]: `every_wire_type_is_in_the_schema` fails
//! without either.

use std::{collections::BTreeMap, path::PathBuf};

use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde_json::{Map, Value, json};

use crate::*;

/// Where the schema is checked in.
fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contract/schema")
}

/// One schema file: its name, and its types' names and schemas, in the
/// order they're listed.
type Roots = Vec<(String, Value)>;

fn root<T: JsonSchema>(g: &mut SchemaGenerator, out: &mut Roots) {
    out.push((T::schema_name().into_owned(), g.subschema_for::<T>().to_value()));
}

macro_rules! files {
    ($($file:literal => [$($t:ty),* $(,)?],)*) => {
        vec![$(($file, &[$(stringify!($t)),*] as &[&str], (|g: &mut SchemaGenerator, out: &mut Roots| {
            $(root::<$t>(g, out);)*
        }) as fn(&mut SchemaGenerator, &mut Roots)),)*]
    };
}

/// Every schema file, and the types it's made from: each module's public
/// types with a serde derive. A generic one (`forge_ops::Answer<T>`) isn't
/// listed: it has no schema of its own until it's given a type.
#[allow(clippy::type_complexity)]
fn files() -> Vec<(&'static str, &'static [&'static str], fn(&mut SchemaGenerator, &mut Roots))> {
    files! {
        "activity" => [
            activity::Activity, activity::MachineActivity,
        ],
        "boards" => [
            boards::Pointer, boards::Status, boards::EntryKind, boards::ZoneKind, boards::Board, boards::BoardProgress, boards::EditBoard, boards::CloseBoard, boards::BoardList, boards::BoardReach, boards::BoardReachProject, boards::NewBoard, boards::LiveState, boards::Live, boards::BlockWord, boards::Diffstat, boards::BlockState, boards::PutBlockState, boards::Lane, boards::AttachRole, boards::Attachment, boards::ProofBy, boards::Proof, boards::AttachRow, boards::Mark, boards::Group, boards::Ask, boards::AnsweredAsk, boards::NewAsk, boards::AnswerAsk, boards::RelationKind, boards::RelationSource, boards::NewRelation, boards::Relation, boards::RelationDirection, boards::RowRelation, boards::Row, boards::Zone, boards::BoardView, boards::PutEntry, boards::MoveEntry, boards::WorkQuery, boards::WorkItem, boards::WorkItems, boards::AttachBlock, boards::BlockOn, boards::BlockAttached,
        ],
        "chat" => [
            chat::ChatKind, chat::ChatTargetKind, chat::ChatPerson, chat::ChatRoom, chat::ChatRooms, chat::ChatRoomDetail, chat::ChatRoomAgent, chat::ChatAgent, chat::ChatPaneRef, chat::ChatPane, chat::ChatMessage, chat::ChatFile, chat::ChatFileKind, chat::ChatUploadQuery, chat::ChatSystem, chat::ChatTopicSet, chat::ChatPin, chat::ChatPins, chat::ChatThread, chat::ChatReaction, chat::ChatReactor, chat::ChatReact, chat::ChatMessagesQuery, chat::ChatMessages, chat::ChatNewChannel, chat::ChatNewDm, chat::ChatChannel, chat::ChatChannels, chat::ChatAddMembers, chat::ChatOpenTarget, chat::ChatRoomChange, chat::ChatPost, chat::ChatPushWhy, chat::ChatPushed, chat::ChatPosted, chat::ChatEdit, chat::ChatRead, chat::ChatUnread, chat::ChatSave, chat::ChatSavedDone, chat::ChatSavedItem, chat::ChatSaved, chat::ChatThreadRead, chat::ChatFollow, chat::ChatThreadItem, chat::ChatThreads, chat::ChatMute, chat::ChatKeywords, chat::ChatTeamSettings, chat::NotifyLevel, chat::ChatLevel, chat::QuietHours, chat::NotifyPrefs, chat::NotifySet, chat::ChatSearchQuery, chat::ChatHit, chat::ChatFound, chat::ChatOpen, chat::PresenceState, chat::PresenceMode, chat::LivePresence, chat::ChatBeat, chat::ChatPresent, chat::ChatPresence, chat::ChatLive, chat::ChatTyping, chat::ChatReaders, chat::ChatReaderList, chat::ChatReader, chat::ChatReadersStored, chat::AgentRooms, chat::AgentRoom, chat::AgentReach, chat::AgentMessagesQuery, chat::AgentMessages,
        ],
        "chat_sidebar" => [
            chat_sidebar::ChatSort, chat_sidebar::ChatSection, chat_sidebar::ChatSidebar,
        ],
        "deploys" => [
            deploys::DeploySource, deploys::DeployPlan,
        ],
        "forge" => [
            forge::ForgeWatch, forge::Standing, forge::ForgeWatching, forge::Poke, forge::ForgePoke, forge::GithubTokenRequest, forge::GithubToken, forge::Ops, forge::OpAsk, forge::OpDone, forge::DaemonOp, forge::ForgeSend, forge::OpAnswer,
        ],
        "forge_ops" => [
            forge_ops::Call, forge_ops::Op, forge_ops::ItemRef, forge_ops::ItemQuery, forge_ops::DraftQuery, forge_ops::InboxQuery, forge_ops::InboxMarkOn, forge_ops::DraftRef, forge_ops::CommentOn, forge_ops::ReviewOf, forge_ops::NewIssue, forge_ops::ItemView, forge_ops::Sent,
        ],
        "hosted" => [
            hosted::PublicPorts, hosted::PublicPort, hosted::EnrollAsk, hosted::EnrollGiven, hosted::Enrolled, hosted::EnrollRequest, hosted::Enrollment, hosted::DelegationPost, hosted::RevokePost, hosted::WakeAnswer, hosted::WakeCode, hosted::SetupPlan, hosted::SetupRepo, hosted::SetupStatus, hosted::SetupReport,
        ],
        "internal" => [
            internal::ResolveArgs, internal::IdsArgs, internal::AccountArgs, internal::WordArgs, internal::ReachArgs, internal::PushArgs, internal::OwnersArgs, internal::MetersArgs, internal::ChangesArgs, internal::DriveArgs, internal::TeamArgs, internal::BlockersArgs, internal::DeletedArgs, internal::NoArgs, internal::PokesArgs, internal::StandingArgs, internal::TokenArgs, internal::GithubIdArgs, internal::SendAuthArgs, internal::WorkspacesArgs, internal::DelegationArgs, internal::SignedArgs, internal::OpArgs, internal::SendArgs, internal::WakeArgs, internal::TurnArgs, internal::WorkArgs, internal::ReportArgs, internal::SessionReportArgs, internal::SessionPushArgs, internal::SessionIssuesArgs, internal::SecretsArgs, internal::HostedArgs, internal::ProjectArgs, internal::DrivesArgs, internal::TranscriptArgs, internal::PreviewArgs, internal::PasskeyArgs, internal::ProductUserArgs, internal::UnlinkArgs, internal::AccessArgs, internal::UploadArgs, internal::GithubUser, internal::LinkedAccount, internal::ProductLink, internal::PasskeyAssertion, internal::PasskeyVerdict, internal::DeviceTrust, internal::PushOptions, internal::Account, internal::Resolved, internal::Team, internal::Member, internal::Daemon, internal::Pushed, internal::Meter, internal::ChangePage, internal::Change, internal::ChangeKind, internal::Deleted, internal::TokenAnswer, internal::UserToken, internal::Outcome, internal::TurnAnswer, internal::Archived, internal::PublicPreview, internal::HostedMachine, internal::BoxMember, internal::BoxPost, internal::BoxPosted, internal::Principal, internal::Vouch, internal::NotVouched, internal::Credential,
        ],
        "issues" => [
            issues::AgentIssueHomeQuery, issues::IssueGithub, issues::Issue, issues::IssueQuery, issues::IssueList, issues::NewIssue, issues::IssueListEdit, issues::IssuePatch, issues::IssueEvent, issues::IssuePatched, issues::IssueEvents, issues::IssueHome, issues::IssueHomeChange, issues::IssueExportQuery, issues::IssueComment, issues::ExportedIssue, issues::ExportedPerson, issues::IssueExport,
        ],
        "join" => [
            join::JoinRequest, join::CliJoinRequest, join::JoinProof, join::JoinStarted, join::JoinPoll, join::PollQuery, join::JoinTeam,
        ],
        "lib" => [
            ControlInfo, ControlAuth, ControlCliJoin, ControlJoinSecret, ControlWhere, ControlDocs, ControlJump, GuestJump, Ack, RemovedAnswer, RemovedAt, DialQuery,
        ],
        "project_docs" => [
            project_docs::Doc, project_docs::Section, project_docs::DocView, project_docs::DocList, project_docs::NewSection, project_docs::NewDoc, project_docs::DocWrite,
        ],
        "projects" => [
            projects::Projects, projects::Me, projects::Project, projects::ProjectTeam, projects::ProjectMember, projects::LinkedRepos, projects::LinkedRepo, projects::RepoSettings, projects::LinkRepo, projects::TrackerConnections, projects::TrackerConnection, projects::ConnectTracker, projects::TrackerSettings, projects::TrackerClose, projects::TrackerWrite, projects::TrackerWriteDone, projects::TrackerPendingWrite, projects::TrackerPendingWrites, projects::TrackerReach, projects::TrackerReachProject, projects::ForgeItems, projects::WithheldRepo, projects::ForgeItem, projects::ForgeChecks, projects::ForgeCheck, projects::ForgeEvents, projects::ForgeEvent, projects::ForgeContent, projects::ForgeText, projects::ForgeRepo, projects::NewDraft, projects::DraftCreated, projects::DraftDiscarded, projects::ForgeDrafts, projects::ForgeDraft, projects::Inbox, projects::InboxEntry, projects::InboxMark, projects::InboxMarked, projects::InboxShadow, projects::ShadowBlock, projects::ShadowCompared, projects::SendDraft, projects::DraftSent, projects::MachineSending,
        ],
        "push" => [
            push::PushSubs, push::PushRequest, push::PushAnswer,
        ],
        "run_logs" => [
            run_logs::Upload, run_logs::UploadGiven, run_logs::Uploaded, run_logs::ArtifactLink, run_logs::RunArtifacts,
        ],
        "runs" => [
            runs::RunStart, runs::RunRevoke, runs::RunListQuery, runs::RunQuery, runs::RunLogQuery, runs::AgentRunStart, runs::RunView, runs::RunDeployView, runs::RunEventView, runs::RunArtifactView, runs::RunList, runs::RunLogPage, runs::RunReach, runs::RunReachProject,
        ],
        "sessions" => [
            sessions::SessionCard, sessions::SessionPlace, sessions::SessionWork, sessions::SessionMove, sessions::ChatAsk, sessions::ChatAskAnswer, sessions::SessionTry, sessions::SessionTryAsk, sessions::SessionPick, sessions::SessionTurn, sessions::SessionFinish, sessions::SessionOpen, sessions::SessionAnswered, sessions::SessionPort, sessions::SessionLine, sessions::SessionView, sessions::SessionAct, sessions::SessionPr, sessions::SessionPrEdit, sessions::SessionPrSend, sessions::SessionPrSent, sessions::SessionSettings, sessions::SessionStats, sessions::IssueAgent, sessions::AgentSessionStart, sessions::AgentSessionStarted, sessions::AgentSessionReply, sessions::AgentSessionReplied, sessions::SessionWaveQuery, sessions::SessionIssue, sessions::SessionRow, sessions::SessionWave, sessions::AgentSessionRead, sessions::SessionAnswer, sessions::AgentSessionWait, sessions::AgentSessionsWaited,
        ],
        "team" => [
            team::TrustAnswer, team::Flags, team::TeamAnswer, team::TeamNone, team::SharedTeamAnswer, team::PeerCerts, team::AccessList, team::Features, team::TeamQuery, team::TeamsQuery, team::PeersQuery,
        ],
        "telemetry" => [
            telemetry::PageConsent, telemetry::SetPageConsent, telemetry::PageReport, telemetry::Report, telemetry::Os, telemetry::Arch, telemetry::Channel, telemetry::ControlKind, telemetry::Event, telemetry::Surface, telemetry::Counts, telemetry::AgentRuntime, telemetry::BlockKind, telemetry::View, telemetry::LabsFlag, telemetry::Crash, telemetry::Process,
        ],
        "transcripts" => [
            transcripts::TranscriptPost, transcripts::ArchiveOpen, transcripts::ArchiveBlocks, transcripts::TranscriptArchived,
        ],
        "work" => [
            work::Work, work::WorkItem, work::DeployWork, work::PasskeyAssertion, work::SecretsAsk, work::SecretsGiven, work::ArtifactGiven, work::Secret, work::SessionWork, work::FinishStep, work::FinishResult, work::SessionAction, work::SessionPrompt, work::SessionReport, work::SessionDetail, work::PushAsk, work::PushGiven, work::SessionIssuesAsk, work::SessionIssuesOp, work::SessionIssuesGiven, work::Report, work::Reported,
        ],
    }
}

/// A file's JSON: its `$defs`, sorted by name, each listed type among them.
fn render(file: &str, add: fn(&mut SchemaGenerator, &mut Roots)) -> String {
    let mut g = SchemaSettings::draft2020_12().into_generator();
    let mut roots = Roots::new();
    add(&mut g, &mut roots);
    let mut defs: BTreeMap<String, Value> = g.take_definitions(true).into_iter().collect();
    for (name, schema) in roots {
        // A type schemars inlines (no `$ref`) is still one of the file's.
        if schema.get("$ref").is_none() {
            defs.entry(name).or_insert(schema);
        }
    }
    let module =
        if file == "lib" { "arugula_control_wire".to_owned() } else { format!("arugula_control_wire::{file}") };
    let doc = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": module,
        "$defs": defs.into_iter().collect::<Map<String, Value>>(),
    });
    serde_json::to_string_pretty(&doc).expect("a schema serializes") + "\n"
}

#[test]
fn the_checked_in_schema_is_current() {
    let write = std::env::var_os("ARUGULA_WRITE_SCHEMA").is_some();
    let dir = dir();
    let mut want = BTreeMap::new();
    for (file, _, add) in files() {
        want.insert(format!("{file}.json"), render(file, add));
    }
    let have: BTreeMap<String, String> = std::fs::read_dir(&dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).unwrap()))
                .collect()
        })
        .unwrap_or_default();
    if write {
        std::fs::create_dir_all(&dir).unwrap();
        for name in have.keys().filter(|n| !want.contains_key(*n)) {
            std::fs::remove_file(dir.join(name)).unwrap();
        }
        for (name, text) in &want {
            if have.get(name) != Some(text) {
                std::fs::write(dir.join(name), text).unwrap();
            }
        }
        return;
    }
    let stale: Vec<_> = want
        .iter()
        .filter(|(n, t)| have.get(*n) != Some(t))
        .map(|(n, _)| n.as_str())
        .chain(have.keys().filter(|n| !want.contains_key(*n)).map(String::as_str))
        .collect();
    assert!(stale.is_empty(), "contract/schema/ is stale ({}): run `just contract-schema`", stale.join(", "));
}

/// Every public type with a serde derive outside the tests: its schema
/// derive beside it, and its name in its file's list.
#[test]
fn every_wire_type_is_in_the_schema() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let listed: BTreeMap<&str, Vec<&str>> = files()
        .into_iter()
        .map(|(file, types, _)| (file, types.iter().map(|t| t.rsplit("::").next().unwrap().trim()).collect()))
        .collect();
    let mut missing = vec![];
    let mut paths = vec![];
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        if e.path().is_dir() {
            paths.extend(std::fs::read_dir(e.path()).unwrap().flatten().map(|e| e.path()));
        } else {
            paths.push(e.path());
        }
    }
    for path in paths {
        let rel = path.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        if !rel.ends_with(".rs") || rel == "tests.rs" || rel == "schema.rs" {
            continue;
        }
        let file = match rel.as_str() {
            "lib.rs" => "lib".to_owned(),
            r if r.starts_with("internal/") => "internal".to_owned(),
            r => r.trim_end_matches(".rs").to_owned(),
        };
        let text = std::fs::read_to_string(&path).unwrap();
        let text = text.split("#[cfg(test)]\nmod tests").next().unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i].trim_start();
            if !l.starts_with("#[derive(") {
                i += 1;
                continue;
            }
            let mut derive = l.to_owned();
            while !lines[i].contains(")]") {
                i += 1;
                derive.push_str(lines[i]);
            }
            i += 1;
            if !derive.contains("Serialize") && !derive.contains("Deserialize") {
                continue;
            }
            let mut schema = false;
            while i < lines.len() {
                let l = lines[i].trim_start();
                if l.starts_with("#[") || l.starts_with("//") {
                    schema |= l.contains("derive(schemars::JsonSchema)");
                    i += 1;
                } else {
                    break;
                }
            }
            let Some(decl) = lines.get(i) else { break };
            let words: Vec<&str> = decl.split_whitespace().collect();
            let (vis, name) = match words.as_slice() {
                ["struct" | "enum", name, ..] => ("", *name),
                [vis, _, name, ..] => (*vis, *name),
                _ => continue,
            };
            let generic = name.contains('<') && !name.contains('=') && !decl.contains(" = ");
            let name = name.split(['<', '(', '{', ';']).next().unwrap();
            if !schema {
                missing.push(format!("{rel}: {name} has no schema derive"));
            }
            if vis == "pub" && !generic && !listed.get(file.as_str()).is_some_and(|t| t.contains(&name)) {
                missing.push(format!("{rel}: {name} isn't in schema.rs's files(), \"{file}\""));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}
