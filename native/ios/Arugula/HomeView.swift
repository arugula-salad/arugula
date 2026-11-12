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

/// A detail's title: large, at the leading edge, on an iPad, where iPadOS puts
/// the tabs in a bar across the top and an inline title would sit under it.
struct DetailTitle: ViewModifier {
    let title: String
    @Environment(\.horizontalSizeClass) private var width

    func body(content: Content) -> some View {
        content
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(width == .regular ? .large : .inline)
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
                ProgressView()
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
                        VStack(alignment: .leading) {
                            Text(project.name)
                            if let team = project.team {
                                Text("\(team.name) · \(team.role)").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                    .accessibilityIdentifier("project-\(project.name)")
                }
            }
            .navigationTitle("Projects")
        } detail: {
            NavigationStack {
                if let picked {
                    ProjectView(control: control, project: picked)
                } else {
                    ContentUnavailableView("Pick a project", systemImage: "folder")
                }
            }
            .id(picked?.id)
        }
        .navigationSplitViewStyle(.balanced)
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
            if section == 0 {
                Loaded(load: { try await control.get("/api/projects/\(project.id)/issues", as: IssueList.self) }) { l in
                    List(l.issues) { issue in
                        NavigationLink(value: issue) {
                            VStack(alignment: .leading) {
                                Text(issue.title).lineLimit(2)
                                Text("#\(issue.number) · \(issue.state)").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                    .overlay { if l.issues.isEmpty { Text("No open issues").foregroundStyle(.secondary) } }
                }
            } else {
                Loaded(load: { try await control.get("/api/projects/\(project.id)/boards", as: BoardList.self) }) { l in
                    List(l.boards) { b in
                        NavigationLink(value: b) {
                            VStack(alignment: .leading) {
                                Text(b.title)
                                if let p = b.progress, let total = p.total, total > 0 {
                                    ProgressView(value: Double(p.done ?? 0), total: Double(total))
                                    Text("\(p.done ?? 0) of \(total) done").font(.caption).foregroundStyle(.secondary)
                                }
                            }
                        }
                        .accessibilityIdentifier("board-\(b.title)")
                    }
                    .overlay { if l.boards.isEmpty { Text("No boards").foregroundStyle(.secondary) } }
                }
            }
        }
        .detailTitle(project.name)
        .navigationDestination(for: Issue.self) { IssueView(issue: $0) }
        .navigationDestination(for: Board.self) { BoardScreen(control: control, board: $0) }
        .navigationDestination(for: IssueRef.self) { ref in
            Loaded(load: { try await control.get("/api/issues/\(ref.id)", as: Issue.self) }) { IssueView(issue: $0) }
        }
    }
}

/// A board: its lanes in order, each with its rows. A row with a native issue
/// opens it.
struct BoardScreen: View {
    let control: Control
    let board: Board

    var body: some View {
        Loaded(load: { try await control.get("/api/boards/\(board.id)", as: BoardDetail.self) }) { b in
            List {
                if let goal = b.board.goal, !goal.isEmpty {
                    Section { Text(goal).font(.callout).foregroundStyle(.secondary) }
                }
                ForEach(b.zones) { lane in
                    Section("\(lane.title) · \(lane.rows.count)") {
                        ForEach(lane.rows) { row in
                            if row.pointer?.kind == "issue", let id = row.pointer?.id {
                                NavigationLink(value: IssueRef(id: id)) { BoardRowLabel(row: row, group: b.groupTitle(row.group)) }
                            } else {
                                BoardRowLabel(row: row, group: b.groupTitle(row.group))
                            }
                        }
                    }
                }
            }
        }
        .detailTitle(board.title)
    }
}

struct BoardRowLabel: View {
    let row: BoardDetail.Row
    let group: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if let live = row.live, let title = live.title {
                Text(live.number.map { "#\($0) \(title)" } ?? title).lineLimit(2)
                if let note = row.note, !note.isEmpty {
                    Text(note).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
            } else {
                Text(row.note.flatMap { $0.isEmpty ? nil : $0 } ?? row.pointer?.url ?? "A note").lineLimit(3)
            }
            let tags = [group, row.live?.state].compactMap { $0 }
            if !tags.isEmpty {
                Text(tags.joined(separator: " · ")).font(.caption2).foregroundStyle(.tertiary)
            }
        }
    }
}

struct IssueView: View {
    let issue: Issue

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text(issue.title).font(.title3.weight(.semibold))
                Text("#\(issue.number) · \(issue.state)").font(.caption).foregroundStyle(.secondary)
                Text((try? AttributedString(markdown: issue.body, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(issue.body))
                    .textSelection(.enabled)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding()
        }
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
                    Section("Signed in as") {
                        Text(p.me.name)
                        Text(p.me.login).foregroundStyle(.secondary)
                    }
                    Section {
                        Button("Forget this \(device)", role: .destructive) { model.forget() }
                    } footer: {
                        Text("Deletes this \(device)'s keys. Remove it from your devices in a browser too, and it can't come back with them.")
                    }
                }
            }
            .navigationTitle("You")
        }
    }
}
