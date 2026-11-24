// Machines: the account's, reached through control's relay over an encrypted
// channel, and their terminals.

import SwiftUI

/// An open channel to one machine. Its socket's thread tells this object when
/// there's something to look at; everything else happens on the main thread.
@MainActor
final class MachineSession: ObservableObject {
    let control: Control
    let machine: Machine
    @Published var panes: [PaneRow] = []
    @Published var problem: String?
    @Published var connected = false
    /// The Claude Code conversation running in each pane, by its title: the
    /// machine's index of them (`GET /api/conversations?live=1`).
    @Published var conversations: [UInt32: String] = [:]
    /// The machine's tabs as it lays them out, and its sessions, which hold them.
    @Published var tabs: [TabLayout] = []
    @Published var sessions: [MuxSession] = []
    /// Its agent blocks, for home (the views here are terminals').
    @Published var agents: [PaneRow] = []
    /// This person's role in each session; nil when the machine says none
    /// (its owner, as the web's fleet reads it).
    @Published var roles: [UInt32: String]?
    /// Its panes as the core gave them, for the core's rule to read again.
    private(set) var summary: String?
    private var link: OpaquePointer?
    private var conversationsAt = Date.distantPast
    /// The terminal being driven, full screen, which takes that pane's bytes.
    weak var screen: TerminalUIView?
    /// The terminals watching a tab, each taking its pane's bytes.
    private var watching: [UInt32: Weak] = [:]
    private final class Weak { weak var view: TerminalUIView? }
    /// The tab being watched, to watch again after a pane was driven.
    private var watchedTab: TabLayout?
    /// Panes drawn on their own, from any tab, by how many views hold each
    /// (the Overview's Work level, Arugula #1307): attached again after a
    /// reconnect.
    private var alone: [UInt32: Int] = [:]

    init(control: Control, machine: Machine) {
        self.control = control
        self.machine = machine
    }

    /// Handed to the core's callback. It can fire once more after the link is
    /// freed, so it isn't tied to this object's life, and it's a few bytes a
    /// connection.
    private final class Notifier { weak var session: MachineSession? }

    func connect() async {
        guard link == nil else { return }
        problem = nil
        let notifier = Notifier()
        notifier.session = self
        let ctx = UInt(bitPattern: Unmanaged.passRetained(notifier).toOpaque())
        let control = self.control
        let id = machine.id
        do {
            let p: UInt = try await Task.detached(priority: .userInitiated) {
                var err: UnsafeMutablePointer<CChar>?
                let got = aru_link_open(control.ptr, id, { ctx in
                    guard let ctx else { return }
                    let n = Unmanaged<Notifier>.fromOpaque(ctx).takeUnretainedValue()
                    DispatchQueue.main.async { MainActor.assumeIsolated { n.session?.pump() } }
                }, UnsafeMutableRawPointer(bitPattern: ctx), &err)
                guard let got else { throw err.map(CoreError.init(taking:)) ?? CoreError("no answer") }
                return UInt(bitPattern: got)
            }.value
            link = OpaquePointer(bitPattern: p)
            connected = true
            screen?.linkOpened()
            pump()
            if screen == nil { rewatch() }
            attachAlone(Array(alone.keys))
        } catch {
            problem = describe(error)
        }
    }

    var linkPointer: OpaquePointer? { link }

    func disconnect() {
        guard let l = link else { return }
        link = nil
        connected = false
        aru_link_free(l)
    }

    /// Take what's queued.
    func pump() {
        guard let l = link else { return }
        // The driven pane first: it takes its bytes even while a watcher has it too.
        var ids: [UInt32] = []
        var terms: [OpaquePointer?] = []
        if let screen, screen.window != nil {
            ids.append(screen.pane)
            terms.append(screen.term)
        }
        for (pane, w) in watching {
            guard let v = w.view else { continue }
            ids.append(pane)
            terms.append(v.term)
        }
        let flags = ids.withUnsafeBufferPointer { p in
            terms.withUnsafeBufferPointer { t in aru_link_feed_many(l, p.baseAddress, t.baseAddress, ids.count) }
        }
        if flags & 2 != 0, let s = takeString(aru_link_panes(l)),
            let list = try? JSONDecoder().decode(PaneList.self, from: Data(s.utf8)) {
            summary = s
            panes = list.panes
            tabs = list.tabs ?? []
            sessions = list.sessions ?? []
            agents = list.agents ?? []
            roles = list.roles.map { Dictionary($0.map { ($0.session, $0.role) }, uniquingKeysWith: { a, _ in a }) }
            // A pane's conversation changes rarely: look again at most every 20 seconds.
            if Date().timeIntervalSince(conversationsAt) > 20 {
                conversationsAt = Date()
                Task { await loadConversations() }
            }
        }
        if flags & 1 != 0 {
            screen?.render()
            for w in watching.values { w.view?.render() }
        }
        if flags & 8 != 0, let said = takeString(aru_link_said(l)) { problem = said }
        if flags & 4 != 0 {
            let why = takeString(aru_link_closed(l))
            disconnect()
            if why != "closed" { problem = why }
        }
    }

    /// Watch `tab` whole, at its own size: its terminals' bytes go to the views
    /// registered for them. Detaches what the last tab had that this one hasn't.
    func watch(_ tab: TabLayout) {
        guard let l = link else {
            watchedTab = tab
            return
        }
        // Detach everything first, the tab's own panes too: attached again, each
        // comes with a fresh snapshot for the views drawing it now (a pane
        // still attached from being driven would only send what's new).
        // A pane drawn on its own elsewhere stays, unless this tab has it.
        let gone = Array(Set((watchedTab?.terminals ?? []) + tab.terminals)).filter { alone[$0] == nil || tab.terminals.contains($0) }
        if !gone.isEmpty { gone.withUnsafeBufferPointer { aru_link_detach(l, $0.baseAddress, gone.count) } }
        watchedTab = tab
        let panes = tab.terminals
        panes.withUnsafeBufferPointer { aru_link_watch(l, tab.id, $0.baseAddress, panes.count, tab.cols, tab.rows) }
    }

    /// A tab's panes, the one that most wants you first.
    func panes(in tab: TabLayout) -> [PaneRow] {
        panes.filter { $0.tab == tab.id }.sorted { ($0.order, -($0.last_ms ?? 0)) < ($1.order, -($1.last_ms ?? 0)) }
    }

    /// What to call a tab, as the web does: its own name, else the conversation
    /// running in it, else what its pane shows or where it is.
    func name(of tab: TabLayout) -> String {
        if let n = tab.name?.trimmingCharacters(in: .whitespaces), !n.isEmpty { return n }
        let mine = panes(in: tab)
        if let c = mine.lazy.compactMap({ self.conversations[$0.id] }).first { return c }
        return mine.first?.name ?? "Empty tab"
    }

    /// Watch the last tab again: after a pane was driven, or a new channel.
    func rewatch() {
        if let t = watchedTab { watch(t) }
    }

    /// Stop watching `tab`, when nothing shows it now. A pane drawn on its
    /// own, or driven, stays.
    func unwatch(_ tab: UInt32) {
        guard let t = watchedTab, t.id == tab else { return }
        watchedTab = nil
        guard let l = link else { return }
        let gone = t.terminals.filter { alone[$0] == nil && screen?.pane != $0 }
        if !gone.isEmpty { gone.withUnsafeBufferPointer { aru_link_detach(l, $0.baseAddress, gone.count) } }
    }

    /// Draw `pane` on its own, at its own size, whatever tab it's in, until
    /// `stopAlone`: a fresh snapshot for the view that now draws it.
    func drawAlone(_ pane: UInt32) {
        alone[pane, default: 0] += 1
        guard let l = link else { return }
        [pane].withUnsafeBufferPointer { aru_link_detach(l, $0.baseAddress, 1) }
        attachAlone([pane])
    }

    func stopAlone(_ pane: UInt32) {
        guard let n = alone[pane] else { return }
        if n > 1 {
            alone[pane] = n - 1
            return
        }
        alone[pane] = nil
        // The tab being watched, or the terminal being driven, still wants it.
        if watchedTab?.terminals.contains(pane) == true || screen?.pane == pane { return }
        guard let l = link else { return }
        [pane].withUnsafeBufferPointer { aru_link_detach(l, $0.baseAddress, 1) }
    }

    /// Each pane as its tab's watch asks for it: a view of its tab that
    /// claims nothing, so nobody's panes resize, and the pane attached.
    private func attachAlone(_ panes: [UInt32]) {
        guard let l = link else { return }
        for pane in panes {
            guard let t = tabs.first(where: { $0.cells.contains { $0.pane == pane } }) else {
                [pane].withUnsafeBufferPointer { aru_link_attach_panes(l, $0.baseAddress, 1) }
                continue
            }
            [pane].withUnsafeBufferPointer { aru_link_watch(l, t.id, $0.baseAddress, 1, t.cols, t.rows) }
        }
        // The tab being watched is the view again.
        if let t = watchedTab {
            let mine = t.terminals.filter { !panes.contains($0) }
            if !mine.isEmpty { mine.withUnsafeBufferPointer { aru_link_watch(l, t.id, $0.baseAddress, 0, t.cols, t.rows) } }
        }
    }

    /// Where a pane sits in its tab, at the machine's size: its columns and rows.
    func grid(of pane: UInt32) -> (UInt16, UInt16)? {
        for t in tabs {
            if let c = t.cells.first(where: { $0.pane == pane }) { return (c.cols, c.rows) }
        }
        return nil
    }

    func register(_ v: TerminalUIView) {
        let w = Weak()
        w.view = v
        watching[v.pane] = w
    }

    func unregister(_ v: TerminalUIView) {
        if watching[v.pane]?.view === v { watching[v.pane] = nil }
    }

    /// The titles of the conversations running in this machine's panes.
    func loadConversations() async {
        guard let l = link else { return }
        let p = UInt(bitPattern: l)
        let s: String? = await Task.detached(priority: .utility) {
            var err: UnsafeMutablePointer<CChar>?
            let got = aru_link_request(OpaquePointer(bitPattern: p), "GET", "/api/conversations?live=1", nil, &err)
            if let err { aru_string_free(err) }
            return takeString(got)
        }.value
        guard let s, let v = try? JSONDecoder().decode(Answer.self, from: Data(s.utf8)) else { return }
        var titles: [UInt32: String] = [:]
        for c in v.body.conversations {
            guard let pane = c.live?.pane ?? c.block else { continue }
            if let t = [c.title, c.last_prompt, c.first_prompt].compactMap({ $0 }).first(where: PaneRow.readable) {
                titles[pane] = t
            }
        }
        conversations = titles
    }

    private struct Answer: Decodable {
        struct Body: Decodable { var conversations: [Row] }
        struct Row: Decodable {
            struct Live: Decodable { var pane: UInt32? }
            var title: String?
            var first_prompt: String?
            var last_prompt: String?
            var live: Live?
            var block: UInt32?
        }
        var body: Body
    }

    /// Its sessions that show in `shown`, as the core's rule keeps them
    /// (Arugula #1352): each with its tabs and their panes, needs you first.
    func cards(in shown: Shown?) -> [Sections.Card] {
        let host = HostIn(id: machine.id, name: machine.name, online: true, owner: machine.owner, panes: summary)
        return Sections.of([host], shown: shown, recent: nil)?.machines.first?.sessions ?? []
    }

    /// This person's role in a pane's session: `owner`, `editor` or `viewer`.
    func role(of pane: PaneRow) -> String {
        guard let roles else { return "owner" }
        guard let session = sessions.first(where: { $0.tabs.contains(pane.tab) }) else { return "viewer" }
        return roles[session.id] ?? "viewer"
    }

    /// One request to the machine's API over the link: its answer's body
    /// (JSON, or a capture's text as a string).
    func request(_ method: String, _ path: String, _ body: [String: Any]? = nil) async throws -> Any? {
        guard let l = link else { throw CoreError("not connected to \(machine.name)") }
        let p = UInt(bitPattern: l)
        let json = try body.map { String(decoding: try JSONSerialization.data(withJSONObject: $0), as: UTF8.self) }
        let s: String = try await Task.detached(priority: .userInitiated) {
            var err: UnsafeMutablePointer<CChar>?
            let got = aru_link_request(OpaquePointer(bitPattern: p), method, path, json, &err)
            guard let s = takeString(got) else { throw err.map(CoreError.init(taking:)) ?? CoreError("no answer") }
            return s
        }.value
        let v = try JSONSerialization.jsonObject(with: Data(s.utf8), options: [.fragmentsAllowed]) as? [String: Any]
        return v?["body"]
    }

    /// A new shell on the machine: its pane.
    func newTerminal() async throws -> UInt32 {
        guard let l = link else { throw CoreError("not connected") }
        let p = UInt(bitPattern: l)
        let s: String = try await Task.detached(priority: .userInitiated) {
            var err: UnsafeMutablePointer<CChar>?
            let got = aru_link_request(OpaquePointer(bitPattern: p), "POST", "/api/run", "{}", &err)
            guard let s = takeString(got) else { throw err.map(CoreError.init(taking:)) ?? CoreError("no answer") }
            return s
        }.value
        let v = try JSONSerialization.jsonObject(with: Data(s.utf8)) as? [String: Any]
        guard let body = v?["body"] as? [String: Any], let pane = body["pane"] as? Int else { throw CoreError("the machine sent no pane") }
        return UInt32(pane)
    }
}

// ---- Machines: one page of machines › sessions › pane rows (Arugula #1357)

/// The pane last opened in a project, on this device: Machines lights its
/// row as "where you were", and an iPad shows its tab live before anything
/// is picked.
enum WhereYouWere {
    struct Pane: Equatable { var machine: String; var pane: UInt32 }

    private static func key(_ project: String?) -> String { "arugula.last-pane.\(project ?? "")" }

    static func of(_ project: String?) -> Pane? {
        let parts = UserDefaults.standard.string(forKey: key(project))?.split(separator: " ") ?? []
        guard parts.count == 2, let pane = UInt32(parts[1]) else { return nil }
        return Pane(machine: String(parts[0]), pane: pane)
    }

    /// It was opened: and its machine is the one used last.
    static func note(_ p: Pane, in project: String?) {
        UserDefaults.standard.set("\(p.machine) \(p.pane)", forKey: key(project))
        UserDefaults.standard.set(p.machine, forKey: MachinesView.lastKey)
    }
}

/// Machines, as the web's page (web/src/app/machines-list.tsx): each machine
/// in the core's order with its sessions in the project as cards, and each
/// card's panes as one-line rows. Machines with nothing in the project fold
/// into one line, and an offline one can't be opened. On an iPhone a
/// session's name opens the session, a tab at a time with its panes
/// stacked, and a row opens its pane full screen to type into. On an iPad
/// the page is the left column with a tab live on the right: before
/// anything is picked, the one you were last in.
struct MachinesView: View {
    let control: Control
    @ObservedObject var fleet: HomeFleet
    @EnvironmentObject private var scope: Scope
    @ObservedObject private var push = PushCenter.shared
    @Environment(\.horizontalSizeClass) private var width
    /// An iPhone's: the session opened.
    @State private var path: [SessionRef] = []
    /// An iPad's: the tab live on the right, and the pane lit in it.
    @State private var picked: Picked?
    @State private var terminal: PhoneHome.PaneOpen?
    /// The machine whose pane is driven, to watch its tab again after.
    @State private var driven: MachineSession?
    @State private var last: WhereYouWere.Pane?
    @State private var problem: String?
    @State private var loaded = false
    @State private var quietOpen = false
    @State private var columns = NavigationSplitViewVisibility.all

    struct SessionRef: Hashable { var machine: String; var session: UInt32; var tab: UInt32? }
    struct Picked: Equatable { var machine: String; var tab: UInt32; var pane: UInt32? }

    private var shown: Shown? { scope.current?.shown }

    var body: some View {
        Group {
            if width == .regular {
                NavigationSplitView(columnVisibility: $columns) {
                    page.navigationSplitViewColumnWidth(min: 320, ideal: 380, max: 460)
                } detail: {
                    NavigationStack { right }
                }
                .navigationSplitViewStyle(.balanced)
            } else {
                NavigationStack(path: $path) {
                    page.navigationDestination(for: SessionRef.self) { ref in
                        if let m = reached(ref.machine) {
                            SessionScreen(session: m, id: ref.session, shown: shown, tab: ref.tab,
                                          last: last?.machine == ref.machine ? last?.pane : nil) { open(m, $0) }
                        } else {
                            Empty(text: "Not reached", symbol: "bolt.horizontal")
                        }
                    }
                }
            }
        }
        .fullScreenCover(item: $terminal, onDismiss: {
            // Let go of the full-screen terminal now, or it would take the
            // pane's fresh snapshot; then the tab it came from, again.
            driven?.screen = nil
            driven?.rewatch()
            driven = nil
        }) { t in
            TerminalScreen(session: t.session, pane: t.pane)
        }
        .task {
            while !Task.isCancelled {
                await fleet.load(control)
                loaded = true
                try? await Task.sleep(for: .seconds(30))
            }
        }
        .onChange(of: scope.current?.id, initial: true) { _, now in
            last = WhereYouWere.of(now)
            picked = nil
            path = []
        }
        // A notification about a pane that home couldn't place (#1260):
        // it opens full screen once its machine is reached.
        .onChange(of: push.route, initial: true) { _, route in
            guard case .pane(let machine, let want)? = route else { return }
            push.route = nil
            Task {
                for _ in 0..<40 {
                    if let m = reached(machine), let row = m.panes.first(where: { $0.id == want }) {
                        drive(m, row)
                        return
                    }
                    try? await Task.sleep(for: .milliseconds(500))
                }
            }
        }
    }

    /// The link to a machine, kept by the fleet.
    private func reached(_ id: String) -> MachineSession? {
        fleet.machines.first { $0.machine.id == id }
    }

    /// The machines as the core lays them out for the project.
    private var sections: Sections? {
        let hosts = fleet.directory.map { d in
            HostIn(id: d.id, name: d.name, online: d.online == true, owner: d.owner, panes: reached(d.id)?.summary)
        }
        return Sections.of(hosts, shown: shown, recent: UserDefaults.standard.string(forKey: Self.lastKey))
    }

    // ---- the page

    private var page: some View {
        let s = sections
        return ScrollView {
            LazyVStack(alignment: .leading, spacing: 22) {
                if let p = problem {
                    Text(p).font(Face.mono(12)).foregroundStyle(Palette.danger).accessibilityIdentifier("machine-problem")
                }
                if fleet.directory.isEmpty {
                    Caps(loaded ? "No machines yet" : "Looking")
                } else if let s, s.machines.isEmpty {
                    Text("Nothing in \(scope.current?.name ?? "this project") is running on a machine.")
                        .font(Face.ui(14)).foregroundStyle(Palette.dim)
                }
                ForEach(s?.machines ?? []) { m in machine(m) }
                if let q = s?.quiet, !q.isEmpty { fold(q) }
            }
            .padding()
        }
        .refreshable { await fleet.load(control) }
        .tabHeading("Machines")
        .background(Palette.void)
    }

    private func machine(_ m: Sections.Section) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            head(m)
            if let link = reached(m.id) {
                ForEach(m.sessions) { c in
                    SessionCardView(
                        session: link, card: c, compact: width != .regular,
                        last: last?.machine == m.id ? last?.pane : nil,
                        lit: picked?.machine == m.id ? picked?.pane : nil,
                        open: { openSession(link, c) },
                        openPane: { openRow(link, c, $0) })
                }
            }
        }
    }

    /// A machine's name, whose it is, and New terminal on yours.
    private func head(_ m: Sections.Section) -> some View {
        HStack(spacing: 8) {
            Circle().fill(m.online ? Palette.ok : Palette.muted).frame(width: 8, height: 8)
                .shadow(color: m.online ? Palette.ok.opacity(0.8) : .clear, radius: 4)
            Text(Self.short(m.name).uppercased()).font(Face.display(20)).foregroundStyle(Palette.fg).lineLimit(1)
            if let owner = m.owner { Caps(owner) }
            Spacer(minLength: 4)
            if m.owner == nil, m.online { newTerminal(m) }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("machine-\(m.name)")
    }

    private func newTerminal(_ m: Sections.Section) -> some View {
        let on = reached(m.id)?.connected == true
        return Button { start(on: m.id) } label: {
            Image(systemName: "plus")
                .font(.system(size: 13, weight: .bold))
                .foregroundStyle(on ? Palette.void : Palette.dim)
                .frame(width: 30, height: 26)
                .background(on ? Palette.accent : Palette.raised, in: RoundedRectangle(cornerRadius: 7))
        }
        .buttonStyle(.plain)
        .disabled(!on)
        .accessibilityLabel("New terminal on \(Self.short(m.name))")
        .accessibilityIdentifier("new-terminal-\(m.name)")
    }

    /// The machines with nothing in the project, as one line that opens to
    /// them. An offline one is only named.
    private func fold(_ quiet: [Sections.Section]) -> some View {
        let n = quiet.count
        let what = scope.current.map { "no \($0.name) sessions" } ?? "no sessions"
        return VStack(alignment: .leading, spacing: 10) {
            Button { withAnimation { quietOpen.toggle() } } label: {
                HStack(spacing: 6) {
                    Image(systemName: quietOpen ? "chevron.down" : "chevron.right").font(.system(size: 11, weight: .bold))
                    Text("\(n) \(n == 1 ? "machine" : "machines") with \(what)").font(Face.ui(13))
                    Spacer(minLength: 0)
                }
                .foregroundStyle(Palette.dim)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("machines-quiet")
            if quietOpen {
                ForEach(quiet) { m in
                    HStack(spacing: 8) {
                        Circle().fill(m.online ? Palette.ok : Palette.muted).frame(width: 7, height: 7)
                        Text(Self.short(m.name)).font(Face.ui(14, "SemiBold"))
                            .foregroundStyle(m.online ? Palette.fg : Palette.dim).lineLimit(1)
                        if let owner = m.owner { Caps(owner) }
                        Spacer(minLength: 4)
                        if !m.online {
                            Caps("offline")
                        } else if reached(m.id)?.connected != true {
                            Caps("connecting")
                        } else if m.owner == nil {
                            newTerminal(m)
                        }
                    }
                    .padding(.leading, 18)
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("machine-\(m.name)")
                }
            }
        }
    }

    // ---- what a tap opens

    /// A session's name: on an iPhone the session, on an iPad its tab live
    /// on the right. Its tab: the one you were in, else one that needs you,
    /// else the first.
    private func openSession(_ m: MachineSession, _ c: Sections.Card) {
        let rows = m.panes + m.agents
        let tab = c.tabs.first { $0.panes.contains { $0 == last?.pane && last?.machine == m.machine.id } }
            ?? c.tabs.first { $0.panes.contains { p in rows.contains { $0.id == p && $0.needsYou } } }
            ?? c.tabs.first
        UserDefaults.standard.set(m.machine.id, forKey: Self.lastKey)
        if width == .regular {
            if let tab { picked = Picked(machine: m.machine.id, tab: tab.id, pane: nil) }
        } else {
            path.append(SessionRef(machine: m.machine.id, session: c.id, tab: tab?.id))
        }
    }

    /// A pane's row: on an iPad its tab on the right, the pane lit; on an
    /// iPhone the pane full screen (an agent's block, in its session).
    private func openRow(_ m: MachineSession, _ c: Sections.Card, _ row: PaneRow) {
        note(m, row)
        if width == .regular {
            picked = Picked(machine: m.machine.id, tab: row.tab, pane: row.id)
        } else if row.isAgent {
            path.append(SessionRef(machine: m.machine.id, session: c.id, tab: row.tab))
        } else {
            drive(m, row)
        }
    }

    /// A pane full screen, typed into, with the key bar.
    private func open(_ m: MachineSession, _ row: PaneRow) {
        note(m, row)
        drive(m, row)
    }

    private func drive(_ m: MachineSession, _ row: PaneRow) {
        driven = m
        terminal = PhoneHome.PaneOpen(session: m, pane: row)
    }

    private func note(_ m: MachineSession, _ row: PaneRow) {
        let p = WhereYouWere.Pane(machine: m.machine.id, pane: row.id)
        WhereYouWere.note(p, in: scope.current?.id)
        last = p
    }

    /// A new shell on one of your machines, open full screen once it's there.
    private func start(on machine: String) {
        guard let m = reached(machine) else { return }
        problem = nil
        Task {
            do {
                let pane = try await m.newTerminal()
                for _ in 0..<40 {
                    if let row = m.panes.first(where: { $0.id == pane }) {
                        open(m, row)
                        return
                    }
                    try? await Task.sleep(for: .milliseconds(250))
                }
                problem = "The new terminal didn't show up on \(Self.short(m.machine.name))."
            } catch {
                problem = describe(error)
            }
        }
    }

    // ---- an iPad's right side

    /// The tab picked, else the one you were last in, live; else nothing yet.
    @ViewBuilder
    private var right: some View {
        if let p = picked, let m = reached(p.machine), let t = live(m, tab: p.tab) {
            LiveTab(session: m, tab: t.0, card: t.1, lit: p.pane, label: nil) { open(m, $0) }
                .id("\(p.machine)/\(p.tab)")
        } else if let l = last, let m = reached(l.machine),
                  let row = (m.panes + m.agents).first(where: { $0.id == l.pane }), let t = live(m, tab: row.tab) {
            LiveTab(session: m, tab: t.0, card: t.1, lit: l.pane, label: "Where you were") { open(m, $0) }
                .id("\(l.machine)/\(row.tab)")
        } else {
            Empty(text: "Pick a session", symbol: "terminal")
        }
    }

    /// A tab of a session the project shows, and its card.
    private func live(_ m: MachineSession, tab: UInt32) -> (TabLayout, Sections.Card)? {
        guard let card = m.cards(in: shown).first(where: { $0.tabs.contains { $0.id == tab } }),
              let layout = m.tabs.first(where: { $0.id == tab }) else { return nil }
        return (layout, card)
    }
}

extension MachinesView {
    /// The machine used last, first of yours.
    static let lastKey = "arugula.last-machine"

    /// A machine's name as a card shows it: a Mac's `.local` (or a LAN's
    /// `.lan`, `.home`) adds nothing (the web's shortHost).
    static func short(_ name: String) -> String {
        name.replacingOccurrences(of: #"\.(local|lan|home)$"#, with: "", options: [.regularExpression, .caseInsensitive])
    }
}

/// A session's card: a dot in its state's tone, its name (which opens it),
/// its tabs and panes counted, and each pane as one line, under its tab's
/// name when there's more than one. Three tabs show, and any that needs you
/// or holds where you were; the rest wait behind "+N more".
struct SessionCardView: View {
    let session: MachineSession
    let card: Sections.Card
    /// An iPhone's: no kind or branch on a row.
    let compact: Bool
    /// The pane you were last in, on this machine.
    let last: UInt32?
    /// The pane lit on an iPad's right side.
    let lit: UInt32?
    let open: () -> Void
    let openPane: (PaneRow) -> Void
    @State private var all = false

    private static let tabs = 3

    var body: some View {
        let tone: Tone = card.state == "needs_input" ? .accent : card.state == "working" ? .info : .muted
        let few = card.tabs.enumerated()
            .filter { i, t in i < Self.tabs || t.panes.contains { $0 == last || row($0)?.needsYou == true } }
            .map(\.element)
        let more = card.tabs.count - few.count
        VStack(alignment: .leading, spacing: 4) {
            Button(action: open) {
                HStack(spacing: 8) {
                    Circle().fill(tone.color).frame(width: 8, height: 8)
                        .shadow(color: card.state == "needs_input" ? tone.color : .clear, radius: 4)
                    Text(card.name).font(Face.ui(16, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                    Spacer(minLength: 4)
                    Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold)).foregroundStyle(Palette.dim)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("session-\(card.name)")
            Text(meta).font(Face.mono(11)).foregroundStyle(Palette.dim)
            ForEach(all ? card.tabs : few) { t in
                if card.tabs.count > 1 {
                    Caps(name(of: t)).padding(.top, 6)
                }
                ForEach(t.panes, id: \.self) { id in
                    if let r = row(id) { line(r) }
                }
            }
            if more > 0 || all {
                Button(all ? "Show fewer" : "+\(more) more \(more == 1 ? "tab" : "tabs")") { withAnimation { all.toggle() } }
                    .font(Face.mono(11, "Bold"))
                    .foregroundStyle(Palette.accent)
                    .buttonStyle(.plain)
                    .padding(.top, 4)
            }
        }
        .card(tone, glow: card.state == "needs_input")
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session-card-\(card.id)")
    }

    private var meta: String {
        let t = card.tabs.count
        return "\(t) \(t == 1 ? "tab" : "tabs") · \(card.panes) \(card.panes == 1 ? "pane" : "panes")"
    }

    private func row(_ id: UInt32) -> PaneRow? {
        session.panes.first { $0.id == id } ?? session.agents.first { $0.id == id }
    }

    private func name(of t: Sections.Card.Tab) -> String {
        if let n = t.name, !n.trimmingCharacters(in: .whitespaces).isEmpty { return n }
        if let layout = session.tabs.first(where: { $0.id == t.id }) { return session.name(of: layout) }
        return "Tab \((card.tabs.firstIndex { $0.id == t.id } ?? 0) + 1)"
    }

    /// A pane on one line: what it does, where you were, and Needs you; on
    /// an iPad its program and branch too.
    private func line(_ r: PaneRow) -> some View {
        let facts = [r.program, r.branch.map { "⎇ \($0)" }].compactMap { $0 }.joined(separator: " · ")
        return Button { openPane(r) } label: {
            HStack(spacing: 8) {
                Circle().fill(PaneLook.tone(r).color).frame(width: 6, height: 6)
                Text(session.conversations[r.id] ?? r.name).font(Face.ui(14)).foregroundStyle(Palette.fg).lineLimit(1)
                if r.id == last {
                    Text("where you were").font(Face.mono(10)).foregroundStyle(Palette.dim).lineLimit(1).layoutPriority(-1)
                }
                Spacer(minLength: 4)
                if r.needsYou { NeedsChip() }
                if !compact, !facts.isEmpty {
                    Text(facts).font(Face.mono(10)).foregroundStyle(Palette.dim).lineLimit(1).truncationMode(.middle)
                        .frame(maxWidth: 130, alignment: .trailing)
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 7)
            .background(
                r.id == lit ? Palette.accent.opacity(0.16) : r.id == last ? Palette.raised.opacity(0.7) : .clear,
                in: RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, -8)
        .accessibilityIdentifier("pane-\(r.id)")
    }
}

/// How a pane looks for what it's doing.
enum PaneLook {
    static func tone(_ p: PaneRow) -> Tone {
        guard p.running else { return .muted }
        if p.needsYou { return p.failed ? .danger : .accent }
        switch p.attention {
        case "working": return .info
        case "done": return .ok
        default: return .muted
        }
    }
}

/// The Needs you chip, as the web's.
struct NeedsChip: View {
    var body: some View {
        Text(NEEDS_YOU.uppercased())
            .font(Face.mono(9, "Bold"))
            .tracking(0.6)
            .foregroundStyle(Palette.accent)
            .lineLimit(1)
            .fixedSize()
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .overlay(Capsule().strokeBorder(Palette.accent.opacity(0.6), lineWidth: 1))
    }
}

/// An iPhone's view of one session, a tab at a time: its tabs as chips (a
/// dot on one that needs you), and the tab's panes stacked, each live at
/// its own size, scaled to the width. Tap a pane to drive it full screen,
/// with the keyboard and the key bar. An agent's block shows what it says,
/// with a box that types to it.
struct SessionScreen: View {
    @ObservedObject var session: MachineSession
    let id: UInt32
    let shown: Shown?
    let last: UInt32?
    let drive: (PaneRow) -> Void
    @State private var tab: UInt32?

    init(session: MachineSession, id: UInt32, shown: Shown?, tab: UInt32?, last: UInt32?, drive: @escaping (PaneRow) -> Void) {
        self.session = session
        self.id = id
        self.shown = shown
        self.last = last
        self.drive = drive
        _tab = State(initialValue: tab)
    }

    var body: some View {
        let card = session.cards(in: shown).first { $0.id == id }
        let tabs = card?.tabs ?? []
        let on = tabs.first { $0.id == tab } ?? tabs.first
        let layout = on.flatMap { t in session.tabs.first { $0.id == t.id } }
        VStack(alignment: .leading, spacing: 0) {
            if tabs.count > 1 { chips(tabs, on: on?.id) }
            if let on, let layout {
                GeometryReader { g in
                    ScrollView {
                        VStack(spacing: 12) {
                            ForEach(on.panes, id: \.self) { p in
                                if let row = session.panes.first(where: { $0.id == p }) ?? session.agents.first(where: { $0.id == p }) {
                                    StackedPane(session: session, row: row, tab: layout, last: p == last, width: g.size.width - 20, drive: drive)
                                }
                            }
                        }
                        .padding(10)
                    }
                }
                .onAppear { session.watch(layout) }
                .onChange(of: layout) { _, now in session.watch(now) }
                .onDisappear { session.unwatch(layout.id) }
            } else if session.connected {
                Empty(text: "This session has no panes now", symbol: "rectangle.split.2x2")
            } else {
                ProgressView { Caps("Connecting") }.tint(Palette.accent).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Palette.void)
        .navigationTitle(card?.name ?? "Session")
        .navigationBarTitleDisplayMode(.inline)
        .accessibilityIdentifier("session-screen")
    }

    private func chips(_ tabs: [Sections.Card.Tab], on: UInt32?) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(tabs) { t in
                    let picked = t.id == on
                    let needs = t.panes.contains { p in (session.panes + session.agents).contains { $0.id == p && $0.needsYou } }
                    Button { tab = t.id } label: {
                        HStack(spacing: 5) {
                            if needs { Circle().fill(picked ? Palette.void : Palette.accent).frame(width: 6, height: 6) }
                            Text(name(of: t, in: tabs)).font(Face.mono(12, "Bold")).lineLimit(1)
                        }
                        .foregroundStyle(picked ? Palette.void : Palette.fg)
                        .padding(.horizontal, 11)
                        .padding(.vertical, 6)
                        .background(picked ? Palette.accent : Palette.surface, in: Capsule())
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("session-tab-\(t.id)")
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
        }
    }

    private func name(of t: Sections.Card.Tab, in tabs: [Sections.Card.Tab]) -> String {
        if let n = t.name, !n.trimmingCharacters(in: .whitespaces).isEmpty { return n }
        if let layout = session.tabs.first(where: { $0.id == t.id }) { return session.name(of: layout) }
        return "Tab \((tabs.firstIndex { $0.id == t.id } ?? 0) + 1)"
    }
}

/// One pane of a session's tab, stacked: what it is and how it stands over
/// it, and the pane, live, scaled to `width`. Its tab's watch feeds it.
struct StackedPane: View {
    @ObservedObject var session: MachineSession
    let row: PaneRow
    let tab: TabLayout
    let last: Bool
    let width: CGFloat
    let drive: (PaneRow) -> Void

    var body: some View {
        let cell = tab.cells.first { $0.pane == row.id }
        let terminal = !row.isAgent && (cell?.kind ?? "terminal") == "terminal"
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Circle().fill(PaneLook.tone(row).color).frame(width: 7, height: 7)
                Text(session.conversations[row.id] ?? row.name).font(Face.ui(14, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                if last {
                    Text("where you were").font(Face.mono(10)).foregroundStyle(Palette.dim).lineLimit(1).layoutPriority(-1)
                }
                Spacer(minLength: 4)
                if row.needsYou { NeedsChip() }
                if terminal {
                    Image(systemName: "keyboard").font(.system(size: 12)).foregroundStyle(Palette.dim)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(Palette.surface)
            if terminal, let cell {
                let ten = TerminalUIView.cell(size: 10)
                let size = (max(3, min(12, 10 * width / (CGFloat(cell.cols) * ten.width))) * 2).rounded(.down) / 2
                let c = TerminalUIView.cell(size: size)
                WatchTerminal(session: session, tab: tab.id, pane: row.id, cols: cell.cols, rows: cell.rows, size: size)
                    .id("\(row.id)-\(cell.cols)x\(cell.rows)@\(size)")
                    .frame(width: CGFloat(cell.cols) * c.width, height: CGFloat(cell.rows) * c.height)
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                    .background(Color.black)
                    .clipped()
                    // A new size makes a new terminal: it needs the pane's snapshot.
                    .onChange(of: size) { _, _ in session.rewatch() }
            } else {
                BlockSlot(session: session, pane: row).frame(height: 260)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(PaneLook.tone(row).color.opacity(row.needsYou ? 0.9 : 0.4), lineWidth: row.needsYou ? 2 : 1))
        .contentShape(Rectangle())
        .onTapGesture { if terminal { drive(row) } }
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(terminal ? .isButton : [])
        .accessibilityIdentifier("stacked-\(row.id)")
    }
}

/// An iPad's right side: a session's tab drawn whole and live, its name over
/// it ("Where you were" before anything is picked). Tap a pane to drive it.
struct LiveTab: View {
    @ObservedObject var session: MachineSession
    let tab: TabLayout
    let card: Sections.Card
    let lit: UInt32?
    let label: String?
    let drive: (PaneRow) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                if let label { Caps(label) }
                if card.tabs.count > 1 {
                    Text(session.name(of: tab)).font(Face.ui(17, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(1)
                }
                Spacer()
                Text(MachinesView.short(session.machine.name)).font(Face.mono(12)).foregroundStyle(Palette.dim)
            }
            TabCanvas(session: session, tab: tab, conversations: session.conversations, lit: lit, focus: drive)
        }
        .padding()
        .background(Palette.void)
        .detailTitle(card.name)
        .onDisappear { session.unwatch(tab.id) }
        .accessibilityIdentifier("live-tab")
    }
}

/// A tab drawn whole and live: every pane at its rectangle in the tab's grid,
/// at the tab's own size, in a font small enough for the tab to fit. Nothing
/// here resizes anyone's panes. A pane's edge takes the tone of what it's
/// doing, or the accent for the one `lit`; tap one to drive it.
struct TabCanvas: View {
    @ObservedObject var session: MachineSession
    let tab: TabLayout
    let conversations: [UInt32: String]
    var lit: UInt32?
    let focus: (PaneRow) -> Void

    var body: some View {
        GeometryReader { g in
            let ten = TerminalUIView.cell(size: 10)
            let fit = min(g.size.width / (CGFloat(tab.cols) * ten.width), g.size.height / (CGFloat(tab.rows) * ten.height))
            let size = (max(3, min(14, 10 * fit)) * 2).rounded(.down) / 2
            let cell = TerminalUIView.cell(size: size)
            ZStack(alignment: .topLeading) {
                ForEach(tab.cells, id: \.pane) { c in
                    paneView(c, size: size)
                        .frame(width: CGFloat(c.cols) * cell.width, height: CGFloat(c.rows) * cell.height)
                        .offset(x: CGFloat(c.x) * cell.width, y: CGFloat(c.y) * cell.height)
                }
            }
            .frame(width: CGFloat(tab.cols) * cell.width, height: CGFloat(tab.rows) * cell.height, alignment: .topLeading)
            .background(Color.black)
            .clipShape(RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Palette.raised, lineWidth: 1))
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            // A new size makes new terminals: they need the panes' snapshots.
            .onChange(of: size) { _, _ in session.rewatch() }
        }
        .onAppear { session.watch(tab) }
        .onChange(of: tab) { _, now in session.watch(now) }
    }

    @ViewBuilder
    private func paneView(_ c: TabLayout.Cell, size: CGFloat) -> some View {
        let row = session.panes.first { $0.id == c.pane }
        if let row, (c.kind ?? "terminal") == "terminal" {
            let tone: Tone = c.pane == lit || row.order == 0 ? .accent : row.order == 1 ? .info : .muted
            let strong = c.pane == lit || row.order == 0
            WatchTerminal(session: session, tab: tab.id, pane: c.pane, cols: c.cols, rows: c.rows, size: size)
                .id("\(c.pane)-\(c.cols)x\(c.rows)@\(size)")
                .overlay(Rectangle().strokeBorder(tone.color.opacity(row.order <= 1 || strong ? 0.9 : 0.35), lineWidth: strong ? 2 : 1))
                .overlay(alignment: .topTrailing) {
                    Text(conversations[c.pane] ?? row.name)
                        .font(Face.mono(10, "Medium"))
                        .lineLimit(1)
                        .foregroundStyle(tone == .muted ? Palette.subtle : tone.color)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Palette.void.opacity(0.85), in: RoundedRectangle(cornerRadius: 4))
                        .padding(4)
                }
                .contentShape(Rectangle())
                .onTapGesture { focus(row) }
                .accessibilityIdentifier("tab-pane-\(c.pane)")
        } else {
            ZStack {
                Rectangle().fill(Palette.surface)
                Caps(c.kind ?? "block")
            }
            .overlay(Rectangle().strokeBorder(Palette.raised, lineWidth: 1))
        }
    }
}

/// A pane's terminal, watched: its own grid, no keyboard.
struct WatchTerminal: UIViewRepresentable {
    let session: MachineSession
    let tab: UInt32
    let pane: UInt32
    let cols: UInt16
    let rows: UInt16
    let size: CGFloat

    func makeUIView(context: Context) -> TerminalUIView {
        let v = TerminalUIView(session: session, tab: tab, pane: pane, fontSize: size, fixed: (cols, rows))
        session.register(v)
        return v
    }

    func updateUIView(_ v: TerminalUIView, context: Context) {}

    static func dismantleUIView(_ v: TerminalUIView, coordinator: ()) {
        MainActor.assumeIsolated { v.session.unregister(v) }
    }
}
