// A pane's terminal. libghostty-vt in the core keeps the screen, the same engine
// and pin as the daemon's, so it matches cell for cell; this draws its rows with
// CoreText, one CALayer each, redrawn only when dirty. Typing goes straight to
// the pane; special keys through libghostty's encoder, which knows the
// terminal's modes.

import CoreText
import SwiftUI
import UIKit

struct TerminalScreen: View {
    @ObservedObject var session: MachineSession
    let pane: PaneRow
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase

    var body: some View {
        NavigationStack {
            TerminalRepresentable(session: session, pane: pane)
                .ignoresSafeArea(edges: .bottom)
                .background(Color.black)
                .navigationTitle(pane.command ?? pane.tab_name ?? "Terminal")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done") { dismiss() }.accessibilityIdentifier("terminal-done")
                    }
                    ToolbarItem(placement: .status) {
                        if !session.connected { Text("Reconnecting…").font(.caption).foregroundStyle(.secondary) }
                    }
                }
        }
        .onChange(of: phase) { _, now in
            if now == .active { Task { await session.connect() } }
        }
    }
}

struct TerminalRepresentable: UIViewRepresentable {
    let session: MachineSession
    let pane: PaneRow

    func makeUIView(context: Context) -> TerminalUIView {
        let v = TerminalUIView(session: session, tab: pane.tab, pane: pane.id)
        session.screen = v
        return v
    }

    func updateUIView(_ v: TerminalUIView, context: Context) {}

    static func dismantleUIView(_ v: TerminalUIView, coordinator: ()) {
        if v.session.screen === v { v.session.screen = nil }
    }
}

private func cgColor(_ rgb: UInt32) -> CGColor {
    CGColor(
        srgbRed: CGFloat((rgb >> 16) & 0xff) / 255, green: CGFloat((rgb >> 8) & 0xff) / 255,
        blue: CGFloat(rgb & 0xff) / 255, alpha: 1)
}

final class TerminalUIView: UIView, UIKeyInput, UITextInputTraits {
    let session: MachineSession
    let tab: UInt32
    let pane: UInt32
    let term: OpaquePointer?
    private let font: CTFont
    private let bold: CTFont
    private let italic: CTFont
    private let cellW: CGFloat
    private let cellH: CGFloat
    private let descent: CGFloat
    private let pad: CGFloat
    /// Watching, not driving: the pane's own grid, drawn as it is, with no
    /// keyboard and no say in its size (a tab's overview). `nil`: driving.
    let fixed: (UInt16, UInt16)?
    private var grid: (UInt16, UInt16) = (0, 0)
    private var attached = false
    private var rowLayers: [CALayer] = []
    private var rowText: [String] = []
    private let cursor = CALayer()
    private var bg: UInt32 = 0
    private var ctrl = false
    private var ctrlButton: UIButton?
    private var scrollCarry: CGFloat = 0

    @MainActor init(session: MachineSession, tab: UInt32, pane: UInt32, fontSize: CGFloat = 12, fixed: (UInt16, UInt16)? = nil) {
        self.session = session
        self.tab = tab
        self.pane = pane
        self.fixed = fixed
        pad = fixed == nil ? 4 : 0
        term = aru_term_new(fixed?.0 ?? 80, fixed?.1 ?? 24)
        font = CTFontCreateWithName("Menlo" as CFString, fontSize, nil)
        bold = CTFontCreateCopyWithSymbolicTraits(font, fontSize, nil, .traitBold, .traitBold) ?? font
        italic = CTFontCreateCopyWithSymbolicTraits(font, fontSize, nil, .traitItalic, .traitItalic) ?? font
        var glyph = CGGlyph(0)
        var ch = UniChar(77)  // M
        CTFontGetGlyphsForCharacters(font, &ch, &glyph, 1)
        var adv = CGSize.zero
        CTFontGetAdvancesForGlyphs(font, .horizontal, &glyph, &adv, 1)
        cellW = adv.width
        cellH = ceil(CTFontGetAscent(font) + CTFontGetDescent(font) + CTFontGetLeading(font))
        descent = CTFontGetDescent(font)
        super.init(frame: .zero)
        backgroundColor = .black
        cursor.backgroundColor = CGColor(gray: 0.85, alpha: 0.65)
        cursor.actions = ["position": NSNull(), "bounds": NSNull(), "hidden": NSNull()]
        layer.addSublayer(cursor)
        isAccessibilityElement = true
        accessibilityIdentifier = "terminal-view"
        accessibilityLabel = "Terminal"
        accessibilityTraits = .allowsDirectInteraction
        if fixed == nil {
            addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped)))
            addGestureRecognizer(UIPanGestureRecognizer(target: self, action: #selector(panned)))
        } else {
            isUserInteractionEnabled = false
        }
    }

    /// One cell's size in Menlo at `size` points, as a terminal view lays it out.
    static func cell(size: CGFloat) -> CGSize {
        let f = CTFontCreateWithName("Menlo" as CFString, size, nil)
        var glyph = CGGlyph(0)
        var ch = UniChar(77)  // M
        CTFontGetGlyphsForCharacters(f, &ch, &glyph, 1)
        var adv = CGSize.zero
        CTFontGetAdvancesForGlyphs(f, .horizontal, &glyph, &adv, 1)
        return CGSize(width: adv.width, height: ceil(CTFontGetAscent(f) + CTFontGetDescent(f) + CTFontGetLeading(f)))
    }

    required init?(coder: NSCoder) { fatalError("made in code") }
    deinit { aru_term_free(term) }

    override var canBecomeFirstResponder: Bool { fixed == nil }
    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil, fixed == nil { _ = becomeFirstResponder() }
    }

    @objc private func tapped() { _ = becomeFirstResponder() }

    @objc private func panned(_ g: UIPanGestureRecognizer) {
        let dy = g.translation(in: self).y
        g.setTranslation(.zero, in: self)
        scrollCarry += dy
        let rows = Int(scrollCarry / cellH)
        if rows != 0 {
            scrollCarry -= CGFloat(rows) * cellH
            aru_term_scroll(term, Int32(-rows))
            render()
        }
    }

    // ---- size and the machine

    override func layoutSubviews() {
        super.layoutSubviews()
        if let f = fixed {
            // Watching: the pane's grid is the machine's, whatever the view's size.
            guard f != grid else { return }
            grid = f
            aru_term_resize(term, f.0, f.1, UInt32(cellW), UInt32(cellH))
            render()
            return
        }
        guard bounds.width > 40, bounds.height > 40 else { return }
        let cols = UInt16(max(10, Int((bounds.width - 2 * pad) / cellW)))
        let rows = UInt16(max(4, Int((bounds.height - 2 * pad) / cellH)))
        guard (cols, rows) != grid else { return }
        grid = (cols, rows)
        aru_term_resize(term, cols, rows, UInt32(cellW), UInt32(cellH))
        for l in rowLayers.dropFirst(Int(rows)) { l.contents = nil }
        MainActor.assumeIsolated {
            if let l = session.linkPointer {
                if attached {
                    aru_link_view(l, tab, pane, cols, rows)
                } else {
                    attached = true
                    aru_link_attach(l, tab, pane, cols, rows)
                }
            }
        }
        render()
    }

    /// A new channel after a reconnect: ask for the pane again, snapshot first.
    func linkOpened() {
        attached = false
        grid = (0, 0)
        setNeedsLayout()
    }

    // ---- drawing

    func render() {
        guard let term else { return }
        var frame = AruFrame()
        let me = Unmanaged.passUnretained(self).toOpaque()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        aru_term_render(term, me, { ctx, row, runs, n in
            let v = Unmanaged<TerminalUIView>.fromOpaque(ctx!).takeUnretainedValue()
            v.draw(row: Int(row), runs: UnsafeBufferPointer(start: runs, count: n))
        }, &frame)
        if frame.bg != bg {
            bg = frame.bg
            layer.backgroundColor = cgColor(bg)
        }
        cursor.isHidden = !frame.cursor_visible
        cursor.frame = CGRect(
            x: pad + CGFloat(frame.cursor_x) * cellW, y: pad + CGFloat(frame.cursor_y) * cellH, width: cellW, height: cellH)
        CATransaction.commit()
        accessibilityValue = rowText.prefix(Int(frame.rows)).joined(separator: "\n")
    }

    private func draw(row: Int, runs: UnsafeBufferPointer<AruRun>) {
        let scale = traitCollection.displayScale
        let width = bounds.width - 2 * pad
        while rowLayers.count <= row {
            let l = CALayer()
            l.actions = ["contents": NSNull(), "position": NSNull(), "bounds": NSNull()]
            l.contentsScale = scale
            l.anchorPoint = .zero
            layer.insertSublayer(l, below: cursor)
            rowLayers.append(l)
        }
        while rowText.count <= row { rowText.append("") }
        let l = rowLayers[row]
        l.frame = CGRect(x: pad, y: pad + CGFloat(row) * cellH, width: width, height: cellH)
        var line = ""
        guard width > 0,
            let ctx = CGContext(
                data: nil, width: Int(width * scale), height: Int(cellH * scale), bitsPerComponent: 8, bytesPerRow: 0,
                space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return }
        ctx.scaleBy(x: scale, y: scale)
        for r in runs {
            let x = CGFloat(r.col) * cellW
            if r.bg != UInt32.max {
                ctx.setFillColor(cgColor(r.bg))
                ctx.fill(CGRect(x: x, y: 0, width: CGFloat(r.cells) * cellW, height: cellH))
            }
            guard r.text_len > 0, let p = r.text else { continue }
            let s = String(decoding: UnsafeBufferPointer(start: p, count: r.text_len), as: UTF8.self)
            line += s
            if s.allSatisfy({ $0 == " " }) { continue }
            let f = r.flags & 1 != 0 ? bold : (r.flags & 2 != 0 ? italic : font)
            let attr = NSAttributedString(
                string: s,
                attributes: [
                    NSAttributedString.Key(kCTFontAttributeName as String): f,
                    NSAttributedString.Key(kCTForegroundColorAttributeName as String): cgColor(r.fg),
                ])
            let ct = CTLineCreateWithAttributedString(attr)
            ctx.textPosition = CGPoint(x: x, y: descent)
            CTLineDraw(ct, ctx)
        }
        l.contents = ctx.makeImage()
        rowText[row] = line.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression)
    }

    // ---- input

    private func send(_ bytes: [UInt8]) {
        guard !bytes.isEmpty else { return }
        MainActor.assumeIsolated {
            guard let l = session.linkPointer else { return }
            aru_term_scroll_bottom(term)
            bytes.withUnsafeBufferPointer { aru_link_input(l, pane, $0.baseAddress, $0.count) }
        }
    }

    private func key(_ code: UInt32, mods: UInt32 = 0) {
        var out = [UInt8](repeating: 0, count: 64)
        let n = aru_term_key(term, code, mods, nil, &out, out.count)
        send(Array(out[..<n]))
    }

    var hasText: Bool { true }

    func insertText(_ text: String) {
        if text == "\n" {
            key(3)
            return
        }
        if ctrl, let c = text.uppercased().unicodeScalars.first, c.value >= 65, c.value <= 90 {
            setCtrl(false)
            key(c.value, mods: 2)
            return
        }
        setCtrl(false)
        send(Array(text.utf8))
    }

    func deleteBackward() { key(4) }

    /// Command-V from a hardware keyboard, as on an iPad.
    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        action == #selector(paste(_:)) ? UIPasteboard.general.hasStrings : super.canPerformAction(action, withSender: sender)
    }

    override func paste(_ sender: Any?) {
        if let s = UIPasteboard.general.string { send(Array(s.utf8)) }
    }

    var autocorrectionType: UITextAutocorrectionType { get { .no } set {} }
    var autocapitalizationType: UITextAutocapitalizationType { get { .none } set {} }
    var spellCheckingType: UITextSpellCheckingType { get { .no } set {} }
    var smartQuotesType: UITextSmartQuotesType { get { .no } set {} }
    var smartDashesType: UITextSmartDashesType { get { .no } set {} }
    var smartInsertDeleteType: UITextSmartInsertDeleteType { get { .no } set {} }
    var keyboardAppearance: UIKeyboardAppearance { get { .dark } set {} }
    var returnKeyType: UIReturnKeyType { get { .default } set {} }

    private func setCtrl(_ on: Bool) {
        ctrl = on
        ctrlButton?.configuration?.baseForegroundColor = on ? .systemGreen : .label
    }

    /// The keys an iPhone's or iPad's on-screen keyboard hasn't got.
    override var inputAccessoryView: UIView? {
        if let bar { return bar }
        let v = UIInputView(frame: CGRect(x: 0, y: 0, width: 0, height: 44), inputViewStyle: .keyboard)
        // The keys in the theme's accent and mono face (Theme.swift).
        v.tintColor = UIColor(Palette.accent)
        let scroll = UIScrollView()
        scroll.showsHorizontalScrollIndicator = false
        scroll.translatesAutoresizingMaskIntoConstraints = false
        let stack = UIStackView()
        stack.axis = .horizontal
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        func button(_ title: String, _ id: String, _ act: @escaping () -> Void) -> UIButton {
            var c = UIButton.Configuration.gray()
            c.attributedTitle = AttributedString(title, attributes: AttributeContainer([.font: Face.uiFont("JetBrainsMono-Regular", 14, fallback: .regular)]))
            c.contentInsets = .init(top: 6, leading: 10, bottom: 6, trailing: 10)
            let b = UIButton(configuration: c, primaryAction: UIAction { _ in act() })
            b.accessibilityIdentifier = "key-\(id)"
            stack.addArrangedSubview(b)
            return b
        }
        _ = button("esc", "esc") { [weak self] in self?.key(1) }
        _ = button("tab", "tab") { [weak self] in self?.key(2) }
        ctrlButton = button("ctrl", "ctrl") { [weak self] in self?.setCtrl(!(self?.ctrl ?? false)) }
        _ = button("↑", "up") { [weak self] in self?.key(5) }
        _ = button("↓", "down") { [weak self] in self?.key(6) }
        _ = button("←", "left") { [weak self] in self?.key(7) }
        _ = button("→", "right") { [weak self] in self?.key(8) }
        for s in ["/", "-", "|", "~"] {
            _ = button(s, s) { [weak self] in self?.send(Array(s.utf8)) }
        }
        _ = button("paste", "paste") { [weak self] in
            if let s = UIPasteboard.general.string { self?.send(Array(s.utf8)) }
        }
        v.addSubview(scroll)
        scroll.addSubview(stack)
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: v.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: v.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: v.topAnchor),
            scroll.bottomAnchor.constraint(equalTo: v.bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor, constant: 8),
            stack.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor, constant: -8),
            stack.centerYAnchor.constraint(equalTo: scroll.centerYAnchor),
        ])
        bar = v
        return v
    }
    private var bar: UIView?

    /// A hardware keyboard's keys with no text.
    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var rest = Set<UIPress>()
        for p in presses {
            guard let k = p.key else {
                rest.insert(p)
                continue
            }
            let mods: UInt32 = (k.modifierFlags.contains(.shift) ? 1 : 0) | (k.modifierFlags.contains(.control) ? 2 : 0)
                | (k.modifierFlags.contains(.alternate) ? 4 : 0)
            switch k.keyCode {
            case .keyboardEscape: key(1, mods: mods)
            case .keyboardUpArrow: key(5, mods: mods)
            case .keyboardDownArrow: key(6, mods: mods)
            case .keyboardLeftArrow: key(7, mods: mods)
            case .keyboardRightArrow: key(8, mods: mods)
            case .keyboardTab: key(2, mods: mods)
            case .keyboardPageUp: key(11, mods: mods)
            case .keyboardPageDown: key(12, mods: mods)
            default:
                if k.modifierFlags.contains(.control), let c = k.charactersIgnoringModifiers.uppercased().unicodeScalars.first,
                    c.value >= 65, c.value <= 90 {
                    key(c.value, mods: mods)
                } else {
                    rest.insert(p)
                }
            }
        }
        if !rest.isEmpty { super.pressesBegan(rest, with: event) }
    }
}
