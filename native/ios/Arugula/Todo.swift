// A to-do (Arugula #1470): work an agent hands its person because only they
// can do it (an approval on GitHub, a script with their credentials). It's
// a row on home's Needs you and in the Inbox that expands in place: why the
// agent can't do it, how (its steps as written, in a mono well, with Copy),
// a note, and Done or Can't. The answer is `POST /api/inbox/{id}` with the
// choice and the note (InboxModel.answer, which an approval's row takes
// too); it wakes the agent that asked. The note goes in that request and
// nowhere else: it isn't kept here once it's sent.

import SwiftUI

extension InboxModel.Item {
    /// An agent's to-do, answered Done or Can't.
    var isTodo: Bool { answer?.via == "todo" }

    /// Why the agent can't do it: its `Why: …` line, without the label.
    var why: String? {
        lines?.first { $0.hasPrefix("Why: ") }.map { String($0.dropFirst(5)) }
    }

    /// How: its other lines, its steps with their newlines, as written.
    var how: String? {
        let rest = (lines ?? []).filter { !$0.hasPrefix("Why: ") }
        let how = rest.joined(separator: "\n").trimmingCharacters(in: .newlines)
        return how.isEmpty ? nil : how
    }
}

/// A to-do expanded: its why, its how with Copy, a note, Done and Can't.
/// `done` says which was sent, and the row clears; a refusal (a bad choice,
/// someone else's or gone, already answered) shows the server's words.
struct TodoAnswer: View {
    let item: InboxModel.Item
    let control: Control
    let done: (String) -> Void
    @ObservedObject private var inbox = InboxModel.shared
    @State private var note = ""
    @State private var busy = false
    @State private var problem: String?
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let why = item.why {
                Text(why).font(Face.ui(14)).foregroundStyle(Palette.subtle)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("todo-why")
            }
            if let how = item.how { well(how) }
            TextField("A note (optional)", text: $note, axis: .vertical)
                .font(Face.ui(14))
                .lineLimit(1...4)
                .autocorrectionDisabled()
                .padding(.horizontal, 10).padding(.vertical, 6)
                .background(Palette.surface, in: RoundedRectangle(cornerRadius: 8))
                .accessibilityIdentifier("todo-note")
            Text("Your agent reads the note: no passwords or keys.")
                .font(Face.mono(11)).foregroundStyle(Palette.dim)
            FlowRow(spacing: 6) {
                let options = item.answer?.options ?? []
                ForEach(Array((options.isEmpty ? ["Done", "Can't"] : options).enumerated()), id: \.offset) { i, c in
                    Button(c) { send(c) }
                        .buttonStyle(AnswerButton(primary: i == 0))
                        .accessibilityIdentifier("todo-\(c)")
                }
            }
            if let problem {
                Text(problem).font(Face.mono(11)).foregroundStyle(Palette.danger).lineLimit(3)
                    .accessibilityIdentifier("todo-problem")
            }
        }
        .disabled(busy)
        .opacity(busy ? 0.5 : 1)
    }

    /// The steps in a mono well, with Copy.
    private func well(_ how: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(verbatim: how)
                .font(Face.mono(12))
                .foregroundStyle(Palette.fg)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            HStack {
                Spacer()
                Button(copied ? "Copied" : "Copy") {
                    UIPasteboard.general.string = how
                    copied = true
                    Task {
                        try? await Task.sleep(for: .seconds(1.5))
                        copied = false
                    }
                }
                .font(Face.mono(11, "Bold"))
                .foregroundStyle(Palette.subtle)
                .buttonStyle(.plain)
                .accessibilityIdentifier("todo-copy")
            }
        }
        .padding(10)
        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Palette.raised, lineWidth: 1))
        .accessibilityIdentifier("todo-how")
    }

    private func send(_ choice: String) {
        busy = true
        problem = nil
        Task {
            do {
                try await inbox.answer(control, item: item.id, choice: choice, note: note.trimmingCharacters(in: .whitespacesAndNewlines))
                note = ""
                done(choice)
            } catch {
                problem = describe(error)
            }
            busy = false
        }
    }
}
