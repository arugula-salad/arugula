// The Overview zoom (Arugula #1304, the web's #1287): Home is the Project
// level, a board's row zooms to its Board level and a ticket there to its
// Work level, and Back, or the path bar over each level, zooms out. Each level
// has an address, the web's paths (web/src/platform/paths.ts) under
// `arugula://`, so a link or a notification opens any of them:
//
//   arugula://p/:project                        Home, in that project
//   arugula://p/:project/board/:board           a board's Board level
//   arugula://p/:project/board/:board/card/:row a card's Work level
//   arugula://p/:project/work/:session          a session on no card, by its
//                                               key: `pane:MACHINE:PANE`,
//                                               `hosted:SESSION` or the web's
//                                               `MACHINE#SESSION`
//
// The web's own `https://…/p/…` links read the same.

import SwiftUI

/// How far in Home is zoomed, below its Project level.
enum Zoom: Hashable {
    case board(String)
    case card(board: String, row: String)
    /// A session on no card, by its key.
    case work(String)
}

/// An address: a project, and how far in.
struct ZoomRoute: Equatable {
    let project: String
    let zoom: Zoom?

    /// `arugula://p/…`, or the web's `https://…/p/…`.
    init?(url: URL) {
        var parts = url.path(percentEncoded: true).split(separator: "/").map(String.init)
        if url.scheme == "arugula" {
            guard url.host() == "p" else { return nil }
        } else if url.scheme == "https" || url.scheme == "http" {
            guard parts.first == "p" else { return nil }
            parts.removeFirst()
        } else {
            return nil
        }
        guard let first = parts.first, let project = Self.id(first, "prj_") else { return nil }
        self.project = project
        let rest = Array(parts.dropFirst())
        switch rest.count {
        case 0:
            zoom = nil
        case 2 where rest[0] == "board":
            guard let board = Self.id(rest[1], "brd_") else { return nil }
            zoom = .board(board)
        case 4 where rest[0] == "board" && rest[2] == "card":
            guard let board = Self.id(rest[1], "brd_"), rest[3].hasPrefix("row_") else { return nil }
            zoom = .card(board: board, row: rest[3])
        case 2 where rest[0] == "work":
            guard let key = rest[1].removingPercentEncoding, !key.isEmpty, key.count <= 200 else { return nil }
            zoom = .work(key)
        default:
            return nil
        }
    }

    init(project: String, zoom: Zoom?) {
        self.project = project
        self.zoom = zoom
    }

    /// Its address in the app.
    var url: URL {
        var path = "arugula://p/\(project)"
        switch zoom {
        case .board(let b): path += "/board/\(b)"
        case .card(let b, let r): path += "/board/\(b)/card/\(r)"
        case .work(let key): path += "/work/\(key.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? key)"
        case nil: break
        }
        return URL(string: path)!
    }

    /// An id from a path's segment, without the slug the web puts after it
    /// (`prj_8f2k…-arugula`).
    private static func id(_ segment: String, _ prefix: String) -> String? {
        let s = segment.removingPercentEncoding ?? segment
        guard s.hasPrefix(prefix) else { return nil }
        let id = s.split(separator: "-", maxSplits: 1).first.map(String.init) ?? s
        return id.count > prefix.count ? id : nil
    }
}

/// Where a link, a notification or the app being opened at an address wants
/// Home to zoom, until Home takes it.
@MainActor
final class ZoomCenter: ObservableObject {
    static let shared = ZoomCenter()
    @Published var route: ZoomRoute?

    /// An address the app was opened at; false for any other link.
    @discardableResult
    func open(_ url: URL) -> Bool {
        guard let r = ZoomRoute(url: url) else { return false }
        route = r
        return true
    }
}

private struct ZoomKey: EnvironmentKey {
    static let defaultValue: ((Zoom?) -> Void)? = nil
}

private struct TalkKey: EnvironmentKey {
    static let defaultValue: ((String) -> Void)? = nil
}

extension EnvironmentValues {
    /// Zoom Home to a level of the current project; nil is its Project level.
    var zoom: ((Zoom?) -> Void)? {
        get { self[ZoomKey.self] }
        set { self[ZoomKey.self] = newValue }
    }

    /// Open an agent, by its session's key, to talk to it (Arugula #1305):
    /// Home zooms to its Work level, Boards pushes it. Unset, Home opens it.
    var talkTo: ((String) -> Void)? {
        get { self[TalkKey.self] }
        set { self[TalkKey.self] = newValue }
    }
}

/// Open an agent to talk to it: where the view is, by `talkTo`, else on
/// Home, in the project.
@MainActor
func openAgent(_ key: String, talkTo: ((String) -> Void)?, project: String?) {
    if let talkTo {
        talkTo(key)
    } else if let project {
        ZoomCenter.shared.route = ZoomRoute(project: project, zoom: .work(key))
    }
}

/// A session's Work level pushed where Home isn't (Boards, Arugula #1305).
struct WorkKey: Hashable { let key: String }

extension LeadLine {
    /// Its session's key: its pane's when a machine here tells of it, else
    /// `lead:MACHINE:SESSION`, which the Work level says it can't reach.
    var key: String { peeked?.key ?? "lead:\(lead.machine):\(lead.session)" }
}

/// A card's Work level, pushed over its board's.
struct ZoomCard: Hashable {
    let board: String
    let row: String
}

extension Peeked {
    /// Its key at the Work level: `pane:MACHINE:PANE` or `hosted:SESSION`.
    var key: String { id }
}

/// A session's key, back to what it names on the machines and in Arugula
/// cloud: a pane by its machine and number, a hosted session, or the web's
/// `MACHINE#SESSION` (a machine's session, by its agent pane, else its
/// pane that most wants you).
@MainActor
func peeked(_ key: String, fleet: HomeFleet, model: HomeModel) -> Peeked? {
    if key.hasPrefix("hosted:") {
        let id = String(key.dropFirst("hosted:".count))
        return model.hosted.first { $0.session == id }.map { .hosted($0) }
    }
    if key.hasPrefix("pane:") {
        let parts = key.split(separator: ":")
        guard parts.count == 3, let pane = UInt32(parts[2]),
              let m = fleet.machines.first(where: { $0.machine.id == parts[1] }),
              let p = (m.agents + m.panes).first(where: { $0.id == pane }) else { return nil }
        return .pane(m, p)
    }
    let parts = key.split(separator: "#")
    guard parts.count == 2, let session = UInt32(parts[1]),
          let m = fleet.machines.first(where: { $0.machine.id == parts[0] }),
          let tabs = m.sessions.first(where: { $0.id == session })?.tabs else { return nil }
    let all = (m.agents + m.panes).filter { tabs.contains($0.tab) }
    return all.min { ($0.isAgent ? 0 : 1, $0.standing) < ($1.isAgent ? 0 : 1, $1.standing) }.map { .pane(m, $0) }
}

// ---- the path bar

/// Over a Board or a Work level: where it is, each part zooming back out to
/// its level (Overview › Launch › #12 · Fix the installer). Its value is the
/// level's address.
struct ZoomPath: View {
    let project: String
    var board: Board?
    /// The Work level's: a card's words, or a session's name.
    var work: String?
    let here: Zoom
    @Environment(\.zoom) private var zoom

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                part("Overview", id: "zoom-path-overview") { go(nil) }
                if let board {
                    sep
                    if work == nil {
                        leaf(board.title)
                    } else {
                        part(board.title, id: "zoom-path-board") { go(.board(board.id)) }
                    }
                }
                if let work {
                    sep
                    leaf(work)
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Palette.void)
        .overlay(alignment: .bottom) { Rectangle().fill(Palette.raised).frame(height: 1) }
    }

    /// Where it is now: its value is the level's address.
    private func leaf(_ text: String) -> some View {
        Text(text).font(Face.mono(12, "Bold")).foregroundStyle(Palette.fg).lineLimit(1)
            .accessibilityIdentifier("zoom-path")
            .accessibilityValue(ZoomRoute(project: project, zoom: here).url.absoluteString)
    }

    /// Zoom out: in Home, in place; anywhere else (a level pushed on
    /// Boards), on Home.
    private func go(_ z: Zoom?) {
        if let zoom { zoom(z) } else { ZoomCenter.shared.route = ZoomRoute(project: project, zoom: z) }
    }

    private var sep: some View {
        Text("›").font(Face.mono(12)).foregroundStyle(Palette.dim)
    }

    private func part(_ text: String, id: String, _ tap: @escaping () -> Void) -> some View {
        Button(action: tap) {
            Text(text).font(Face.mono(12)).foregroundStyle(Palette.accent).lineLimit(1)
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(id)
    }
}

// ---- the Board level

/// A board, zoomed to from Home: its goal and progress and its open tickets,
/// each zooming to its Work level. Its thread and its lanes are a tap away.
struct BoardLevel: View {
    let control: Control
    let project: String
    let board: Board
    @ObservedObject var model: HomeModel
    @Environment(\.zoom) private var zoom

    var body: some View {
        let view = model.views[board.id]
        let line = model.boards.first { $0.id == board.id } ?? board
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                head(line)
                if let view {
                    tickets(view)
                } else {
                    Loaded(load: { try await control.get("/api/boards/\(board.id)", as: BoardDetail.self) }) { tickets($0) }
                        .frame(minHeight: 120)
                }
            }
            .padding(.vertical)
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Palette.void)
        .safeAreaInset(edge: .top, spacing: 0) {
            ZoomPath(project: project, board: line, here: .board(board.id))
        }
        .detailTitle(line.title)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                NavigationLink(value: BoardThreadRef(board: line)) {
                    Text("Thread").font(Face.mono(12, "Bold"))
                }
                .accessibilityIdentifier("board-thread")
            }
            ToolbarItem(placement: .topBarTrailing) {
                NavigationLink(value: BoardLanes(board: line)) {
                    Text("Lanes").font(Face.mono(12, "Bold"))
                }
                .accessibilityIdentifier("open-board")
            }
        }
        .accessibilityIdentifier("board-level")
    }

    private func head(_ b: Board) -> some View {
        let p = b.progress
        let total = p?.total ?? 0, done = p?.done ?? 0, review = p?.review ?? 0, doing = p?.doing ?? 0
        return VStack(alignment: .leading, spacing: 8) {
            if let goal = b.goal, !goal.isEmpty {
                Text(goal).font(Face.ui(14)).foregroundStyle(Palette.subtle).lineLimit(4)
            }
            LaneBar(done: done, review: review, doing: doing, total: max(total, 1))
            Text("\(done) done · \(review) review · \(doing) doing · \(max(0, total - done - review - doing)) next")
                .font(Face.mono(11)).foregroundStyle(Palette.dim)
        }
        .padding(.horizontal)
    }

    @ViewBuilder
    private func tickets(_ v: BoardDetail) -> some View {
        let open = v.zones.filter { !$0.id.hasSuffix("done") }
        VStack(alignment: .leading, spacing: 14) {
            ForEach(open) { lane in
                if !lane.rows.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        Caps("\(lane.title) · \(lane.rows.count)")
                        ForEach(lane.rows) { row in
                            Button { zoom?(.card(board: board.id, row: row.id)) } label: {
                                TicketLine(row: row, tone: LaneKind(rawValue: lane.id)?.tone ?? .muted)
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("ticket-\(row.live?.number.map(String.init) ?? row.id)")
                        }
                    }
                }
            }
            if open.allSatisfy(\.rows.isEmpty) {
                Text("No open tickets").font(Face.ui(14)).foregroundStyle(Palette.dim)
            }
        }
        .padding(.horizontal)
    }
}

/// A ticket on the Board level, on a line: its number and title, and what it
/// says of itself.
struct TicketLine: View {
    let row: BoardDetail.Row
    let tone: Tone

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Circle().fill(tone.color).frame(width: 7, height: 7)
            Text(row.words).font(Face.ui(15, "Medium")).foregroundStyle(Palette.fg).lineLimit(2)
            Spacer(minLength: 4)
            if let worst = row.says.min() {
                Text(worst.word).font(Face.mono(11, "Bold")).foregroundStyle(worst.tone.color)
            }
            Image(systemName: "chevron.right").font(.system(size: 11, weight: .semibold)).foregroundStyle(Palette.muted)
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 10)
        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 8))
        .contentShape(Rectangle())
    }
}

// ---- the Work level

/// A card's work, or one session's on no card: the sessions doing it, each
/// opening its terminal (a pane's) or its peek.
struct WorkLevel: View {
    let control: Control
    let project: Projects.Project
    /// The card's board and row; nil for a session on no card.
    var board: Board?
    var row: String?
    /// A session on no card: its key.
    var session: String?
    @ObservedObject var model: HomeModel
    @ObservedObject var fleet: HomeFleet
    let peek: (Peeked) -> Void
    let terminal: (MachineSession, PaneRow) -> Void

    var body: some View {
        let state = HomeState(project: project, fleet: fleet, model: model)
        let card = board.flatMap { b in row.flatMap { r in model.views[b.id]?.zones.flatMap(\.rows).first { $0.id == r } } }
        let lone = session.flatMap { peeked($0, fleet: fleet, model: model) }
        let sessions: [BoardSession] = if let board, let row {
            state.cards["\(board.id)/\(row)"] ?? []
        } else if let lone {
            [BoardSession(peeked: lone, name: lone.name, card: "", standing: lone.standing)]
        } else {
            []
        }
        let title = card?.words ?? lone?.name ?? leadName ?? (session == nil ? "A card" : "A session")
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if sessions.isEmpty {
                    Text(session == nil
                        ? (model.loaded ? "No agent works on this card yet." : "Looking…")
                        : away)
                        .font(Face.ui(14)).foregroundStyle(Palette.dim)
                        .accessibilityIdentifier("work-none")
                }
                ForEach(sessions) { s in
                    WorkSessionRow(session: s, peek: peek, terminal: terminal)
                }
            }
            .padding()
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Palette.void)
        .safeAreaInset(edge: .top, spacing: 0) {
            ZoomPath(
                project: project.id, board: board.map { b in model.boards.first { $0.id == b.id } ?? b }, work: title,
                here: board.map { b in .card(board: b.id, row: row ?? "") } ?? .work(session ?? ""))
        }
        .detailTitle(title)
        .accessibilityIdentifier("work-level")
    }

    /// A lead's name, from its key, when its machine isn't here.
    private var leadName: String? {
        guard let key = session, key.hasPrefix("lead:") else { return nil }
        return key.dropFirst("lead:".count).split(separator: ":", maxSplits: 1).dropFirst().first.map(String.init)
    }

    /// Why a session on no card isn't here: a lead's machine, by name.
    private var away: String {
        guard let key = session, key.hasPrefix("lead:") else { return "This session isn't on a machine this \(device) reaches." }
        let parts = key.dropFirst("lead:".count).split(separator: ":", maxSplits: 1).map(String.init)
        let machine = parts.first.flatMap { m in fleet.directory.first { $0.id == m }?.name } ?? "its machine"
        let name = parts.count > 1 ? parts[1] : "It"
        return "\(name) runs on \(machine), which this \(device) doesn't reach now."
    }
}

/// One session at the Work level: how it stands, and its terminal or peek.
struct WorkSessionRow: View {
    let session: BoardSession
    let peek: (Peeked) -> Void
    let terminal: (MachineSession, PaneRow) -> Void

    var body: some View {
        HStack(spacing: 10) {
            Circle().fill(session.standing.tone.color).frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 2) {
                Text(session.name).font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                Text(session.peeked.place + (session.standing == .idle ? "" : " · \(session.standing.word)"))
                    .font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
            }
            Spacer(minLength: 6)
            if case .pane(let m, let p) = session.peeked, !p.isAgent {
                Button("Open terminal") { terminal(m, p) }
                    .buttonStyle(AnswerButton(primary: true))
                    .accessibilityIdentifier("work-terminal-\(p.id)")
            } else {
                Button("Peek") { peek(session.peeked) }
                    .buttonStyle(AnswerButton())
                    .accessibilityIdentifier("work-peek-\(session.id)")
            }
        }
        .card(session.standing.tone)
        .accessibilityIdentifier("work-session-\(session.id)")
    }
}

extension Peeked {
    /// What a session is called: its conversation, its pane, or its card.
    @MainActor
    var name: String {
        switch self {
        case .pane(let m, let p): m.conversations[p.id] ?? p.name
        case .hosted(let s): s.issue.map { "#\($0.number) \($0.title)" } ?? "Arugula cloud"
        }
    }

    /// Where it runs.
    @MainActor
    var place: String {
        switch self {
        case .pane(let m, _): m.machine.name
        case .hosted: "Arugula cloud"
        }
    }

    @MainActor
    var standing: Standing {
        switch self {
        case .pane(let m, let p): (m.agents + m.panes).first { $0.id == p.id }?.standing ?? p.standing
        case .hosted(let s): s.standing
        }
    }
}
