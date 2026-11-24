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
    /// The session that leads it: a post on its thread reaches it.
    struct Lead: Decodable { var machine: String; var machine_name: String?; var session: String }
    var id: String
    var title: String
    var goal: String?
    var progress: Progress?
    var closed_at: Int?
    var lead: Lead?
    static func == (a: Board, b: Board) -> Bool { a.id == b.id }
    func hash(into h: inout Hasher) { h.combine(id) }
}

/// One board: its lanes (Next, Doing, Review, Done) with their rows, and its
/// groups. Only what the board screen shows.
struct BoardDetail: Decodable {
    struct Info: Decodable { var title: String; var goal: String?; var closed_at: Int?; var lead: Board.Lead? }
    /// The issue's or PR's state as the board read it.
    struct Live: Decodable { var kind: String?; var number: Int?; var title: String?; var state: String?; var repo: String? }
    /// What a row points at; a block names its machine and pane.
    struct Pointer: Decodable { var kind: String; var id: String?; var url: String?; var machine: String?; var pane: String? }
    /// The row's one open question: `mine` when it's for the reader.
    /// `who` is the person it asks (empty: the board raised it), `who_name` their name.
    struct Ask: Decodable { var id: String; var question: String; var options: [String]; var mine: Bool; var who: String?; var who_name: String?; var can_answer: Bool?; var asked_by: String?; var raised: Bool?; var created_at: Int? }
    struct Attachment: Decodable { var role: String; var pointer: Pointer; var released: Bool?; var live: Live? }
    struct Mark: Decodable { var kind: String }
    struct Row: Decodable, Identifiable {
        var id: String
        var note: String?
        var group: String?
        var lane: String?
        var live: Live?
        var pointer: Pointer?
        var ask: Ask?
        var attachments: [Attachment]?
        var marks: [Mark]?
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

/// An issue by its number in a project, as a page's address names it.
struct ProjectIssue: Hashable { var project: String; var number: String }

struct BoardList: Decodable { var boards: [Board] }

/// A project Doc on its project's list (the web's `ProjectDoc`), its times
/// in milliseconds.
struct ProjectDoc: Decodable, Identifiable, Hashable {
    var id: String
    var project: String
    var title: String
    /// The account that made it, a machine's agent's owner's included; "" for
    /// one an agent session made.
    var author: String
    /// `tabs` or `prototype`; absent (or one the app doesn't know) is a plain Doc.
    var layout: String?
    var created_at: Int
    var updated_at: Int
    var updated: Date { Date(timeIntervalSince1970: Double(updated_at) / 1000) }
    var ref: DocRef { DocRef(project: project, doc: id, label: title) }
}

/// `GET /api/projects/:project/docs`: the latest changed first.
struct ProjectDocList: Decodable { var docs: [ProjectDoc] }

struct ChatRoom: Decodable, Identifiable, Hashable {
    var id: String
    var kind: String
    var name: String
    var last: Int
    var unread: Int
    var mentions: Int
    var archived: Bool
    var writable: Bool
    /// The team a channel or DM is in.
    var team: String?
    var with: Person?
    var others: [Person]?
    /// The project an object's thread or a staff room is in.
    var project: String?
    /// A staff room (Arugula #1275): a DM of the reader and their own
    /// agents, one per project, the chief of staff's.
    var staff: Bool?
    /// The session that leads a staff room: the reader's posts there reach it.
    var lead: Board.Lead?
    struct Person: Decodable { var name: String }
    static func == (a: ChatRoom, b: ChatRoom) -> Bool { a.id == b.id }

    var isStaff: Bool { staff == true }

    /// A channel or a DM: what the chat list shows. An issue's, a pane's or a
    /// session's thread belongs with its thing.
    var isConversation: Bool { kind == "channel" || kind == "dm" }

    /// What the list and the room call it: a channel by its name, a DM by
    /// its name if it was given one, else by the people in it.
    var title: String {
        switch kind {
        case "channel": return "# \(name)"
        case "dm":
            if isStaff { return "Chief of staff" }
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
    /// Someone else's (M19): its owner's name.
    var owner_name: String?
    /// The team's own box, not a person's machine (Arugula #1182).
    var team_box: Bool?
    /// Whose it is when it isn't yours, as the web's fleet says it; nil: yours.
    var owner: String? { account.map { owner_name ?? $0 } }
    var yours: Bool { account == nil }
    static func == (a: Machine, b: Machine) -> Bool { a.id == b.id }
    func hash(into h: inout Hasher) { h.combine(id) }
}

struct Directory: Decodable { var daemons: [Machine] }

struct PaneRow: Decodable, Identifiable, Hashable {
    struct Reason: Decodable, Hashable {
        var kind: String
        var headline: String
        var since_ms: Int?
        /// What it asks, answered by its `id` (`POST /api/attention/act`).
        var ask: AskRef?
    }
    /// A pending question or approval: `approve` or `question`.
    struct AskRef: Decodable, Hashable {
        struct Option: Decodable, Hashable { var label: String; var description: String? }
        struct Question: Decodable, Hashable {
            var question: String
            var header: String?
            var multiSelect: Bool?
            var options: [Option]
        }
        var id: String
        var what: String
        var agent: String?
        var tool: String?
        var questions: [Question]?
    }
    /// What it asks while it waits on a person: a permission, questions.
    struct Ask: Decodable, Hashable {
        var id: String
        /// `questions`, `form`, `url` or `permission`.
        var kind: String
        var message: String
        var tool: String?
        var at_ms: Int?
        var questions: [AskRef.Question]?
    }
    struct ProjectRef: Decodable, Hashable { var id: String }
    /// The card its agent was started on.
    struct OnIssue: Decodable, Hashable {
        struct OnBoard: Decodable, Hashable { var board: String; var row: String }
        var issue: String
        var project: String?
        var boards: [OnBoard]?
    }
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
    /// Its git project, by name (the daemon sends `{name, root}`).
    var project: Named?
    var branch: String?
    var last_ms: Int?
    /// `terminal`, or `agent` for an agent block.
    var type: String?
    var ask: Ask?
    var session_project: ProjectRef?
    var on_issue: OnIssue?
    /// Teammates' agents may not mention it (#579).
    var mentions_off: Bool?

    /// The program, without its arguments: "claude", "vim".
    var program: String? {
        command?.split(separator: " ").first.map { String($0.split(separator: "/").last ?? $0) }
    }

    /// What to call it: its program's title (without a spinner in front),
    /// else the conversation it resumes, else its program. A title that's
    /// only an id (a conversation's hash) isn't a name.
    var name: String {
        let cleaned = (title ?? "").drop { !$0.isLetter && !$0.isNumber }.trimmingCharacters(in: .whitespaces)
        // A shell's own "user@host: dir" isn't a name: its folder is.
        let shellTitle = cleaned.range(of: #"^[^\s@]+@[^\s:]+:"#, options: .regularExpression) != nil
        if Self.readable(cleaned), !shellTitle, cleaned != program, cleaned.lowercased() != "claude code" { return cleaned }
        if let r = resumes?.replacingOccurrences(of: "Claude Code conversation ", with: ""), Self.readable(r) { return r }
        if let project { return "\(program ?? "shell") in \(project.name)" }
        if let cwd {
            // A home folder is "~", as the web writes it.
            let home = cwd.range(of: #"^/(home|Users)/[^/]+/?$"#, options: .regularExpression) != nil
            let folder = home ? "~" : (cwd.split(separator: "/").last.map(String.init) ?? "/")
            return "\(program ?? "shell") in \(folder)"
        }
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

struct PaneList: Decodable {
    var panes: [PaneRow]
    /// Agent blocks, apart from the terminals.
    var agents: [PaneRow]?
    var tabs: [TabLayout]?
    var sessions: [MuxSession]?
    /// This person's role in each session, `[session, role]`; none: the owner.
    var roles: [RoleOf]?

    enum CodingKeys: String, CodingKey { case panes, agents, tabs, sessions, roles }

    /// One at a time: a pane or session this build can't read is left out,
    /// not the machine's whole list.
    init(from d: Decoder) throws {
        let c = try d.container(keyedBy: CodingKeys.self)
        panes = try c.decode([Lenient<PaneRow>].self, forKey: .panes).compactMap(\.value)
        agents = try? c.decodeIfPresent([Lenient<PaneRow>].self, forKey: .agents)?.compactMap(\.value)
        tabs = try? c.decodeIfPresent([Lenient<TabLayout>].self, forKey: .tabs)?.compactMap(\.value)
        sessions = try? c.decodeIfPresent([Lenient<MuxSession>].self, forKey: .sessions)?.compactMap(\.value)
        roles = try? c.decodeIfPresent([RoleOf].self, forKey: .roles)
    }

    struct RoleOf: Decodable {
        var session: UInt32
        var role: String
        init(from d: Decoder) throws {
            var c = try d.unkeyedContainer()
            session = try c.decode(UInt32.self)
            role = try c.decode(String.self)
        }
    }
}

/// One of a machine's sessions: a named group of its tabs, maybe in a project.
struct MuxSession: Decodable, Identifiable, Equatable {
    var id: UInt32
    var name: String
    struct Project: Decodable, Equatable { var id: String; var name: String }
    /// The Arugula project it's in.
    var project: Project?
    var tabs: [UInt32]
}

/// A tab as the machine lays it out: its grid, and each pane's rectangle in it
/// (blocks too, `kind` other than `terminal`).
struct TabLayout: Decodable, Identifiable, Equatable {
    struct Cell: Decodable, Equatable {
        var pane: UInt32
        var kind: String?
        var x: UInt16
        var y: UInt16
        var cols: UInt16
        var rows: UInt16
    }
    var id: UInt32
    var name: String?
    var cols: UInt16
    var rows: UInt16
    var cells: [Cell]

    var terminals: [UInt32] { cells.filter { ($0.kind ?? "terminal") == "terminal" }.map(\.pane) }
}

/// A project's agent sessions in Arugula cloud, newest first
/// (`GET /api/projects/{p}/sessions`): what each waits on the reader for.
struct SessionWave: Decodable {
    struct Issue: Decodable, Hashable { var id: String; var number: Int; var title: String }
    struct Row: Decodable, Identifiable, Hashable {
        var session: String
        /// Its thread: the room and the message it hangs off.
        var room: String
        var parent: Int
        var state: String
        var issue: Issue?
        /// What waits on the reader, in a few words ("its question", "a
        /// permission", "its PR to send"), if anything.
        var waiting: String?
        var question: String?
        var updated_at: Int
        var id: String { session }
        static func == (a: Row, b: Row) -> Bool { a.session == b.session }
        func hash(into h: inout Hasher) { h.combine(session) }
    }
    var sessions: [Row]
}

/// One hosted session (`GET /api/sessions/{ses}`): what it asks now, whether
/// the reader may answer it, and its history in words.
struct SessionView: Decodable {
    struct Open: Decodable {
        var request: String?
        /// `permission` or `question`.
        var ask: String?
        var question: String?
        var options: [String]
    }
    struct Answered: Decodable { var kind: String; var name: String? }
    struct Card: Decodable {
        var session: String
        var state: String
        var open: Open?
        var answered: Answered?
    }
    struct Line: Decodable { var seq: Int; var text: String }
    var card: Card
    /// An owner or editor of its project: it may answer it and stop it.
    var may: Bool
    var events: [Line]
}

/// Something named: a project as a daemon sends it, `{name, ...}`, or by its
/// name alone.
struct Named: Decodable, Hashable, CustomStringConvertible {
    var name: String
    var description: String { name }

    private struct Object: Decodable { var name: String }

    init(from d: Decoder) throws {
        if let s = try? d.singleValueContainer().decode(String.self) {
            name = s
        } else {
            name = try Object(from: d).name
        }
    }
}

/// An element of a list that decodes, or nil, so one odd element doesn't
/// cost the list.
struct Lenient<T: Decodable>: Decodable {
    var value: T?
    init(from d: Decoder) throws { value = try? T(from: d) }
}
