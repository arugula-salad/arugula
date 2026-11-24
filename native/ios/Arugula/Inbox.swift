// The Inbox behind the bell (Arugula #1365), as the web's (web/src/platform/
// pages/inbox.tsx): deploys waiting for your approval first, then what wants
// you grouped by why, the threads with news chat doesn't list, what's ready,
// merged or closed, and the drafts waiting for you to send. Opening an entry
// marks it seen; Dismiss calls it done (a thread's: reads it). The bell's
// count is the web's (inbox-count.tsx): the unseen entries, the threads
// with news and the deploys waiting. A tapped push about an item marks it
// seen and opens it (Push.swift).

import SwiftUI

/// What the Inbox shows and the bell counts, for the current project, read
/// when the app comes up or back, every minute while it's open, and again
/// whenever the Inbox marks something.
@MainActor
final class InboxModel: ObservableObject {
    static let shared = InboxModel()

    struct Entry: Decodable, Identifiable, Hashable {
        var item: String
        var reason: String
        var repo: String
        var number: Int
        var kind: String
        var state: String
        var title: String
        var author: String?
        var url: String
        var project: String?
        var room: String?
        var updated_at: Int
        var seen_at: Int?
        var done_at: Int?
        var id: String { "\(item):\(reason)" }

        /// Marked seen, or done (a PR ready, merged or closed, an issue
        /// closed), which tells the reader something and asks nothing.
        var seen: Bool { seen_at != nil || reason == "done" }
    }
    private struct List: Decodable { var entries: [Entry] }

    @Published private(set) var entries: [Entry]?
    /// Every room, for New in threads.
    @Published private(set) var rooms: [ChatRoom] = []
    @Published var problem: String?
    private(set) var project: String?

    /// The entries that aren't drafts.
    var shown: [Entry] { (entries ?? []).filter { $0.reason != "draft" } }
    var drafts: [Entry] { (entries ?? []).filter { $0.reason == "draft" } }

    /// New in threads: each object's thread with something unread, unless
    /// an unseen mention in the inbox already points at it; mentions first,
    /// then the most unread (the web's `threadNews`).
    var threads: [ChatRoom] {
        guard entries != nil else { return [] }
        var covered = Set<String>()
        for e in shown where (e.reason == "mentioned" || e.reason == "thread_mention") && e.seen_at == nil && e.done_at == nil {
            covered.insert(e.item)
            if let r = e.room { covered.insert(r) }
        }
        return rooms
            .filter { $0.kind == "object" && $0.counted > 0 && !covered.contains($0.id) && !covered.contains($0.target ?? "") }
            .filter { r in project == nil || r.project == nil || r.project == project }
            .sorted { a, b in
                if (a.mentions > 0) != (b.mentions > 0) { return a.mentions > 0 }
                if a.counted != b.counted { return a.counted > b.counted }
                return a.name < b.name
            }
    }

    /// The bell's count: unseen entries, threads with news and deploys
    /// waiting for you.
    var count: Int { shown.filter { !$0.seen }.count + threads.count + Approvals.shared.waiting.count }

    func load(_ control: Control, project: String?) async {
        if project != self.project { entries = nil }
        self.project = project
        let path = project.map { "/api/inbox?project=\($0)" } ?? "/api/inbox"
        async let list = control.get(path, as: List.self)
        async let chat = try? control.get("/api/chat", as: ChatRooms.self)
        async let deploys: Void = Approvals.shared.load(control)
        do {
            let l = try await list
            guard project == self.project else { return }
            entries = l.entries
            problem = nil
        } catch is CancellationError {
        } catch {
            problem = describe(error)
        }
        if let c = await chat { rooms = c.rooms }
        await deploys
    }

    /// Mark an item's entries seen or done: one reason's, or all of them.
    func mark(_ control: Control, item: String, reason: String? = nil, seen: Bool? = nil, done: Bool? = nil) async throws {
        var body: [String: Any] = [:]
        if let reason { body["reason"] = reason }
        if let seen { body["seen"] = seen }
        if let done { body["done"] = done }
        let id = item.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? item
        _ = try await control.call("POST", "/api/inbox/\(id)", body: body)
        await load(control, project: project)
    }

    /// Read a thread to its last message, as Dismiss does on the web.
    func read(_ control: Control, room: ChatRoom) async {
        _ = try? await control.call("POST", "/api/chat/rooms/\(room.id)/read", body: ["seq": room.last])
        await load(control, project: project)
    }
}

extension ChatRoom {
    /// How many of its messages count, at the level it's followed at (the
    /// web's `counted`).
    var counted: Int {
        switch level {
        case "nothing": mentions
        case "mentions": kind == "object" ? (wants ?? unread) : kind == "channel" ? mentions : unread
        case "all": unread
        default: muted == true ? mentions : unread
        }
    }
}

/// The bell, with what's waiting on it: in a place's heading, and at the
/// foot of an iPad's sidebar.
struct InboxBell: View {
    let action: () -> Void
    @ObservedObject private var inbox = InboxModel.shared
    @ObservedObject private var approvals = Approvals.shared

    var body: some View {
        let n = inbox.count
        HeadingButton(symbol: n > 0 ? "bell.badge" : "bell", label: n > 0 ? "Inbox, \(n)" : "Inbox", action: action)
            .overlay(alignment: .topTrailing) {
                if n > 0 {
                    Text(verbatim: n > 99 ? "99+" : "\(n)")
                        .font(Face.mono(10, "Bold"))
                        .foregroundStyle(Palette.void)
                        .padding(.horizontal, 4)
                        .frame(minWidth: 17, minHeight: 17)
                        .background(approvals.waiting.isEmpty ? Palette.accent : Palette.warn, in: Capsule())
                        .offset(x: 5, y: -4)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
            }
    }
}

/// The Inbox, over whichever place is open. `focus` is an item a push
/// opened, scrolled to and outlined.
struct InboxSheet: View {
    let control: Control
    let live: LiveChat
    var focus: String?
    let close: () -> Void
    @EnvironmentObject private var scope: Scope
    @ObservedObject private var inbox = InboxModel.shared
    @ObservedObject private var approvals = Approvals.shared
    @Environment(\.openURL) private var openURL
    @State private var path = NavigationPath()
    @State private var busy: String?
    @State private var problem: String?

    /// The reasons, most pressing first, as headings; New in threads sits
    /// between Assigned and Ready (the web's GROUPS).
    private static let groups: [(String, [String], String)] = [
        ("review_requested", ["review_requested"], "Reviews requested"),
        ("changes_requested", ["changes_requested"], "Changes requested"),
        ("checks_failed", ["checks_failed"], "Checks failed"),
        ("run_failed", ["run_failed"], "Runs failed"),
        ("mentioned", ["mentioned", "thread_mention"], "Mentions"),
        ("assigned", ["assigned"], "Assigned to you"),
        ("threads", [], "New in threads"),
        ("done", ["done"], "Ready, merged or closed"),
    ]

    var body: some View {
        NavigationStack(path: $path) {
            content
                .background(Palette.void)
                .navigationTitle("Inbox")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done", action: close).accessibilityIdentifier("inbox-done")
                    }
                }
                .navigationDestination(for: IssueRef.self) { ref in
                    Loaded(load: { try await control.get("/api/issues/\(ref.id)", as: Issue.self) }) { IssueView(issue: $0) }
                }
                .navigationDestination(for: ChatRoom.self) { room in
                    ChatRoomView(control: control, room: room, live: live)
                        .onDisappear { Task { await reload() } }
                }
        }
        .task { await reload() }
        .alert("That didn't work", isPresented: .constant(problem != nil), presenting: problem) { _ in
            Button("OK") { problem = nil }
        } message: { Text($0) }
    }

    @ViewBuilder private var content: some View {
        if inbox.entries == nil {
            if let p = inbox.problem {
                Problem(message: p) { Task { await reload() } }
            } else {
                ProgressView().tint(Palette.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        } else {
            let shown = inbox.shown
            let threads = inbox.threads
            let empty = shown.isEmpty && threads.isEmpty && inbox.drafts.isEmpty && approvals.waiting.isEmpty
            ScrollViewReader { reader in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 18) {
                        stats(shown: shown, threads: threads)
                        if empty {
                            Caps("Nothing wants you right now")
                                .frame(maxWidth: .infinity)
                                .padding(.top, 40)
                                .accessibilityIdentifier("inbox-empty")
                        }
                        if !approvals.waiting.isEmpty {
                            section("Deploys waiting", approvals.waiting.count, key: "deploys") {
                                ForEach(approvals.waiting) { deployRow($0) }
                            }
                        }
                        ForEach(Self.groups, id: \.0) { key, reasons, heading in
                            if key == "threads" {
                                if !threads.isEmpty {
                                    section(heading, threads.count, key: key) { ForEach(threads) { threadRow($0) } }
                                }
                            } else {
                                let group = shown.filter { reasons.contains($0.reason) }
                                if !group.isEmpty {
                                    section(heading, group.count, key: key) { ForEach(group) { entryRow($0) } }
                                }
                            }
                        }
                        if !inbox.drafts.isEmpty {
                            section("Drafts waiting for you", inbox.drafts.count, key: "draft") {
                                ForEach(inbox.drafts) { draftRow($0) }
                            }
                        }
                    }
                    .padding()
                }
                .refreshable { await reload() }
                .onAppear { if let focus { reader.scrollTo(focus, anchor: .center) } }
            }
        }
    }

    private func reload() async {
        await inbox.load(control, project: scope.current?.id)
    }

    /// How many are unseen, and in all.
    private func stats(shown: [InboxModel.Entry], threads: [ChatRoom]) -> some View {
        let unseen = shown.filter { !$0.seen }.count + threads.count
        let all = shown.count + threads.count
        return HStack(spacing: 14) {
            Count(n: unseen, label: "unseen", tone: unseen > 0 ? .accent : .muted)
            Count(n: all, label: all == 1 ? "item" : "items", tone: .muted)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("inbox-stats")
    }

    private func section<C: View>(_ heading: String, _ n: Int, key: String, @ViewBuilder _ rows: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(heading.uppercased()).font(Face.display(20)).foregroundStyle(Palette.fg)
                Text(verbatim: "\(n)").font(Face.mono(12, "Bold")).foregroundStyle(Palette.dim)
            }
            rows()
        }
        .accessibilityIdentifier("inbox-group-\(key)")
    }

    // ---- rows

    private func entryRow(_ e: InboxModel.Entry) -> some View {
        let key = e.id
        return VStack(alignment: .leading, spacing: 6) {
            Button { Task { await open(e) } } label: {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline, spacing: 7) {
                        if !e.seen { Circle().fill(Self.tone(e.reason).color).frame(width: 7, height: 7) }
                        Text(verbatim: e.kind == "run" ? "\(e.repo) ·" : "\(e.repo)#\(e.number)")
                            .font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
                        Spacer(minLength: 4)
                        StateChip(state: Self.stateWord(e))
                    }
                    Text(e.title).font(Face.ui(15, e.seen ? "Regular" : "SemiBold")).foregroundStyle(Palette.fg).lineLimit(3)
                    Text(verbatim: meta(e)).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("inbox-entry-\(e.item)")
            HStack {
                Spacer()
                Button("Dismiss") { Task { await dismiss(e) } }
                    .font(Face.mono(11, "Bold"))
                    .foregroundStyle(Palette.subtle)
                    .buttonStyle(.plain)
                    .disabled(busy == key)
                    .accessibilityIdentifier("inbox-done-\(e.item)")
            }
        }
        .card(Self.tone(e.reason))
        .opacity(e.seen ? 0.7 : 1)
        .overlay {
            if focus == e.item { RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.accent, lineWidth: 2) }
        }
        .id(e.item)
    }

    private func threadRow(_ r: ChatRoom) -> some View {
        let tone: Tone = r.mentions > 0 ? .warn : .accent
        return VStack(alignment: .leading, spacing: 6) {
            NavigationLink(value: r) {
                VStack(alignment: .leading, spacing: 4) {
                    if let name = scope.projects.first(where: { $0.id == r.project })?.name {
                        Text(verbatim: "\(name) ·").font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
                    }
                    Text(r.name).font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(2)
                    Text(verbatim: "\(r.counted) new\(r.mentions > 0 ? " · mentions you" : "")").font(Face.mono(11)).foregroundStyle(Palette.dim)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("inbox-thread-\(r.id)")
            HStack {
                Spacer()
                Button("Dismiss") { Task { await inbox.read(control, room: r) } }
                    .font(Face.mono(11, "Bold"))
                    .foregroundStyle(Palette.subtle)
                    .buttonStyle(.plain)
                    .accessibilityHint("Marks its thread read")
            }
        }
        .card(tone)
        .id(r.id)
    }

    private func deployRow(_ d: DeployView) -> some View {
        Button {
            // One sheet at a time: the deploy's opens once this one's gone.
            close()
            Task {
                try? await Task.sleep(for: .milliseconds(450))
                approvals.open = .deploy(d.id)
            }
        } label: {
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: "\(d.project.name) · \(d.environment.name)").font(Face.mono(11)).foregroundStyle(Palette.dim)
                Text(d.subject ?? d.title).font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(2)
                if let who = d.planned_by_name { Text("by \(who)").font(Face.mono(11)).foregroundStyle(Palette.dim) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .card(.warn)
        .accessibilityIdentifier("inbox-deploy-\(d.id)")
    }

    /// A draft is sent from Arugula's page, where its words can be read whole.
    private func draftRow(_ e: InboxModel.Entry) -> some View {
        Button {
            Task { if let p = await Platform.origin(control) { openURL(p.appending(path: "inbox")) } }
        } label: {
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: "\(e.repo)#\(e.number)").font(Face.mono(11)).foregroundStyle(Palette.dim)
                Text(e.title).font(Face.ui(15, "Medium")).foregroundStyle(Palette.fg).lineLimit(2)
                Caps("Send it from Arugula's page")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .card(.muted)
    }

    // ---- what a row says

    /// The groups whose heading already says why: the card says who.
    private static let headingSays: Set<String> = ["review_requested", "changes_requested", "checks_failed", "run_failed"]

    private func meta(_ e: InboxModel.Entry) -> String {
        let by = e.author.map { e.kind == "run" ? "started by \($0)" : "by \($0)" }
        if Self.headingSays.contains(e.reason) { return by ?? "" }
        let kind = e.kind == "pr" ? "PR" : e.kind == "run" ? "Run" : "Issue"
        return ([kind] + (e.reason == "thread_mention" ? ["in its thread"] : []) + (by.map { [$0] } ?? [])).joined(separator: " · ")
    }

    /// An open PR that's done is ready.
    static func stateWord(_ e: InboxModel.Entry) -> String {
        e.reason == "done" && e.state == "open" ? "ready" : e.state
    }

    /// Why it wants you, as the web's swarm colors it.
    static func tone(_ reason: String) -> Tone {
        switch reason {
        case "review_requested": .accent
        case "changes_requested": .warn
        case "checks_failed", "run_failed": .danger
        case "mentioned", "thread_mention": .info
        case "assigned": .teal
        case "done": .ok
        default: .muted
        }
    }

    // ---- doing

    /// Mark it seen, then open it: a native issue here, a thread's mention
    /// in its thread, a run on Arugula's page and anything else on GitHub.
    private func open(_ e: InboxModel.Entry) async {
        if !e.seen { Task { try? await inbox.mark(control, item: e.item, reason: e.reason, seen: true) } }
        if e.item.hasPrefix("iss_") {
            path.append(IssueRef(id: e.item))
        } else if e.reason == "thread_mention", let id = e.room, let room = inbox.rooms.first(where: { $0.id == id }) {
            path.append(room)
        } else if let u = URL(string: e.url), u.scheme != nil {
            openURL(u)
        } else if let p = await Platform.origin(control) {
            openURL(p.appending(path: e.url))
        }
    }

    private func dismiss(_ e: InboxModel.Entry) async {
        busy = e.id
        defer { busy = nil }
        do {
            try await inbox.mark(control, item: e.item, reason: e.reason, done: true)
        } catch {
            problem = describe(error)
        }
    }
}
