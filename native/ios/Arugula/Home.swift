// Home (Arugula #1255): the project's conversations, sorted by what needs you,
// as the desktop's Overview reads them (web/src/app/overview.tsx). In a team's
// project the chief of staff comes first (Arugula #1279): the reader's staff
// room and who leads it. Then Needs you, merged from every machine's panes,
// the boards' rows and the hosted sessions, worst first; then one row per open
// board, with its lane bar, its count, its lead and what's going on in words;
// then the people, its channels and DMs. Home is the Overview's Project level
// (Arugula #1304): a board's row zooms to its Board level and a ticket there
// to its Work level (Zoom.swift), each at an address a link or a notification
// opens. Home is the first of the app's tabs (Arugula #1262). On an iPad home
// is the left column of a split view, and what it opens sits beside it. A row of Needs you is answered where it is
// (Answer.swift), and a session opens a peek. New tells a board or starts a
// New idea (New.swift), whose conversations are listed under New ideas.

import Combine
import SwiftUI

/// What home opens beside it (on an iPhone, over it).
enum Opened: Hashable {
    /// A board's Board level (Arugula #1304).
    case board(Board)
    /// A board's thread: telling a board opens it.
    case thread(Board)
    /// The Work level of a session on no card, by its key.
    case work(String)
    case room(ChatRoom)
    /// The reader's staff room in a project (Arugula #1275): where they talk
    /// to their chief of staff.
    case staff(project: String)
    case session(SessionWave.Row)
    /// A New idea's conversation: its session's thread (`room`, and the
    /// message it hangs off), or the issue's thread when it has no session.
    case idea(room: String?, parent: Int?, issue: String? = nil, title: String)
    /// A native issue, by its id: a notification about one opens it here.
    case issue(String)
}

/// The app's tabs, always on its bar (Arugula #1262): home first. Issues
/// aren't a tab: a board's rows open theirs, and the project's menu opens
/// the list.
enum AppTab: String, CaseIterable, Identifiable {
    case home = "Home", boards = "Boards", chat = "Chat", machines = "Machines", you = "You"
    var id: String { rawValue }
    var symbol: String {
        switch self {
        case .home: "house"
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

private struct ShowIssuesKey: EnvironmentKey {
    static let defaultValue: (() -> Void)? = nil
}

extension EnvironmentValues {
    /// Select the Home tab.
    var goHome: (() -> Void)? {
        get { self[GoHomeKey.self] }
        set { self[GoHomeKey.self] = newValue }
    }

    /// Open the current project's issues, from its menu.
    var showIssues: (() -> Void)? {
        get { self[ShowIssuesKey.self] }
        set { self[ShowIssuesKey.self] = newValue }
    }
}

// ---- the machines, as the web's fleet (web/src/shell/fleet.ts)

/// A link to each of the account's machines that's online, kept while the
/// app runs, for what their panes are up to. Opened a few at a time.
@MainActor
final class HomeFleet: ObservableObject {
    @Published private(set) var machines: [MachineSession] = []
    /// Every machine the directory lists, online or not: whose each is.
    @Published private(set) var directory: [Machine] = []
    private var watching: [String: AnyCancellable] = [:]
    private static let atOnce = 4

    func load(_ control: Control) async {
        guard let d = try? await control.get("/api/directory", as: Directory.self) else { return }
        directory = d.daemons
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
    /// The project's open New ideas: its issues titled "Idea: …".
    @Published private(set) var ideas: [Issue] = []
    /// What the reader is called, for an idea's agent.
    @Published private(set) var me: String?
    /// When it last asked, to space out what the stream sets off.
    private(set) var loadedAt: Date?
    /// The project it reads.
    private(set) var project: String?

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
        async let i = control.get("/api/projects/\(project)/issues", as: IssueList.self)
        async let who = control.get("/api/projects", as: Projects.self)
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
        ideas = ((try? await i.issues) ?? []).filter { $0.title.hasPrefix("Idea: ") }
        if me == nil { me = try? await who.me.name }
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
        // Another project was picked meanwhile: its own load has run, and
        // this one would put the last project's back.
        guard project == self.project else { return }
        await load(control, project: project)
    }
    private var pending = false

    /// The reader's staff room in a project, as the rooms list has it: its
    /// lead and unread count. None until it's first opened.
    func staffListed(project: String) -> ChatRoom? {
        rooms.first { $0.isStaff && $0.project == project }
    }

    /// The reader's staff room in a team's project (Arugula #1275), made the
    /// first time it's opened.
    func staff(_ control: Control, project: String) async throws -> ChatRoom {
        let d = try await control.call("POST", "/api/chat/projects/\(project)/staff")
        let room = try JSONDecoder().decode(ChatRoom.self, from: d)
        if !rooms.contains(room) { rooms.append(room) }
        return room
    }

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

/// How a card or a session stands, most urgent first. `waits`: it, or its
/// question, waits on someone else (Arugula #1392).
enum Standing: Int, Comparable {
    case needs, asks, failed, checksFailing, waits, quiet, working, checksRunning, review, idle

    static func < (a: Standing, b: Standing) -> Bool { a.rawValue < b.rawValue }

    /// The one word a row says of how it stands.
    var word: String {
        switch self {
        case .needs: "needs you"
        case .asks: "asks you"
        case .failed: "failed"
        case .checksFailing: "checks failing"
        case .waits: "asks someone else"
        case .quiet: "quiet"
        case .working: "working"
        case .checksRunning: "checks running"
        case .review: "in review"
        case .idle: ""
        }
    }

    /// Its word, naming whom it waits on when that's someone else: `asks
    /// Aaron`, or `asks someone` where that isn't known.
    func word(asks who: String?) -> String {
        guard self == .waits else { return word }
        return "asks \((who ?? "").isEmpty ? "someone" : who!)"
    }

    var tone: Tone {
        switch self {
        case .needs, .asks: .accent
        case .failed, .checksFailing: .danger
        case .quiet: .warn
        case .working: .ok
        case .waits, .checksRunning, .review: .info
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

extension MachineSession {
    /// Whose it is when it isn't the reader's: its owner's name (Arugula #1392).
    var owner: String? { machine.owner_name.flatMap { $0.isEmpty ? nil : $0 } }

    /// How a pane stands for the reader: one on someone else's machine
    /// that needs them waits on them, not on you (Arugula #1392).
    func standing(of p: PaneRow) -> Standing {
        p.standing == .needs && owner != nil ? .waits : p.standing
    }
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
        if let a = ask { if a.mine { out.append(.asks) } else if !(a.who ?? "").isEmpty { out.append(.waits) } }
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
    /// Whom it waits on, when that's someone else (`waits`).
    var asks: String? = nil
    var id: String { peeked.id }
    var word: String { standing == .idle ? "quiet" : standing.word(asks: asks) }
}

/// A lead, a board's (#414) or a staff room's (Arugula #1275), as its chip
/// reads it, as the web's (web/src/shell/swarm/overview.ts `boardLead`): how
/// its session stands from its panes on its machine (none when no machine
/// here tells), its name, a teammate's with theirs, and its machine. Its
/// session's agent pane, else its worst pane, is what a peek opens.
struct LeadLine {
    let lead: Board.Lead
    /// A teammate's name, when the lead runs on their machine.
    let who: String?
    let machine: String
    let standing: Standing?
    let peeked: Peeked?

    var name: String { who.map { "\($0)'s \(lead.session)" } ?? lead.session }

    @MainActor
    init(_ lead: Board.Lead, fleet: HomeFleet) {
        self.lead = lead
        let listed = fleet.directory.first { $0.id == lead.machine }
        // A team's own box has no one to name.
        who = listed.flatMap { $0.team_box == true ? nil : $0.owner_name }.flatMap { $0.isEmpty ? nil : $0 }
        machine = (lead.machine_name ?? "").isEmpty ? (listed?.name ?? "") : lead.machine_name!
        guard let m = fleet.machines.first(where: { $0.machine.id == lead.machine }) else {
            standing = nil
            peeked = nil
            return
        }
        // The daemon names a lead by its Claude Code conversation's title (an
        // agent block's own title), else as the web does, by its session's.
        let tabs = Set(m.sessions.filter { $0.name == lead.session }.flatMap(\.tabs))
        let all = m.agents + m.panes.filter { p in !m.agents.contains { $0.id == p.id } }
        let panes = all.filter { p in
            m.conversations[p.id] == lead.session
                || (p.type == "agent" && p.title?.trimmingCharacters(in: .whitespaces) == lead.session)
                || tabs.contains(p.tab)
        }
        standing = panes.map { m.standing(of: $0) }.min()
        let best = panes.min { ($0.isAgent ? 0 : 1, $0.standing) < ($1.isAgent ? 0 : 1, $1.standing) }
        peeked = best.map { .pane(m, $0) }
    }
}

/// One open board as home's row reads it.
struct BoardLine: Identifiable {
    let board: Board
    let lead: LeadLine?
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
    /// The sessions on each open card, by `BOARD/ROW` (Arugula #1304).
    var cards: [String: [BoardSession]] = [:]

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
                    all.append(m.standing(of: p))
                    on.append(BoardSession(peeked: .pane(m, p), name: m.conversations[p.id] ?? p.name, card: r.words, standing: m.standing(of: p), asks: m.owner))
                }
                for s in model.hosted where s.issue?.id == r.pointer?.id && r.pointer?.kind == "issue" {
                    all.append(s.standing)
                    on.append(BoardSession(peeked: .hosted(s), name: "Arugula cloud", card: r.words, standing: s.standing))
                }
                return all.min() ?? .idle
            }
            sessions[b.id] = on.sorted { ($0.standing, $0.name) < ($1.standing, $1.name) }
            // Every open card's sessions, for its Work level (Arugula #1304).
            for r in rows where !(r.lane ?? "").hasSuffix("done") {
                var mine: [BoardSession] = []
                for (m, p) in panes where Self.on(r, board: b.id, machine: m.machine.id, pane: p) {
                    mine.append(BoardSession(peeked: .pane(m, p), name: m.conversations[p.id] ?? p.name, card: r.words, standing: m.standing(of: p), asks: m.owner))
                }
                for s in model.hosted where s.issue?.id == r.pointer?.id && r.pointer?.kind == "issue" {
                    mine.append(BoardSession(peeked: .hosted(s), name: "Arugula cloud", card: r.words, standing: s.standing))
                }
                cards["\(b.id)/\(r.id)"] = mine.sorted { ($0.standing, $0.name) < ($1.standing, $1.name) }
            }
            var says = [Standing.needs, .asks, .failed, .checksFailing, .waits, .quiet, .working, .checksRunning, .review]
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
                board: b, lead: b.lead.map { LeadLine($0, fleet: fleet) }, done: p?.done ?? 0, review: p?.review ?? 0, doing: p?.doing ?? 0, total: p?.total ?? 0,
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
    @ObservedObject private var push = PushCenter.shared
    @ObservedObject private var zoomCenter = ZoomCenter.shared
    @ObservedObject private var approvals = Approvals.shared
    @EnvironmentObject private var scope: Scope
    @Environment(\.scenePhase) private var phase
    @State private var opened: Opened?
    /// A card to zoom to once its board has opened.
    @State private var zoomingTo: ZoomCard?
    @State private var columns = NavigationSplitViewVisibility.all
    @State private var terminal: PaneOpen?
    @State private var path = NavigationPath()
    /// What was answered here, by need and ask: folded away.
    @State private var folded: Set<String> = []
    @State private var said: String?
    @State private var peek: Peeked?
    @State private var making = false
    /// The Doc open (Arugula #1259): the right-hand column on an iPad, over
    /// everything on a phone.
    @State private var doc: DocRef?
    @State private var docOver: DocRef?
    @Environment(\.horizontalSizeClass) private var width

    /// A pane's terminal, over everything.
    struct PaneOpen: Identifiable {
        let session: MachineSession
        let pane: PaneRow
        var id: String { "\(session.machine.id):\(pane.id)" }
    }

    var body: some View {
        Group {
            // With a Doc open, an iPad's three columns: home's list, the
            // conversation, the Doc. Without one, the list and the conversation,
            // as on a phone that's wide enough.
            if width == .regular, let doc {
                NavigationSplitView(columnVisibility: $columns) {
                    sidebar
                } content: {
                    conversation
                } detail: {
                    DocScreen(ref: doc, close: closeDoc, open: follow).id(doc)
                }
            } else {
                NavigationSplitView(columnVisibility: $columns) {
                    sidebar
                } detail: {
                    conversation
                }
            }
        }
        .environment(\.openDoc) { d in present(d) }
        .environment(\.zoom) { z in zoom(z) }
        // A lead's or an agent's name opens that agent (Arugula #1305): over
        // what's open, so Back comes back to it.
        .environment(\.talkTo) { key in
            if opened == nil { opened = .work(key) } else { path.append(WorkKey(key: key)) }
        }
        .fullScreenCover(item: $docOver) { d in DocScreen(ref: d, close: { docOver = nil }, open: follow) }
        .navigationSplitViewStyle(.balanced)
        .fullScreenCover(item: $terminal) { t in TerminalScreen(session: t.session, pane: t.pane) }
        .sheet(isPresented: $making) {
            NewSheet(control: control, model: model, fleet: fleet, open: { opened = $0 }, toast: show) {
                Task { await refresh() }
            }
                .environmentObject(scope)
        }
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
        .onChange(of: opened) { _, _ in
            path = NavigationPath()
            if let card = zoomingTo {
                zoomingTo = nil
                // Once the board is up: on an iPhone a path set as it opens
                // is lost to its push.
                Task {
                    try? await Task.sleep(for: .milliseconds(450))
                    if opened == .board(board(card.board)) { path.append(card) }
                }
            }
            // The Doc was the conversation's: another conversation, no Doc.
            if doc != nil { closeDoc() }
        }
        .onChange(of: scope.current?.id) { _, _ in opened = nil }
        // A notification about an issue (#1260): its card's Work level when
        // it's on one of the project's open boards (Arugula #1304), else the
        // issue, beside home.
        .onChange(of: push.route, initial: true) { _, route in
            guard case .issue(let id)? = route else { return }
            push.route = nil
            Task {
                // Opened from a notification, home may not have read yet.
                for _ in 0..<20 where !model.loaded { try? await Task.sleep(for: .milliseconds(400)) }
                if let c = card(ofIssue: id) {
                    zoom(.card(board: c.0.id, row: c.1))
                } else {
                    opened = .issue(id)
                }
            }
        }
        // An address (Arugula #1304): a link, or a notification about a pane.
        .onChange(of: zoomCenter.route, initial: true) { _, _ in follow() }
        .onChange(of: model.loaded) { _, _ in follow() }
        .onChange(of: scope.projects) { _, _ in follow() }
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

    private var sidebar: some View {
        list
            .homeHeading { making = true }
            .background(Palette.void)
    }

    private var conversation: some View {
        NavigationStack(path: $path) {
            Group {
                switch opened {
                case .board(let b):
                    if let p = scope.current {
                        BoardLevel(control: control, project: p, board: b, model: model, fleet: fleet, askChief: askChief(b, in: p))
                    }
                case .thread(let b):
                    BoardThread(control: control, board: b, model: model, fleet: fleet, live: live, sessions: state.sessions[b.id] ?? []) { peek = $0 }
                case .work(let key):
                    if let p = scope.current {
                        WorkLevel(control: control, project: p, session: key, model: model, fleet: fleet, peek: { peek = $0 }) { m, pane in
                            terminal = PaneOpen(session: m, pane: pane)
                        }
                    }
                case .room(let r): ChatRoomView(control: control, room: r, live: live)
                case .staff(let project):
                    StaffRoom(control: control, project: project, model: model, fleet: fleet, live: live) { peek = $0 }
                case .session(let s): ChatRoomView(control: control, room: s.thread, live: live, thread: s.parent)
                case .idea(let room, let parent, let issue, let title):
                    IdeaThread(control: control, room: room, parent: parent, issue: issue, title: title, live: live)
                case .issue(let id):
                    Loaded(load: { try await control.get("/api/issues/\(id)", as: Issue.self) }) { IssueView(issue: $0) }
                case nil: Empty(text: "Pick a conversation", symbol: "bubble.left.and.bubble.right")
                }
            }
            .navigationDestination(for: BoardLanes.self) {
                BoardScreen(control: control, board: $0.board).backBesideTabs("Back") { path.removeLast() }
            }
            .navigationDestination(for: BoardThreadRef.self) { ref in
                BoardsThread(control: control, board: ref.board, model: model, fleet: fleet, live: live) { peek = $0 }
                    .backBesideTabs("Board") { path.removeLast() }
            }
            .navigationDestination(for: ZoomCard.self) { c in
                if let p = scope.current {
                    WorkLevel(
                        control: control, project: p, board: board(c.board), row: c.row, model: model, fleet: fleet,
                        peek: { peek = $0 }
                    ) { m, pane in
                        terminal = PaneOpen(session: m, pane: pane)
                    }
                    .backBesideTabs("Board") { path.removeLast() }
                }
            }
            .navigationDestination(for: WorkKey.self) { w in
                if let p = scope.current {
                    WorkLevel(control: control, project: p, session: w.key, model: model, fleet: fleet, peek: { peek = $0 }) { m, pane in
                        terminal = PaneOpen(session: m, pane: pane)
                    }
                    .backBesideTabs("Back") { path.removeLast() }
                }
            }
            .navigationDestination(for: IssueRef.self) { ref in
                Loaded(load: { try await control.get("/api/issues/\(ref.id)", as: Issue.self) }) { IssueView(issue: $0) }
                    .backBesideTabs("Back") { path.removeLast() }
            }
            .navigationDestination(for: ProjectIssue.self) { ref in
                Loaded(load: { try await control.get("/api/projects/\(ref.project)/issues/\(ref.number)", as: Issue.self) }) {
                    IssueView(issue: $0)
                }
                    .backBesideTabs("Back") { path.removeLast() }
            }
        }
        .id(opened)
    }

    /// A Doc, opened: on an iPad in the right-hand column, with home's list
    /// folded away so the conversation and the Doc share the width (the
    /// sidebar button brings it back); on a phone over everything.
    private func present(_ d: DocRef) {
        if width == .regular {
            doc = d
            columns = .doubleColumn
        } else {
            docOver = d
        }
    }

    /// The iPad's Doc closed: back to the list and the conversation.
    private func closeDoc() {
        doc = nil
        columns = .all
    }

    /// A link in a Doc (Arugula #1259): another Doc opens in its place; a
    /// board or a room the app knows opens as home would open it; an issue in
    /// the conversation's column; anything else in the browser.
    private func follow(_ link: DocLink) {
        switch link {
        case .doc(let d):
            present(d)
        case .board(_, let id, _):
            guard model.boards.contains(where: { $0.id == id }) else { return link.openOutside() }
            docOver = nil
            zoom(.board(id))
        case .room(let id, _):
            guard let r = model.rooms.first(where: { $0.id == id }) else { return link.openOutside() }
            docOver = nil
            opened = .room(r)
        case .issue(let project, let id):
            docOver = nil
            if id.hasPrefix("iss_") { path.append(IssueRef(id: id)) } else { path.append(ProjectIssue(project: project, number: id)) }
        case .web(let u):
            // An Overview address opens here (Arugula #1304).
            if zoomCenter.open(u) {
                docOver = nil
            } else {
                UIApplication.shared.open(u)
            }
        }
    }

    /// Zoom to a level of the current project (Arugula #1304); nil is the
    /// Project level, home itself.
    private func zoom(_ z: Zoom?) {
        switch z {
        case nil:
            opened = nil
            path = NavigationPath()
        case .board(let id):
            if opened == .board(board(id)) { path = NavigationPath() } else { opened = .board(board(id)) }
        case .card(let id, let row):
            let card = ZoomCard(board: id, row: row)
            if opened == .board(board(id)) {
                path = NavigationPath()
                path.append(card)
            } else {
                zoomingTo = card
                opened = .board(board(id))
            }
        case .work(let key):
            opened = .work(key)
        }
    }

    /// A board no one leads, in a team's project where the reader has a
    /// staff room: ask the chief of staff to start a lead there, and open the
    /// room to read the answer (Arugula #1306).
    private func askChief(_ b: Board, in p: Projects.Project) -> (() -> Void)? {
        guard p.team != nil, (model.boards.first { $0.id == b.id } ?? b).lead == nil,
              let room = model.staffListed(project: p.id) else { return nil }
        return {
            Task {
                do {
                    _ = try await control.call("POST", "/api/chat/rooms/\(room.id)/messages", body: [
                        "text": "Please start a session to lead the board \"\(b.title)\" (\(b.id)): read its goal and open tickets, then lead it.",
                    ])
                    opened = .staff(project: p.id)
                } catch {
                    show("Couldn't ask the chief of staff: \(describe(error))")
                }
            }
        }
    }

    /// One of the project's boards, by its id; a closed one by its id alone.
    private func board(_ id: String) -> Board {
        model.boards.first { $0.id == id } ?? Board(id: id, title: "A board")
    }

    /// The open card an issue is on, on one of the project's open boards.
    private func card(ofIssue id: String) -> (Board, String)? {
        for b in model.boards {
            for r in model.views[b.id]?.zones.flatMap(\.rows) ?? [] where r.pointer?.kind == "issue" && r.pointer?.id == id {
                if !(r.lane ?? "").hasSuffix("done") { return (b, r.id) }
            }
        }
        return nil
    }

    /// Go to the address waiting, once home reads its project: another
    /// project is picked first, and an address in a project the reader isn't
    /// in is dropped.
    private func follow() {
        guard let r = zoomCenter.route, !scope.projects.isEmpty else { return }
        guard let p = scope.projects.first(where: { $0.id == r.project }) else {
            zoomCenter.route = nil
            return
        }
        if scope.current?.id != p.id {
            scope.pick(p)
            return
        }
        guard model.loaded, model.project == p.id else { return }
        zoomCenter.route = nil
        zoom(r.zoom)
    }

    private func refresh(soon: Bool = false) async {
        guard let p = scope.current else { return }
        if !soon { await approvals.load(control) }
        if soon { await model.soon(control, project: p.id) } else { await model.load(control, project: p.id) }
    }

    /// The New ideas, newest first, each with its session if it has one.
    private var ideaLines: [IdeaLine] {
        model.ideas.sorted { $0.number > $1.number }.map { i in
            IdeaLine(issue: i, session: model.hosted.first { $0.issue?.id == i.id })
        }
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
        // The staff room has its own row, first.
        let people = model.rooms.filter { $0.isConversation && !$0.isStaff && !$0.archived && scope.showsRoom($0) }
            .sorted { ($0.unread > 0 ? 1 : 0, $0.last) > ($1.unread > 0 ? 1 : 0, $1.last) }
        return List(selection: $opened) {
            // The chief of staff, first (Arugula #1279): the reader's staff
            // room in the project, a team's only, as chat is a team's.
            if let project = scope.current, project.team != nil {
                Section {
                    let room = model.staffListed(project: project.id)
                    NavigationLink(value: Opened.staff(project: project.id)) {
                        StaffRow(room: room, lead: room?.lead.map { LeadLine($0, fleet: fleet) }, open: opened == .staff(project: project.id))
                    }
                    .voidRow()
                    .accessibilityIdentifier("home-staff")
                } header: {
                    Caps("Chief of staff")
                }
            }
            Section {
                // Deploys waiting on the reader, in any project they own,
                // first: they open over the app (#604).
                ForEach(approvals.waiting) { d in
                    Button { approvals.open = .deploy(d.id) } label: { DeployNeedRow(deploy: d) }
                        .buttonStyle(.plain)
                        .voidRow()
                        .accessibilityIdentifier("need-deploy-\(d.id)")
                }
                if needs.isEmpty && approvals.waiting.isEmpty {
                    Text(model.loaded ? "Nothing needs you." : "Looking…")
                        .font(Face.ui(14)).foregroundStyle(Palette.dim)
                        .voidRow()
                        .accessibilityIdentifier("needs-none")
                }
                ForEach(needs) { n in needRow(n) }
            } header: {
                let count = needs.count + approvals.waiting.count
                Caps(count == 0 ? NEEDS_YOU : "\(NEEDS_YOU) · \(count)")
            }
            if !ideaLines.isEmpty {
                Section {
                    ForEach(ideaLines) { line in
                        Button {
                            if let s = line.session {
                                opened = .idea(room: s.room, parent: s.parent, title: line.issue.title)
                            } else {
                                opened = .idea(room: nil, parent: nil, issue: line.issue.id, title: line.issue.title)
                            }
                        } label: {
                            IdeaRow(line: line)
                        }
                        .buttonStyle(.plain)
                        .voidRow()
                        .accessibilityIdentifier("home-idea-\(line.issue.number)")
                    }
                } header: {
                    Caps("New ideas")
                }
            }
            Section {
                if model.loaded && state.boards.isEmpty {
                    Text("No open boards").font(Face.ui(14)).foregroundStyle(Palette.dim).voidRow()
                }
                // The row zooms to the board; its lead's name opens that
                // agent, and the board's thread is a button of its own
                // (Arugula #1305).
                ForEach(state.boards) { b in
                    BoardLineRow(
                        line: b,
                        zoom: { zoom(.board(b.board.id)) },
                        talk: b.lead.map { l in { zoom(.work(l.opening)) } },
                        thread: { opened = .thread(b.board) })
                    .voidRow()
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
                // A board's row zooms to its card's work (Arugula #1304).
                case .board(let b, let r):
                    if let r { zoom(.card(board: b.id, row: r.id)) } else { opened = .board(b) }
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

/// Home's heading: the project, as a menu of every project and its issues,
/// and New.
struct HomeHeading: ViewModifier {
    let new: () -> Void
    @EnvironmentObject private var scope: Scope

    func body(content: Content) -> some View {
        content
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .safeAreaInset(edge: .top, spacing: 0) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(alignment: .firstTextBaseline) {
                        Menu {
                            ProjectMenu()
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Text((scope.current?.name ?? "Arugula").uppercased())
                                    .font(Face.display(30)).foregroundStyle(Palette.fg).lineLimit(1)
                                Image(systemName: "chevron.down").font(.system(size: 13, weight: .bold)).foregroundStyle(Palette.accent)
                            }
                        }
                        .accessibilityIdentifier("project-switcher")
                        Spacer(minLength: 8)
                        Button("New", action: new)
                            .buttonStyle(AnswerButton(primary: true))
                            .accessibilityIdentifier("home-new")
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
    func homeHeading(new: @escaping () -> Void) -> some View { modifier(HomeHeading(new: new)) }
}

/// The project menu's items: every project to pick, then the current one's
/// issues.
struct ProjectMenu: View {
    @EnvironmentObject private var scope: Scope
    @Environment(\.showIssues) private var showIssues

    var body: some View {
        Section("Projects") {
            ForEach(scope.projects) { p in
                Button { scope.pick(p) } label: {
                    if p == scope.current { Label(p.name, systemImage: "checkmark") } else { Text(p.name) }
                }
            }
        }
        Section {
            if let showIssues {
                Button(action: showIssues) { Label("Issues", systemImage: "smallcircle.filled.circle") }
                    .accessibilityIdentifier("project-issues")
            }
            // Its standing grant for runs (#604).
            if let p = scope.current {
                Button { Approvals.shared.open = .runs(project: p.id, name: p.name) } label: {
                    Label("Runs", systemImage: "play.circle")
                }
                .accessibilityIdentifier("project-runs")
            }
        }
    }
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

/// A board on home: its title and count, its lane bar and what's going on,
/// which zoom to the board; under them who leads it, which opens that agent,
/// and the board's thread (Arugula #1305).
struct BoardLineRow: View {
    let line: BoardLine
    var zoom: () -> Void = {}
    var talk: (() -> Void)?
    var thread: () -> Void = {}

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button(action: zoom) {
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
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .accessibilityIdentifier("home-board-\(line.board.title)")
            HStack(spacing: 8) {
                if let talk {
                    Button(action: talk) { LeadChip(lead: line.lead) }
                        .buttonStyle(.borderless)
                        .accessibilityIdentifier("home-lead-\(line.board.title)")
                } else {
                    LeadChip(lead: line.lead)
                }
                Spacer(minLength: 4)
                ThreadButton(action: thread).accessibilityIdentifier("home-thread-\(line.board.title)")
            }
        }
        .padding(.vertical, 4)
    }

    /// Who leads it, in words: a session on a machine, or nobody.
    static func lead(_ l: Board.Lead?) -> String {
        guard let l else { return "No lead" }
        let on = (l.machine_name ?? "").isEmpty ? "" : " on \(l.machine_name!)"
        return "Led by \(l.session)\(on)"
    }
}

/// Who leads a board or a staff room, on one line: a dot for how its session
/// stands (hollow where nothing here tells), its name, a teammate's with
/// theirs, and its machine; or nobody.
struct LeadChip: View {
    let lead: LeadLine?
    var none = "No lead"

    var body: some View {
        Group {
            if let lead {
                (Text(lead.standing == nil ? "○ " : "● ")
                    .foregroundColor(lead.standing?.tone.color ?? Palette.dim)
                    + Text("Led by ").foregroundColor(Palette.dim)
                    + Text(lead.name).font(Face.mono(11, "Bold")).foregroundColor(Palette.fg)
                    + Text(lead.machine.isEmpty ? "" : " on \(lead.machine)").foregroundColor(Palette.dim)
                    + Text(lead.standing.map { $0 == .idle ? "" : " · \($0.word(asks: lead.who))" } ?? "").foregroundColor(lead.standing?.tone.color ?? Palette.dim))
            } else {
                Text(none).foregroundColor(Palette.dim)
            }
        }
        .font(Face.mono(11))
        .lineLimit(1)
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
    @ObservedObject var fleet: HomeFleet
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
            LeadHeading(lead: line.lead.map { LeadLine($0, fleet: fleet) }, id: "board-lead", peek: peek)
            WhoLine(text: "Talking in \(line.title)'s thread: a post goes to the board's thread, where its lead and anyone mentioned read it.")
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
                                    Text("\(s.card) · \(s.word)")
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

/// A board's thread, as a button of its own: where a post reaches its lead
/// and anyone mentioned, not the lead alone (Arugula #1305).
struct ThreadButton: View {
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label("Board thread", systemImage: "bubble.left")
                .font(Face.mono(11, "Bold"))
                .foregroundStyle(Palette.accent)
                .labelStyle(.titleAndIcon)
        }
        .buttonStyle(.borderless)
        .accessibilityHint("Its lead and anyone mentioned read it")
    }
}

/// Who leads a thread, over it: the lead's chip, whose name opens that agent
/// (Arugula #1305), and Peek at its session when a machine here tells of it.
struct LeadHeading: View {
    let lead: LeadLine?
    let id: String
    var none = "No lead"
    let peek: (Peeked) -> Void
    @Environment(\.talkTo) private var talkTo
    @EnvironmentObject private var scope: Scope

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            LeadChip(lead: lead, none: none)
                .accessibilityIdentifier(id)
                .onTapGesture { talk() }
                .accessibilityAction { talk() }
                .accessibilityHint(lead == nil ? "" : "Opens the lead")
            Spacer(minLength: 4)
            if let p = lead?.peeked {
                Button { peek(p) } label: {
                    Text("Peek").font(Face.mono(11, "Bold")).foregroundStyle(Palette.accent)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("lead-peek")
            }
        }
    }

    private func talk() {
        guard let lead else { return }
        openAgent(lead.opening, talkTo: talkTo, project: scope.current?.id)
    }
}

/// Under a conversation's heading: who you're talking to, and where what you
/// post goes (Arugula #1305).
struct WhoLine: View {
    let text: String

    var body: some View {
        Text(text)
            .font(Face.ui(12)).foregroundStyle(Palette.dim)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityIdentifier("who-line")
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

/// A New idea's conversation: its session's thread, or its issue's.
struct IdeaThread: View {
    let control: Control
    let room: String?
    let parent: Int?
    let issue: String?
    let title: String
    @ObservedObject var live: LiveChat
    @State private var found: String?
    @State private var problem: String?

    var body: some View {
        Group {
            if let id = room ?? found {
                ChatRoomView(
                    control: control,
                    room: ChatRoom(id: id, kind: "thread", name: title, last: 0, unread: 0, mentions: 0, archived: false, writable: true),
                    live: live, title: title, thread: parent)
            } else if let problem {
                Problem(message: problem)
            } else {
                ProgressView().tint(Palette.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task {
            guard room == nil, let issue else { return }
            do {
                let d = try await control.call("POST", "/api/chat/targets", body: ["machine": "", "kind": "object", "target": issue])
                found = try JSONDecoder().decode(ChatRoom.self, from: d).id
            } catch {
                problem = describe(error)
            }
        }
    }
}

/// The staff room on home: ask the chief of staff, who leads it, and how
/// many messages wait there.
struct StaffRow: View {
    let room: ChatRoom?
    let lead: LeadLine?
    let open: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                Image(systemName: "person.badge.key").font(.system(size: 14, weight: .semibold)).foregroundStyle(Palette.accent)
                Text("Ask your chief of staff").font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                Spacer(minLength: 4)
                if let room, room.unread > 0, !open {
                    Text(verbatim: "\(room.unread)")
                        .font(Face.mono(11, "Bold"))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(Palette.accent, in: Capsule())
                        .foregroundStyle(Palette.void)
                }
            }
            LeadChip(lead: lead, none: "Nobody leads it yet")
        }
        .padding(.vertical, 4)
    }
}

/// The reader's staff room in a project (Arugula #1275, #1279): them and
/// their own agents, where they talk to the chief of staff, the session
/// that leads it. Its lead over it, with a peek at its session.
struct StaffRoom: View {
    let control: Control
    let project: String
    @ObservedObject var model: HomeModel
    @ObservedObject var fleet: HomeFleet
    @ObservedObject var live: LiveChat
    let peek: (Peeked) -> Void
    @State private var room: ChatRoom?
    @State private var problem: String?

    var body: some View {
        Group {
            if let room {
                ChatRoomView(control: control, room: room, live: live, title: "Chief of staff", heading: AnyView(heading))
            } else if let problem {
                Problem(message: problem) { Task { await open() } }
            } else {
                ProgressView().tint(Palette.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Palette.bg)
        .task { await open() }
    }

    private func open() async {
        do {
            room = try await model.staff(control, project: project)
            problem = nil
        } catch {
            problem = describe(error)
        }
    }

    private var heading: some View {
        // The rooms list is the newer word on who leads it.
        let lead = (model.staffListed(project: project) ?? room)?.lead
        return VStack(alignment: .leading, spacing: 4) {
            LeadHeading(lead: lead.map { LeadLine($0, fleet: fleet) }, id: "staff-lead", none: "Nobody leads it yet", peek: peek)
            Text(lead == nil
                ? "Only you and your own agents are here. What you post waits until one of your sessions takes the lead."
                : "Only you and your own agents are here. What you post reaches the session that leads it.")
                .font(Face.ui(12)).foregroundStyle(Palette.dim)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Palette.void)
    }
}

/// Back, where iPadOS's tabs cover it (Arugula #1262): on an iPad the tab bar
/// floats over the top of the split view, across the detail's back button,
/// so what home's detail pushes gets one on the other side ("‹ Thread", as
/// the prototype has it). An iPhone keeps its own.
struct BackBesideTabs: ViewModifier {
    let title: String
    let back: () -> Void
    @Environment(\.horizontalSizeClass) private var width

    func body(content: Content) -> some View {
        content.toolbar {
            if width == .regular {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: back) { Label(title, systemImage: "chevron.left").labelStyle(.titleAndIcon) }
                        .font(Face.mono(12, "Bold"))
                        .accessibilityIdentifier("detail-back")
                }
            }
        }
    }
}

extension View {
    func backBesideTabs(_ title: String, back: @escaping () -> Void) -> some View {
        modifier(BackBesideTabs(title: title, back: back))
    }
}
