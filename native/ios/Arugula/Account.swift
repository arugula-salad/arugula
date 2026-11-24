// The account, in every place's heading (Arugula #1361), now that You has
// left the tab bar: your picture opens your name, Settings, Send feedback,
// the app's version and Sign out, as the web's avatar menu; the bell beside
// it opens the Inbox. Settings is the web's page: control doesn't take this
// app's session on it, so it opens in the browser, at the platform control's
// `/control.json` names. Send feedback goes to control's `POST
// /api/feedback` as the person signed in; while control refuses the app
// there (`page_only`), the sheet says so and offers the web's page instead.

import SwiftUI

/// Your picture: your initials in your tone (as chat shows people), opening
/// the account's menu.
struct AccountMenu: View {
    @EnvironmentObject private var scope: Scope
    @EnvironmentObject private var app: AppModel
    @Environment(\.openURL) private var openURL
    @State private var feedback = false
    @State private var problem: String?

    var body: some View {
        Menu {
            Section {
                Button {} label: {
                    Text(scope.me?.name ?? "Signed in")
                    if let login = scope.me?.login { Text(login) }
                }
                .disabled(true)
            }
            Section {
                Button { Task { await settings() } } label: { Label("Settings", systemImage: "gearshape") }
                    .accessibilityIdentifier("account-settings")
                Button { feedback = true } label: { Label("Send feedback", systemImage: "exclamationmark.bubble") }
                    .accessibilityIdentifier("account-feedback")
            }
            Section {
                Text(verbatim: "Arugula \(Self.version)")
                Button("Sign out", role: .destructive) { app.signOut() }
                    .accessibilityIdentifier("sign-out")
            }
        } label: {
            Avatar(name: scope.me?.name ?? scope.me?.login ?? "?", size: 36)
                .clipShape(Circle())
        }
        .accessibilityLabel("You: \(scope.me?.name ?? "your account")")
        .accessibilityIdentifier("account")
        .sheet(isPresented: $feedback) {
            if let control = app.control { FeedbackSheet(control: control) { feedback = false } }
        }
        .alert("Settings didn't open", isPresented: .constant(problem != nil), presenting: problem) { _ in
            Button("OK") { problem = nil }
        } message: { Text($0) }
    }

    /// The app's version and build, as Settings › General › About says it.
    static var version: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String
        return build.map { "\(short) (\($0))" } ?? short
    }

    private func settings() async {
        guard let platform = await Platform.origin(app.control) else {
            problem = "Control didn't say where its page is."
            return
        }
        openURL(platform.appending(path: "settings"))
    }
}

/// The platform page's address, which `/control.json` names: where the
/// web's Settings and anything else the app can't open itself live.
enum Platform {
    static func origin(_ control: Control?) async -> URL? {
        guard let base = control?.url.flatMap({ URL(string: $0) }) else { return nil }
        struct Info: Decodable { var platform: String? }
        guard let (data, _) = try? await URLSession.shared.data(from: base.appending(path: "control.json")),
            let p = (try? JSONDecoder().decode(Info.self, from: data))?.platform
        else { return nil }
        return URL(string: p)
    }
}

/// Send feedback, as the web's sheet: what it is, a title and what happened.
struct FeedbackSheet: View {
    let control: Control
    let close: () -> Void
    @Environment(\.openURL) private var openURL
    @State private var kind = "bug"
    @State private var title = ""
    @State private var words = ""
    @State private var contact = true
    @State private var busy = false
    @State private var problem: String?
    /// Control won't take it from the app: send it from the web's page.
    @State private var pageOnly = false
    @State private var sent: String?

    private static let kinds: [(String, String)] = [
        ("bug", "Something's broken"), ("missing", "Something's missing"), ("idea", "An idea"), ("praise", "Praise"), ("other", "Other"),
    ]

    var body: some View {
        NavigationStack {
            Form {
                Picker("What is it", selection: $kind) {
                    ForEach(Self.kinds, id: \.0) { Text($0.1).tag($0.0) }
                }
                TextField("Title", text: $title).accessibilityIdentifier("feedback-title")
                TextField("What happened", text: $words, axis: .vertical)
                    .lineLimit(4...12)
                    .accessibilityIdentifier("feedback-body")
                Toggle("You may contact me about it", isOn: $contact)
                if let sent {
                    Text("Sent as \(sent). Thank you.").foregroundStyle(Palette.ok)
                } else if let problem {
                    Text(problem).font(Face.mono(12)).foregroundStyle(Palette.danger)
                    if pageOnly {
                        Button("Send it from Arugula's page") {
                            Task { if let p = await Platform.origin(control) { openURL(p) } }
                        }
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(Palette.void)
            .navigationTitle("Send feedback")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(sent == nil ? "Cancel" : "Done", action: close) }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") { Task { await send() } }
                        .disabled(busy || sent != nil || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("feedback-send")
                }
            }
        }
    }

    private func send() async {
        busy = true
        defer { busy = false }
        let body: [String: Any] = [
            "kind": kind, "title": String(title.prefix(200)), "body": words, "contact_ok": contact,
            "source": "person", "surface": "other",
            "context": ["version": AccountMenu.version, "os": "other"],
        ]
        do {
            let got = try await control.call("POST", "/api/feedback", body: body)
            let shown = (try? JSONSerialization.jsonObject(with: got) as? [String: Any])?["shown"] as? String
            sent = shown.flatMap { $0.isEmpty ? nil : $0 } ?? "feedback"
            problem = nil
        } catch let e as CoreError where e.reason == "page_only" {
            pageOnly = true
            problem = "Control doesn't take feedback from the app yet."
        } catch {
            problem = describe(error)
        }
    }
}

/// What's waiting for you, as the web's Inbox lists it: PRs, issues and runs
/// in the current project, newest first; one opens on its page. Its count on
/// the bell, and a push landing here, are Arugula #1365.
struct InboxSheet: View {
    let control: Control
    let close: () -> Void
    @EnvironmentObject private var scope: Scope
    @Environment(\.openURL) private var openURL

    struct Entry: Decodable, Identifiable {
        var item: String
        var reason: String
        var repo: String
        var number: Int
        var kind: String
        var state: String
        var title: String
        var url: String
        var updated_at: Int
        var seen_at: Int?
        var id: String { "\(item):\(reason)" }
    }
    struct Inbox: Decodable { var entries: [Entry] }

    var body: some View {
        NavigationStack {
            Loaded(load: { try await control.get(path, as: Inbox.self) }) { l in
                List(l.entries.sorted { $0.updated_at > $1.updated_at }) { e in
                    Button { Task { await open(e) } } label: { row(e) }
                        .buttonStyle(.plain)
                        .voidRow()
                }
                .voidList()
                .overlay { if l.entries.isEmpty { Caps("Nothing waiting for you") } }
            }
            .background(Palette.void)
            .navigationTitle("Inbox")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done", action: close).accessibilityIdentifier("inbox-done")
                }
            }
        }
    }

    private var path: String {
        guard let p = scope.current?.id else { return "/api/inbox" }
        return "/api/inbox?project=\(p)"
    }

    private func row(_ e: Entry) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 7) {
                Circle().fill(e.seen_at == nil ? Palette.accent : .clear).frame(width: 7, height: 7)
                Text(e.number > 0 ? "\(e.repo) #\(e.number)" : e.repo).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
                Spacer(minLength: 4)
                StateChip(state: e.state)
            }
            Text(e.title).font(Face.ui(15, e.seen_at == nil ? "SemiBold" : "Regular")).foregroundStyle(Palette.fg).lineLimit(2)
            Caps(e.reason.replacingOccurrences(of: "_", with: " "))
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }

    /// GitHub's items by their address; a native issue's or a run's is a path
    /// on the platform page.
    private func open(_ e: Entry) async {
        if let u = URL(string: e.url), u.scheme != nil {
            openURL(u)
        } else if let p = await Platform.origin(control) {
            openURL(p.appending(path: e.url))
        }
    }
}
