// Home (Arugula #1255): the project's conversations, sorted by what needs you,
// as the desktop's Overview reads them (web/src/app/overview.tsx). Needs you
// first, merged from every machine's panes, the boards' rows and the hosted
// sessions, worst first; then one row per open board, with its lane bar,
// its count, its lead and what's going on in words; then the people, its
// channels and DMs. A board's row opens the board's thread, whose Board
// button opens its lanes. The tabs are a menu away. On an iPad home is the
// left column of a split view, and what it opens sits beside it. A row of
// Needs you is answered where it is (Answer.swift), and a session opens a
// peek.

import Combine
import SwiftUI

/// What home opens beside it (on an iPhone, over it).
enum Opened: Hashable {
    case board(Board)
    case room(ChatRoom)
    case session(SessionWave.Row)
}

/// The tabs of the app before home, still a menu away.
enum AppTab: String, CaseIterable, Identifiable {
    case issues = "Issues", boards = "Boards", chat = "Chat", machines = "Machines", you = "You"
    var id: String { rawValue }
    var symbol: String {
        switch self {
        case .issues: "smallcircle.filled.circle"
        case .boards: "rectangle.split.3x1"
        case .chat: "bubble.left.and.bubble.right"
        case .machines: "terminal"
        case .you: "person.crop.circle"
        }
    }
}

private struct GoHomeKey: EnvironmentKey {
    static let defaultValue: (() -> Void)? = nil
}

extension EnvironmentValues {
    /// Back to home from a tab.
    var goHome: (() -> Void)? {
        get { self[GoHomeKey.self] }
        set { self[GoHomeKey.self] = newValue }
    }
}

// ---- the machines, as the web's fleet (web/src/shell/fleet.ts)

/// A link to each of the account's machines that's online, kept while the
/// app runs, for what their panes are up to. Opened a few at a time.
@MainActor
final class HomeFleet: ObservableObject {
    @Published private(set) var machines: [MachineSession] = []
    private var watching: [String: AnyCancellable] = [:]
    private static let atOnce = 4

    func load(_ control: Control) async {
        guard let d = try? await control.get("/api/directory", as: Directory.self) else { return }
        let online = d.daemons.filter { $0.online == true }
        let ids = Set(online.map(\.id))
        for gone in machines where !ids.contains(gone.machine.id) {
            gone.disconnect()
            watching[gone.machine.id] = nil
        }
        var kept = machines.filter { ids.contains($0.machine.id) }
        for m in online where !kept.contains(where: { $0.machine.id == m.id }) {
            let s = MachineSession(control: control, machine: m)
            watching[m.id] = s.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
            kept.append(s)
        }
        machines = kept
        await reconnect()
    }

    /// Open every link that isn't, a few at a time.
    func reconnect() async {
        let closed = machines.filter { !$0.connected }
        for start in stride(from: 0, to: closed.count, by: Self.atOnce) {
            await withTaskGroup(of: Void.self) { g in
                for s in closed[start..<min(start + Self.atOnce, closed.count)] {
                    g.addTask { await s.connect() }
                }
            }
        }
    }
}

// ---- what home reads from control

@MainActor
final class HomeModel: ObservableObject {
    /// The project's open boards, as its list orders them.
    @Published private(set) var boards: [Board] = []
    /// Each open board's rows, by its id.
    @Published private(set) var views: [String: BoardDetail] = [:]
    @Published private(set) var rooms: [ChatRoom] = []
    @Published private(set) var hosted: [SessionWave.Row] = []
    /// What each hosted session that waits on you asks, and whether you may answer.
    @Published private(set) var opened: [String: SessionView] = [:]
    /// Each open board's thread, by the board's id: its changes come on it.
    @Published private(set) var threads: [String: ChatRoom] = [:]
    @Published private(set) var problem: String?
    @Published private(set) var loaded = false
    /// When it last asked, to space out what the stream sets off.
    private(set) var loadedAt: Date?
    private var project: String?

    func load(_ control: Control, project: String) async {
        loadedAt = Date()
        if project != self.project {
            self.project = project
            boards = []
            views = [:]
            hosted = []
            loaded = false
        }
        async let b = control.get("/api/projects/\(project)/boards", as: BoardList.self)
        async let r = control.get("/api/chat", as: ChatRooms.self)
        async let s = control.get("/api/projects/\(project)/sessions", as: SessionWave.self)
        do {
            let open = try await b.boards.filter { $0.closed_at == nil }
            var read: [String: BoardDetail] = [:]
            await withTaskGroup(of: (String, BoardDetail?).self) { g in
                for board in open {
                    g.addTask { (board.id, try? await control.get("/api/boards/\(board.id)", as: BoardDetail.self)) }
                }
                for await (id, v) in g { if let v { read[id] = v } }
            }
            guard self.project == project else { return }
            boards = open
            views = read
            problem = nil
        } catch is CancellationError {
            return
        } catch {
            problem = describe(error)
        }
        if let rooms = try? await r.rooms { self.rooms = rooms }
        // A hub or an account without hosted sessions has none to wait on.
        hosted = (try? await s.sessions) ?? []
        var asks: [String: SessionView] = [:]
        for w in hosted where w.waiting != nil {
            if let v = try? await control.get("/api/sessions/\(w.session)", as: SessionView.self) { asks[w.session] = v }
        }
        opened = asks
        loaded = true
        await openThreads(control)
    }

    /// Load again, but no sooner than five seconds after the last: what the
    /// stream sets off in a burst comes to one load.
    func soon(_ control: Control, project: String) async {
        guard !pending else { return }
        pending = true
        let wait = 5 - Date().timeIntervalSince(loadedAt ?? .distantPast)
        if wait > 0 { try? await Task.sleep(for: .seconds(wait)) }
        pending = false
        await load(control, project: project)
    }
    private var pending = false

    /// The board's thread, made the first time someone opens it.
    func thread(_ control: Control, board: Board) async throws -> ChatRoom {
        if let t = threads[board.id] { return t }
        let d = try await control.call("POST", "/api/chat/targets", body: ["machine": "", "kind": "object", "target": board.id])
        let room = try JSONDecoder().decode(ChatRoom.self, from: d)
        threads[board.id] = room
        return room
    }

    /// Open each board's thread once, as the web's Overview does: a board's
    /// change comes as a change on its thread.
    private func openThreads(_ control: Control) async {
        for b in boards where threads[b.id] == nil {
            _ = try? await thread(control, board: b)
        }
    }

    /// The board whose thread this room is.
    func board(ofThread room: String) -> Board? {
        guard let id = threads.first(where: { $0.value.id == room })?.key else { return nil }
        return boards.first { $0.id == id }
    }
}

// ---- how things stand, in the Overview's words (web/src/shell/swarm/overview.ts)

/// How a card or a session stands, most urgent first.
enum Standing: Int, Comparable {
    case needs, asks, failed, checksFailing, quiet, working, checksRunning, review, idle

    static func < (a: Standing, b: Standing) -> Bool { a.rawValue < b.rawValue }

    /// The one word a row says of how it stands.
    var word: String {
        switch self {
        case .needs: "needs you"
        case .asks: "asks you"
        case .failed: "failed"
        case .checksFailing: "checks failing"
        case .quiet: "quiet"
        case .working: "working"
        case .checksRunning: "checks running"
        case .review: "in review"
        case .idle: ""
        }
    }

    var tone: Tone {
        switch self {
        case .needs, .asks: .accent
        case .failed, .checksFailing: .danger
        case .quiet: .warn
        case .working: .ok
        case .checksRunning, .review: .info
        case .idle: .muted
        }
    }
}

extension PaneRow {
    /// It needs you: it asks, or it failed (web/src/shared/needs-you.ts).
    var needsYou: Bool {
        attention == "needs_input" || (attention == "done" && failed)
    }

    var failed: Bool { reason?.kind == "failed" || reason?.kind == "exited" }

    /// Its place among what wants you: a gate first, then the other asks, then failures.
    var needsYouRank: Int {
        if attention == "needs_input" { return reason?.kind == "gate" ? 0 : 1 }
        return failed ? 2 : 3
    }

    var standing: Standing {
        if needsYou { return needsYouRank <= 1 ? .needs : .failed }
        return attention == "working" ? .working : .quiet
    }

    var isAgent: Bool { type == "agent" || work == "agent" }
}

extension SessionWave.Row {
    var standing: Standing {
        if waiting != nil { return .needs }
        switch state {
        case "failed", "error", "unresponsive": return .failed
        case "working", "running", "queued": return .working
        default: return .quiet
        }
    }
}

extension BoardDetail.Row {
    /// What a row says of itself, besides its sessions.
    var says: [Standing] {
        var out: [Standing] = []
        if ask?.mine == true { out.append(.asks) }
        if marks?.contains(where: { $0.kind == "stalled" }) == true { out.append(.quiet) }
        let work = (attachments ?? []).filter { $0.role == "work" && $0.released != true }.compactMap(\.live)
        for l in ([live].compactMap { $0 } + work) where l.kind == "pr" {
            switch l.state {
            case "checks_failed": out.append(.checksFailing)
            case "checks_running": out.append(.checksRunning)
            case "open": out.append(.review)
            default: break
            }
        }
        if lane == "lane_review" || lane == "review" { out.append(.review) }
        return out
    }

    var inFlight: Bool { ["doing", "review", "lane_doing", "lane_review"].contains(lane ?? "") }

    /// `#983 · Its title`, or its note's first line.
    var words: String {
        let label: String? = live.flatMap { l in
            guard let n = l.number, n > 0 else { return nil }
            return l.kind == "native" ? "#\(n)" : "\(l.repo ?? "")#\(n)"
        }
        let title = live?.title ?? note?.split(separator: "\n").first.map(String.init) ?? ""
        let w = [label, title].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
        return w.isEmpty ? "A card" : w
    }
}

/// One thing that needs you, as a row of home's first section.
struct Need: Identifiable {
    enum Target {
        case pane(MachineSession, PaneRow)
        /// A board's row: its ask, its failing checks, its review.
        case board(Board, BoardDetail.Row?)
        case session(SessionWave.Row)
    }
    let id: String
    /// What it asks, exactly: answered, the row folds away until it asks
    /// something else.
    let ask: String
    let rank: Int
    let since: Int
    /// Who or what: an agent, a board's asker, a PR.
    let who: String
    let standing: Standing
    /// Where: its machine or board, and its card.
    let on: String
    let text: String
    let mono: Bool
    let target: Target
}

/// A session working on one of a board's cards, as its thread lists it.
struct BoardSession: Identifiable {
    let peeked: Peeked
    let name: String
    /// The card: `#983 · Its title`.
    let card: String
    let standing: Standing
    var id: String { peeked.id }
}

/// One open board as home's row reads it.
struct BoardLine: Identifiable {
    let board: Board
    let done: Int, review: Int, doing: Int, total: Int
    let standing: Standing
    /// What's going on, the worst first: `1 needs you · 2 working`.
    let says: [(Standing, String)]
    let still: Bool
    var id: String { board.id }
}

/// What home shows, from what the machines, the boards and the hosted
/// sessions say: worked out each time any of them moves.
struct HomeState {
    var needs: [Need] = []
    var boards: [BoardLine] = []
    /// The sessions on each board's cards in flight, by the board's id.
    var sessions: [String: [BoardSession]] = [:]

    init() {}

    @MainActor
    init(project: Projects.Project, fleet: HomeFleet, model: HomeModel) {
        let personal = project.team == nil
        // The project's panes: its sessions', its cards', or none's in the personal project.
        var panes: [(MachineSession, PaneRow)] = []
        for m in fleet.machines {
            for p in m.panes + m.agents {
                let mine = p.session_project?.id == project.id || p.on_issue?.project == project.id
                    || (personal && p.session_project == nil && p.on_issue?.project == nil)
                if mine { panes.append((m, p)) }
            }
        }

        for (m, p) in panes where p.needsYou {
            let card = p.on_issue.map { " · \($0.issue)" } ?? ""
            let text = p.reason?.headline.isEmpty == false ? p.reason!.headline : (p.ask?.message ?? "")
            needs.append(Need(
                id: "pane:\(m.machine.id):\(p.id)", ask: p.asking?.id ?? "\(p.reason?.kind ?? ""):\(p.reason?.since_ms ?? 0)",
                rank: p.needsYouRank,
                since: p.reason?.since_ms ?? p.ask?.at_ms ?? 0,
                who: m.conversations[p.id] ?? p.name, standing: p.standing, on: m.machine.name + card,
                text: text, mono: p.asking?.approve == true || p.failed, target: .pane(m, p)))
        }

        for b in model.boards {
            guard let v = model.views[b.id] else { continue }
            for r in v.zones.flatMap(\.rows) where !(r.lane ?? "").hasSuffix("done") {
                let where_ = "\(b.title) · \(r.words)"
                if let a = r.ask, a.mine {
                    needs.append(Need(
                        id: "ask:\(b.id):\(r.id)", ask: a.id, rank: 1, since: a.created_at ?? 0,
                        who: (a.asked_by ?? "").isEmpty ? "The board" : a.asked_by!, standing: .asks, on: where_,
                        text: a.question, mono: false, target: .board(b, r)))
                } else if let worst = r.says.min(), worst == .checksFailing || worst == .review {
                    needs.append(Need(
                        id: "row:\(b.id):\(r.id)", ask: "\(worst)", rank: worst == .checksFailing ? 2 : 3, since: r.updated_at ?? 0,
                        who: r.words, standing: worst, on: b.title,
                        text: worst == .checksFailing ? "Its checks fail." : "It's ready for review.", mono: false,
                        target: .board(b, r)))
                }
            }
        }

        for s in model.hosted {
            guard let waiting = s.waiting else { continue }
            let asks = waiting.contains("question") || waiting.contains("permission")
            needs.append(Need(
                id: "hosted:\(s.session)", ask: model.opened[s.session]?.card.open?.request ?? waiting, rank: asks ? 1 : 2, since: s.updated_at,
                who: s.issue.map { "#\($0.number) \($0.title)" } ?? "A session", standing: .needs,
                on: "Arugula cloud", text: s.question ?? waiting, mono: false, target: .session(s)))
        }
        needs.sort { ($0.rank, $0.since, $0.id) < ($1.rank, $1.since, $1.id) }

        for b in model.boards {
            let v = model.views[b.id]
            let rows = v?.zones.flatMap(\.rows) ?? []
            var on: [BoardSession] = []
            let flight = rows.filter(\.inFlight).map { r -> Standing in
                var all = r.says
                for (m, p) in panes where Self.on(r, board: b.id, machine: m.machine.id, pane: p) {
                    all.append(p.standing)
                    on.append(BoardSession(peeked: .pane(m, p), name: m.conversations[p.id] ?? p.name, card: r.words, standing: p.standing))
                }
                for s in model.hosted where s.issue?.id == r.pointer?.id && r.pointer?.kind == "issue" {
                    all.append(s.standing)
                    on.append(BoardSession(peeked: .hosted(s), name: "Arugula cloud", card: r.words, standing: s.standing))
                }
                return all.min() ?? .idle
            }
            sessions[b.id] = on.sorted { ($0.standing, $0.name) < ($1.standing, $1.name) }
            var says = [Standing.needs, .asks, .failed, .checksFailing, .quiet, .working, .checksRunning, .review]
                .map { st in (st, flight.filter { $0 == st }.count) }
                .filter { $0.1 > 0 }
                .map { ($0.0, "\($0.1) \($0.0.word)") }
            let idle = flight.filter { $0 == .idle }.count
            if flight.isEmpty {
                says = [(.idle, "nothing in flight")]
            } else if idle > 0 {
                says.append((.idle, says.isEmpty ? "\(idle) in flight" : "\(idle) more in flight"))
            }
            let p = b.progress
            boards.append(BoardLine(
                board: b, done: p?.done ?? 0, review: p?.review ?? 0, doing: p?.doing ?? 0, total: p?.total ?? 0,
                standing: flight.min() ?? .idle, says: says, still: flight.isEmpty))
        }
        boards.sort { a, b in
            (a.still ? 1 : 0, a.standing, a.board.title.lowercased()) < (b.still ? 1 : 0, b.standing, b.board.title.lowercased())
        }
    }

    /// Whether a pane works on a row: its agent's card, or the row's
    /// attachment of its block.
    private static func on(_ r: BoardDetail.Row, board: String, machine: String, pane p: PaneRow) -> Bool {
        if let on = p.on_issue {
            if on.boards?.contains(where: { $0.board == board && $0.row == r.id }) == true { return true }
            if r.pointer?.kind == "issue", on.issue == r.pointer?.id { return true }
            if r.live?.kind == "native", let n = r.live?.number, on.issue.trimmingCharacters(in: .whitespaces) == "#\(n)" { return true }
        }
        return (r.attachments ?? []).contains { a in
            a.role == "work" && a.released != true && a.pointer.kind == "block" && a.pointer.machine == machine
                && a.pointer.pane?.trimmingCharacters(in: CharacterSet(charactersIn: "%")) == String(p.id)
        }
    }
}

// ---- home

struct PhoneHome: View {
    let control: Control
    @ObservedObject var model: HomeModel
    @ObservedObject var fleet: HomeFleet
    @ObservedObject var live: LiveChat
    let go: (AppTab) -> Void
    @EnvironmentObject private var scope: Scope
    @Environment(\.scenePhase) private var phase
    @State private var opened: Opened?
    @State private var columns = NavigationSplitViewVisibility.all
    @State private var terminal: PaneOpen?
    @State private var path = NavigationPath()
    /// What was answered here, by need and ask: folded away.
    @State private var folded: Set<String> = []
    @State private var said: String?
    @State private var peek: Peeked?

    /// A pane's terminal, over everything.
    struct PaneOpen: Identifiable {
        let session: MachineSession
        let pane: PaneRow
        var id: String { "\(session.machine.id):\(pane.id)" }
    }

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            list
                .homeHeading(go: go)
                .background(Palette.void)
        } detail: {
            NavigationStack(path: $path) {
                Group {
                    switch opened {
                    case .board(let b):
                        BoardThread(control: control, board: b, model: model, live: live, sessions: state.sessions[b.id] ?? []) { peek = $0 }
                    case .room(let r): ChatRoomView(control: control, room: r, live: live)
                    case .session(let s): ChatRoomView(control: control, room: s.thread, live: live, thread: s.parent)
                    case nil: Empty(text: "Pick a conversation", symbol: "bubble.left.and.bubble.right")
                    }
                }
                .navigationDestination(for: BoardLanes.self) { BoardScreen(control: control, board: $0.board) }
                .navigationDestination(for: IssueRef.self) { ref in
                    Loaded(load: { try await control.get("/api/issues/\(ref.id)", as: Issue.self) }) { IssueView(issue: $0) }
                }
            }
            .id(opened)
        }
        .navigationSplitViewStyle(.balanced)
        .fullScreenCover(item: $terminal) { t in TerminalScreen(session: t.session, pane: t.pane) }
        .sheet(item: $peek) { p in
            SessionPeek(
                control: control, peeked: p,
                openPane: { m, pane in
                    // After the sheet has gone: one presentation at a time.
                    Task {
                        try? await Task.sleep(for: .milliseconds(450))
                        terminal = PaneOpen(session: m, pane: pane)
                    }
                },
                openThread: { opened = .session($0) },
                toast: show)
        }
        .overlay(alignment: .bottom) {
            if let said { Toast(text: said).transition(.opacity).allowsHitTesting(false) }
        }
        .onChange(of: opened) { _, _ in path = NavigationPath() }
        .onChange(of: scope.current?.id) { _, _ in opened = nil }
        .task {
            live.start()
            await fleet.load(control)
        }
        // Control's word, again every half minute; the stream says sooner.
        .task(id: scope.current?.id) {
            while !Task.isCancelled {
                await refresh()
                try? await Task.sleep(for: .seconds(30))
            }
        }
        // A message anywhere moves the people's counts; a board's thread
        // changing is the board changing. At most every few seconds.
        .onChange(of: live.rooms) { _, _ in Task { await refresh(soon: true) } }
        .onChange(of: live.changed) { was, now in
            if now.keys.contains(where: { now[$0] != was[$0] && model.board(ofThread: $0) != nil }) {
                Task { await refresh(soon: true) }
            }
        }
        .onChange(of: phase) { _, now in
            if now == .active {
                live.start()
                Task {
                    await fleet.load(control)
                    await refresh()
                }
            }
        }
    }

    private func refresh(soon: Bool = false) async {
        guard let p = scope.current else { return }
        if soon { await model.soon(control, project: p.id) } else { await model.load(control, project: p.id) }
    }

    private var state: HomeState {
        scope.current.map { HomeState(project: $0, fleet: fleet, model: model) } ?? HomeState()
    }

    /// A short word of what was done.
    private func show(_ text: String) {
        withAnimation { said = text }
        Task {
            try? await Task.sleep(for: .seconds(1.8))
            if said == text { withAnimation { said = nil } }
        }
    }

    private var list: some View {
        let state = self.state
        let needs = state.needs.filter { !folded.contains("\($0.id)|\($0.ask)") }
        let people = model.rooms.filter { $0.isConversation && !$0.archived && scope.showsRoom($0) }
            .sorted { ($0.unread > 0 ? 1 : 0, $0.last) > ($1.unread > 0 ? 1 : 0, $1.last) }
        return List(selection: $opened) {
            Section {
                if needs.isEmpty {
                    Text(model.loaded ? "Nothing needs you." : "Looking…")
                        .font(Face.ui(14)).foregroundStyle(Palette.dim)
                        .voidRow()
                        .accessibilityIdentifier("needs-none")
                }
                ForEach(needs) { n in needRow(n) }
            } header: {
                Caps(needs.isEmpty ? NEEDS_YOU : "\(NEEDS_YOU) · \(needs.count)")
            }
            Section {
                if model.loaded && state.boards.isEmpty {
                    Text("No open boards").font(Face.ui(14)).foregroundStyle(Palette.dim).voidRow()
                }
                ForEach(state.boards) { b in
                    NavigationLink(value: Opened.board(b.board)) { BoardLineRow(line: b) }
                        .voidRow()
                        .accessibilityIdentifier("home-board-\(b.board.title)")
                }
            } header: {
                Caps("Boards")
            }
            Section {
                ForEach(people) { r in
                    NavigationLink(value: Opened.room(r)) { PersonRow(room: r, open: opened == .room(r)) }
                        .voidRow()
                        .accessibilityIdentifier("home-room-\(r.name)")
                }
            } header: {
                Caps("People")
            }
        }
        .voidList()
        .refreshable {
            await fleet.load(control)
            await refresh()
        }
    }

    /// A row of Needs you: tapped, a session's peek or a board's thread;
    /// its answer under it.
    private func needRow(_ n: Need) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Button {
                switch n.target {
                case .pane(let m, let p): peek = .pane(m, p)
                case .board(let b, _): opened = .board(b)
                case .session(let s): peek = .hosted(s)
                }
            } label: {
                NeedRow(need: n)
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("need-\(n.id)")
            NeedAnswer(need: n, control: control, model: model) { said in
                withAnimation { _ = folded.insert("\(n.id)|\(n.ask)") }
                show("\(n.who): \(said)")
                Task { await refresh(soon: true) }
            }
        }
        .voidRow()
    }
}

/// The label for what needs you, as the web's (web/src/shared/needs-you.ts).
let NEEDS_YOU = "Needs you"

/// Home's heading: the project, as a menu of every project and of the tabs.
struct HomeHeading: ViewModifier {
    let go: (AppTab) -> Void
    @EnvironmentObject private var scope: Scope

    func body(content: Content) -> some View {
        content
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .safeAreaInset(edge: .top, spacing: 0) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(alignment: .firstTextBaseline) {
                        Menu {
                            Section("Projects") {
                                ForEach(scope.projects) { p in
                                    Button { scope.pick(p) } label: {
                                        if p == scope.current { Label(p.name, systemImage: "checkmark") } else { Text(p.name) }
                                    }
                                }
                            }
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Text((scope.current?.name ?? "Arugula").uppercased())
                                    .font(Face.display(30)).foregroundStyle(Palette.fg).lineLimit(1)
                                Image(systemName: "chevron.down").font(.system(size: 13, weight: .bold)).foregroundStyle(Palette.accent)
                            }
                        }
                        .accessibilityIdentifier("project-switcher")
                        Spacer(minLength: 8)
                        Menu {
                            ForEach(AppTab.allCases) { t in
                                Button { go(t) } label: { Label(t.rawValue, systemImage: t.symbol) }
                                    .accessibilityIdentifier("go-\(t.rawValue)")
                            }
                        } label: {
                            Image(systemName: "square.grid.2x2").font(.system(size: 18, weight: .semibold)).foregroundStyle(Palette.accent)
                                .frame(width: 36, height: 36)
                        }
                        .accessibilityLabel("Go to")
                        .accessibilityIdentifier("home-menu")
                    }
                    if let team = scope.current?.team {
                        Caps("\(team.name) · \(team.role)")
                    }
                    if let problem = scope.problem {
                        Text(problem).font(Face.mono(11)).foregroundStyle(Palette.danger).lineLimit(2)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal)
                .padding(.bottom, 10)
                .background(Palette.void)
            }
    }
}

extension View {
    func homeHeading(go: @escaping (AppTab) -> Void) -> some View { modifier(HomeHeading(go: go)) }
}

/// A row of Needs you: a dot, who, how it stands, where, and what it asks.
struct NeedRow: View {
    let need: Need

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 7) {
                Circle().fill(need.standing.tone.color).frame(width: 7, height: 7)
                Text(need.who).font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                Spacer(minLength: 4)
                Text(need.standing.word).font(Face.mono(11, "Bold")).foregroundStyle(need.standing.tone.color)
            }
            Text(need.on).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
            if !need.text.isEmpty {
                Text(need.text)
                    .font(need.mono ? Face.mono(12) : Face.ui(14))
                    .foregroundStyle(Palette.subtle)
                    .lineLimit(3)
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

/// A board on home: its title and count, its lane bar, what's going on and
/// who leads it.
struct BoardLineRow: View {
    let line: BoardLine

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(line.board.title).font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(2)
                Spacer(minLength: 4)
                Text(verbatim: line.total > 0 ? "\(line.done)/\(line.total)" : "–")
                    .font(Face.mono(12, "Bold")).foregroundStyle(Palette.dim)
                    .accessibilityIdentifier("board-count")
            }
            LaneBar(done: line.done, review: line.review, doing: line.doing, total: max(line.total, 1))
            line.says.enumerated().reduce(Text("")) { acc, x in
                let (i, s) = x
                let word = Text(s.1).foregroundColor(s.0 == .idle ? Palette.dim : s.0.tone.color)
                return i == 0 ? word : acc + Text(" · ").foregroundColor(Palette.dim) + word
            }
            .font(Face.ui(13))
            .lineLimit(2)
            Text(BoardLineRow.lead(line.board.lead)).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
        }
        .padding(.vertical, 4)
    }

    /// Who leads it: a session on a machine, or nobody.
    static func lead(_ l: Board.Lead?) -> String {
        guard let l else { return "No lead" }
        let on = (l.machine_name ?? "").isEmpty ? "" : " on \(l.machine_name!)"
        return "Led by \(l.session)\(on)"
    }
}

/// A channel or DM on home, with how many messages wait in it.
struct PersonRow: View {
    let room: ChatRoom
    let open: Bool

    var body: some View {
        HStack(spacing: 8) {
            if room.kind == "channel" {
                Text("#").font(Face.mono(15, "Bold")).foregroundStyle(Palette.dim).frame(width: 20)
            } else {
                Avatar(name: room.title, size: 20)
            }
            Text(room.kind == "channel" ? room.name : room.title)
                .font(Face.ui(15, room.unread > 0 && !open ? "Bold" : "Regular"))
                .foregroundStyle(room.unread > 0 && !open ? Palette.fg : Palette.subtle)
                .lineLimit(1)
            Spacer()
            if room.unread > 0 && !open {
                Text(verbatim: "\(room.unread)")
                    .font(Face.mono(11, "Bold"))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 1)
                    .background(room.mentions > 0 ? Palette.danger : Palette.accent, in: Capsule())
                    .foregroundStyle(Palette.void)
            }
        }
    }
}

// ---- a board's thread

/// A board's lanes, pushed from its thread.
struct BoardLanes: Hashable { let board: Board }

/// A board's thread: its lane bar and who leads it over the conversation,
/// which reaches the session that leads it, and the sessions on its cards,
/// each opening its peek. Board opens its lanes.
struct BoardThread: View {
    let control: Control
    let board: Board
    @ObservedObject var model: HomeModel
    @ObservedObject var live: LiveChat
    let sessions: [BoardSession]
    let peek: (Peeked) -> Void
    @State private var room: ChatRoom?
    @State private var problem: String?

    var body: some View {
        Group {
            if let room {
                ChatRoomView(control: control, room: room, live: live, title: board.title, heading: AnyView(heading))
            } else if let problem {
                Problem(message: problem) { Task { await open() } }
            } else {
                ProgressView().tint(Palette.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Palette.bg)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                NavigationLink(value: BoardLanes(board: board)) {
                    Text("Board").font(Face.mono(12, "Bold"))
                }
                .accessibilityIdentifier("open-board")
            }
        }
        .task { await open() }
    }

    private func open() async {
        do {
            room = try await model.thread(control, board: board)
            problem = nil
        } catch {
            problem = describe(error)
        }
    }

    private var heading: some View {
        let line = model.boards.first { $0.id == board.id } ?? board
        let p = line.progress
        let total = p?.total ?? 0, done = p?.done ?? 0, review = p?.review ?? 0, doing = p?.doing ?? 0
        return VStack(alignment: .leading, spacing: 5) {
            LaneBar(done: done, review: review, doing: doing, total: max(total, 1))
            Text("\(done) done · \(review) review · \(doing) doing · \(max(0, total - done - review - doing)) next")
                .font(Face.mono(11)).foregroundStyle(Palette.dim)
            Text(BoardLineRow.lead(line.lead)).font(Face.mono(11)).foregroundStyle(Palette.subtle)
                .accessibilityIdentifier("board-lead")
            if !sessions.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(sessions) { s in
                            Button { peek(s.peeked) } label: {
                                VStack(alignment: .leading, spacing: 2) {
                                    HStack(spacing: 6) {
                                        Circle().fill(s.standing.tone.color).frame(width: 6, height: 6)
                                        Text(s.name).font(Face.ui(13, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                                    }
                                    Text("\(s.card) · \(s.standing == .idle ? "quiet" : s.standing.word)")
                                        .font(Face.mono(10)).foregroundStyle(Palette.dim).lineLimit(1)
                                }
                                .frame(maxWidth: 240, alignment: .leading)
                                .card(s.standing.tone)
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("board-session-\(s.id)")
                        }
                    }
                }
            }
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Palette.void)
    }
}

extension SessionWave.Row {
    /// Its thread's room, for the chat view.
    var thread: ChatRoom {
        ChatRoom(
            id: room, kind: "thread", name: issue.map { "#\($0.number) \($0.title)" } ?? "Session", last: 0, unread: 0,
            mentions: 0, archived: false, writable: true)
    }
}
