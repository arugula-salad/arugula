// One navigation: projects, chat and machines are tabs of one app, whichever
// process behind control answers them. Each tab is a list beside what's picked
// in it on an iPad, and one screen at a time on an iPhone.

import SwiftUI
import UIKit

/// "iPhone" or "iPad", for what the app says about the device it's on.
@MainActor var device: String { UIDevice.current.model }

struct HomeView: View {
    let control: Control

    var body: some View {
        TabView {
            // Tabs and titles in the theme (Theme.swift); each tab's lists sit
            // on the void.
            ProjectsView(control: control)
                .tabItem { Label("Projects", systemImage: "folder") }
            ChatRoomsView(control: control)
                .tabItem { Label("Chat", systemImage: "bubble.left.and.bubble.right") }
            MachinesView(control: control)
                .tabItem { Label("Machines", systemImage: "terminal") }
            YouView(control: control)
                .tabItem { Label("You", systemImage: "person.crop.circle") }
        }
    }
}

/// A detail's title. On an iPhone it's the bar's inline title. On an iPad,
/// where iPadOS floats the tabs across the top of the bar (and a large title
/// shrinks back under them as the page scrolls), the bar keeps only its back
/// button and the title is a heading under it, in the display face.
struct DetailTitle: ViewModifier {
    let title: String
    @Environment(\.horizontalSizeClass) private var width

    func body(content: Content) -> some View {
        if width == .regular {
            content
                .navigationTitle("")
                .navigationBarTitleDisplayMode(.inline)
                .safeAreaInset(edge: .top, spacing: 0) {
                    Text(title.uppercased())
                        .font(Face.display(34))
                        .foregroundStyle(Palette.fg)
                        .lineLimit(1)
                        .minimumScaleFactor(0.6)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal)
                        .padding(.bottom, 8)
                        .background(Palette.void)
                }
        } else {
            content
                .navigationTitle(title)
                .navigationBarTitleDisplayMode(.inline)
        }
    }
}

extension View {
    func detailTitle(_ title: String) -> some View { modifier(DetailTitle(title: title)) }
}

// ---- a list that loads

struct Loaded<T, Content: View>: View {
    var load: () async throws -> T
    /// Look again this often, for a list that sits beside what it opens.
    var every: Duration?
    @ViewBuilder var content: (T) -> Content
    @State private var value: T?
    @State private var problem: String?

    var body: some View {
        Group {
            if let value {
                content(value)
            } else if let problem {
                Problem(message: problem) { Task { await refresh() } }
            } else {
                ProgressView().tint(Palette.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task {
            await refresh()
            guard let every else { return }
            while !Task.isCancelled {
                try? await Task.sleep(for: every)
                if Task.isCancelled { break }
                await refresh()
            }
        }
        .refreshable { await refresh() }
    }

    private func refresh() async {
        do {
            value = try await load()
            problem = nil
        } catch is CancellationError {
        } catch {
            problem = describe(error)
        }
    }
}

// ---- projects

struct ProjectsView: View {
    let control: Control
    @State private var picked: Projects.Project?
    @State private var columns = NavigationSplitViewVisibility.all

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            Loaded(load: { try await control.get("/api/projects", as: Projects.self) }) { p in
                List(p.projects, selection: $picked) { project in
                    NavigationLink(value: project) {
                        VStack(alignment: .leading, spacing: 3) {
                            Text(project.name).font(Face.ui(16, "SemiBold"))
                            Text(project.team.map { "\($0.name) · \($0.role)" } ?? "personal")
                                .font(Face.mono(11)).foregroundStyle(Palette.dim)
                        }
                        .padding(.vertical, 4)
                    }
                    .voidRow()
                    .accessibilityIdentifier("project-\(project.name)")
                }
                .voidList()
            }
            .navigationTitle("PROJECTS")
            .background(Palette.void)
        } detail: {
            NavigationStack {
                if let picked {
                    ProjectView(control: control, project: picked)
                } else {
                    Empty(text: "Pick a project", symbol: "folder")
                }
            }
            .id(picked?.id)
        }
        .navigationSplitViewStyle(.balanced)
    }
}

/// What a detail shows before anything is picked.
struct Empty: View {
    let text: String
    let symbol: String

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: symbol).font(.system(size: 34, weight: .light)).foregroundStyle(Palette.muted)
            Caps(text)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Palette.void)
    }
}

struct ProjectView: View {
    let control: Control
    let project: Projects.Project
    @State private var section = 0

    var body: some View {
        VStack(spacing: 0) {
            Picker("Show", selection: $section) {
                Text("Issues").tag(0)
                Text("Boards").tag(1)
            }
            .pickerStyle(.segmented)
            .padding(.horizontal)
            .padding(.vertical, 8)
            if section == 0 {
                Loaded(load: { try await control.get("/api/projects/\(project.id)/issues", as: IssueList.self) }) { l in
                    List(l.issues) { issue in
                        NavigationLink(value: issue) { IssueRow(issue: issue) }.voidRow()
                    }
                    .voidList()
                    .overlay { if l.issues.isEmpty { Caps("No open issues") } }
                }
            } else {
                Loaded(load: { try await control.get("/api/projects/\(project.id)/boards", as: BoardList.self) }) { l in
                    ScrollView {
                        LazyVStack(spacing: 12) {
                            ForEach(l.boards) { b in
                                NavigationLink(value: b) { BoardCard(board: b) }
                                    .buttonStyle(.plain)
                                    .accessibilityIdentifier("board-\(b.title)")
                            }
                        }
                        .padding()
                    }
                    .overlay { if l.boards.isEmpty { Caps("No boards") } }
                }
            }
        }
        .background(Palette.void)
        .detailTitle(project.name)
        .navigationDestination(for: Issue.self) { IssueView(issue: $0) }
        .navigationDestination(for: Board.self) { BoardScreen(control: control, board: $0) }
        .navigationDestination(for: IssueRef.self) { ref in
            Loaded(load: { try await control.get("/api/issues/\(ref.id)", as: Issue.self) }) { IssueView(issue: $0) }
        }
    }
}

struct IssueRow: View {
    let issue: Issue

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(verbatim: "#\(issue.number)").font(Face.mono(12, "Medium")).foregroundStyle(Palette.dim)
                .frame(minWidth: 44, alignment: .leading)
            Text(issue.title).font(Face.ui(15, "Medium")).lineLimit(2)
            Spacer(minLength: 0)
            StateChip(state: issue.state)
        }
        .padding(.vertical, 4)
    }
}

/// An issue's or PR's state, as a small chip in its tone.
struct StateChip: View {
    let state: String

    var tone: Tone {
        switch state {
        case "open": .ok
        case "merged": .accent
        case "closed": .muted
        default: .info
        }
    }

    var body: some View {
        Text(state.uppercased())
            .font(Face.mono(9, "Bold"))
            .tracking(0.6)
            .foregroundStyle(tone.color)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .overlay(Capsule().strokeBorder(tone.color.opacity(0.5), lineWidth: 1))
    }
}

// ---- boards

/// The four lanes: their ids, words and tones, as the web's board has them
/// (Next plain, Doing warn, Review accent, Done ok).
enum LaneKind: String {
    case next = "lane_next", doing = "lane_doing", review = "lane_review", done = "lane_done"

    var tone: Tone {
        switch self {
        case .next: .muted
        case .doing: .warn
        case .review: .accent
        case .done: .ok
        }
    }
}

/// A board on a project's list: its title, its lanes as one bar, and the counts.
struct BoardCard: View {
    let board: Board

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(board.title.uppercased()).font(Face.display(24)).foregroundStyle(Palette.fg).lineLimit(2)
            if let goal = board.goal, !goal.isEmpty {
                Text(goal).font(Face.ui(13)).foregroundStyle(Palette.subtle).lineLimit(2)
            }
            if let p = board.progress, let total = p.total, total > 0 {
                LaneBar(done: p.done ?? 0, review: p.review ?? 0, doing: p.doing ?? 0, total: total)
                HStack(spacing: 14) {
                    Count(n: p.done ?? 0, of: total, label: "done", tone: .ok)
                    if let r = p.review, r > 0 { Count(n: r, label: "review", tone: .accent) }
                    if let d = p.doing, d > 0 { Count(n: d, label: "doing", tone: .warn) }
                }
            }
        }
        .card(.accent)
    }
}

/// A board's progress as one bar: done, review and doing in their lanes' tones.
struct LaneBar: View {
    let done: Int, review: Int, doing: Int, total: Int

    var body: some View {
        GeometryReader { g in
            HStack(spacing: 0) {
                ForEach(Array([(done, Palette.ok), (review, Palette.accent), (doing, Palette.warn)].enumerated()), id: \.offset) { _, part in
                    Rectangle().fill(part.1).frame(width: g.size.width * CGFloat(part.0) / CGFloat(max(total, 1)))
                }
                Spacer(minLength: 0)
            }
        }
        .frame(height: 6)
        .background(Palette.raised)
        .clipShape(Capsule())
    }
}

/// A count in the display face, with its word in mono capitals.
struct Count: View {
    let n: Int
    var of: Int?
    let label: String
    let tone: Tone

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(verbatim: of.map { "\(n)/\($0)" } ?? "\(n)").font(Face.display(20)).foregroundStyle(tone.color)
            Caps(label)
        }
    }
}

/// A board: its goal, the four lanes' counts, and the lanes with their rows as
/// cards in each lane's tone. On an iPad the lanes stand side by side and
/// scroll sideways; on an iPhone they stack. A row with a native issue opens it.
struct BoardScreen: View {
    let control: Control
    let board: Board
    @Environment(\.horizontalSizeClass) private var width

    var body: some View {
        Loaded(load: { try await control.get("/api/boards/\(board.id)", as: BoardDetail.self) }) { b in
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    header(b)
                    if width == .regular {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(alignment: .top, spacing: 14) {
                                ForEach(b.zones) { lane in column(lane, b).frame(width: 290) }
                            }
                            .padding(.horizontal)
                        }
                    } else {
                        VStack(alignment: .leading, spacing: 22) {
                            ForEach(b.zones) { lane in column(lane, b) }
                        }
                        .padding(.horizontal)
                    }
                }
                .padding(.vertical)
            }
            .background(Palette.void)
        }
        .background(Palette.void)
        .detailTitle(board.title)
    }

    private func header(_ b: BoardDetail) -> some View {
        let total = b.zones.reduce(0) { $0 + $1.rows.count }
        let n = { (k: LaneKind) in b.zones.first { $0.id == k.rawValue }?.rows.count ?? 0 }
        return VStack(alignment: .leading, spacing: 12) {
            if let goal = b.board.goal, !goal.isEmpty {
                Text(goal).font(Face.ui(15)).foregroundStyle(Palette.subtle).frame(maxWidth: 720, alignment: .leading)
            }
            HStack(alignment: .bottom, spacing: 22) {
                ForEach(b.zones) { lane in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(verbatim: "\(lane.rows.count)").font(Face.display(40, "Black"))
                            .foregroundStyle((LaneKind(rawValue: lane.id)?.tone ?? .muted).color)
                        Caps(lane.title)
                    }
                }
            }
            LaneBar(done: n(.done), review: n(.review), doing: n(.doing), total: total)
                .frame(maxWidth: 720)
        }
        .padding(.horizontal)
    }

    private func column(_ lane: BoardDetail.Lane, _ b: BoardDetail) -> some View {
        let kind = LaneKind(rawValue: lane.id)
        let tone = kind?.tone ?? .muted
        return VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(lane.title.uppercased()).font(Face.display(22)).foregroundStyle(tone.color)
                Text(verbatim: "\(lane.rows.count)").font(Face.mono(12, "Bold")).foregroundStyle(Palette.dim)
            }
            Rectangle().fill(tone.color.opacity(0.6)).frame(height: 1)
            if lane.rows.isEmpty {
                Caps("Nothing here").padding(.vertical, 6)
            }
            ForEach(lane.rows) { row in
                let card = BoardRowCard(row: row, group: b.groupTitle(row.group), tone: tone, live: kind == .doing)
                if row.pointer?.kind == "issue", let id = row.pointer?.id {
                    NavigationLink(value: IssueRef(id: id)) { card }.buttonStyle(.plain)
                } else {
                    card
                }
            }
        }
    }
}

/// A board's row as a card in its lane's tone: the item's number and kind,
/// its title, the row's note and its group. Rows being worked on glow.
struct BoardRowCard: View {
    let row: BoardDetail.Row
    let group: String?
    let tone: Tone
    let live: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 6) {
                if let live = row.live, let n = live.number {
                    Text(verbatim: "#\(n)").font(Face.mono(12, "Bold")).foregroundStyle(tone.color)
                    if let kind = live.kind { Caps(kind == "native" ? "issue" : kind) }
                } else {
                    Caps("note")
                }
                Spacer(minLength: 0)
                if let state = row.live?.state { StateChip(state: state) }
            }
            if let title = row.live?.title {
                Text(title).font(Face.ui(15, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(3)
            }
            if let note = row.note, !note.isEmpty, !Self.repeats(note, row.live?.title) {
                Text(note).font(Face.ui(13)).foregroundStyle(Palette.subtle).lineLimit(row.live == nil ? 5 : 2)
            } else if row.live == nil, let url = row.pointer?.url {
                Text(url).font(Face.mono(11)).foregroundStyle(Palette.subtle).lineLimit(2)
            }
            if let group { Caps(group) }
        }
        .card(tone, glow: live)
    }

    /// A note that only says the title again (rows brought in from GitHub
    /// were noted with their title).
    static func repeats(_ note: String, _ title: String?) -> Bool {
        guard let title, !title.isEmpty else { return false }
        return note.lowercased().hasPrefix(title.lowercased())
    }
}

struct IssueView: View {
    let issue: Issue

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 8) {
                    Text(verbatim: "#\(issue.number)").font(Face.mono(13, "Bold")).foregroundStyle(Palette.accent)
                    StateChip(state: issue.state)
                    ForEach(issue.labels, id: \.self) { Caps($0) }
                }
                Text(issue.title).font(Face.display(32)).foregroundStyle(Palette.fg)
                ChatText(text: issue.body)
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding()
        }
        .background(Palette.void)
        .navigationBarTitleDisplayMode(.inline)
    }
}

// ---- you

struct YouView: View {
    let control: Control
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            Loaded(load: { try await control.get("/api/projects", as: Projects.self) }) { p in
                List {
                    Section {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(p.me.name.uppercased()).font(Face.display(30))
                            Text(p.me.login).font(Face.mono(12)).foregroundStyle(Palette.dim)
                        }
                        .padding(.vertical, 6)
                        .voidRow()
                    } header: {
                        Caps("Signed in as")
                    }
                    Section {
                        Button("Forget this \(device)", role: .destructive) { model.forget() }.voidRow()
                    } footer: {
                        Text("Deletes this \(device)'s keys. Remove it from your devices in a browser too, and it can't come back with them.")
                            .font(Face.ui(12)).foregroundStyle(Palette.dim)
                    }
                }
                .voidList()
            }
            .navigationTitle("YOU")
            .background(Palette.void)
        }
    }
}
