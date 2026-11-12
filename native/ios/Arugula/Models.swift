// What the app reads from control: only the fields it shows, so anything else
// control adds changes nothing here.

import Foundation

struct Projects: Decodable {
    struct Me: Decodable { var account: String; var login: String; var name: String }
    struct Team: Decodable { var id: String; var name: String; var role: String }
    struct Project: Decodable, Identifiable, Hashable {
        var id: String
        var name: String
        var team: Team?
        static func == (a: Project, b: Project) -> Bool { a.id == b.id }
        func hash(into h: inout Hasher) { h.combine(id) }
    }
    var me: Me
    var projects: [Project]
}

struct Issue: Decodable, Identifiable, Hashable {
    var id: String
    var number: Int
    var title: String
    var body: String
    var state: String
    var labels: [String]
}

struct IssueList: Decodable { var issues: [Issue] }

struct Board: Decodable, Identifiable, Hashable {
    struct Progress: Decodable { var done: Int?; var total: Int? }
    var id: String
    var title: String
    var goal: String?
    var progress: Progress?
    static func == (a: Board, b: Board) -> Bool { a.id == b.id }
    func hash(into h: inout Hasher) { h.combine(id) }
}

/// One board: its lanes (Next, Doing, Review, Done) with their rows, and its
/// groups. Only what the board screen shows.
struct BoardDetail: Decodable {
    struct Info: Decodable { var title: String; var goal: String? }
    /// The issue's or PR's state as the board read it.
    struct Live: Decodable { var kind: String?; var number: Int?; var title: String?; var state: String? }
    struct Pointer: Decodable { var kind: String; var id: String?; var url: String? }
    struct Row: Decodable, Identifiable {
        var id: String
        var note: String?
        var group: String?
        var live: Live?
        var pointer: Pointer?
    }
    struct Lane: Decodable, Identifiable { var id: String; var title: String; var rows: [Row] }
    struct Group: Decodable, Identifiable { var id: String; var title: String }
    var board: Info
    var zones: [Lane]
    var groups: [Group]?

    func groupTitle(_ id: String?) -> String? { id.flatMap { g in groups?.first { $0.id == g }?.title } }
}

/// A native issue to load by its id, as a board's row names it.
struct IssueRef: Hashable { var id: String }

struct BoardList: Decodable { var boards: [Board] }

struct ChatRoom: Decodable, Identifiable, Hashable {
    var id: String
    var kind: String
    var name: String
    var last: Int
    var unread: Int
    var mentions: Int
    var archived: Bool
    var writable: Bool
    var with: Person?
    var others: [Person]?
    struct Person: Decodable { var name: String }
    static func == (a: ChatRoom, b: ChatRoom) -> Bool { a.id == b.id }

    /// A channel or a DM: what the chat list shows. An issue's, a pane's or a
    /// session's thread belongs with its thing.
    var isConversation: Bool { kind == "channel" || kind == "dm" }

    /// What the list and the room call it: a channel by its name, a DM by
    /// its name if it was given one, else by the people in it.
    var title: String {
        switch kind {
        case "channel": return "# \(name)"
        case "dm":
            if !name.isEmpty { return name }
            let people = (others ?? []).map(\.name).joined(separator: ", ")
            return people.isEmpty ? (with?.name ?? "Direct message") : people
        default: return name
        }
    }
    func hash(into h: inout Hasher) { h.combine(id) }
}

struct ChatRooms: Decodable { var rooms: [ChatRoom] }

struct ChatMessage: Decodable, Identifiable {
    var seq: Int
    var at: Int
    var name: String
    var text: String
    var deleted: Int?
    var id: Int { seq }
}

struct ChatMessages: Decodable { var messages: [ChatMessage] }

struct Machine: Decodable, Identifiable, Hashable {
    var id: String
    var name: String
    var online: Bool?
    var account: String?
    static func == (a: Machine, b: Machine) -> Bool { a.id == b.id }
    func hash(into h: inout Hasher) { h.combine(id) }
}

struct Directory: Decodable { var daemons: [Machine] }

struct PaneRow: Decodable, Identifiable, Hashable {
    var id: UInt32
    var tab: UInt32
    var tab_name: String?
    var command: String?
    var cwd: String?
    var running: Bool
}

struct PaneList: Decodable { var panes: [PaneRow] }
