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
    private var link: OpaquePointer?
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
        }
        if flags & 1 != 0 { screen?.render() }
        if flags & 8 != 0, let said = takeString(aru_link_said(l)) { problem = said }
        if flags & 4 != 0 {
            let why = takeString(aru_link_closed(l))
            disconnect()
            if why != "closed" { problem = why }
        }
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
                    ForEach(session.panes) { pane in
                        Button { opened = pane } label: { PaneCard(pane: pane, machine: session.machine.name) }
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

/// A pane as the swarm's rail shows one: its identity in mono ("%62 · tab 3 ·
/// geek"), what runs in it in the display face, and where. Running panes take
/// the ok tone and glow.
struct PaneCard: View {
    let pane: PaneRow
    let machine: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("%\(pane.id) · \(pane.tab_name ?? "tab \(pane.tab)") · \(machine)")
                .font(Face.mono(11)).foregroundStyle(Palette.dim).lineLimit(1)
            Text((pane.command ?? "shell").uppercased())
                .font(Face.display(22)).foregroundStyle(pane.running ? Palette.ok : Palette.subtle).lineLimit(1)
            if let cwd = pane.cwd {
                Text(cwd).font(Face.mono(12)).foregroundStyle(Palette.subtle).lineLimit(1).truncationMode(.head)
            }
        }
        .card(pane.running ? .ok : .muted, glow: pane.running)
    }
}
