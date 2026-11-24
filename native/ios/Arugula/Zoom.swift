// The Overview zoom (Arugula #1304, the web's #1287): the Overview is the Project
// level, a board's row zooms to its Board level and a ticket there to its
// Work level, and Back, or the path bar over each level, zooms out. Each level
// has an address, the web's paths (web/src/platform/paths.ts) under
// `arugula://`, so a link or a notification opens any of them:
//
//   arugula://p/:project                        the Overview, in that project
//   arugula://p/:project/board/:board           a board's Board level
//   arugula://p/:project/board/:board/card/:row a card's Work level
//   arugula://p/:project/work/:session          a session on no card, by its
//                                               key: `pane:MACHINE:PANE`,
//                                               `hosted:SESSION` or the web's
//                                               `MACHINE#SESSION`
//
// The web's own `https://…/p/…` links read the same.

import SwiftUI

/// How far in the Overview is zoomed, below its Project level.
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
/// the Overview to zoom, until it takes it.
@MainActor
final class ZoomCenter: ObservableObject {
    static let shared = ZoomCenter()
    @Published var route: ZoomRoute?
    /// What a session was called where it was opened from (a lead's chip
    /// names its session), by its key: until its machine names its
    /// conversation, the Work level calls it that.
    var names: [String: String] = [:]

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
    /// Zoom the Overview to a level of the current project; nil is its Project level.
    var zoom: ((Zoom?) -> Void)? {
        get { self[ZoomKey.self] }
        set { self[ZoomKey.self] = newValue }
    }

    /// Open an agent, by its session's key, to talk to it (Arugula #1305):
    /// the Overview zooms to its Work level, Boards pushes it. Unset, the Overview opens it.
    var talkTo: ((String) -> Void)? {
        get { self[TalkKey.self] }
        set { self[TalkKey.self] = newValue }
    }
}

/// Open an agent to talk to it: where the view is, by `talkTo`, else on
/// The Overview, in the project.
@MainActor
func openAgent(_ key: String, talkTo: ((String) -> Void)?, project: String?) {
    if let talkTo {
        talkTo(key)
    } else if let project {
        ZoomCenter.shared.route = ZoomRoute(project: project, zoom: .work(key))
    }
}

/// A session's Work level pushed where the Overview isn't (Boards, Arugula #1305).
struct WorkKey: Hashable { let key: String }

extension LeadLine {
    /// Its session's key: its pane's when a machine here tells of it, else
    /// `lead:MACHINE:SESSION`, which the Work level says it can't reach.
    var key: String { peeked?.key ?? "lead:\(lead.machine):\(lead.session)" }

    /// Its key, with its name kept for the Work level to call it by.
    @MainActor
    var opening: String {
        ZoomCenter.shared.names[key] = lead.session
        return key
    }
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

    /// Zoom out: in the Overview, in place; anywhere else (a level pushed on
    /// Boards), on the Overview.
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

extension Peeked {
    /// What a session is called: its conversation, its pane, or its card.
    @MainActor
    var name: String {
        switch self {
        // Its conversation's title, an agent block's own, else its session's
        // name (a lead's, as its chip names it), else what the pane shows.
        case .pane(let m, let p):
            m.conversations[p.id] ?? ZoomCenter.shared.names[key]
                ?? (p.isAgent ? nil : m.sessions.first { $0.tabs.contains(p.tab) }?.name) ?? p.name
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
        case .pane(let m, let p): m.standing(of: (m.agents + m.panes).first { $0.id == p.id } ?? p)
        case .hosted(let s): s.standing
        }
    }

    /// Whose it is when it isn't the reader's (Arugula #1392).
    @MainActor
    var owner: String? {
        if case .pane(let m, _) = self { return m.owner }
        return nil
    }
}
