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
    private var link: OpaquePointer?
    private var conversationsAt = Date.distantPast
    /// The terminal being driven, full screen, which takes that pane's bytes.
    weak var screen: TerminalUIView?
    /// The terminals watching a tab, each taking its pane's bytes.
    private var watching: [UInt32: Weak] = [:]
    private final class Weak { weak var view: TerminalUIView? }
    /// The tab being watched, to watch again after a pane was driven.
    private var watchedTab: TabLayout?

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
        if let screen {
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
            panes = list.panes
            tabs = list.tabs ?? []
            sessions = list.sessions ?? []
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
        let gone = (watchedTab?.terminals ?? []).filter { !tab.terminals.contains($0) }
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

struct MachinesView: View {
    let control: Control
    @State private var picked: Machine?
    @State private var columns = NavigationSplitViewVisibility.all

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            Loaded(load: { try await control.get("/api/directory", as: Directory.self) }) { d in
                List(d.daemons, selection: $picked) { m in
                    NavigationLink(value: m) {
                        HStack(spacing: 10) {
                            Circle().fill(m.online == true ? Palette.ok : Palette.muted).frame(width: 8, height: 8)
                                .shadow(color: m.online == true ? Palette.ok.opacity(0.8) : .clear, radius: 4)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(m.name).font(Face.ui(16, "SemiBold"))
                                Caps(m.online == true ? "online" : "offline")
                            }
                        }
                        .padding(.vertical, 4)
                    }
                    .voidRow()
                    .accessibilityIdentifier("machine-\(m.name)")
                }
                .voidList()
                .overlay { if d.daemons.isEmpty { Caps("No machines yet") } }
            }
            .tabHeading("Machines")
            .background(Palette.void)
        } detail: {
            NavigationStack {
                if let picked {
                    MachineView(control: control, machine: picked)
                } else {
                    Empty(text: "Pick a machine", symbol: "terminal")
                }
            }
            .id(picked?.id)
        }
        .navigationSplitViewStyle(.balanced)
        // A machine takes the whole width once it's picked (a tab's panes want
        // it); the sidebar button brings the list back.
        .onChange(of: picked) { _, now in
            if now != nil { withAnimation { columns = .detailOnly } }
        }
    }
}

struct MachineView: View {
    @StateObject private var session: MachineSession
    @State private var opened: PaneRow?
    @State private var pendingPane: UInt32?
    @State private var problem: String?
    /// The tab shown whole, on an iPad.
    @State private var tabID: UInt32?
    /// The pane cards instead of the tab, on an iPad.
    @State private var cards = false
    @Environment(\.scenePhase) private var phase
    @Environment(\.horizontalSizeClass) private var width

    init(control: Control, machine: Machine) {
        _session = StateObject(wrappedValue: MachineSession(control: control, machine: machine))
    }

    var body: some View {
        Group {
            if width == .regular && !cards {
                tabView
            } else {
                cardView
            }
        }
        .background(Palette.void)
        .overlay {
            if !session.connected && session.problem == nil {
                ProgressView { Caps("Connecting") }.tint(Palette.accent)
            }
        }
        .detailTitle(session.machine.name)
        .toolbar {
            if width == .regular {
                ToolbarItem(placement: .topBarTrailing) {
                    Picker("Show", selection: $cards) {
                        Image(systemName: "rectangle.split.2x2").tag(false)
                        Image(systemName: "square.grid.2x2").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .accessibilityIdentifier("machine-show")
                }
            }
        }
        .task { await session.connect() }
        .onChange(of: session.panes) { _, _ in openPending() }
        .onChange(of: phase) { _, now in
            if now == .active { Task { await session.connect() } }
        }
        .onChange(of: opened) { _, now in
            // Driving a pane: its tab is the one to come back to. Back from
            // it: watch that tab again, at the tab's size.
            if let now { tabID = now.tab } else { session.rewatch() }
        }
        .onDisappear { if opened == nil { session.disconnect() } }
        .fullScreenCover(item: $opened) { pane in
            TerminalScreen(session: session, pane: pane)
        }
    }

    private var newTerminal: some View {
        Button {
            Task {
                do {
                    pendingPane = try await session.newTerminal()
                    openPending()
                } catch {
                    problem = describe(error)
                }
            }
        } label: {
            Label("New terminal", systemImage: "plus")
                .font(Face.mono(13, "Bold"))
                .foregroundStyle(session.connected ? Palette.void : Palette.dim)
                .padding(.horizontal, 14)
                .padding(.vertical, 9)
                .background(session.connected ? Palette.accent : Palette.raised, in: RoundedRectangle(cornerRadius: 8))
        }
        .disabled(!session.connected)
        .accessibilityIdentifier("new-terminal")
    }

    /// The pane cards: every terminal, the ones that want you first.
    private var cardView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if let p = session.problem ?? problem {
                    Text(p).font(Face.mono(12)).foregroundStyle(Palette.danger).accessibilityIdentifier("machine-problem")
                }
                newTerminal
                if session.connected {
                    Caps("\(session.panes.count) panes")
                }
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 260), spacing: 12)], spacing: 12) {
                    ForEach(session.panes.sorted { ($0.order, -($0.last_ms ?? 0)) < ($1.order, -($1.last_ms ?? 0)) }) { pane in
                        Button { opened = pane } label: {
                            PaneCard(pane: pane, machine: session.machine.name, conversation: session.conversations[pane.id])
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .padding()
        }
    }

    /// The machine's sessions as a rail (each with its tabs, and each tab with
    /// its panes and what they're doing), beside the tab picked, drawn whole
    /// and live, every pane where the machine puts it. Tap a pane to drive it
    /// full screen.
    private var tabView: some View {
        HStack(spacing: 0) {
            SessionRail(session: session, shown: shownTab?.id) { tabID = $0 }
                .frame(width: 300)
            Rectangle().fill(Palette.raised).frame(width: 1)
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    if let t = shownTab {
                        Text(session.name(of: t)).font(Face.ui(17, "SemiBold")).lineLimit(1)
                    }
                    Spacer()
                    newTerminal
                }
                if let p = session.problem ?? problem {
                    Text(p).font(Face.mono(12)).foregroundStyle(Palette.danger).accessibilityIdentifier("machine-problem")
                }
                if let t = shownTab {
                    TabCanvas(session: session, tab: t, conversations: session.conversations) { opened = $0 }
                } else if session.connected {
                    Empty(text: "No tabs", symbol: "rectangle.split.2x2")
                }
            }
            .padding()
        }
    }

    /// The tab picked, else the one whose pane most wants you, else the first.
    private var shownTab: TabLayout? {
        if let id = tabID, let t = session.tabs.first(where: { $0.id == id }) { return t }
        let urgent = session.panes.min { ($0.order, -($0.last_ms ?? 0)) < ($1.order, -($1.last_ms ?? 0)) }
        return session.tabs.first { $0.id == urgent?.tab } ?? session.tabs.first
    }

    /// The new pane's row, once the machine's state has it.
    private func openPending() {
        guard let want = pendingPane, let row = session.panes.first(where: { $0.id == want }) else { return }
        pendingPane = nil
        opened = row
    }
}

/// A machine's sessions, in order, each with its tabs: a tab by its name, a
/// dot in the tone of its most urgent pane, and its panes, each with what
/// it's doing and what it's called (its conversation, for an agent).
struct SessionRail: View {
    @ObservedObject var session: MachineSession
    let shown: UInt32?
    let pick: (UInt32) -> Void

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 18) {
                ForEach(groups, id: \.0) { _, name, project, tabs in
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Text(name.uppercased()).font(Face.display(18)).foregroundStyle(Palette.fg).lineLimit(1)
                            if let project { Caps(project) }
                        }
                        ForEach(tabs) { t in tabRow(t) }
                    }
                }
            }
            .padding(12)
        }
        .background(Palette.surface.opacity(0.5))
    }

    /// The sessions with their tabs, and any tab no session lists, last.
    private var groups: [(UInt32, String, String?, [TabLayout])] {
        let byID = Dictionary(session.tabs.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        var out = session.sessions.map { s in (s.id, s.name, s.project, s.tabs.compactMap { byID[$0] }) }
        let listed = Set(session.sessions.flatMap(\.tabs))
        let rest = session.tabs.filter { !listed.contains($0.id) }
        if !rest.isEmpty { out.append((UInt32.max, "Other tabs", nil, rest)) }
        return out.filter { !$0.3.isEmpty }
    }

    private func tabRow(_ t: TabLayout) -> some View {
        let panes = session.panes(in: t)
        let urgent = panes.first?.order ?? 3
        let tone: Tone = urgent == 0 ? .accent : urgent == 1 ? .info : urgent == 2 ? .ok : .muted
        let on = t.id == shown
        return Button { pick(t.id) } label: {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 7) {
                    Circle().fill(tone.color).frame(width: 7, height: 7)
                        .shadow(color: urgent == 0 ? tone.color : .clear, radius: 4)
                    Text(session.name(of: t)).font(Face.ui(14, "SemiBold")).foregroundStyle(on ? Palette.fg : Palette.subtle).lineLimit(1)
                    Spacer(minLength: 0)
                    Text(verbatim: "\(t.cells.count)").font(Face.mono(10)).foregroundStyle(Palette.dim)
                }
                if panes.count > 1 || urgent <= 1 {
                    ForEach(panes) { p in
                        HStack(spacing: 6) {
                            Text(word(p)).font(Face.mono(9, "Bold")).tracking(0.5).foregroundStyle(toneOf(p).color)
                                .frame(width: 58, alignment: .leading)
                            Text(session.conversations[p.id] ?? p.name).font(Face.ui(12)).foregroundStyle(Palette.dim).lineLimit(1)
                        }
                        .padding(.leading, 14)
                    }
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 7)
            .background(on ? Palette.accent.opacity(0.14) : .clear, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(on ? Palette.accent.opacity(0.6) : .clear, lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("tab-\(t.id)")
    }

    private func word(_ p: PaneRow) -> String {
        guard p.running else { return "STOPPED" }
        switch p.attention {
        case "needs_input": return p.reason?.kind == "failed" ? "FAILED" : "WANTS YOU"
        case "working": return "WORKING"
        case "done": return "DONE"
        default: return "IDLE"
        }
    }

    private func toneOf(_ p: PaneRow) -> Tone {
        guard p.running else { return .muted }
        switch p.attention {
        case "needs_input": return p.reason?.kind == "failed" ? .danger : .accent
        case "working": return .info
        case "done": return .ok
        default: return .muted
        }
    }
}

/// A tab drawn whole and live: every pane at its rectangle in the tab's grid,
/// at the tab's own size, in a font small enough for the tab to fit. Nothing
/// here resizes anyone's panes. A pane's edge takes the tone of what it's
/// doing; tap one to drive it.
struct TabCanvas: View {
    @ObservedObject var session: MachineSession
    let tab: TabLayout
    let conversations: [UInt32: String]
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
            let tone: Tone = row.order == 0 ? .accent : row.order == 1 ? .info : .muted
            WatchTerminal(session: session, tab: tab.id, pane: c.pane, cols: c.cols, rows: c.rows, size: size)
                .id("\(c.pane)-\(c.cols)x\(c.rows)@\(size)")
                .overlay(Rectangle().strokeBorder(tone.color.opacity(row.order <= 1 ? 0.9 : 0.35), lineWidth: row.order == 0 ? 2 : 1))
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

/// A pane as the swarm's rail shows one: whether it wants you, in that state's
/// tone, what it's called (an agent's conversation), why it wants you, and its
/// identity in mono ("%62 · claude · arugula ⎇ main"). Panes that want you or
/// are working glow.
struct PaneCard: View {
    let pane: PaneRow
    let machine: String
    /// The title of the Claude Code conversation running in it, if any.
    var conversation: String?

    private var state: (String, Tone) {
        if !pane.running { return ("Stopped", .muted) }
        switch pane.attention {
        case "needs_input": return (pane.reason?.kind == "failed" ? "Failed" : "Wants you", pane.reason?.kind == "failed" ? .danger : .accent)
        case "working": return ("Working", .info)
        case "done": return ("Done", .ok)
        default: return ("Idle", .muted)
        }
    }

    var body: some View {
        let (word, tone) = state
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(word.uppercased()).font(Face.mono(10, "Bold")).tracking(0.8).foregroundStyle(tone.color)
                Spacer(minLength: 0)
                if let ms = pane.last_ms, ms > 0 {
                    Text(Self.ago(ms)).font(Face.mono(10)).foregroundStyle(Palette.dim)
                }
            }
            Text(conversation ?? pane.name).font(Face.ui(16, "SemiBold")).foregroundStyle(Palette.fg).lineLimit(2)
            if let r = pane.reason, !r.headline.isEmpty {
                Text(r.headline).font(Face.ui(13)).foregroundStyle(Palette.subtle).lineLimit(2)
            }
            Text(meta).font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1).truncationMode(.middle)
        }
        .frame(maxWidth: .infinity, minHeight: 96, alignment: .topLeading)
        .card(tone, glow: pane.attention == "needs_input")
    }

    private var meta: String {
        var bits = ["%\(pane.id)"]
        if let p = pane.program { bits.append(p) }
        if let project = pane.project {
            bits.append(pane.branch.map { "\(project) ⎇ \($0)" } ?? project)
        } else if let cwd = pane.cwd {
            bits.append(cwd.split(separator: "/").last.map(String.init) ?? cwd)
        }
        return bits.joined(separator: " · ")
    }

    static func ago(_ ms: Int) -> String {
        let s = max(0, Int(Date().timeIntervalSince1970) - ms / 1000)
        switch s {
        case ..<60: return "now"
        case ..<3600: return "\(s / 60)m"
        case ..<86400: return "\(s / 3600)h"
        default: return "\(s / 86400)d"
        }
    }
}
