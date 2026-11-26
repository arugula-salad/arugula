// Answering in the row (Arugula #1256): what needs you is answered where it
// is on home, without opening anything, and the row folds away. A machine
// pane's question or approval goes to its daemon over the link (`POST
// /api/attention/act`, as the web's attention drawer sends it); a hosted
// session's to control (`POST /api/sessions/{ses}/actions`, as its card
// does); a board row's ask to its board (`POST /api/boards/{id}/asks/{ask}/
// answer`). A session's peek is a sheet: its last lines, a reply to its
// agent, Silence, Stop (asked first) and its terminal or thread. What a
// person may do is their role's: a viewer reads, with no buttons.

import SwiftUI

// ---- answering

/// The answer under a row of Needs you, as its kind takes one.
struct NeedAnswer: View {
    let need: Need
    let control: Control
    @ObservedObject var model: HomeModel
    /// Answered: what to say of it, and the row folds away.
    let done: (String) -> Void
    @State private var busy = false
    @State private var problem: String?
    @State private var words = ""
    /// A pane's questions, picked so far, by their number.
    @State private var picks: [Int: String] = [:]

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
        switch need.target {
        case .pane(let m, let p):
            if m.role(of: p) == "viewer" {
                Caps("View only")
            } else if let ask = p.asking, ask.approve {
                choices(["Allow", "Deny"]) { c in
                    try await act(m, p, ["action": c == "Allow" ? "allow" : "deny", "id": ask.id])
                    return c == "Allow" ? "Allowed" : "Denied"
                }
            } else if let ask = p.asking, !ask.questions.isEmpty {
                questions(m, p, ask.id, ask.questions)
            } else if p.failed {
                choices(["Dismiss"]) { _ in
                    try await act(m, p, ["action": "dismiss"])
                    return "Dismissed"
                }
            }
        case .board(let b, let row):
            if let a = row?.ask, a.can_answer != false {
                if a.options.isEmpty {
                    own("Your answer") { text in
                        try await answerBoard(b, a, ["text": text])
                        return "Answered"
                    }
                } else {
                    choices(a.options) { c in
                        try await answerBoard(b, a, ["option": c])
                        return c
                    }
                }
            }
        case .session(let s):
            if let v = model.opened[s.session], v.may, let o = v.card.open {
                hosted(s, v, o)
            } else if model.opened[s.session]?.may == false {
                Caps("View only")
            }
        case .todo:
            // Answered by its own row (TodoAnswer).
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

    /// The asker's own words, sent with Send.
    private func own(_ prompt: String, _ send: @escaping (String) async throws -> String) -> some View {
        HStack(spacing: 6) {
            TextField(prompt, text: $words)
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

    /// A pane's questions: one question with one pick answers at the tap;
    /// several, or a multiple choice, are picked and then sent.
    @ViewBuilder
    private func questions(_ m: MachineSession, _ p: PaneRow, _ ask: String, _ qs: [PaneRow.AskRef.Question]) -> some View {
        if qs.count == 1, qs[0].multiSelect != true {
            choices(qs[0].options.map(\.label)) { c in
                try await act(m, p, ["action": "answer", "id": ask, "content": ["question_0": c]])
                return c
            }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array(qs.enumerated()), id: \.offset) { n, q in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(q.header ?? q.question).font(Face.mono(11, "Bold")).foregroundStyle(Palette.dim)
                        FlowRow(spacing: 6) {
                            ForEach(q.options, id: \.label) { o in
                                Button(o.label) { picks[n] = o.label }
                                    .buttonStyle(AnswerButton(primary: picks[n] == o.label))
                            }
                        }
                    }
                }
                Button("Send") {
                    var content: [String: Any] = [:]
                    for (n, c) in picks { content["question_\(n)"] = qs[n].multiSelect == true ? [c] : c }
                    run {
                        try await act(m, p, ["action": "answer", "id": ask, "content": content])
                        return "Answered"
                    }
                }
                .buttonStyle(AnswerButton(primary: true))
                .disabled(picks.count < qs.count)
                .accessibilityIdentifier("answer-send")
            }
        }
    }

    /// A hosted session's question or permission, as its card answers it.
    @ViewBuilder
    private func hosted(_ s: SessionWave.Row, _ v: SessionView, _ o: SessionView.Open) -> some View {
        if v.card.answered != nil {
            Caps("Answered: waiting for its box to take it")
        } else if o.ask == "permission" {
            let ids = o.options.isEmpty ? ["allow", "deny"] : o.options
            choices(ids.map(optionLabel)) { label in
                let id = ids[ids.map(optionLabel).firstIndex(of: label) ?? 0]
                let refuses = id.lowercased().hasPrefix("reject") || id.lowercased().hasPrefix("deny")
                var a: [String: Any] = ["kind": refuses ? "deny" : "approve"]
                if let r = o.request { a["request"] = r }
                if !refuses, !o.options.isEmpty { a["option"] = id }
                try await actHosted(s, a)
                return label
            }
        } else if !o.options.isEmpty {
            choices(o.options) { c in
                try await actHosted(s, answer(o, c))
                return c
            }
        } else {
            own("Your answer") { text in
                try await actHosted(s, answer(o, text))
                return "Answered"
            }
        }
    }

    private func answer(_ o: SessionView.Open, _ text: String) -> [String: Any] {
        var a: [String: Any] = ["kind": "answer", "answer": ["question_0": text]]
        if let r = o.request { a["request"] = r }
        return a
    }

    private func run(_ send: @escaping () async throws -> String) {
        busy = true
        problem = nil
        Task {
            do {
                let said = try await send()
                done(said)
            } catch {
                problem = describe(error)
            }
            busy = false
        }
    }

    private func act(_ m: MachineSession, _ p: PaneRow, _ body: [String: Any]) async throws {
        var b = body
        b["pane"] = p.id
        let got = try await m.request("POST", "/api/attention/act", b) as? [String: Any]
        let results = got?["results"] as? [[String: Any]] ?? []
        if let bad = results.first(where: { $0["ok"] as? Bool == false }) {
            throw CoreError(bad["error"] as? String ?? "the machine didn't take it")
        }
    }

    private func answerBoard(_ b: Board, _ a: BoardDetail.Ask, _ body: [String: Any]) async throws {
        _ = try await control.call("POST", "/api/boards/\(b.id)/asks/\(a.id)/answer", body: body)
    }

    private func actHosted(_ s: SessionWave.Row, _ body: [String: Any]) async throws {
        _ = try await control.call("POST", "/api/sessions/\(s.session)/actions", body: body)
    }
}

extension PaneRow {
    /// What it asks, to answer: its reason's ask, else the pane's own card
    /// (Claude Code's hooks in a terminal), as the web's drawer reads it.
    var asking: (id: String, approve: Bool, questions: [AskRef.Question])? {
        guard attention == "needs_input" else { return nil }
        if let a = reason?.ask { return (a.id, a.what == "approve", a.questions ?? []) }
        if let a = ask { return (a.id, a.kind == "permission", a.questions ?? []) }
        return nil
    }
}

/// A permission's option id in words: `allow_always` is "Allow always".
func optionLabel(_ id: String) -> String {
    let w = id.replacingOccurrences(of: "[-_]+", with: " ", options: .regularExpression).trimmingCharacters(in: .whitespaces)
    return w.isEmpty ? id : w.prefix(1).uppercased() + w.dropFirst()
}

/// An answer's button: the accent filled for the first choice, an outline
/// for the rest.
struct AnswerButton: ButtonStyle {
    var primary = false
    var danger = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(Face.ui(14, "SemiBold"))
            .lineLimit(1)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .foregroundStyle(primary ? Palette.void : danger ? Palette.danger : Palette.fg)
            .background(primary ? Palette.accent : Palette.surface, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(primary ? Color.clear : Palette.raised, lineWidth: 1))
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}

/// Buttons in a row, wrapping onto the next line when they don't fit.
struct FlowRow: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(proposal.width ?? .infinity, subviews)
        return CGSize(width: proposal.width ?? rows.map(\.width).max() ?? 0, height: rows.reduce(0) { $0 + $1.height } + spacing * CGFloat(max(rows.count - 1, 0)))
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in arrange(bounds.width, subviews) {
            var x = bounds.minX
            for i in row.items {
                let s = subviews[i].sizeThatFits(.unspecified)
                subviews[i].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(s))
                x += s.width + spacing
            }
            y += row.height + spacing
        }
    }

    private func arrange(_ width: CGFloat, _ subviews: Subviews) -> [(items: [Int], width: CGFloat, height: CGFloat)] {
        var rows: [(items: [Int], width: CGFloat, height: CGFloat)] = []
        var cur: [Int] = [], w: CGFloat = 0, h: CGFloat = 0
        for i in subviews.indices {
            let s = subviews[i].sizeThatFits(.unspecified)
            if !cur.isEmpty && w + spacing + s.width > width {
                rows.append((cur, w, h))
                cur = []; w = 0; h = 0
            }
            w += (cur.isEmpty ? 0 : spacing) + s.width
            h = max(h, s.height)
            cur.append(i)
        }
        if !cur.isEmpty { rows.append((cur, w, h)) }
        return rows
    }
}

/// A short word of what was done, at the bottom of home.
struct Toast: View {
    let text: String

    var body: some View {
        Text(text)
            .font(Face.ui(14, "SemiBold"))
            .foregroundStyle(Palette.void)
            .padding(.horizontal, 16)
            .padding(.vertical, 9)
            .background(Palette.fg, in: Capsule())
            .padding(.bottom, 24)
            .accessibilityIdentifier("toast")
    }
}

// ---- a session's peek

/// A session to peek at: a pane on a machine, or a hosted session.
enum Peeked: Identifiable {
    case pane(MachineSession, PaneRow)
    case hosted(SessionWave.Row)

    var id: String {
        switch self {
        case .pane(let m, let p): "pane:\(m.machine.id):\(p.id)"
        case .hosted(let s): "hosted:\(s.session)"
        }
    }
}

/// A session's last lines and what can be done with it, as a sheet.
struct SessionPeek: View {
    let control: Control
    let peeked: Peeked
    /// Go on to its terminal, or its thread.
    let openPane: (MachineSession, PaneRow) -> Void
    let openThread: (SessionWave.Row) -> Void
    let toast: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var lines = ""
    @State private var reply = ""
    @State private var problem: String?
    @State private var confirmStop = false
    @State private var may = false
    @State private var silenced = false

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                WhoLine(text: who)
                ScrollViewReader { proxy in
                    ScrollView {
                        Text(lines.isEmpty ? "…" : lines)
                            .font(Face.mono(11))
                            .foregroundStyle(Palette.subtle)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .textSelection(.enabled)
                            .id("end")
                            .accessibilityIdentifier("peek-lines")
                    }
                    .padding(10)
                    .background(Palette.void, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.raised, lineWidth: 1))
                    .onChange(of: lines) { _, _ in proxy.scrollTo("end", anchor: .bottom) }
                }
                if let problem {
                    Text(problem).font(Face.mono(11)).foregroundStyle(Palette.danger)
                }
                if may {
                    HStack(spacing: 6) {
                        TextField("Reply to the agent…", text: $reply, axis: .vertical)
                            .lineLimit(1...4)
                            .font(Face.ui(15))
                            .padding(.horizontal, 10).padding(.vertical, 8)
                            .background(Palette.surface, in: RoundedRectangle(cornerRadius: 10))
                            .accessibilityIdentifier("peek-reply")
                        Button("Send") { Task { await send() } }
                            .buttonStyle(AnswerButton(primary: true))
                            .disabled(reply.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                            .accessibilityIdentifier("peek-send")
                    }
                }
                FlowRow(spacing: 6) {
                    if case .pane(let m, let p) = peeked, m.role(of: p) == "owner" {
                        Button(silenced ? "Unsilence" : "Silence") { Task { await silence() } }
                            .buttonStyle(AnswerButton())
                            .accessibilityIdentifier("peek-silence")
                    }
                    if may {
                        Button("Stop") { confirmStop = true }
                            .buttonStyle(AnswerButton(danger: true))
                            .accessibilityIdentifier("peek-stop")
                    }
                    switch peeked {
                    case .pane(let m, let p) where p.type != "agent":
                        Button("Open terminal") {
                            dismiss()
                            openPane(m, p)
                        }
                        .buttonStyle(AnswerButton())
                        .accessibilityIdentifier("peek-terminal")
                    case .pane:
                        EmptyView()
                    case .hosted(let s):
                        Button("Open thread") {
                            dismiss()
                            openThread(s)
                        }
                        .buttonStyle(AnswerButton())
                        .accessibilityIdentifier("peek-thread")
                    }
                }
                Spacer(minLength: 0)
            }
            .padding()
            .background(Palette.bg)
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") { dismiss() }.accessibilityIdentifier("peek-close")
                }
            }
            .confirmationDialog("Stop \(title)?", isPresented: $confirmStop, titleVisibility: .visible) {
                Button("Stop", role: .destructive) { Task { await stop() } }
                    .accessibilityIdentifier("peek-stop-confirm")
            } message: {
                Text(stopWords)
            }
        }
        .presentationDetents([.medium, .large])
        .task {
            while !Task.isCancelled {
                await look()
                try? await Task.sleep(for: .seconds(3))
            }
        }
    }

    private var title: String {
        switch peeked {
        case .pane(let m, let p): m.conversations[p.id] ?? p.name
        case .hosted(let s): s.issue.map { "#\($0.number) \($0.title)" } ?? "Session"
        }
    }

    /// Who this is and where a reply goes (Arugula #1305).
    private var who: String {
        switch peeked {
        case .pane(let m, let p):
            let on = "\(title) on \(m.machine.name)"
            if m.role(of: p) == "viewer" { return "Watching \(on): you can read it, not type to it." }
            return p.isAgent || m.conversations[p.id] != nil
                ? "Talking to \(on): a reply goes straight to this agent."
                : "Typing into \(on): a reply goes to this terminal."
        case .hosted:
            return may
                ? "Talking to \(title) in Arugula cloud: a reply goes to its session's thread, which it reads."
                : "Watching \(title) in Arugula cloud: you can read it, not reply."
        }
    }

    private var stopWords: String {
        switch peeked {
        case .pane(_, let p): p.isAgent ? "Its turn ends where it is." : "Its terminal gets Escape, which stops an agent's turn."
        case .hosted: "The session ends, in Arugula cloud."
        }
    }

    /// Its last lines: the pane's screen, or the session's history.
    private func look() async {
        do {
            switch peeked {
            case .pane(let m, let p):
                may = m.role(of: p) != "viewer"
                silenced = m.panes.first { $0.id == p.id }?.mentions_off ?? p.mentions_off ?? false
                let text = try await m.request("GET", "/api/panes/\(p.id)/capture") as? String ?? ""
                lines = Self.last(text, 24)
            case .hosted(let s):
                let v = try await control.get("/api/sessions/\(s.session)", as: SessionView.self)
                may = v.may
                lines = v.events.suffix(16).map(\.text).joined(separator: "\n")
            }
            problem = nil
        } catch is CancellationError {
        } catch {
            problem = describe(error)
        }
    }

    /// The last `n` lines with something on them.
    static func last(_ text: String, _ n: Int) -> String {
        var all = text.components(separatedBy: "\n").map { $0.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression) }
        while all.last?.isEmpty == true { all.removeLast() }
        return all.suffix(n).joined(separator: "\n")
    }

    /// The reply, typed to the agent in the pane, or posted in the session's thread.
    private func send() async {
        let text = reply.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            switch peeked {
            case .pane(let m, let p):
                _ = try await m.request("POST", "/api/panes/\(p.id)/send", ["text": text, "enter": true])
            case .hosted(let s):
                _ = try await control.call("POST", "/api/chat/rooms/\(s.room)/messages", body: ["text": text, "parent": s.parent])
            }
            reply = ""
            toast("Sent to \(title)")
            await look()
        } catch {
            problem = describe(error)
        }
    }

    /// Teammates' agents may mention it, or not.
    private func silence() async {
        guard case .pane(let m, let p) = peeked else { return }
        do {
            _ = try await m.request("POST", "/api/panes/\(p.id)/mentions", ["on": silenced])
            silenced.toggle()
            toast(silenced ? "Silenced: teammates' agents can't mention it" : "Teammates' agents can mention it again")
        } catch {
            problem = describe(error)
        }
    }

    private func stop() async {
        do {
            switch peeked {
            case .pane(let m, let p):
                if p.type == "agent" {
                    _ = try await m.request("POST", "/api/blocks/\(p.id)/call/cancel", [:])
                } else {
                    _ = try await m.request("POST", "/api/panes/\(p.id)/keys", ["keys": ["Escape"]])
                }
            case .hosted(let s):
                _ = try await control.call("POST", "/api/sessions/\(s.session)/stop", body: [:])
            }
            toast("Stopped \(title)")
            dismiss()
        } catch {
            problem = describe(error)
        }
    }
}
