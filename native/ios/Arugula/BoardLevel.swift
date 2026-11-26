// The Board level (Arugula #1306, the web's #1288): a board zoomed to from
// the Overview. Its goal and progress; a card for its lead, with Talk to the lead
// (that agent) and Board thread as two buttons, and its grant with Revoke or
// Run this board (Arugula #1456, the web's #1416); and its open tickets as the
// board groups them, each with its PR or checks, a word for how it stands
// and the agents on it, whose names open them. A ticket zooms to its Work
// level. Done tickets fold into one line.

import SwiftUI

struct BoardLevel: View {
    let control: Control
    let project: Projects.Project
    let board: Board
    @ObservedObject var model: HomeModel
    @ObservedObject var fleet: HomeFleet
    /// Run this board: on a board with no lead and no grant, in a team's
    /// project. Its card goes in the reader's Needs you.
    var run: (() -> Void)?
    /// Revoke the board's grant, when the reader gave it.
    var revoke: (() -> Void)?
    @Environment(\.zoom) private var zoom
    @Environment(\.talkTo) private var talkTo
    @State private var allDone = false

    var body: some View {
        let line = model.boards.first { $0.id == board.id } ?? board
        let state = HomeState(project: project, fleet: fleet, model: model)
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                head(line)
                LeadCard(
                    lead: line.lead.map { LeadLine($0, fleet: fleet, in: project.shown) },
                    leads: line.lead.map { l in model.boards.filter { $0.lead?.machine == l.machine && $0.lead?.session == l.session }.count } ?? 0,
                    talk: line.lead.map { l in { talk(LeadLine(l, fleet: fleet, in: project.shown).key) } },
                    thread: BoardThreadRef(board: line),
                    grant: model.views[board.id]?.grant,
                    run: run,
                    revoke: revoke)
                if let view = model.views[board.id] {
                    tickets(view, state: state)
                } else {
                    Loaded(load: { try await control.get("/api/boards/\(board.id)", as: BoardDetail.self) }) { tickets($0, state: state) }
                        .frame(minHeight: 120)
                }
            }
            .padding(.vertical)
            .padding(.horizontal)
            .frame(maxWidth: 1100, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Palette.void)
        .safeAreaInset(edge: .top, spacing: 0) {
            ZoomPath(project: project.id, board: line, here: .board(board.id))
        }
        .detailTitle(line.title)
        .accessibilityIdentifier("board-level")
    }

    private func talk(_ key: String) {
        openAgent(key, talkTo: talkTo, project: project.id)
    }

    /// Its goal, its lane bar with the counts, and All lanes.
    private func head(_ b: Board) -> some View {
        let p = b.progress
        let total = p?.total ?? 0, done = p?.done ?? 0, review = p?.review ?? 0, doing = p?.doing ?? 0
        return VStack(alignment: .leading, spacing: 10) {
            if let goal = b.goal, !goal.isEmpty {
                Text(goal).font(Face.ui(14)).foregroundStyle(Palette.subtle).lineLimit(5)
                    .frame(maxWidth: 760, alignment: .leading)
            }
            HStack(alignment: .firstTextBaseline, spacing: 18) {
                Count(n: done, of: total, label: "done", tone: .ok)
                if review > 0 { Count(n: review, label: "review", tone: .accent) }
                if doing > 0 { Count(n: doing, label: "doing", tone: .warn) }
                Spacer(minLength: 8)
                NavigationLink(value: BoardLanes(board: b)) {
                    Label("All lanes", systemImage: "rectangle.split.3x1")
                        .font(Face.mono(11, "Bold")).foregroundStyle(Palette.accent).labelStyle(.titleAndIcon)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("open-board")
            }
            LaneBar(done: done, review: review, doing: doing, total: max(total, 1))
                .frame(height: 10)
                .frame(maxWidth: 760)
        }
    }

    /// The open tickets, as the board groups them (its groups in its order,
    /// then those in none), Doing, then Review, then Next in each; then Done,
    /// folded.
    @ViewBuilder
    private func tickets(_ v: BoardDetail, state: HomeState) -> some View {
        let rows = v.zones.flatMap(\.rows)
        let open = rows.filter { !($0.lane ?? "").hasSuffix("done") }.sorted { Self.rank($0) < Self.rank($1) }
        let done = rows.filter { ($0.lane ?? "").hasSuffix("done") }.sorted { ($0.updated_at ?? 0) > ($1.updated_at ?? 0) }
        let groups: [(id: String?, title: String)] = (v.groups ?? []).map { (id: Optional($0.id), title: $0.title) } + [(id: nil, title: "Not in a group")]
        let shown = groups.compactMap { g -> (title: String, rows: [BoardDetail.Row])? in
            let mine = open.filter { r in g.id == nil ? !(v.groups ?? []).contains { $0.id == r.group } : r.group == g.id }
            return mine.isEmpty ? nil : (title: g.id == nil && v.groups?.isEmpty != false ? "Open tickets" : g.title, rows: mine)
        }
        VStack(alignment: .leading, spacing: 14) {
            if open.isEmpty {
                Text("No open tickets").font(Face.ui(14)).foregroundStyle(Palette.dim)
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), spacing: 14, alignment: .top)], alignment: .leading, spacing: 14) {
                ForEach(shown, id: \.title) { g in
                    VStack(alignment: .leading, spacing: 8) {
                        Caps("\(g.title) · \(g.rows.count)")
                        ForEach(g.rows) { row in
                            TicketCard(
                                row: row, agents: state.cards["\(board.id)/\(row.id)"] ?? [],
                                open: { zoom?(.card(board: board.id, row: row.id)) },
                                talk: talk)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
            }
            if !done.isEmpty {
                Button {
                    withAnimation { allDone.toggle() }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: allDone ? "chevron.down" : "chevron.right").font(.system(size: 11, weight: .bold))
                        Text(done.count == 1 ? "1 done ticket" : "\(done.count) done tickets")
                    }
                    .font(Face.mono(12, "Bold")).foregroundStyle(Palette.ok)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("board-done")
                if allDone {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(done) { row in
                            DoneRow(row: row)
                            Rectangle().fill(Palette.raised.opacity(0.6)).frame(height: 1)
                        }
                    }
                }
            }
        }
    }

    /// Doing, then Review, then Next.
    private static func rank(_ r: BoardDetail.Row) -> Int {
        switch r.lane ?? "" {
        case "lane_doing", "doing": 0
        case "lane_review", "review": 1
        default: 2
        }
    }
}

/// Who leads a board, as a card: its name, how its session stands, its
/// machine and how many boards it leads, with Talk to the lead (that agent)
/// and Board thread (where its lead and anyone mentioned read a post) as two
/// buttons. With no lead, it says so. Under it, the board's grant to start
/// agents, with Revoke for the person who gave it; a board with neither
/// offers Run this board (Arugula #1456).
struct LeadCard: View {
    let lead: LeadLine?
    let leads: Int
    let talk: (() -> Void)?
    let thread: BoardThreadRef
    var grant: BoardDetail.Grant?
    var run: (() -> Void)?
    var revoke: (() -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Caps("Lead")
            if let lead {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Circle().fill(lead.standing?.tone.color ?? Palette.dim).frame(width: 8, height: 8)
                    Text(lead.name).font(Face.ui(17, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                        .accessibilityIdentifier("lead-card-name")
                }
                Text(facts(lead)).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(2)
            } else {
                Text("No lead").font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.subtle)
                    .accessibilityIdentifier("lead-card-name")
                Text("Nobody leads this board: a post in its thread waits for one.")
                    .font(Face.ui(12)).foregroundStyle(Palette.dim)
            }
            if let grant {
                Text(grant.line).font(Face.mono(11)).foregroundStyle(Palette.subtle).lineLimit(3)
                    .accessibilityIdentifier("board-grant-line")
            }
            FlowRow(spacing: 8) {
                if let talk {
                    Button("Talk to the lead", action: talk)
                        .buttonStyle(AnswerButton(primary: true))
                        .accessibilityIdentifier("lead-talk")
                } else if grant == nil, let run {
                    Button("Run this board", action: run)
                        .buttonStyle(AnswerButton(primary: true))
                        .accessibilityHint("Its card goes in your Needs you: Approve starts its lead and grants it agents")
                        .accessibilityIdentifier("board-run")
                }
                if grant?.mine == true, let revoke {
                    Button("Revoke", action: revoke)
                        .buttonStyle(AnswerButton())
                        .accessibilityHint("Nothing more starts under it; its lead and agents run on")
                        .accessibilityIdentifier("board-grant-revoke")
                }
                NavigationLink(value: thread) {
                    Text("Board thread")
                }
                .buttonStyle(AnswerButton())
                .accessibilityHint("Its lead and anyone mentioned read it")
                .accessibilityIdentifier("board-thread")
            }
        }
        .frame(maxWidth: 760, alignment: .leading)
        .card(lead?.standing?.tone ?? .muted)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("lead-card")
    }

    private func facts(_ l: LeadLine) -> String {
        var bits: [String] = []
        if !l.machine.isEmpty { bits.append("on \(l.machine)") }
        if let s = l.standing { bits.append(s == .idle ? "quiet" : s.word(asks: l.who)) } else { bits.append("not reached from here") }
        if leads > 1 { bits.append("leads \(leads) boards") }
        return bits.joined(separator: " · ")
    }
}

/// A ticket on the Board level: its number and title, its PR or checks, a
/// word for how it stands, and the agents on it, whose names open them. It
/// zooms to its Work level.
struct TicketCard: View {
    let row: BoardDetail.Row
    let agents: [BoardSession]
    let open: () -> Void
    let talk: (String) -> Void

    var body: some View {
        let (word, tone) = standing
        VStack(alignment: .leading, spacing: 7) {
            Button(action: open) {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(row.words).font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(3)
                            .multilineTextAlignment(.leading)
                        Spacer(minLength: 4)
                        Text(word).font(Face.mono(11, "Bold")).foregroundStyle(tone.color)
                            .accessibilityIdentifier("ticket-word")
                    }
                    if let line = prLine ?? (agents.isEmpty ? "no agent yet" : nil) {
                        Text(line).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("ticket-\(row.live?.number.map(String.init) ?? row.id)")
            if !agents.isEmpty {
                FlowRow(spacing: 6) {
                    ForEach(agents) { a in
                        Button { talk(a.peeked.key) } label: {
                            HStack(spacing: 5) {
                                Circle().fill(a.standing.tone.color).frame(width: 6, height: 6)
                                Text(a.name).font(Face.mono(11, "Bold")).foregroundStyle(Palette.fg).lineLimit(1)
                            }
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(Palette.surface, in: Capsule())
                            .overlay(Capsule().strokeBorder(a.standing.tone.color.opacity(0.5), lineWidth: 1))
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("ticket-agent-\(a.id)")
                    }
                }
            }
        }
        .card(tone, glow: tone == .accent)
    }

    /// Its word: asks you (its ask, or an agent of yours on it waiting); asks
    /// someone else by name (Arugula #1392); working, in review or next.
    private var standing: (String, Tone) {
        if row.ask?.mine == true || agents.contains(where: { $0.standing == .needs }) { return ("asks you", .accent) }
        if let a = row.ask, !(a.who ?? "").isEmpty { return (Standing.waits.word(asks: a.who_name), .info) }
        if let a = agents.first(where: { $0.standing == .waits }) { return (a.word, .info) }
        switch row.lane ?? "" {
        case "lane_doing", "doing": return ("working", .warn)
        case "lane_review", "review": return ("in review", .info)
        default: return ("next", .muted)
        }
    }

    /// Its PR, its own or the first attached as work, in the board's words.
    private var prLine: String? {
        let work = (row.attachments ?? []).filter { $0.role == "work" && $0.released != true }.compactMap(\.live)
        guard let pr = ([row.live].compactMap { $0 } + work).first(where: { $0.kind == "pr" }) else { return nil }
        let n = pr.number.map { "PR #\($0)" } ?? "A PR"
        let state: String = switch pr.state ?? "" {
        case "checks_failed": "checks failing"
        case "checks_running": "checks running"
        case "checks_passed": "checks pass"
        case "merged": "merged"
        case "closed": "closed"
        case "open": "open"
        default: pr.state ?? ""
        }
        return state.isEmpty ? n : "\(n) · \(state)"
    }
}
