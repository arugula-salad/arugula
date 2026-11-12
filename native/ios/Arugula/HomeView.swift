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
                        VStack(alignment: .leading) {
                            Text(b.title)
                            if let p = b.progress, let total = p.total, total > 0 {
                                ProgressView(value: Double(p.done ?? 0), total: Double(total))
                                Text("\(p.done ?? 0) of \(total) done").font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                    .overlay { if l.boards.isEmpty { Text("No boards").foregroundStyle(.secondary) } }
                }
            }
        }
        .navigationTitle(project.name)
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(for: Issue.self) { IssueView(issue: $0) }
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
