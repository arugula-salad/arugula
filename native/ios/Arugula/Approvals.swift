// What a passkey approves (Arugula #604): a deploy waiting on its project's
// owners, and a project's standing grant for runs. The app shows what's
// asked, and Approve opens control's own dialog for it (`#deploy=…&app`,
// `#run-grant=…&app`) in a browser sheet (`ASWebAuthenticationSession`), where
// the account's passkeys are: their RP is control's host, which this app isn't
// associated with, so it never asks for one itself. The dialog goes back to
// `arugula://back` when it's done or closed, and the app reads it again.

import AuthenticationServices
import SwiftUI

/// A deploy as the platform answers `GET /api/deploys/{id}` (and each of
/// `/api/deploys/waiting`): what the app shows of it.
struct DeployView: Decodable, Identifiable, Equatable {
    struct Named: Decodable, Equatable { var id: String; var name: String }
    struct Source: Decodable, Equatable {
        var kind: String
        var ref: String
        var `where`: String
    }
    struct Agent: Decodable, Equatable { var machine_name: String? }
    struct Stale: Decodable, Equatable { var deploy: String?; var why: String? }

    var id: String
    var project: Named
    var environment: Named
    var source: Source
    var state: String
    var planned_by_name: String?
    var agent: Agent?
    var subject: String?
    var expected_version: String?
    var requested_at: Double?
    var plan_text: String?
    var stale: Stale?
    var may_approve: Bool

    /// The push's words: "Approve a deploy to production?"
    var title: String { "Deploy to \(environment.name)" }

    /// "commit abc123def456 from acme/app"
    var what: String { "\(source.kind) \(String(source.ref.prefix(12))) from \(source.where)" }

    /// Who asked, as the page says it.
    var asker: String {
        let who = planned_by_name.flatMap { $0.isEmpty ? nil : $0 } ?? "someone"
        if let m = agent?.machine_name, !m.isEmpty { return "\(who)'s agent on \(m)" }
        return who
    }

    /// Its state in a word, and the tone it's shown in.
    var standing: (word: String, tone: Tone) {
        switch state {
        case "pending": stale == nil ? ("waiting", .warn) : ("stale", .muted)
        case "approved": ("approved", .ok)
        case "applying": ("applying", .info)
        case "applied": ("applied", .ok)
        case "denied": ("denied", .danger)
        case "failed": ("failed", .danger)
        default: (state.replacingOccurrences(of: "_", with: " "), .muted)
        }
    }
}

struct WaitingDeploys: Decodable { var deploys: [DeployView] }

/// A project's standing grant for runs, as `GET
/// /api/projects/{project}/runs/grant` answers.
struct RunGrantView: Decodable {
    struct Grant: Decodable { var id: String; var made: Double; var expires: Double? }
    struct Live: Decodable { var grant: Grant }

    var role: String
    var grant: Live?
    var stale: Bool
    var renew: Bool
    var signer_gone: Bool?
    var workspaces: Int
    var offer: Bool
}

/// What's open over the app: a deploy, or a project's runs.
enum Approving: Identifiable, Equatable {
    case deploy(String)
    case runs(project: String, name: String)

    var id: String {
        switch self {
        case .deploy(let d): "deploy:\(d)"
        case .runs(let p, _): "runs:\(p)"
        }
    }
}

/// The deploys waiting on the reader, and what's open over the app.
@MainActor
final class Approvals: ObservableObject {
    static let shared = Approvals()
    @Published var waiting: [DeployView] = []
    @Published var open: Approving?

    func load(_ control: Control) async {
        // A hub or an account without deploys has none waiting.
        if let w = try? await control.get("/api/deploys/waiting", as: WaitingDeploys.self) { waiting = w.deploys }
    }
}

/// Control's dialog for a passkey, in a browser sheet: what to open, and
/// how the sheet comes back.
@MainActor
struct PasskeySheet {
    /// A UI test can't use a passkey in a web sheet: with
    /// ARUGULA_APPROVE_BY_HAND it shows the address it would have opened, for
    /// something else to approve, and Done stands in for the sheet's return.
    static let byHand = ProcessInfo.processInfo.environment["ARUGULA_APPROVE_BY_HAND"] == "1"

    /// Control's page at `#hash&app`.
    static func link(_ control: Control, _ hash: String) -> URL? {
        guard let base = control.url, var c = URLComponents(string: base) else { return nil }
        c.path = "/"
        c.query = nil
        // The fragment without its `#`, which URLComponents would escape.
        c.fragment = "\(hash.hasPrefix("#") ? String(hash.dropFirst()) : hash)&app"
        return c.url
    }

    /// Open it, and wait for the sheet to come back, however it ends.
    static func open(_ link: URL, with webAuth: WebAuthenticationSession) async {
        _ = try? await webAuth.authenticate(using: link, callbackURLScheme: "arugula", preferredBrowserSession: .shared)
    }
}

/// A deploy waiting for approval: what it is, from whom, and Approve, which
/// opens control's dialog where the passkey approves or denies it.
struct DeploySheet: View {
    let control: Control
    let id: String
    let close: () -> Void
    @Environment(\.webAuthenticationSession) private var webAuth
    @ObservedObject private var approvals = Approvals.shared
    @State private var deploy: DeployView?
    @State private var problem: String?
    @State private var busy = false
    /// The address shown for a UI test to approve (`PasskeySheet.byHand`).
    @State private var handLink: URL?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if let d = deploy {
                        details(d)
                    } else if let problem {
                        Text(problem).font(Face.ui(14)).foregroundStyle(Palette.danger)
                    } else {
                        ProgressView().frame(maxWidth: .infinity)
                    }
                }
                .padding()
                .frame(maxWidth: 560, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .background(Palette.void)
            .navigationTitle(deploy?.title ?? "A deploy")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close", action: close).accessibilityIdentifier("deploy-close")
                }
            }
        }
        .task { await load() }
    }

    @ViewBuilder
    private func details(_ d: DeployView) -> some View {
        HStack(spacing: 8) {
            Circle().fill(d.standing.tone.color).frame(width: 8, height: 8)
            Text(d.standing.word)
                .font(Face.mono(12, "Bold")).foregroundStyle(d.standing.tone.color)
                .accessibilityIdentifier("deploy-state")
                .accessibilityValue(d.state)
        }
        VStack(alignment: .leading, spacing: 4) {
            Caps(d.project.name)
            Text(d.what).font(Face.mono(13)).foregroundStyle(Palette.fg).textSelection(.enabled)
            Text("Asked by \(d.asker)").font(Face.ui(14)).foregroundStyle(Palette.subtle)
            if let subject = d.subject ?? d.expected_version, !subject.isEmpty {
                // The platform's reading, not what the passkey signs.
                Text("Arugula reads this as: \(subject)").font(Face.ui(13)).foregroundStyle(Palette.dim)
            }
        }
        if let plan = d.plan_text, !plan.isEmpty {
            DisclosureGroup("What its driver will do") {
                Text(plan)
                    .font(Face.mono(11)).foregroundStyle(Palette.subtle)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
            }
            .font(Face.ui(14)).tint(Palette.accent)
        }
        if d.may_approve {
            VStack(alignment: .leading, spacing: 8) {
                Button {
                    Task { await approve(d) }
                } label: {
                    HStack(spacing: 8) {
                        if busy { ProgressView() }
                        Text("Approve or deny with passkey")
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(AnswerButton(primary: true))
                .disabled(busy)
                .accessibilityIdentifier("deploy-approve")
                Text("Opens Arugula's approval in a browser sheet: it checks what's signed and asks for your passkey. You come back here after.")
                    .font(Face.ui(12)).foregroundStyle(Palette.dim)
            }
        } else if d.state == "pending" {
            Text(d.stale != nil ? "A newer deploy or an apply has taken its place." : "It isn't waiting for your approval.")
                .font(Face.ui(14)).foregroundStyle(Palette.subtle)
        }
        if let handLink {
            Text(handLink.absoluteString).font(Face.mono(11)).accessibilityIdentifier("approve-link")
            Button("Done") { Task { await back() } }
                .buttonStyle(AnswerButton())
                .accessibilityIdentifier("approve-done")
        }
        if let problem {
            Text(problem).font(Face.ui(13)).foregroundStyle(Palette.danger)
        }
    }

    private func load() async {
        do {
            deploy = try await control.get("/api/deploys/\(id)", as: DeployView.self)
            problem = nil
        } catch {
            problem = describe(error)
        }
    }

    private func approve(_ d: DeployView) async {
        guard let link = PasskeySheet.link(control, "#deploy=\(d.id)") else {
            problem = "This sign-in doesn't say where Arugula is: sign in again."
            return
        }
        if PasskeySheet.byHand {
            handLink = link
            return
        }
        busy = true
        await PasskeySheet.open(link, with: webAuth)
        await back()
    }

    /// Back from the sheet: what became of it, and what's still waiting.
    private func back() async {
        busy = false
        handLink = nil
        await load()
        await approvals.load(control)
    }
}

/// A project's standing grant for runs: whether runs may start on its cloud
/// workspaces, and Allow, Renew or Revoke on control's dialog for it.
struct RunsSheet: View {
    let control: Control
    let project: String
    let name: String
    let close: () -> Void
    @Environment(\.webAuthenticationSession) private var webAuth
    @State private var view: RunGrantView?
    @State private var problem: String?
    @State private var busy = false
    @State private var handLink: URL?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if let v = view {
                        details(v)
                    } else if let problem {
                        Text(problem).font(Face.ui(14)).foregroundStyle(Palette.danger)
                    } else {
                        ProgressView().frame(maxWidth: .infinity)
                    }
                }
                .padding()
                .frame(maxWidth: 560, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .background(Palette.void)
            .navigationTitle("Runs in \(name)")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close", action: close).accessibilityIdentifier("runs-close")
                }
            }
        }
        .task { await load() }
    }

    @ViewBuilder
    private func details(_ v: RunGrantView) -> some View {
        let state = v.grant == nil ? (v.offer ? "offered" : "no") : (v.renew || v.stale || v.signer_gone == true ? "renew" : "yes")
        Text(says(v))
            .font(Face.ui(15)).foregroundStyle(Palette.fg)
            .accessibilityIdentifier("runs-grant")
            .accessibilityValue(state)
        Text(v.workspaces == 1 ? "1 cloud workspace" : "\(v.workspaces) cloud workspaces")
            .font(Face.mono(12)).foregroundStyle(Palette.dim)
        if v.role == "owner" {
            VStack(alignment: .leading, spacing: 8) {
                Button {
                    Task { await change() }
                } label: {
                    HStack(spacing: 8) {
                        if busy { ProgressView() }
                        Text(v.grant == nil ? "Allow runs with passkey" : "Renew or revoke with passkey")
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(AnswerButton(primary: true))
                .disabled(busy)
                .accessibilityIdentifier("runs-change")
                Text("Opens Arugula's page for this project's grant in a browser sheet, where your passkey confirms it. You come back here after.")
                    .font(Face.ui(12)).foregroundStyle(Palette.dim)
            }
        } else {
            Text("A project owner allows runs, or revokes them, with a passkey.")
                .font(Face.ui(13)).foregroundStyle(Palette.subtle)
        }
        if let handLink {
            Text(handLink.absoluteString).font(Face.mono(11)).accessibilityIdentifier("approve-link")
            Button("Done") { Task { await back() } }
                .buttonStyle(AnswerButton())
                .accessibilityIdentifier("approve-done")
        }
        if let problem {
            Text(problem).font(Face.ui(13)).foregroundStyle(Palette.danger)
        }
    }

    /// What the grant allows, as the project's settings say it.
    private func says(_ v: RunGrantView) -> String {
        guard let g = v.grant?.grant else {
            return v.offer
                ? "No runs start yet. This project has cloud workspaces: an owner can allow runs on them."
                : "No runs or agent sessions start here: nobody has allowed them, or the last grant expired or was revoked."
        }
        if v.stale { return "The grant is for an earlier delegation, so workspaces enrolled since refuse it: sign it again." }
        if v.signer_gone == true { return "The owner who confirmed it isn't an owner now, so workspaces refuse it: sign it again." }
        let until = g.expires.map { " until \(Date(timeIntervalSince1970: $0 / 1000).formatted(date: .abbreviated, time: .omitted))" } ?? ""
        return "Arugula may start runs and agent sessions on this project's cloud workspaces\(until)." + (v.renew ? " It expires soon." : "")
    }

    private func load() async {
        do {
            view = try await control.get("/api/projects/\(project)/runs/grant", as: RunGrantView.self)
            problem = nil
        } catch {
            problem = describe(error)
        }
    }

    private func change() async {
        guard let link = PasskeySheet.link(control, "#run-grant=\(project)") else {
            problem = "This sign-in doesn't say where Arugula is: sign in again."
            return
        }
        if PasskeySheet.byHand {
            handLink = link
            return
        }
        busy = true
        await PasskeySheet.open(link, with: webAuth)
        await back()
    }

    private func back() async {
        busy = false
        handLink = nil
        await load()
    }
}

/// A deploy waiting on the reader, as a row of Needs you.
struct DeployNeedRow: View {
    let deploy: DeployView

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 7) {
                Circle().fill(Palette.warn).frame(width: 7, height: 7)
                Text("Approve a deploy to \(deploy.environment.name)?")
                    .font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                Spacer(minLength: 4)
                Text("deploy").font(Face.mono(11, "Bold")).foregroundStyle(Palette.warn)
            }
            Text(deploy.project.name).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
            Text("\(deploy.what), asked by \(deploy.asker)")
                .font(Face.mono(12)).foregroundStyle(Palette.subtle).lineLimit(2)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}
