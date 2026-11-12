// Chat: the current project's team's channels and DMs (every room in the
// personal project), and one room, read the way a team chat reads:
// an avatar beside each run of messages, the name and time on one line, the
// text tight under it, and a long message folded. Issues', panes' and
// sessions' threads aren't listed here. No live stream yet, so an open room
// looks again every few seconds.

import SwiftUI

struct ChatRoomsView: View {
    let control: Control
    @EnvironmentObject private var scope: Scope
    @State private var picked: ChatRoom?
    @State private var columns = NavigationSplitViewVisibility.all

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            // Beside an open room, the list keeps its counts current.
            Loaded(load: { try await control.get("/api/chat", as: ChatRooms.self) }, every: .seconds(10)) { r in
                let rooms = r.rooms.filter { $0.isConversation && !$0.archived && scope.showsRoom($0) }.sorted { ($0.unread > 0 ? 1 : 0, $0.last) > ($1.unread > 0 ? 1 : 0, $1.last) }
                let channels = rooms.filter { $0.kind == "channel" }
                let dms = rooms.filter { $0.kind == "dm" }
                List(selection: $picked) {
                    if !channels.isEmpty {
                        Section { ForEach(channels) { RoomRow(room: $0, open: $0 == picked) } } header: { Caps("Channels") }
                    }
                    if !dms.isEmpty {
                        Section { ForEach(dms) { RoomRow(room: $0, open: $0 == picked) } } header: { Caps("Direct messages") }
                    }
                }
                .voidList()
                .overlay { if rooms.isEmpty { Caps("No rooms yet") } }
            }
            .navigationTitle("CHAT")
            .background(Palette.void)
        } detail: {
            NavigationStack {
                if let picked {
                    ChatRoomView(control: control, room: picked)
                } else {
                    Empty(text: "Pick a room", symbol: "bubble.left.and.bubble.right")
                }
            }
            .id(picked?.id)
        }
        .navigationSplitViewStyle(.balanced)
        .onChange(of: scope.current?.id) { _, _ in picked = nil }
    }
}

/// A room in the list: a channel's # or a DM's avatar, its name, and how many
/// messages wait in it.
struct RoomRow: View {
    let room: ChatRoom
    let open: Bool

    var body: some View {
        NavigationLink(value: room) {
            HStack(spacing: 8) {
                if room.kind == "channel" {
                    Text("#").font(Face.mono(15, "Bold")).foregroundStyle(Palette.dim).frame(width: 20)
                } else {
                    Avatar(name: room.title, size: 20)
                }
                Text(room.kind == "channel" ? room.name : room.title)
                    .font(Face.ui(15, room.unread > 0 && !open ? "Bold" : "Regular"))
                    .foregroundStyle(room.unread > 0 && !open ? Palette.fg : Palette.subtle)
                    .lineLimit(1)
                Spacer()
                if room.unread > 0 && !open {
                    Text(verbatim: "\(room.unread)")
                        .font(Face.mono(11, "Bold"))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(room.mentions > 0 ? Palette.danger : Palette.accent, in: Capsule())
                        .foregroundStyle(Palette.void)
                }
            }
        }
        .voidRow()
        .listRowSeparator(.hidden)
        .accessibilityIdentifier("room-\(room.name)")
    }
}

struct ChatRoomView: View {
    let control: Control
    let room: ChatRoom
    @State private var messages: [ChatMessage] = []
    @State private var draft = ""
    @State private var problem: String?
    @State private var sending = false

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    let shown = messages.filter { $0.deleted == nil }
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(shown.enumerated()), id: \.element.seq) { i, m in
                            if i == 0 || !Calendar.current.isDate(shown[i - 1].date, inSameDayAs: m.date) {
                                DayRule(date: m.date)
                            }
                            MessageView(message: m, continues: i > 0 && continues(shown[i - 1], m))
                                .id(m.seq)
                        }
                    }
                    // A readable line on an iPad's width.
                    .frame(maxWidth: 820, alignment: .leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 8)
                }
                .onChange(of: messages.last?.seq) { _, last in
                    if let last { withAnimation { proxy.scrollTo(last, anchor: .bottom) } }
                }
            }
            if let problem {
                Text(problem).font(Face.mono(12)).foregroundStyle(Palette.danger).padding(.horizontal)
            }
            if room.writable {
                HStack(alignment: .bottom, spacing: 8) {
                    TextField("Message \(room.title)", text: $draft, axis: .vertical)
                        .lineLimit(1...6)
                        .font(Face.ui(16))
                        .padding(.horizontal, 12)
                        .padding(.vertical, 9)
                        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 10))
                        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Palette.raised, lineWidth: 1))
                        .accessibilityIdentifier("chat-draft")
                    Button {
                        Task { await send() }
                    } label: {
                        Image(systemName: "arrow.up").font(.system(size: 16, weight: .bold))
                            .foregroundStyle(Palette.void)
                            .frame(width: 38, height: 38)
                            .background(empty ? Palette.muted : Palette.accent, in: RoundedRectangle(cornerRadius: 10))
                    }
                    .disabled(empty || sending)
                    .accessibilityLabel("Send")
                    .accessibilityIdentifier("chat-send")
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(Palette.bg)
            }
        }
        .background(Palette.bg)
        .detailTitle(room.title)
        .task {
            while !Task.isCancelled {
                await load()
                try? await Task.sleep(for: .seconds(4))
            }
        }
    }

    private var empty: Bool { draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    /// The same person (or agent) again within five minutes, on the same day:
    /// no avatar, name and time over it.
    private func continues(_ a: ChatMessage, _ b: ChatMessage) -> Bool {
        a.from == b.from && a.author == b.author && b.at - a.at < 5 * 60 * 1000
            && Calendar.current.isDate(a.date, inSameDayAs: b.date)
    }

    private func load() async {
        do {
            let page = try await control.get("/api/chat/rooms/\(room.id)/messages", as: ChatMessages.self)
            messages = page.messages.sorted { $0.seq < $1.seq }
            problem = nil
            if let last = messages.last?.seq {
                _ = try? await control.call("POST", "/api/chat/rooms/\(room.id)/read", body: ["seq": last])
            }
        } catch is CancellationError {
        } catch {
            problem = describe(error)
        }
    }

    private func send() async {
        sending = true
        defer { sending = false }
        do {
            _ = try await control.call("POST", "/api/chat/rooms/\(room.id)/messages", body: ["text": draft])
            draft = ""
            await load()
        } catch {
            problem = describe(error)
        }
    }
}

/// A day's line between messages.
struct DayRule: View {
    let date: Date

    var body: some View {
        HStack(spacing: 10) {
            Rectangle().fill(Palette.raised).frame(height: 1)
            Caps(Calendar.current.isDateInToday(date) ? "Today" : date.formatted(.dateTime.weekday(.wide).month(.wide).day()))
                .fixedSize()
            Rectangle().fill(Palette.raised).frame(height: 1)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }
}

/// Someone's square: their initials on a color their name picks, the same
/// every time. An agent's has a chip in the corner.
struct Avatar: View {
    let name: String
    var size: CGFloat = 36
    var agent = false

    private static let tones: [Color] = [
        Palette.accent, Palette.info, Palette.ok, Palette.peach, Palette.teal, Palette.pink, Palette.warn, Palette.merged,
    ]

    var body: some View {
        let words = name.split(whereSeparator: { $0 == " " || $0 == "-" || $0 == "_" || $0 == "." })
        let initials = words.prefix(2).compactMap(\.first).map(String.init).joined().uppercased()
        let hash = name.unicodeScalars.reduce(UInt32(5381)) { ($0 &* 33) &+ $1.value }
        RoundedRectangle(cornerRadius: size * 0.22)
            .fill(Avatar.tones[Int(hash % UInt32(Avatar.tones.count))])
            .frame(width: size, height: size)
            .overlay {
                Text(initials.isEmpty ? "?" : initials)
                    .font(Face.display(size * 0.5, "Black"))
                    .foregroundStyle(Palette.void)
            }
            .overlay(alignment: .bottomTrailing) {
                if agent {
                    Image(systemName: "cpu")
                        .font(.system(size: size * 0.3, weight: .bold))
                        .foregroundStyle(Palette.fg)
                        .padding(2)
                        .background(Palette.bg, in: RoundedRectangle(cornerRadius: 4))
                        .offset(x: 4, y: 4)
                }
            }
    }
}

/// One message, as a team chat shows it: the avatar beside the first of a run
/// with the name and time on one line, then the text, tight. A long message is
/// folded under "Show more".
struct MessageView: View {
    let message: ChatMessage
    let continues: Bool
    @State private var expanded = false

    /// How much shows before "Show more".
    private static let foldLines = 12
    private static let foldChars = 900

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            if continues {
                Color.clear.frame(width: 36, height: 1)
            } else {
                Avatar(name: message.name.isEmpty ? message.from : message.name, agent: message.agent != nil)
                    .padding(.top, 2)
            }
            VStack(alignment: .leading, spacing: 3) {
                if !continues {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(message.name.isEmpty ? message.from : message.name).font(Face.ui(15, "Bold"))
                        if let agent = message.agent {
                            Text("AGENT").font(Face.mono(9, "Bold")).tracking(0.6).foregroundStyle(Palette.teal)
                                .padding(.horizontal, 4).padding(.vertical, 1)
                                .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(Palette.teal.opacity(0.6), lineWidth: 1))
                            Text("on \(agent.machine_name)").font(Face.mono(11)).foregroundStyle(Palette.dim)
                        } else if let via = message.via, !via.isEmpty {
                            Text("via \(via)").font(Face.mono(11)).foregroundStyle(Palette.dim)
                        }
                        Text(message.date.formatted(date: .omitted, time: .shortened))
                            .font(Face.mono(11)).foregroundStyle(Palette.dim)
                    }
                }
                if let quote = message.quote, !quote.isEmpty {
                    CodeBlock(text: quote)
                }
                if !message.text.isEmpty {
                    ChatText(text: folded ? Self.fold(message.text) : message.text)
                    if long {
                        Button(expanded ? "Show less" : "Show more") { expanded.toggle() }
                            .font(Face.mono(12, "Bold"))
                            .foregroundStyle(Palette.accent)
                            .buttonStyle(.plain)
                    }
                }
                if message.edited != nil {
                    Text("(edited)").font(Face.mono(10)).foregroundStyle(Palette.muted)
                }
                if let files = message.files, !files.isEmpty {
                    ForEach(Array(files.enumerated()), id: \.offset) { _, f in
                        Label(f.name, systemImage: "paperclip").font(Face.mono(12)).foregroundStyle(Palette.subtle)
                    }
                }
                if let reactions = message.reactions, !reactions.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(Array(reactions.enumerated()), id: \.offset) { _, r in
                            Text(verbatim: "\(r.emoji) \(r.count)")
                                .font(Face.mono(12))
                                .padding(.horizontal, 7)
                                .padding(.vertical, 2)
                                .background(r.mine ? Palette.accent.opacity(0.18) : Palette.raised.opacity(0.6), in: Capsule())
                                .overlay(Capsule().strokeBorder(r.mine ? Palette.accent.opacity(0.7) : .clear, lineWidth: 1))
                        }
                    }
                    .padding(.top, 2)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 16)
        .padding(.top, continues ? 2 : 10)
    }

    private var long: Bool {
        message.text.count > Self.foldChars || message.text.split(separator: "\n", omittingEmptySubsequences: false).count > Self.foldLines
    }
    private var folded: Bool { long && !expanded }

    /// The first lines of a long message, cut short when they run long, with
    /// any code block it cuts into closed.
    static func fold(_ text: String) -> String {
        var out = text.split(separator: "\n", omittingEmptySubsequences: false).prefix(foldLines).joined(separator: "\n")
        if out.count > foldChars { out = String(out.prefix(foldChars)) }
        let fences = out.split(separator: "\n", omittingEmptySubsequences: false)
            .filter { $0.trimmingCharacters(in: .whitespaces).hasPrefix("```") }.count
        return out + (fences % 2 == 1 ? "\n```" : "") + " …"
    }
}

/// A message's text with the chat's small Markdown, as the web shows it:
/// fenced code as a block, `code`, **bold**, *italic*, links; headings and
/// bullets as bold lines and dots. Nothing in a code block is formatted.
struct ChatText: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(ChatText.parts(text).enumerated()), id: \.offset) { _, part in
                switch part {
                case .code(let s): CodeBlock(text: s)
                case .prose(let s):
                    Text(ChatText.inline(s))
                        .font(Face.ui(15))
                        .lineSpacing(2)
                        .foregroundStyle(Palette.fg)
                        .textSelection(.enabled)
                }
            }
        }
    }

    enum Part { case prose(String), code(String) }

    static func parts(_ text: String) -> [Part] {
        var out: [Part] = []
        var prose: [Substring] = []
        var code: [Substring]?
        func flushProse() {
            let s = prose.joined(separator: "\n").trimmingCharacters(in: .newlines)
            if !s.isEmpty { out.append(.prose(s)) }
            prose = []
        }
        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                if let c = code {
                    out.append(.code(c.joined(separator: "\n")))
                    code = nil
                } else {
                    flushProse()
                    code = []
                }
            } else if code != nil {
                code!.append(line)
            } else {
                prose.append(line)
            }
        }
        if let c = code { out.append(.code(c.joined(separator: "\n"))) }  // never closed
        flushProse()
        return out
    }

    /// Inline Markdown, keeping the lines; a heading becomes a bold line and
    /// a bullet a dot. `code` is in the mono face on a raised fill, and links
    /// take the accent.
    static func inline(_ s: String) -> AttributedString {
        let lines = s.split(separator: "\n", omittingEmptySubsequences: false).map { raw -> String in
            let line = String(raw)
            let t = line.drop { $0 == " " }
            if t.hasPrefix("#") {
                let title = t.drop { $0 == "#" }.trimmingCharacters(in: .whitespaces)
                return title.isEmpty ? line : "**\(title)**"
            }
            for bullet in ["- ", "* ", "+ "] where t.hasPrefix(bullet) {
                let indent = String(repeating: " ", count: line.count - t.count)
                return indent + "• " + t.dropFirst(2)
            }
            return line
        }
        let joined = lines.joined(separator: "\n")
        var out = (try? AttributedString(markdown: joined, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(joined)
        for run in out.runs {
            if run.inlinePresentationIntent?.contains(.code) == true {
                out[run.range].font = Face.mono(13)
                out[run.range].backgroundColor = Palette.raised
                out[run.range].foregroundColor = Palette.peach
            }
            if run.link != nil { out[run.range].foregroundColor = Palette.accent }
        }
        return out
    }
}

/// Lines shown as they are: monospaced, scrolling sideways when long.
struct CodeBlock: View {
    let text: String

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Text(text)
                .font(Face.mono(13))
                .foregroundStyle(Palette.fg)
                .textSelection(.enabled)
                .fixedSize(horizontal: true, vertical: false)
                .padding(10)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Palette.void, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Palette.raised, lineWidth: 1))
    }
}
