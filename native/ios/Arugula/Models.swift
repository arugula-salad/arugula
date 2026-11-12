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
    struct Progress: Decodable { var done: Int?; var review: Int?; var doing: Int?; var total: Int? }
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
        var updated_at: Int?
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
    struct Agent: Decodable { var machine_name: String }
    struct Reaction: Decodable { var emoji: String; var count: Int; var mine: Bool }
    struct File: Decodable { var name: String }
    var seq: Int
    /// When it was posted, in milliseconds.
    var at: Int
    var author: String?
    var name: String
    var text: String
    var quote: String?
    var deleted: Int?
    var edited: Int?
    var agent: Agent?
    var via: String?
    var reactions: [Reaction]?
    var files: [File]?
    var id: Int { seq }

    /// Who it's from, as the web says it.
    var from: String {
        if let agent { return "\(name)'s agent on \(agent.machine_name)" }
        if let via, !via.isEmpty, !name.isEmpty { return "\(name) via \(via)" }
        return name.isEmpty ? "Arugula" : name
    }
    var date: Date { Date(timeIntervalSince1970: Double(at) / 1000) }
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
    struct Reason: Decodable, Hashable { var kind: String; var headline: String }
    var id: UInt32
    var tab: UInt32
    var tab_name: String?
    var command: String?
    var cwd: String?
    var running: Bool
    /// idle, working, needs_input or done.
    var attention: String?
    var reason: Reason?
    /// What its program calls it (OSC 0/2): Claude Code's is its conversation.
    var title: String?
    var work: String?
    /// What a restart resumes: "Claude Code conversation <title>".
    var resumes: String?
    var project: String?
    var branch: String?
    var last_ms: Int?

    /// The program, without its arguments: "claude", "vim".
    var program: String? {
        command?.split(separator: " ").first.map { String($0.split(separator: "/").last ?? $0) }
    }

    /// What to call it: its program's title (without a spinner in front),
    /// else the conversation it resumes, else its program. A title that's
    /// only an id (a conversation's hash) isn't a name.
    var name: String {
        let cleaned = (title ?? "").drop { !$0.isLetter && !$0.isNumber }.trimmingCharacters(in: .whitespaces)
        if Self.readable(cleaned), cleaned != program, cleaned.lowercased() != "claude code" { return cleaned }
        if let r = resumes?.replacingOccurrences(of: "Claude Code conversation ", with: ""), Self.readable(r) { return r }
        if let project { return "\(program ?? "shell") in \(project)" }
        return program ?? "shell"
    }

    /// Words a person can read: not empty, and not a hex id.
    static func readable(_ s: String) -> Bool {
        let t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return false }
        let hex = t.unicodeScalars.allSatisfy { CharacterSet(charactersIn: "0123456789abcdefABCDEF-").contains($0) }
        return !(hex && t.count >= 6)
    }

    /// Wants you first, then working, done and idle; inside each, latest first.
    var order: Int {
        switch attention {
        case "needs_input": 0
        case "working": 1
        case "done": 2
        default: 3
        }
    }
}

struct PaneList: Decodable { var panes: [PaneRow] }
