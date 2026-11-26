// The Inbox behind the bell (Arugula #1365), the one Inbox (Arugula #1434):
// the server's one list (`items` of `GET /api/inbox`) in its groups, Decide,
// Names you and Chat, then the drafts waiting for you to send. A teammate's
// asks the reader sees fold under one row, "2 asks for Jake ›", as the web's
// (web/src/platform/pages/inbox.tsx). The bell shows the server's `count`
// and adds nothing to it; the `inbox` event on chat's live stream says when
// to read it again. An item is answered where it's listed when it can be:
// an agent's ask over its machine's link, as Needs you does (Answer.swift),
// an agent's card (Start, Not now) or a to-do (Done, Can't, with a note)
// with its choice, a board's ask on its board. Opening one marks it seen;
// Dismiss calls it done. A tapped push about an item marks it seen and
// opens it (Push.swift).

import SwiftUI

/// What the Inbox shows and the bell counts, for the current project, read
/// when the app comes up or back, when the live stream says it moved, and
/// again whenever the Inbox marks or answers something.
@MainActor
final class InboxModel: ObservableObject {
    static let shared = InboxModel()

    /// One item of the one Inbox (control-wire's `InboxItem`).
    struct Item: Decodable, Identifiable, Hashable {
        struct From: Decodable, Hashable {
            var account: String?
            var name: String?
            var agent: String?
            var machine: String?
        }
        struct Subject: Decodable, Hashable {
            var type: String
            var id: String
            var url: String
            var project: String?
            var machine: String?
            /// The asker's block on `machine`, as `%N`.
            var pane: String?
            var room: String?
            var board: String?
        }
        struct Answer: Decodable, Hashable {
            var via: String
            var options: [String]?
        }
        var id: String
        var kind: String
        var group: String
        var counts: Bool
        /// The account it waits on, when it isn't the reader's own.
        var waitsOn: String?
        var escalated: Bool?
        var title: String
        var snippet: String?
        var lines: [String]?
        var from: From?
        var subject: Subject
        var answer: Answer?
        var since: Int
        var seen_at: Int?
        var done_at: Int?

        enum CodingKeys: String, CodingKey {
            case id, kind, group, counts, escalated, title, snippet, lines, from, subject, answer, since, seen_at, done_at
            case waitsOn = "for"
        }

        var seen: Bool { seen_at != nil }
    }

    /// Today's entries: only their drafts are read, which aren't items.
    struct Entry: Decodable, Identifiable, Hashable {
        var item: String
        var reason: String
        var repo: String
        var number: Int
        var title: String
        var id: String { "\(item):\(reason)" }
    }
    private struct List: Decodable {
        var entries: [Entry]?
        var count: Int?
        var items: [Item]?
        var handled: Int?
    }

    @Published private(set) var items: [Item]?
    /// The one count: the bell's, and nothing added to it.
    @Published private(set) var count = 0
    /// How many asks agents are handling among themselves: a line, never counted.
    @Published private(set) var handled = 0
    @Published private(set) var drafts: [Entry] = []
    /// Every room, for opening a thread or a DM in place.
    @Published private(set) var rooms: [ChatRoom] = []
    @Published var problem: String?
    private(set) var project: String?

    /// What the list shows: not a finished turn (`ready`), nor what's done.
    var shown: [Item] { (items ?? []).filter { $0.group != "ready" && $0.done_at == nil } }

    /// A deploy waits on the reader: the bell's count wears the warning.
    var deployWaiting: Bool { shown.contains { $0.kind == "deploy_approval" && $0.counts } }

    func load(_ control: Control, project: String?) async {
        if project != self.project { items = nil }
        self.project = project
        let path = project.map { "/api/inbox?project=\($0)" } ?? "/api/inbox"
        async let list = control.get(path, as: List.self)
        async let chat = try? control.get("/api/chat", as: ChatRooms.self)
        do {
            let l = try await list
            guard project == self.project else { return }
            items = l.items ?? []
            count = l.count ?? 0
            handled = l.handled ?? 0
            drafts = (l.entries ?? []).filter { $0.reason == "draft" }
            problem = nil
        } catch is CancellationError {
        } catch {
            problem = describe(error)
        }
        if let c = await chat { rooms = c.rooms }
    }

    /// Mark an item seen or done.
    func mark(_ control: Control, item: String, seen: Bool? = nil, done: Bool? = nil) async throws {
        var body: [String: Any] = [:]
        if let seen { body["seen"] = seen }
        if let done { body["done"] = done }
        _ = try await control.call("POST", Self.path(item), body: body)
        if done == true { PushCenter.withdraw([item]) }
        await load(control, project: project)
    }

    /// Answer an agent's card or a to-do with one of its options, and a
    /// to-do's note: the one call both rows make (Arugula #1470). Refused,
    /// it throws the server's words and the list stays as it was.
    func answer(_ control: Control, item: String, choice: String, note: String? = nil) async throws {
        var body: [String: Any] = ["choice": choice]
        if let note, !note.isEmpty { body["note"] = note }
        _ = try await control.call("POST", Self.path(item), body: body)
        PushCenter.withdraw([item])
        await load(control, project: project)
    }

    static func path(_ item: String) -> String {
        "/api/inbox/\(item.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? item)"
    }
}

/// The bell, with what's waiting on it: in a place's heading, and at the
/// foot of an iPad's sidebar.
struct InboxBell: View {
    let action: () -> Void
    @ObservedObject private var inbox = InboxModel.shared

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
                        .background(inbox.deployWaiting ? Palette.warn : Palette.accent, in: Capsule())
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
    /// Home's machines and sessions, whose asks are answered here as Needs
    /// you answers them.
    @ObservedObject var fleet: HomeFleet
    @ObservedObject var model: HomeModel
    var focus: String?
    let close: () -> Void
    @EnvironmentObject private var scope: Scope
    @ObservedObject private var inbox = InboxModel.shared
    @ObservedObject private var approvals = Approvals.shared
    @Environment(\.openURL) private var openURL
    @State private var path = NavigationPath()
    @State private var busy: String?
    @State private var problem: String?
    /// The teammates whose folded asks are open, by group.
    @State private var unfolded: Set<String> = []
    /// What was said of an item answered here, until it leaves the list.
    @State private var answered: [String: String] = [:]
    /// The to-dos open in their rows (Todo.swift).
    @State private var expanded: Set<String> = []

    /// The groups, as the server ranks them (the web's GROUPS).
    private static let groups: [(String, String)] = [
        ("decide", "Decide"),
        ("named", "Names you"),
        ("chat", "Chat"),
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
        if inbox.items == nil {
            if let p = inbox.problem {
                Problem(message: p) { Task { await reload() } }
            } else {
                ProgressView().tint(Palette.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        } else {
            let shown = inbox.shown
            let known = Set(Self.groups.map(\.0))
            let others = shown.filter { !known.contains($0.group) }
            ScrollViewReader { reader in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 18) {
                        stats(shown)
                        if inbox.handled > 0 {
                            Caps("\(inbox.handled) \(inbox.handled == 1 ? "ask" : "asks") agents are handling")
                                .accessibilityIdentifier("inbox-handled")
                        }
                        if shown.isEmpty && inbox.drafts.isEmpty {
                            Caps("Nothing wants you right now")
                                .frame(maxWidth: .infinity)
                                .padding(.top, 40)
                                .accessibilityIdentifier("inbox-empty")
                        }
                        ForEach(Self.groups, id: \.0) { key, heading in
                            group(heading, key: key, shown.filter { $0.group == key })
                        }
                        // A group this doesn't know still shows what's in it.
                        group("More", key: "more", others)
                        if !inbox.drafts.isEmpty {
                            section("Drafts waiting for you", inbox.drafts.count, key: "draft") {
                                ForEach(inbox.drafts) { draftRow($0) }
                            }
                        }
                    }
                    .padding()
                }
                .refreshable { await reload() }
                .onAppear {
                    if let focus, let i = shown.first(where: { Self.matches($0, focus) }) {
                        if let w = i.waitsOn, !mine(i) { unfolded.insert("\(i.group) \(w)") }
                        if i.isTodo { expanded.insert(i.id) }
                        reader.scrollTo(i.id, anchor: .center)
                    }
                }
            }
        }
    }

    private func reload() async {
        await inbox.load(control, project: scope.current?.id)
    }

    /// A push names an item by its id, or by the thing it's about.
    static func matches(_ i: InboxModel.Item, _ focus: String) -> Bool {
        i.id == focus || i.id.hasPrefix(focus + ":") || i.subject.id == focus
    }

    /// The reader's own: not one waiting on a teammate.
    private func mine(_ i: InboxModel.Item) -> Bool {
        guard let w = i.waitsOn else { return true }
        return w == scope.me?.account
    }

    /// What needs you, and how many items in all.
    private func stats(_ shown: [InboxModel.Item]) -> some View {
        let all = shown.filter(mine).count
        return HStack(spacing: 14) {
            Count(n: inbox.count, label: inbox.count == 1 ? "needs you" : "need you", tone: inbox.count > 0 ? .accent : .muted)
            Count(n: all, label: all == 1 ? "item" : "items", tone: .muted)
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("inbox-stats")
    }

    /// One group: the reader's own rows, then a teammate's asks folded
    /// under one row each, most first. Its count is the reader's own.
    @ViewBuilder
    private func group(_ heading: String, key: String, _ items: [InboxModel.Item]) -> some View {
        let own = items.filter(mine)
        let theirs = Dictionary(grouping: items.filter { !mine($0) }, by: { $0.waitsOn ?? "" })
            .sorted { $0.value.count != $1.value.count ? $0.value.count > $1.value.count : $0.key < $1.key }
        if !items.isEmpty {
            section(heading, own.count, key: key) {
                ForEach(own) { itemRow($0) }
                ForEach(theirs, id: \.key) { who, asks in
                    foldRow(key: "\(key) \(who)", who: who, n: asks.count)
                    if unfolded.contains("\(key) \(who)") {
                        ForEach(asks) { itemRow($0) }
                    }
                }
            }
        }
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

    private func itemRow(_ i: InboxModel.Item) -> some View {
        let (ref, title) = Self.titled(i.title)
        let over = ref ?? scope.projects.first(where: { $0.id == i.subject.project }).map { "\($0.name) ·" }
        return VStack(alignment: .leading, spacing: 6) {
            Button { Task { i.isTodo ? toggle(i) : await open(i) } } label: {
                VStack(alignment: .leading, spacing: 4) {
                    if over != nil || !i.seen {
                        HStack(alignment: .firstTextBaseline, spacing: 7) {
                            if !i.seen { Circle().fill(Self.tone(i.kind).color).frame(width: 7, height: 7) }
                            if let over { Text(verbatim: over).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1) }
                            Spacer(minLength: 4)
                        }
                    }
                    Text(title).font(Face.ui(15, i.seen ? "Regular" : "SemiBold")).foregroundStyle(Palette.fg).lineLimit(3)
                    let m = meta(i)
                    if !m.isEmpty { Text(verbatim: m).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(2) }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("inbox-entry-\(i.id)")
            if let lines = i.lines, !lines.isEmpty, !i.isTodo {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(lines.enumerated()), id: \.offset) { _, l in
                        Text(verbatim: l).font(Face.mono(11)).foregroundStyle(Palette.subtle).lineLimit(4)
                    }
                }
            }
            if let said = answered[i.id] {
                Caps(said)
            } else if i.isTodo {
                // A to-do opens in its row; a teammate's is theirs to answer.
                if expanded.contains(i.id) && mine(i) {
                    TodoAnswer(item: i, control: control) { said in
                        answered[i.id] = said
                    }
                }
            } else if mine(i) {
                ItemAnswer(item: i, control: control, need: need(i), model: model) { said in
                    answered[i.id] = said
                    Task { await reload() }
                }
            }
            HStack {
                Spacer()
                Button("Dismiss") { Task { await dismiss(i) } }
                    .font(Face.mono(11, "Bold"))
                    .foregroundStyle(Palette.subtle)
                    .buttonStyle(.plain)
                    .disabled(busy == i.id)
                    .accessibilityIdentifier("inbox-done-\(i.id)")
            }
        }
        .card(Self.tone(i.kind))
        .opacity(i.seen ? 0.7 : 1)
        .overlay {
            if let focus, Self.matches(i, focus) { RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.accent, lineWidth: 2) }
        }
        .id(i.id)
    }

    /// A teammate's asks, folded: "2 asks for Jake ›".
    private func foldRow(key: String, who: String, n: Int) -> some View {
        let open = unfolded.contains(key)
        return Button {
            if open { unfolded.remove(key) } else { unfolded.insert(key) }
        } label: {
            Text(verbatim: "\(n) \(n == 1 ? "ask" : "asks") for \(memberName(who)) \(open ? "⌄" : "›")")
                .font(Face.ui(15, "Medium")).foregroundStyle(Palette.fg)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .card(.muted)
        .accessibilityIdentifier("inbox-fold-\(who)")
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

    /// A kind in words (the web's kindWords); a kind not here says nothing,
    /// and its title says it.
    private static let kindWords: [String: String] = [
        "agent_ask": "An agent asks",
        "permission": "An agent asks for a permission",
        "agent_failed": "An agent failed",
        "approval": "Your agent proposes",
        "todo": "A to-do for you",
        "board_ask": "A board asks",
        "deploy_approval": "Waits for your approval",
        "checks_failed": "Checks failed",
        "run_failed": "Run failed",
        "changes_requested": "Changes requested",
        "review_request": "Review requested",
        "mention": "Mentions you",
        "assigned": "Assigned to you",
        "dm": "Message",
        "thread_news": "New in its thread",
    ]

    private func meta(_ i: InboxModel.Item) -> String {
        let parts = [Self.kindWords[i.kind], i.from?.name.map { "from \($0)" }, i.escalated == true ? "no agent answered it" : nil]
        let line = parts.compactMap { $0 }.joined(separator: " · ")
        guard let s = i.snippet, !s.isEmpty else { return line }
        return line.isEmpty ? s : "\(line): \(s)"
    }

    /// `owner/repo#12 Its title`: the reference above, the title alone.
    static func titled(_ t: String) -> (String?, String) {
        guard let space = t.firstIndex(of: " ") else { return (nil, t) }
        let ref = t[..<space]
        guard ref.range(of: #"^\S*#\d+$"#, options: .regularExpression) != nil else { return (nil, t) }
        return (String(ref), String(t[t.index(after: space)...]))
    }

    /// A teammate's name, as their teams list it, else their account.
    private func memberName(_ account: String) -> String {
        for p in scope.projects {
            if let m = p.team?.members?.first(where: { $0.account == account }), !m.name.isEmpty { return m.name }
        }
        return account
    }

    /// Why it wants you, as the web's swarm colors it (swarm.ts's itemTone).
    static func tone(_ kind: String) -> Tone {
        switch kind {
        case "agent_ask", "approval", "todo", "board_ask", "review_request", "dm", "thread_news": .accent
        case "permission", "deploy_approval", "changes_requested": .warn
        case "agent_failed", "checks_failed", "run_failed": .danger
        case "mention": .info
        case "assigned": .teal
        default: .muted
        }
    }

    /// An agent's ask as home's Needs you has it, to answer it the same way:
    /// a machine's pane over its link, a hosted session through control.
    private func need(_ i: InboxModel.Item) -> Need? {
        guard let project = scope.current else { return nil }
        let id: String
        if let m = i.subject.machine, let pane = i.subject.pane {
            id = "pane:\(m):\(pane.trimmingCharacters(in: CharacterSet(charactersIn: "%")))"
        } else if i.subject.id.hasPrefix("ses_") {
            id = "hosted:\(i.subject.id)"
        } else {
            return nil
        }
        return HomeState(project: project, fleet: fleet, model: model).needs.first { $0.id == id }
    }

    // ---- doing

    /// Mark it seen, then open it: a native issue, a thread or a DM here, a
    /// deploy's sheet, an agent's pane on the Overview, and anything else on
    /// Arugula's page or GitHub.
    private func open(_ i: InboxModel.Item) async {
        if !i.seen { Task { try? await inbox.mark(control, item: i.id, seen: true) } }
        let s = i.subject
        if s.id.hasPrefix("iss_") {
            path.append(IssueRef(id: s.id))
        } else if let id = s.room, let room = inbox.rooms.first(where: { $0.id == id }) {
            path.append(room)
        } else if s.type == "deploy" || i.kind == "deploy_approval" {
            // One sheet at a time: the deploy's opens once this one's gone.
            close()
            try? await Task.sleep(for: .milliseconds(450))
            approvals.open = .deploy(s.id)
        } else if let m = s.machine, let p = s.pane.flatMap({ UInt32($0.trimmingCharacters(in: CharacterSet(charactersIn: "%"))) }) {
            close()
            PushCenter.shared.route = .pane(machine: m, pane: p)
        } else if let u = URL(string: s.url), u.scheme != nil {
            openURL(u)
        } else if let p = await Platform.origin(control) {
            openURL(p.appending(path: s.url))
        }
    }

    /// Open a to-do in its row, or fold it; opening marks it seen.
    private func toggle(_ i: InboxModel.Item) {
        if expanded.remove(i.id) == nil { expanded.insert(i.id) }
        if !i.seen { Task { try? await inbox.mark(control, item: i.id, seen: true) } }
    }

    private func dismiss(_ i: InboxModel.Item) async {
        busy = i.id
        defer { busy = nil }
        do {
            try await inbox.mark(control, item: i.id, done: true)
        } catch {
            problem = describe(error)
        }
    }
}

/// How an item is answered in its row, by its `answer.via`: an agent's card
/// with its choice, a board's ask on its board, an agent's ask as Needs you
/// answers it. A to-do has its own (TodoAnswer); any other is opened.
private struct ItemAnswer: View {
    let item: InboxModel.Item
    let control: Control
    let need: Need?
    @ObservedObject var model: HomeModel
    let done: (String) -> Void
    @ObservedObject private var inbox = InboxModel.shared
    @State private var busy = false
    @State private var problem: String?
    @State private var words = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            content
            if let problem {
                Text(problem).font(Face.mono(11)).foregroundStyle(Palette.danger).lineLimit(3)
            }
        }
        .disabled(busy)
        .opacity(busy ? 0.5 : 1)
    }

    @ViewBuilder
    private var content: some View {
        let options = item.answer?.options ?? []
        switch item.answer?.via {
        case "card":
            choices(options.isEmpty ? ["Approve", "Deny"] : options) { c in
                try await inbox.answer(control, item: item.id, choice: c)
                return c
            }
        case "board_answer":
            if let board = item.subject.board {
                let ask = item.subject.id.hasPrefix("ask_") ? item.subject.id : item.id
                if options.isEmpty {
                    own { text in
                        _ = try await control.call("POST", "/api/boards/\(board)/asks/\(ask)/answer", body: ["text": text])
                        return "Answered"
                    }
                } else {
                    choices(options) { c in
                        _ = try await control.call("POST", "/api/boards/\(board)/asks/\(ask)/answer", body: ["option": c])
                        return c
                    }
                }
            }
        case "attention_act", "session_action":
            // The question stays on its machine: read over its link, as
            // home reads it.
            if let need {
                NeedAnswer(need: need, control: control, model: model, done: done)
            }
        default:
            EmptyView()
        }
    }

    /// A button per choice, the first one prominent.
    private func choices(_ all: [String], _ send: @escaping (String) async throws -> String) -> some View {
        FlowRow(spacing: 6) {
            ForEach(Array(all.enumerated()), id: \.offset) { i, c in
                Button(c) { run { try await send(c) } }
                    .buttonStyle(AnswerButton(primary: i == 0))
                    .accessibilityIdentifier("answer-\(c)")
            }
        }
    }

    /// The reader's own words, sent with Send.
    private func own(_ send: @escaping (String) async throws -> String) -> some View {
        HStack(spacing: 6) {
            TextField("Your answer", text: $words)
                .font(Face.ui(14))
                .padding(.horizontal, 10).padding(.vertical, 6)
                .background(Palette.surface, in: RoundedRectangle(cornerRadius: 8))
                .accessibilityIdentifier("answer-words")
            Button("Send") {
                let t = words.trimmingCharacters(in: .whitespacesAndNewlines)
                run { try await send(t) }
            }
            .buttonStyle(AnswerButton(primary: true))
            .disabled(words.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            .accessibilityIdentifier("answer-send")
        }
    }

    private func run(_ send: @escaping () async throws -> String) {
        busy = true
        problem = nil
        Task {
            do {
                done(try await send())
            } catch {
                problem = describe(error)
            }
            busy = false
        }
    }
}
