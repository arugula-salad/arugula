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
    private var link: OpaquePointer?
    private var conversationsAt = Date.distantPast
    /// The terminal on screen, which takes that pane's bytes.
    weak var screen: TerminalUIView?

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
        let flags = aru_link_feed(l, screen?.term, screen?.pane ?? 0)
        if flags & 2 != 0, let s = takeString(aru_link_panes(l)),
            let list = try? JSONDecoder().decode(PaneList.self, from: Data(s.utf8)) {
            panes = list.panes
            // A pane's conversation changes rarely: look again at most every 20 seconds.
            if Date().timeIntervalSince(conversationsAt) > 20 {
                conversationsAt = Date()
                Task { await loadConversations() }
            }
        }
        if flags & 1 != 0 { screen?.render() }
        if flags & 8 != 0, let said = takeString(aru_link_said(l)) { problem = said }
        if flags & 4 != 0 {
            let why = takeString(aru_link_closed(l))
            disconnect()
            if why != "closed" { problem = why }
        }
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
            .navigationTitle("MACHINES")
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
    }
}

struct MachineView: View {
    @StateObject private var session: MachineSession
    @State private var opened: PaneRow?
    @State private var pendingPane: UInt32?
    @State private var problem: String?
    @Environment(\.scenePhase) private var phase

    init(control: Control, machine: Machine) {
        _session = StateObject(wrappedValue: MachineSession(control: control, machine: machine))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if let p = session.problem ?? problem {
                    Text(p).font(Face.mono(12)).foregroundStyle(Palette.danger).accessibilityIdentifier("machine-problem")
                }
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
        .background(Palette.void)
        .overlay {
            if !session.connected && session.problem == nil {
                ProgressView { Caps("Connecting") }.tint(Palette.accent)
            }
        }
        .detailTitle(session.machine.name)
        .task { await session.connect() }
        .onChange(of: session.panes) { _, _ in openPending() }
        .onChange(of: phase) { _, now in
            if now == .active { Task { await session.connect() } }
        }
        .onDisappear { if opened == nil { session.disconnect() } }
        .fullScreenCover(item: $opened) { pane in
            TerminalScreen(session: session, pane: pane)
        }
    }

    /// The new pane's row, once the machine's state has it.
    private func openPending() {
        guard let want = pendingPane, let row = session.panes.first(where: { $0.id == want }) else { return }
        pendingPane = nil
        opened = row
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
