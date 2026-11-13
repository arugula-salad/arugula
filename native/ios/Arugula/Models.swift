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

struct Board: Decodable, Identifiable {
    struct Progress: Decodable { var done: Int?; var total: Int? }
    var id: String
    var title: String
    var goal: String?
    var progress: Progress?
}

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
    static func == (a: ChatRoom, b: ChatRoom) -> Bool { a.id == b.id }
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
