// New on home (Arugula #1257): tell a board, or start a New idea.
//
// - Tell a board: pick one; its thread opens, and a post there reaches the
//   session that leads it.
// - New idea: what you say (typed, or dictated with the keyboard's mic)
//   becomes a native issue, "Idea: …", whose body holds the agent's
//   instructions and your words. Then `POST /api/issues/{issue}/agent`
//   starts a hosted session whose prompt is the issue's thread, as Agent on
//   this does: the conversation is that session's thread, and home lists it
//   under New ideas. Where the account has no hosted sessions, the idea runs
//   as an agent block on a machine you pick (`POST /api/blocks` over its
//   link), on the same issue, and the conversation is the issue's thread.

import SwiftUI

/// What the agent of a New idea is asked to do, before your words.
func ideaInstructions(by name: String) -> String {
    """
    A new idea from \(name), on their phone. Their words are below.

    Ask at most two short questions here, in this thread, if you need to. Then write a project Doc that sets the idea out (the arugula doc tool, in this project), with a prototype section when one fits, and post its link here. When \(name) says so, turn the Doc into a board with its rows.
    """
}

/// An idea's title: its first line, cut short.
func ideaTitle(_ words: String) -> String {
    let first = words.split(whereSeparator: \.isNewline).first.map(String.init) ?? words
    let t = first.trimmingCharacters(in: .whitespaces)
    return "Idea: " + (t.count > 70 ? String(t.prefix(69)) + "…" : t)
}

/// `POST /api/issues/{issue}/agent`'s answer: the thread and the message that
/// asked, and the session it started, if it did.
struct IssueAgent: Decodable {
    var room: String
    var seq: Int
    var session: String?
}

/// New's two choices, then the one picked.
struct NewSheet: View {
    let control: Control
    @ObservedObject var model: HomeModel
    @ObservedObject var fleet: HomeFleet
    /// Open what was made: a board's thread, or an idea's conversation.
    let open: (Opened) -> Void
    let toast: (String) -> Void
    /// Something was made: home looks again.
    let made: () -> Void
    @EnvironmentObject private var scope: Scope
    @Environment(\.dismiss) private var dismiss
    @State private var step = Step.choose
    @State private var words = ""
    @State private var busy = false
    @State private var problem: String?
    /// The idea's issue, when hosted sessions weren't there for it: it runs
    /// on a machine instead.
    @State private var unhosted: Issue?

    enum Step { case choose, board, idea }

    var body: some View {
        NavigationStack {
            Group {
                switch step {
                case .choose: choose
                case .board: boards
                case .idea: idea
                }
            }
            .padding()
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(Palette.bg)
            .navigationTitle(step == .choose ? "New" : step == .board ? "Tell a board" : "New idea")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }.accessibilityIdentifier("new-cancel")
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private var choose: some View {
        VStack(alignment: .leading, spacing: 10) {
            choice("Tell a board", "Posts in its thread; the session leading it picks it up", id: "new-board") { step = .board }
            choice("New idea", "An agent asks a question or two and writes a Doc", id: "new-idea") { step = .idea }
        }
    }

    private func choice(_ title: String, _ says: String, id: String, _ go: @escaping () -> Void) -> some View {
        Button(action: go) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(Face.ui(16, "SemiBold")).foregroundStyle(Palette.fg)
                Text(says).font(Face.ui(13)).foregroundStyle(Palette.dim)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(Palette.surface, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Palette.raised, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(id)
    }

    /// The open boards, those with a lead first: a post on one's thread
    /// reaches its lead.
    private var boards: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                if model.boards.isEmpty { Caps("No open boards") }
                ForEach(model.boards.sorted { ($0.lead == nil ? 1 : 0, $0.title) < ($1.lead == nil ? 1 : 0, $1.title) }) { b in
                    choice(b.title, BoardLineRow.lead(b.lead), id: "tell-\(b.title)") {
                        dismiss()
                        open(.board(b))
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var idea: some View {
        if let issue = unhosted {
            machines(issue)
        } else {
            VStack(alignment: .leading, spacing: 10) {
                // The keyboard's mic dictates into it.
                TextField("Say it however it comes out…", text: $words, axis: .vertical)
                    .lineLimit(4...10)
                    .font(Face.ui(16))
                    .padding(10)
                    .background(Palette.surface, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Palette.raised, lineWidth: 1))
                    .accessibilityIdentifier("idea-words")
                if let problem {
                    Text(problem).font(Face.mono(11)).foregroundStyle(Palette.danger)
                }
                Button(busy ? "Starting…" : "Start") { Task { await start() } }
                    .buttonStyle(AnswerButton(primary: true))
                    .disabled(busy || words.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .accessibilityIdentifier("idea-start")
                Text("An agent in Arugula cloud reads it, may ask you a question or two, then writes a Doc.")
                    .font(Face.ui(12)).foregroundStyle(Palette.dim)
            }
        }
    }

    /// No hosted session for it: pick a machine of yours to run it on.
    private func machines(_ issue: Issue) -> some View {
        let online = fleet.machines.filter(\.connected)
        return VStack(alignment: .leading, spacing: 10) {
            Text("Arugula cloud can't run it for you\(problem.map { ": \($0)" } ?? ""). Run it on one of your machines?")
                .font(Face.ui(14)).foregroundStyle(Palette.subtle)
            if online.isEmpty { Caps("No machine is online") }
            ForEach(online, id: \.machine.id) { m in
                choice(m.machine.name, "A Claude Code agent block there, on #\(issue.number)", id: "idea-on-\(m.machine.name)") {
                    Task { await onMachine(issue, m) }
                }
                .disabled(busy)
            }
        }
    }

    /// File the idea, then ask Arugula cloud's agent to take it.
    private func start() async {
        guard let p = scope.current else { return }
        busy = true
        defer { busy = false }
        let text = words.trimmingCharacters(in: .whitespacesAndNewlines)
        let me = model.me ?? "the person"
        let issue: Issue
        do {
            let d = try await control.call("POST", "/api/projects/\(p.id)/issues", body: [
                "title": ideaTitle(text), "body": ideaInstructions(by: me) + "\n\n---\n\n" + text,
            ])
            issue = try JSONDecoder().decode(Issue.self, from: d)
        } catch {
            problem = describe(error)
            return
        }
        do {
            let d = try await control.call("POST", "/api/issues/\(issue.id)/agent", body: [:])
            let a = try JSONDecoder().decode(IssueAgent.self, from: d)
            guard a.session != nil else {
                // The thread says why (no box, no repo): try a machine.
                problem = nil
                unhosted = issue
                return
            }
            dismiss()
            made()
            toast("Started: \(issue.title)")
            open(.idea(room: a.room, parent: a.seq, title: issue.title))
        } catch {
            problem = describe(error)
            unhosted = issue
        }
    }

    /// The idea as an agent block on a machine, working on its issue; its
    /// conversation is the issue's thread.
    private func onMachine(_ issue: Issue, _ m: MachineSession) async {
        guard let p = scope.current else { return }
        busy = true
        defer { busy = false }
        do {
            let prompt = """
            \(issue.body)

            This is Arugula issue #\(issue.number) (\(issue.id)) in project \(p.id). Talk with them in its thread (post_thread with room \(issue.id), and read_thread there for their answers).
            """
            _ = try await m.request("POST", "/api/blocks", [
                "type": "agent",
                "config": ["agent": "claude", "prompt": prompt, "title": issue.title, "work": ["issue": "#\(issue.number)", "project": p.id]],
            ])
            let d = try await control.call("POST", "/api/chat/targets", body: ["machine": "", "kind": "object", "target": issue.id])
            let room = try JSONDecoder().decode(ChatRoom.self, from: d)
            dismiss()
            made()
            toast("Started on \(m.machine.name): \(issue.title)")
            open(.idea(room: room.id, parent: nil, title: issue.title))
        } catch {
            problem = describe(error)
        }
    }
}

/// A New idea on home: its title, and where its agent is.
struct IdeaLine: Identifiable {
    let issue: Issue
    let session: SessionWave.Row?
    var id: String { issue.id }

    var says: String {
        guard let s = session else { return "on a machine, or not started" }
        if s.waiting != nil { return s.question ?? "asks you" }
        switch s.state {
        case "queued", "starting": return "starting…"
        case "working", "running": return "writing…"
        case "failed", "error": return "failed"
        case "stopped": return "stopped"
        default: return "replied"
        }
    }

    var standing: Standing {
        guard let s = session else { return .idle }
        if s.waiting != nil { return .asks }
        return s.standing
    }
}

struct IdeaRow: View {
    let line: IdeaLine

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 7) {
                Circle().fill(line.standing.tone.color).frame(width: 7, height: 7)
                Text(line.issue.title.replacingOccurrences(of: "Idea: ", with: ""))
                    .font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(2)
            }
            Text(line.says).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(2)
        }
        .padding(.vertical, 4)
    }
}
