// The Work level (Arugula #1307, the web's #1289): a card's sessions, or one
// session on no card, live. A terminal is drawn by the core's terminal at the
// pane's own size, so nobody's panes resize, and a tap types into it. An
// agent block shows what it says as it says it, with a box that types to it;
// a session in Arugula cloud, its last words and a reply in its thread. What
// an agent asks sits over the panes with its answers as buttons, answered as
// Needs you answers it. On an iPad up to four panes stand side by side and
// the rest wait in a strip; on an iPhone one at a time, the others in a strip
// over it. A teammate's session is read, not typed into.

import SwiftUI

/// One session's pane at the Work level.
enum WorkSlot: Identifiable {
    case terminal(MachineSession, PaneRow)
    case block(MachineSession, PaneRow)
    case hosted(SessionWave.Row)

    init(_ p: Peeked) {
        switch p {
        case .pane(let m, let row): self = row.type == "agent" ? .block(m, row) : .terminal(m, row)
        case .hosted(let s): self = .hosted(s)
        }
    }

    var peeked: Peeked {
        switch self {
        case .terminal(let m, let p), .block(let m, let p): .pane(m, p)
        case .hosted(let s): .hosted(s)
        }
    }

    var id: String { peeked.key }
}

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
    /// Drive a terminal full screen, at the device's size.
    let terminal: (MachineSession, PaneRow) -> Void
    @Environment(\.horizontalSizeClass) private var width
    @Environment(\.talkTo) private var talkTo
    /// The pane in front: the one shown on an iPhone, the first on an iPad.
    @State private var front: String?
    /// What was answered here, by need and ask: folded away.
    @State private var folded: Set<String> = []
    @State private var said: String?

    /// How many panes stand side by side on an iPad.
    private static let side = 4

    var body: some View {
        let state = HomeState(project: project, fleet: fleet, model: model)
        let card = board.flatMap { b in row.flatMap { r in model.views[b.id]?.zones.flatMap(\.rows).first { $0.id == r } } }
        let lone = session.flatMap { peeked($0, fleet: fleet, model: model) }
        let sessions: [Peeked] = if let board, let row {
            (state.cards["\(board.id)/\(row)"] ?? []).map(\.peeked)
        } else {
            lone.map { [$0] } ?? []
        }
        let slots = sessions.map(WorkSlot.init)
        let title = card?.words ?? lone?.name ?? leadName ?? (session == nil ? "A card" : "A session")
        VStack(alignment: .leading, spacing: 0) {
            asks(slots)
            if slots.isEmpty {
                none
            } else if width == .regular {
                wide(slots)
            } else {
                narrow(slots)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .background(Palette.void)
        .safeAreaInset(edge: .top, spacing: 0) {
            ZoomPath(
                project: project.id, board: board.map { b in model.boards.first { $0.id == b.id } ?? b }, work: title,
                here: board.map { b in .card(board: b.id, row: row ?? "") } ?? .work(session ?? ""))
        }
        .overlay(alignment: .bottom) {
            if let said { Toast(text: said).transition(.opacity).allowsHitTesting(false) }
        }
        .detailTitle(title)
        .accessibilityIdentifier("work-level")
    }

    // ---- what's asked

    /// What the card's agents ask, over their panes, with the answers as
    /// buttons; answered, each folds away until it asks something else.
    @ViewBuilder
    private func asks(_ slots: [WorkSlot]) -> some View {
        let needs = slots.compactMap(need).filter { !folded.contains("\($0.id)|\($0.ask)") }
        if !needs.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
                ForEach(needs) { n in
                    VStack(alignment: .leading, spacing: 8) {
                        NeedRow(need: n)
                        NeedAnswer(need: n, control: control, model: model) { words in
                            withAnimation { _ = folded.insert("\(n.id)|\(n.ask)") }
                            show("\(n.who): \(words)")
                        }
                    }
                    .card(.accent, glow: true)
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("work-ask-\(n.id)")
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 10)
        }
    }

    /// A pane's question, permission or failure, or a hosted session's wait,
    /// as Needs you reads it.
    private func need(_ slot: WorkSlot) -> Need? {
        switch slot {
        case .terminal(let m, let row), .block(let m, let row):
            let p = (m.agents + m.panes).first { $0.id == row.id } ?? row
            guard p.needsYou else { return nil }
            let text = p.reason?.headline.isEmpty == false ? p.reason!.headline : (p.ask?.message ?? "")
            return Need(
                id: "pane:\(m.machine.id):\(p.id)", ask: p.asking?.id ?? "\(p.reason?.kind ?? ""):\(p.reason?.since_ms ?? 0)",
                rank: p.needsYouRank, since: p.reason?.since_ms ?? 0, who: m.conversations[p.id] ?? p.name, standing: p.standing,
                on: m.machine.name, text: text, mono: p.asking?.approve == true || p.failed, target: .pane(m, p))
        case .hosted(let s):
            guard let waiting = s.waiting else { return nil }
            return Need(
                id: "hosted:\(s.session)", ask: model.opened[s.session]?.card.open?.request ?? waiting, rank: 1, since: s.updated_at,
                who: s.issue.map { "#\($0.number) \($0.title)" } ?? "A session", standing: .needs, on: "Arugula cloud",
                text: s.question ?? waiting, mono: false, target: .session(s))
        }
    }

    // ---- the panes

    /// An iPad: up to four side by side, the pane in front first, the rest in
    /// a strip under them.
    private func wide(_ slots: [WorkSlot]) -> some View {
        let ordered = order(slots)
        let shown = Array(ordered.prefix(Self.side))
        let more = Array(ordered.dropFirst(Self.side))
        return VStack(alignment: .leading, spacing: 10) {
            GeometryReader { g in
                let columns = shown.count == 1 ? 1 : 2
                let rows = (shown.count + columns - 1) / columns
                let gap: CGFloat = 10
                let w = (g.size.width - gap * CGFloat(columns - 1)) / CGFloat(columns)
                let h = (g.size.height - gap * CGFloat(rows - 1)) / CGFloat(rows)
                VStack(spacing: gap) {
                    ForEach(0..<rows, id: \.self) { r in
                        HStack(spacing: gap) {
                            ForEach(shown[(r * columns)..<min(shown.count, (r + 1) * columns)]) { s in
                                pane(s).frame(width: w, height: h)
                            }
                        }
                    }
                }
            }
            if !more.isEmpty {
                strip(more, label: "More here:")
            }
        }
        .padding()
    }

    /// An iPhone: the pane in front, the others in a strip over it.
    private func narrow(_ slots: [WorkSlot]) -> some View {
        let ordered = order(slots)
        return VStack(alignment: .leading, spacing: 8) {
            if ordered.count > 1 {
                strip(ordered, label: nil)
            }
            if let first = ordered.first {
                pane(first).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
    }

    /// The pane in front first (the one picked, else the session zoomed to,
    /// else the one that most wants you), then the rest as they came.
    private func order(_ slots: [WorkSlot]) -> [WorkSlot] {
        let key = front ?? session
        guard let i = slots.firstIndex(where: { $0.id == key }) else { return slots }
        var out = slots
        out.insert(out.remove(at: i), at: 0)
        return out
    }

    private func strip(_ slots: [WorkSlot], label: String?) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                if let label { Caps(label) }
                ForEach(slots) { s in
                    let on = s.id == (front ?? order(slots).first?.id) && label == nil
                    Button { withAnimation { front = s.id } } label: {
                        HStack(spacing: 5) {
                            Circle().fill(s.peeked.standing.tone.color).frame(width: 6, height: 6)
                            Text(s.peeked.name).font(Face.mono(11, "Bold")).lineLimit(1)
                        }
                        .foregroundStyle(on ? Palette.void : Palette.fg)
                        .padding(.horizontal, 9)
                        .padding(.vertical, 5)
                        .background(on ? Palette.accent : Palette.surface, in: Capsule())
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("work-pick-\(s.id)")
                }
            }
        }
    }

    private func pane(_ s: WorkSlot) -> some View {
        WorkPane(slot: s, control: control, outlined: s.id == session, terminal: terminal)
            .id(s.id)
    }

    // ---- no agent

    private var none: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(session == nil
                ? (model.loaded ? "No agent works on this card yet." : "Looking…")
                : away)
                .font(Face.ui(14)).foregroundStyle(Palette.dim)
                .accessibilityIdentifier("work-none")
            if session == nil, let b = board {
                let line = model.boards.first { $0.id == b.id } ?? b
                if let lead = line.lead {
                    Button("Ask the lead to start one") {
                        openAgent(LeadLine(lead, fleet: fleet).key, talkTo: talkTo, project: project.id)
                    }
                    .buttonStyle(AnswerButton(primary: true))
                    .accessibilityIdentifier("work-ask-lead")
                } else {
                    NavigationLink(value: BoardThreadRef(board: line)) { Text("Board thread") }
                        .buttonStyle(AnswerButton())
                }
            }
        }
        .padding()
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

    private func show(_ text: String) {
        withAnimation { said = text }
        Task {
            try? await Task.sleep(for: .seconds(1.8))
            if said == text { withAnimation { said = nil } }
        }
    }
}

/// One pane at the Work level: its name, machine, branch and how it stands
/// over it, and the pane itself, live.
struct WorkPane: View {
    let slot: WorkSlot
    let control: Control
    /// The one zoomed to: outlined.
    let outlined: Bool
    let terminal: (MachineSession, PaneRow) -> Void

    var body: some View {
        let standing = slot.peeked.standing
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Circle().fill(standing.tone.color).frame(width: 7, height: 7)
                VStack(alignment: .leading, spacing: 1) {
                    Text(slot.peeked.name).font(Face.ui(14, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                    Text(facts(standing)).font(Face.mono(10)).foregroundStyle(Palette.dim).lineLimit(1).truncationMode(.middle)
                }
                Spacer(minLength: 6)
                if case .terminal(let m, let p) = slot {
                    Button { terminal(m, p) } label: {
                        Image(systemName: "arrow.up.left.and.arrow.down.right")
                    }
                    .buttonStyle(AnswerButton())
                    .accessibilityLabel("Full screen")
                    .accessibilityIdentifier("work-full-\(p.id)")
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(Palette.surface)
            switch slot {
            case .terminal(let m, let p):
                WorkTerminal(session: m, pane: p)
            case .block(let m, let p):
                BlockSlot(session: m, pane: p)
            case .hosted(let s):
                HostedSlot(control: control, session: s)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .strokeBorder(outlined ? Palette.accent : standing.tone.color.opacity(0.45), lineWidth: outlined ? 2 : 1))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("work-pane-\(slot.id)")
    }

    private func facts(_ standing: Standing) -> String {
        var bits = [slot.peeked.place]
        switch slot {
        case .terminal(_, let p), .block(_, let p):
            if let b = p.branch { bits.append("⎇ \(b)") }
        case .hosted:
            break
        }
        bits.append(standing == .idle ? "quiet" : standing.word)
        return bits.joined(separator: " · ")
    }
}

/// A terminal, live at its own size, scaled to fit, typed into with a tap. A
/// viewer's isn't typed into.
struct WorkTerminal: View {
    @ObservedObject var session: MachineSession
    let pane: PaneRow

    var body: some View {
        GeometryReader { g in
            let (cols, rows) = session.grid(of: pane.id) ?? (80, 24)
            let ten = TerminalUIView.cell(size: 10)
            let fit = min(g.size.width / (CGFloat(cols) * ten.width), g.size.height / (CGFloat(rows) * ten.height))
            let size = (max(5, min(14, 10 * fit)) * 2).rounded(.down) / 2
            let cell = TerminalUIView.cell(size: size)
            let w = CGFloat(cols) * cell.width, h = CGFloat(rows) * cell.height
            ScrollView([.horizontal, .vertical], showsIndicators: false) {
                TypeTerminal(session: session, pane: pane, cols: cols, rows: rows, size: size, typeable: session.role(of: pane) != "viewer")
                    .id("\(pane.id)-\(cols)x\(rows)@\(size)")
                    .frame(width: w, height: h)
            }
            .scrollDisabled(w <= g.size.width && h <= g.size.height)
            .frame(width: g.size.width, height: g.size.height, alignment: .topLeading)
        }
        .background(Color.black)
    }
}

/// The core's terminal for a pane drawn on its own, from any tab.
struct TypeTerminal: UIViewRepresentable {
    let session: MachineSession
    let pane: PaneRow
    let cols: UInt16
    let rows: UInt16
    let size: CGFloat
    let typeable: Bool

    func makeUIView(context: Context) -> TerminalUIView {
        let v = TerminalUIView(session: session, tab: pane.tab, pane: pane.id, fontSize: size, fixed: (cols, rows), typeable: typeable)
        v.accessibilityIdentifier = "work-terminal-\(pane.id)"
        session.register(v)
        session.drawAlone(pane.id)
        return v
    }

    func updateUIView(_ v: TerminalUIView, context: Context) {}

    static func dismantleUIView(_ v: TerminalUIView, coordinator: ()) {
        MainActor.assumeIsolated {
            v.session.unregister(v)
            v.session.stopAlone(v.pane)
        }
    }
}

/// An agent block: what it says, as it says it, and a box that types to it.
struct BlockSlot: View {
    @ObservedObject var session: MachineSession
    let pane: PaneRow
    @State private var lines = ""
    @State private var words = ""
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    Text(lines.isEmpty ? "…" : lines)
                        .font(Face.mono(11)).foregroundStyle(Palette.subtle)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .textSelection(.enabled)
                        .padding(8)
                        .id("end")
                        .accessibilityIdentifier("work-block-\(pane.id)")
                }
                .onChange(of: lines) { _, _ in proxy.scrollTo("end", anchor: .bottom) }
            }
            if let problem {
                Text(problem).font(Face.mono(11)).foregroundStyle(Palette.danger).padding(.horizontal, 8)
            }
            if session.role(of: pane) != "viewer" {
                HStack(spacing: 6) {
                    TextField("Type to the agent…", text: $words, axis: .vertical)
                        .lineLimit(1...4)
                        .font(Face.ui(14))
                        .padding(.horizontal, 8).padding(.vertical, 6)
                        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 8))
                        .accessibilityIdentifier("work-say-\(pane.id)")
                    Button("Send") { Task { await send() } }
                        .buttonStyle(AnswerButton(primary: true))
                        .disabled(words.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        .accessibilityIdentifier("work-send-\(pane.id)")
                }
                .padding(8)
            }
        }
        .background(Palette.void)
        .task {
            while !Task.isCancelled {
                await look()
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    private func look() async {
        do {
            let text = try await session.request("GET", "/api/panes/\(pane.id)/capture") as? String ?? ""
            lines = SessionPeek.last(text, 200)
            problem = nil
        } catch is CancellationError {
        } catch {
            problem = describe(error)
        }
    }

    private func send() async {
        let text = words.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            _ = try await session.request("POST", "/api/panes/\(pane.id)/send", ["text": text, "enter": true])
            words = ""
            await look()
        } catch {
            problem = describe(error)
        }
    }
}

/// A session in Arugula cloud: its last words, and a reply in its thread.
struct HostedSlot: View {
    let control: Control
    let session: SessionWave.Row
    @State private var lines = ""
    @State private var may = false
    @State private var words = ""
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ScrollView {
                Text(lines.isEmpty ? "…" : lines)
                    .font(Face.mono(11)).foregroundStyle(Palette.subtle)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
            }
            if let problem {
                Text(problem).font(Face.mono(11)).foregroundStyle(Palette.danger).padding(.horizontal, 8)
            }
            if may {
                HStack(spacing: 6) {
                    TextField("Reply in its thread…", text: $words, axis: .vertical)
                        .lineLimit(1...4)
                        .font(Face.ui(14))
                        .padding(.horizontal, 8).padding(.vertical, 6)
                        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 8))
                    Button("Send") { Task { await send() } }
                        .buttonStyle(AnswerButton(primary: true))
                        .disabled(words.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                .padding(8)
            }
        }
        .background(Palette.void)
        .task {
            while !Task.isCancelled {
                await look()
                try? await Task.sleep(for: .seconds(3))
            }
        }
    }

    private func look() async {
        do {
            let v = try await control.get("/api/sessions/\(session.session)", as: SessionView.self)
            may = v.may
            lines = v.events.suffix(40).map(\.text).joined(separator: "\n")
            problem = nil
        } catch is CancellationError {
        } catch {
            problem = describe(error)
        }
    }

    private func send() async {
        let text = words.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            _ = try await control.call("POST", "/api/chat/rooms/\(session.room)/messages", body: ["text": text, "parent": session.parent])
            words = ""
            await look()
        } catch {
            problem = describe(error)
        }
    }
}
