// One navigation: the web's five places, always on the bar (Arugula #1262,
// #1361): Overview (Home.swift) first, then the project's boards and Docs,
// the machines and chat, whichever process behind control answers them. On
// an iPad the bar folds out into a sidebar, with the bell and your picture at
// its foot (Arugula #1366). Every place is headed by the project's menu and
// New, and on an iPhone the bell and your picture too (TabHeading); the
// project's issues open from its menu and from a board's rows. Each place is a list
// beside what's picked in it on an iPad, and one screen at a time on an
// iPhone.

import SwiftUI
import UIKit

/// "iPhone" or "iPad", for what the app says about the device it's on.
@MainActor var device: String { UIDevice.current.model }

struct HomeView: View {
    let control: Control
    @StateObject private var scope = Scope()
    @ObservedObject private var push = PushCenter.shared
    @ObservedObject private var zoom = ZoomCenter.shared
    @ObservedObject private var approvals = Approvals.shared
    /// What the Overview reads, kept while another place is open, so coming
    /// back is at once.
    @StateObject private var model = HomeModel()
    @StateObject private var fleet = HomeFleet()
    @StateObject private var live: LiveChat
    @Environment(\.scenePhase) private var phase
    /// Whether an iPad's sidebar is out, which holds the bell and your picture.
    @State private var sidebar = false
    @State private var tab = AppTab.overview
    /// The project's issues, from its menu.
    @State private var issues = false
    /// New, from any place's heading, and what it made, for the Overview.
    @State private var making = false
    @State private var made: Opened?
    @State private var inbox = false
    /// The Inbox item a push opened, which the Inbox scrolls to.
    @State private var focus: String?
    @State private var said: String?

    init(control: Control) {
        self.control = control
        _live = StateObject(wrappedValue: LiveChat(control: control))
    }

    var body: some View {
        places
        // A tapped notification (#1260): its place, whose view opens the
        // thing; an issue opens beside the Overview. A pane's opens its Work
        // level on the Overview (Arugula #1304), else its machine.
        .onChange(of: push.route, initial: true) { _, route in
            switch route {
            case .pane(let machine, let pane): open(machine: machine, pane: pane)
            case .room: tab = .chat
            case .issue: tab = .overview
            // A deploy waiting for approval opens over whatever's open (#604).
            case .deploy(let id):
                push.route = nil
                approvals.open = .deploy(id)
            // Anything else in the Inbox (a PR, a run): the Inbox, at it.
            case .inbox(let item):
                push.route = nil
                focus = item.isEmpty ? nil : item
                inbox = true
            case nil: break
            }
        }
        // An address (Arugula #1304): the Overview zooms to it.
        .onChange(of: zoom.route, initial: true) { _, route in
            if route != nil { tab = .overview }
        }
        // What a passkey approves (#604): a deploy, a project's runs.
        .sheet(item: $approvals.open) { what in
            switch what {
            case .deploy(let id):
                DeploySheet(control: control, id: id) { approvals.open = nil }
            case .runs(let project, let name):
                RunsSheet(control: control, project: project, name: name) { approvals.open = nil }
            }
        }
        .sheet(isPresented: $issues) {
            IssuesView(control: control) { issues = false }
                .environmentObject(scope)
                .environment(\.showIssues, nil)
        }
        // New over whichever place is open; what it made opens on the
        // Overview, which lists it.
        .sheet(isPresented: $making) {
            NewSheet(control: control, model: model, fleet: fleet, open: { o in
                tab = .overview
                made = o
            }, toast: show) {
                Task { if let p = scope.current { await model.load(control, project: p.id) } }
            }
            .environmentObject(scope)
        }
        .sheet(isPresented: $inbox, onDismiss: { focus = nil }) {
            InboxSheet(control: control, live: live, fleet: fleet, model: model, focus: focus) { inbox = false }.environmentObject(scope)
        }
        .overlay(alignment: .bottom) {
            if let said { Toast(text: said).transition(.opacity).allowsHitTesting(false) }
        }
        .background(Palette.void)
        .environmentObject(scope)
        .environment(\.goOverview, { tab = .overview })
        .environment(\.showIssues, { issues = true })
        .environment(\.makeNew, { making = true })
        .environment(\.showInbox, { inbox = true })
        .environment(\.sidebarFoot, sidebar)
        .task { await scope.load(control) }
        // The bell's count (Arugula #1434): the current project's, again
        // when chat's live stream says it moved or opens anew, and when the
        // app comes back.
        .task(id: scope.current?.id) {
            live.start()
            guard let p = scope.current?.id else { return }
            await InboxModel.shared.load(control, project: p)
        }
        .onChange(of: live.inbox) { _, _ in reloadInbox() }
        .onChange(of: live.epoch) { _, _ in reloadInbox() }
        .onChange(of: phase) { _, now in
            if now == .active { reloadInbox() }
        }
    }

    /// The five places: the tab bar on an iPhone, and on an iPad (iOS 18
    /// and later) a sidebar that folds into the bar in portrait. The
    /// sidebar's foot holds the bell and your picture (Arugula #1366); with
    /// it folded, as in portrait, they're back in the place's heading.
    @ViewBuilder private var places: some View {
        if #available(iOS 18.0, *) {
            TabView(selection: $tab) {
                ForEach(AppTab.allCases) { t in
                    Tab(t.rawValue, systemImage: t.symbol, value: t) {
                        place(t).background(SidebarProbe { sidebar = $0 })
                    }
                }
            }
            .tabViewStyle(.sidebarAdaptable)
            .tabViewSidebarBottomBar {
                if sidebar {
                    HStack(spacing: 12) {
                        InboxBell { inbox = true }
                            .accessibilityIdentifier("sidebar-inbox")
                        Text("Inbox").font(Face.ui(15, "Medium")).foregroundStyle(Palette.fg)
                        Spacer(minLength: 8)
                        AccountMenu()
                    }
                    .padding(.horizontal, 4)
                    .padding(.vertical, 6)
                }
            }
        } else {
            TabView(selection: $tab) {
                ForEach(AppTab.allCases) { t in
                    place(t).tabItem { Label(t.rawValue, systemImage: t.symbol) }.tag(t)
                }
            }
        }
    }

    @ViewBuilder private func place(_ t: AppTab) -> some View {
        switch t {
        case .overview: PhoneHome(control: control, model: model, fleet: fleet, live: live, handed: $made)
        case .boards: BoardsView(control: control, model: model, fleet: fleet, live: live)
        case .docs: DocsPlace(control: control)
        case .machines: MachinesView(control: control, fleet: fleet)
        case .chat: ChatRoomsView(control: control, live: live)
        }
    }

    /// A short word of what was done, over whichever place is open.
    private func show(_ text: String) {
        withAnimation { said = text }
        Task {
            try? await Task.sleep(for: .seconds(1.8))
            if said == text { withAnimation { said = nil } }
        }
    }

    /// A notification about a pane: its card's Work level when it's on one,
    /// else its own, once the machine is reached; else Machines opens it.
    private func reloadInbox() {
        guard let p = scope.current?.id else { return }
        Task { await InboxModel.shared.load(control, project: p) }
    }

    private func open(machine: String, pane: UInt32) {
        tab = .overview
        Task {
            for _ in 0..<20 {
                if let r = route(machine: machine, pane: pane) {
                    if push.route == .pane(machine: machine, pane: pane) { push.route = nil }
                    zoom.route = r
                    return
                }
                try? await Task.sleep(for: .milliseconds(400))
            }
            tab = .machines
        }
    }

    private func route(machine: String, pane: UInt32) -> ZoomRoute? {
        guard let m = fleet.machines.first(where: { $0.machine.id == machine }),
              let p = (m.agents + m.panes).first(where: { $0.id == pane }) else { return nil }
        let personal = scope.projects.first { $0.team == nil }?.id
        if let on = p.on_issue, let card = on.boards?.first, let project = on.project ?? personal {
            return ZoomRoute(project: project, zoom: .card(board: card.board, row: card.row))
        }
        guard let project = p.session_project?.id ?? personal else { return nil }
        return ZoomRoute(project: project, zoom: .work("pane:\(machine):\(pane)"))
    }
}

/// Whether the places' sidebar is out (iOS 18 and later): SwiftUI doesn't
/// say, so its tab bar controller is asked whenever the place is laid out
/// again, as it is when the sidebar folds or unfolds.
struct SidebarProbe: UIViewRepresentable {
    let shown: (Bool) -> Void

    func makeUIView(context: Context) -> Probe { Probe(shown: shown) }
    func updateUIView(_ view: Probe, context: Context) { view.shown = shown }

    final class Probe: UIView {
        var shown: (Bool) -> Void
        private var last: Bool?

        init(shown: @escaping (Bool) -> Void) {
            self.shown = shown
            super.init(frame: .zero)
            isUserInteractionEnabled = false
        }

        required init?(coder: NSCoder) { nil }

        override func didMoveToWindow() {
            super.didMoveToWindow()
            check()
        }

        override func layoutSubviews() {
            super.layoutSubviews()
            check()
        }

        private func check() {
            guard window != nil else { return }
            var r: UIResponder? = self
            while let next = r?.next, !(next is UIViewController) { r = next }
            var out = false
            if #available(iOS 18.0, *), let bar = (r?.next as? UIViewController)?.tabBarController {
                out = bar.mode != .tabBar && !bar.sidebar.isHidden && traitCollection.horizontalSizeClass == .regular
            }
            guard out != last else { return }
            last = out
            // Not while laying out: SwiftUI's state changes after.
            DispatchQueue.main.async { [shown] in shown(out) }
        }
    }
}

/// The project the app is in, as the web's current project (Arugula #880):
/// Overview, Boards, Docs, Machines, Chat and its issues show what's in it. Kept on this device; the
/// reader's personal project until they pick another.
@MainActor
final class Scope: ObservableObject {
    @Published var projects: [Projects.Project] = []
    @Published var current: Projects.Project?
    /// Who's signed in, for your picture in the heading.
    @Published var me: Projects.Me?
    @Published var problem: String?
    private static let key = "arugula.current-project"

    func load(_ control: Control) async {
        do {
            let p = try await control.get("/api/projects", as: Projects.self)
            projects = p.projects
            me = p.me
            let kept = UserDefaults.standard.string(forKey: Self.key)
            current = p.projects.first { $0.id == kept } ?? p.projects.first { $0.team == nil } ?? p.projects.first
            problem = nil
        } catch {
            problem = describe(error)
        }
    }

    func pick(_ p: Projects.Project) {
        current = p
        UserDefaults.standard.set(p.id, forKey: Self.key)
    }

    /// Chat's rooms: the project's team's, or every room in the personal
    /// project, which has no team (as the web's chat-scope.ts).
    func showsRoom(_ room: ChatRoom) -> Bool {
        guard let team = current?.team else { return true }
        return room.team == team.id
    }
}

/// The heading at the top of every place's list, as the web phone's top bar
/// (Arugula #1361): the current project, as a pull-down menu of every
/// project and its issues, with whose it is; then New, the bell (the Inbox,
/// with its count) and your picture (Settings, Send feedback, Sign out),
/// which an iPad's sidebar holds at its foot instead. There's no separate
/// bar for something that's seldom changed, and the place's own name is
/// already on the tab bar. The three buttons take 120pt, so the four fit on
/// a 390pt phone; a long project name truncates, the buttons don't. A
/// sheet's heading (the project's Issues) has the menu alone.
struct TabHeading: ViewModifier {
    let title: String
    var buttons = true
    @EnvironmentObject private var scope: Scope
    @Environment(\.makeNew) private var makeNew
    @Environment(\.showInbox) private var showInbox
    @Environment(\.sidebarFoot) private var sidebarFoot

    func body(content: Content) -> some View {
        content
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .safeAreaInset(edge: .top, spacing: 0) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(alignment: .center, spacing: 6) {
                        Menu {
                            ProjectMenu()
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Text((scope.current?.name ?? title).uppercased())
                                    .font(Face.display(30)).foregroundStyle(Palette.fg).lineLimit(1)
                                Image(systemName: "chevron.down").font(.system(size: 13, weight: .bold)).foregroundStyle(Palette.accent)
                            }
                        }
                        .accessibilityIdentifier("project-switcher")
                        Spacer(minLength: 8)
                        if buttons, let makeNew {
                            HeadingButton(symbol: "plus", label: "New", primary: true, action: makeNew)
                                .accessibilityIdentifier("heading-new")
                        }
                        // Beside an iPad's sidebar they're at its foot (Arugula #1366).
                        if buttons, !sidebarFoot, let showInbox {
                            InboxBell(action: showInbox)
                                .accessibilityIdentifier("heading-inbox")
                        }
                        if buttons, !sidebarFoot { AccountMenu() }
                    }
                    if let team = scope.current?.team {
                        Caps("\(team.name) · \(team.role)")
                    }
                    if let problem = scope.problem {
                        Text(problem).font(Face.mono(11)).foregroundStyle(Palette.danger).lineLimit(2)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal)
                .padding(.bottom, 10)
                .background(Palette.void)
            }
    }
}

extension View {
    func tabHeading(_ title: String, buttons: Bool = true) -> some View {
        modifier(TabHeading(title: title, buttons: buttons))
    }
}

/// One of the heading's round buttons: 36pt, with its word for VoiceOver.
struct HeadingButton: View {
    let symbol: String
    let label: String
    var primary = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 15, weight: .bold))
                .foregroundStyle(primary ? Palette.void : Palette.fg)
                .frame(width: 36, height: 36)
                .background(primary ? Palette.accent : Palette.raised, in: Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
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
    /// Look again whenever this changes: a live stream's cue.
    var trigger: Int = 0
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
        .onChange(of: trigger) { _, _ in Task { await refresh() } }
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

// ---- the current project's issues and boards

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

/// The current project's open issues, beside the one that's open: from the
/// project's menu, over the tabs, with Done to close.
struct IssuesView: View {
    let control: Control
    var close: (() -> Void)?
    @EnvironmentObject private var scope: Scope
    @State private var picked: Issue?
    @State private var columns = NavigationSplitViewVisibility.all

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            Group {
                if let project = scope.current {
                    Loaded(load: { try await control.get("/api/projects/\(project.id)/issues", as: IssueList.self) }) { l in
                        List(l.issues, selection: $picked) { issue in
                            NavigationLink(value: issue) { IssueRow(issue: issue) }.voidRow()
                        }
                        .voidList()
                        .overlay { if l.issues.isEmpty { Caps("No open issues") } }
                    }
                    .id(project.id)
                } else {
                    ProgressView().tint(Palette.accent)
                }
            }
            .tabHeading("Issues", buttons: false)
            .background(Palette.void)
            .toolbar {
                if let close {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done", action: close).accessibilityIdentifier("issues-done")
                    }
                }
            }
        } detail: {
            NavigationStack {
                if let picked {
                    IssueView(issue: picked)
                } else {
                    Empty(text: "Pick an issue", symbol: "smallcircle.filled.circle")
                }
            }
            .id(picked?.id)
        }
        .navigationSplitViewStyle(.balanced)
        // Another project: nothing of the last one stays open. (Not an `id` on
        // the split view: rebuilding it under a laying-out bar aborts UIKit.)
        .onChange(of: scope.current?.id) { _, _ in picked = nil }
    }
}

/// A board's thread, pushed from its lead on Boards.
struct BoardThreadRef: Hashable { let board: Board }

/// A lead's chip that follows its session on the fleet.
struct LiveLeadChip: View {
    let lead: Board.Lead
    let shown: Shown?
    @ObservedObject var fleet: HomeFleet

    var body: some View { LeadChip(lead: LeadLine(lead, fleet: fleet, in: shown)) }
}

/// A board's thread from Boards, with the sessions on its cards in flight
/// as home reads them.
struct BoardsThread: View {
    let control: Control
    let board: Board
    @ObservedObject var model: HomeModel
    @ObservedObject var fleet: HomeFleet
    let live: LiveChat
    let peek: (Peeked) -> Void
    @EnvironmentObject private var scope: Scope

    var body: some View {
        let sessions = scope.current.map { HomeState(project: $0, fleet: fleet, model: model).sessions[board.id] ?? [] } ?? []
        BoardThread(control: control, board: board, shown: scope.current?.shown, model: model, fleet: fleet, live: live, sessions: sessions, peek: peek)
    }
}

/// The current project's boards as cards; a board opens to its lanes, its
/// lead's name (Arugula #1279) to that agent (Arugula #1305), and Board thread
/// to its thread, where a post reaches the lead. Sessions on its cards open a
/// peek there, as on home.
struct BoardsView: View {
    let control: Control
    /// Home's, for the boards' threads and the sessions on their cards:
    /// watched only by what shows them, as the fleet changes with every
    /// pane's output.
    let model: HomeModel
    let fleet: HomeFleet
    let live: LiveChat
    @EnvironmentObject private var scope: Scope
    @State private var path = NavigationPath()
    @State private var peek: Peeked?
    @State private var terminal: PhoneHome.PaneOpen?
    @State private var said: String?

    var body: some View {
        NavigationStack(path: $path) {
            Group {
                if let project = scope.current {
                    Loaded(load: { try await control.get("/api/projects/\(project.id)/boards", as: BoardList.self) }) { l in
                        ScrollView {
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: 320), spacing: 14)], spacing: 14) {
                                ForEach(l.boards) { b in boardCard(b) }
                            }
                            .padding()
                        }
                        .overlay { if l.boards.isEmpty { Caps("No boards") } }
                    }
                    .id(project.id)
                } else {
                    ProgressView().tint(Palette.accent)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Palette.void)
            .tabHeading("Boards")
            .navigationDestination(for: Board.self) { BoardScreen(control: control, board: $0) }
            .navigationDestination(for: BoardThreadRef.self) { ref in
                BoardsThread(control: control, board: ref.board, model: model, fleet: fleet, live: live) { peek = $0 }
                    .backBesideTabs("Boards") { path.removeLast() }
            }
            .navigationDestination(for: BoardLanes.self) {
                BoardScreen(control: control, board: $0.board).backBesideTabs("Thread") { path.removeLast() }
            }
            .navigationDestination(for: SessionWave.Row.self) { s in
                ChatRoomView(control: control, room: s.thread, live: live, thread: s.parent)
                    .backBesideTabs("Back") { path.removeLast() }
            }
            .navigationDestination(for: IssueRef.self) { ref in
                Loaded(load: { try await control.get("/api/issues/\(ref.id)", as: Issue.self) }) { IssueView(issue: $0) }
            }
            // A lead's name opens that agent (Arugula #1305), here on Boards.
            .navigationDestination(for: WorkKey.self) { w in
                if let p = scope.current {
                    WorkLevel(control: control, project: p, session: w.key, model: model, fleet: fleet, peek: { peek = $0 }) { m, pane in
                        terminal = PhoneHome.PaneOpen(session: m, pane: pane)
                    }
                    .backBesideTabs("Back") { path.removeLast() }
                }
            }
        }
        .environment(\.talkTo) { key in path.append(WorkKey(key: key)) }
        .sheet(item: $peek) { p in
            SessionPeek(
                control: control, peeked: p,
                openPane: { m, pane in
                    // After the sheet has gone: one presentation at a time.
                    Task {
                        try? await Task.sleep(for: .milliseconds(450))
                        terminal = PhoneHome.PaneOpen(session: m, pane: pane)
                    }
                },
                openThread: { path.append($0) },
                toast: show)
        }
        .fullScreenCover(item: $terminal) { t in TerminalScreen(session: t.session, pane: t.pane) }
        .overlay(alignment: .bottom) {
            if let said { Toast(text: said).transition(.opacity).allowsHitTesting(false) }
        }
        .onChange(of: scope.current?.id) { _, _ in path = NavigationPath() }
    }

    /// A board's card, which opens its lanes, and under it its lead, whose
    /// name opens that agent, and its thread, a button of its own (Arugula
    /// #1305).
    private func boardCard(_ b: Board) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            NavigationLink(value: b) { BoardCard(board: b) }
                .buttonStyle(.plain)
                .accessibilityIdentifier("board-\(b.title)")
            HStack(spacing: 6) {
                if let lead = b.lead {
                    Button { path.append(WorkKey(key: LeadLine(lead, fleet: fleet, in: scope.current?.shown).opening)) } label: {
                        LiveLeadChip(lead: lead, shown: scope.current?.shown, fleet: fleet).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("board-lead-\(b.title)")
                } else {
                    LeadChip(lead: nil)
                }
                Spacer(minLength: 4)
                NavigationLink(value: BoardThreadRef(board: b)) {
                    Label("Board thread", systemImage: "bubble.left")
                        .font(Face.mono(11, "Bold"))
                        .foregroundStyle(Palette.accent)
                        .labelStyle(.titleAndIcon)
                }
                .buttonStyle(.plain)
                .accessibilityHint("Its lead and anyone mentioned read it")
                .accessibilityIdentifier("board-thread-\(b.title)")
            }
            .padding(.top, 8)
            .overlay(alignment: .top) { Rectangle().fill(Palette.raised).frame(height: 1) }
        }
        .card(.accent)
    }

    private func show(_ text: String) {
        withAnimation { said = text }
        Task {
            try? await Task.sleep(for: .seconds(1.8))
            if said == text { withAnimation { said = nil } }
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
        .frame(maxWidth: .infinity, alignment: .leading)
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
/// cards in each lane's tone. On an iPad the lanes stand side by side under a
/// fixed header, each scrolling on its own, so a long lane doesn't make the
/// page long; on an iPhone they stack. Done shows its latest rows as single
/// lines until it's opened. A row with a native issue opens it.
struct BoardScreen: View {
    let control: Control
    let board: Board
    @Environment(\.horizontalSizeClass) private var width
    @State private var allDone = false
    @State private var fullGoal = false

    /// How many of Done's rows show before "Show all".
    private static let doneShown = 8

    var body: some View {
        Loaded(load: { try await control.get("/api/boards/\(board.id)", as: BoardDetail.self) }) { b in
            if width == .regular {
                VStack(alignment: .leading, spacing: 14) {
                    header(b)
                    GeometryReader { g in
                        let gap: CGFloat = 14
                        let lanes = CGFloat(max(b.zones.count, 1))
                        let w = max(240, (g.size.width - 32 - gap * (lanes - 1)) / lanes)
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(alignment: .top, spacing: gap) {
                                ForEach(b.zones) { lane in
                                    VStack(alignment: .leading, spacing: 10) {
                                        laneHead(lane)
                                        ScrollView(showsIndicators: false) {
                                            LazyVStack(alignment: .leading, spacing: 10) { rows(lane, b) }
                                                .padding(.bottom, 24)
                                        }
                                    }
                                    .frame(width: w)
                                }
                            }
                            .padding(.horizontal)
                            .frame(height: g.size.height)
                        }
                    }
                }
                .padding(.top, 4)
                .background(Palette.void)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 22) {
                        header(b)
                        ForEach(b.zones) { lane in
                            VStack(alignment: .leading, spacing: 10) {
                                laneHead(lane)
                                rows(lane, b)
                            }
                            .padding(.horizontal)
                        }
                    }
                    .padding(.vertical)
                }
                .background(Palette.void)
            }
        }
        .background(Palette.void)
        .detailTitle(board.title)
    }

    private func header(_ b: BoardDetail) -> some View {
        let total = b.zones.reduce(0) { $0 + $1.rows.count }
        let n = { (k: LaneKind) in b.zones.first { $0.id == k.rawValue }?.rows.count ?? 0 }
        return VStack(alignment: .leading, spacing: 12) {
            if let goal = b.board.goal, !goal.isEmpty {
                Text(goal).font(Face.ui(14)).foregroundStyle(Palette.subtle)
                    .lineLimit(fullGoal ? nil : 3)
                    .frame(maxWidth: 760, alignment: .leading)
                    .contentShape(Rectangle())
                    .onTapGesture { fullGoal.toggle() }
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
                .frame(maxWidth: 760)
        }
        .padding(.horizontal)
    }

    private func laneHead(_ lane: BoardDetail.Lane) -> some View {
        let tone = LaneKind(rawValue: lane.id)?.tone ?? .muted
        return VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(lane.title.uppercased()).font(Face.display(22)).foregroundStyle(tone.color)
                Text(verbatim: "\(lane.rows.count)").font(Face.mono(12, "Bold")).foregroundStyle(Palette.dim)
            }
            Rectangle().fill(tone.color.opacity(0.6)).frame(height: 1)
        }
    }

    @ViewBuilder
    private func rows(_ lane: BoardDetail.Lane, _ b: BoardDetail) -> some View {
        let kind = LaneKind(rawValue: lane.id)
        let tone = kind?.tone ?? .muted
        if lane.rows.isEmpty {
            Caps("Nothing here").padding(.vertical, 6)
        } else if kind == .done {
            // Done is the record, not the work: its latest rows, one line each.
            let latest = lane.rows.sorted { ($0.updated_at ?? 0) > ($1.updated_at ?? 0) }
            let shown = allDone ? latest : Array(latest.prefix(Self.doneShown))
            VStack(alignment: .leading, spacing: 0) {
                ForEach(shown) { row in
                    link(row) { DoneRow(row: row) }
                    Rectangle().fill(Palette.raised.opacity(0.6)).frame(height: 1)
                }
            }
            if lane.rows.count > Self.doneShown {
                Button(allDone ? "Show the latest \(Self.doneShown)" : "Show all \(lane.rows.count)") { allDone.toggle() }
                    .font(Face.mono(12, "Bold"))
                    .foregroundStyle(Palette.ok)
                    .buttonStyle(.plain)
            }
        } else {
            ForEach(lane.rows) { row in
                link(row) { BoardRowCard(row: row, group: b.groupTitle(row.group), tone: tone, live: kind == .doing) }
            }
        }
    }

    /// A row that opens its native issue; any other row stays put.
    @ViewBuilder
    private func link<V: View>(_ row: BoardDetail.Row, @ViewBuilder _ label: () -> V) -> some View {
        if row.pointer?.kind == "issue", let id = row.pointer?.id {
            NavigationLink(value: IssueRef(id: id)) { label() }.buttonStyle(.plain)
        } else {
            label()
        }
    }
}

/// A done row on one line: a check, its number and its title.
struct DoneRow: View {
    let row: BoardDetail.Row

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "checkmark").font(.system(size: 11, weight: .bold)).foregroundStyle(Palette.ok)
            if let n = row.live?.number {
                Text(verbatim: "#\(n)").font(Face.mono(11, "Bold")).foregroundStyle(Palette.dim)
            }
            Text(row.live?.title ?? row.note ?? "A note")
                .font(Face.ui(13)).foregroundStyle(Palette.subtle).lineLimit(1)
            Spacer(minLength: 0)
        }
        .padding(.vertical, 7)
        .contentShape(Rectangle())
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
