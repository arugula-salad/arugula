// Arugula's look, as the web draws it (web/src/shared/style.css and the
// swarm's rules): Catppuccin Mocha on a near-black void, Big Shoulders Display
// for titles and numbers, Instrument Sans for text, JetBrains Mono for labels
// and ids, and cards tinted by a tone with a bar down their left edge.

import CoreText
import SwiftUI
import UIKit

enum Palette {
    static let void = Color(hex: 0x11111b)
    static let bg = Color(hex: 0x1e1e2e)
    static let surface = Color(hex: 0x181825)
    static let raised = Color(hex: 0x313244)
    static let fg = Color(hex: 0xcdd6f4)
    static let subtle = Color(hex: 0xa6adc8)
    static let dim = Color(hex: 0x7f849c)
    static let muted = Color(hex: 0x585b70)
    static let accent = Color(hex: 0xcba6f7)
    static let ok = Color(hex: 0xa6e3a1)
    static let warn = Color(hex: 0xf9e2af)
    static let danger = Color(hex: 0xf38ba8)
    static let info = Color(hex: 0x89b4fa)
    static let peach = Color(hex: 0xfab387)
    static let teal = Color(hex: 0x94e2d5)
    static let pink = Color(hex: 0xf5c2e7)
    static let merged = Color(hex: 0xb4befe)
}

extension Color {
    init(hex: UInt32) {
        self.init(
            .sRGB, red: Double((hex >> 16) & 0xff) / 255, green: Double((hex >> 8) & 0xff) / 255,
            blue: Double(hex & 0xff) / 255)
    }
}

/// The three faces, by their named weights. Until the fonts are registered
/// (a build without them), each falls back to the system's.
enum Face {
    static func display(_ size: CGFloat, _ weight: String = "ExtraBold") -> Font {
        .custom("BigShouldersDisplay-\(weight)", size: size)
    }
    static func ui(_ size: CGFloat, _ weight: String = "Regular", relativeTo style: Font.TextStyle = .body) -> Font {
        .custom("InstrumentSans-\(weight)", size: size, relativeTo: style)
    }
    static func mono(_ size: CGFloat, _ weight: String = "Regular", relativeTo style: Font.TextStyle = .caption) -> Font {
        .custom(weight == "Regular" ? "JetBrainsMono-Regular" : "JetBrainsMonoRoman-\(weight)", size: size, relativeTo: style)
    }

    static func uiFont(_ name: String, _ size: CGFloat, fallback: UIFont.Weight) -> UIFont {
        UIFont(name: name, size: size) ?? .systemFont(ofSize: size, weight: fallback)
    }

    /// The fonts build.sh put in the bundle.
    static func register() {
        for url in Bundle.main.urls(forResourcesWithExtension: "ttf", subdirectory: nil) ?? [] {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
    }
}

/// A card's tone, as the web's cards and the swarm's take them.
enum Tone {
    case accent, ok, warn, danger, info, muted, peach, teal

    var color: Color {
        switch self {
        case .accent: Palette.accent
        case .ok: Palette.ok
        case .warn: Palette.warn
        case .danger: Palette.danger
        case .info: Palette.info
        case .muted: Palette.muted
        case .peach: Palette.peach
        case .teal: Palette.teal
        }
    }
}

/// A card: the surface tinted by its tone, a 1px edge in it, and a 3px bar
/// down the left. Live work glows.
struct ToneCard: ViewModifier {
    var tone: Tone
    var glow = false

    func body(content: Content) -> some View {
        content
            .padding(.vertical, 10)
            .padding(.leading, 14)
            .padding(.trailing, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background {
                RoundedRectangle(cornerRadius: 10)
                    .fill(Palette.surface)
                    .overlay(RoundedRectangle(cornerRadius: 10).fill(tone.color.opacity(0.07)))
            }
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(tone.color.opacity(0.45), lineWidth: 1))
            .overlay(alignment: .leading) {
                UnevenRoundedRectangle(bottomTrailingRadius: 3, topTrailingRadius: 3)
                    .fill(tone.color)
                    .frame(width: 3)
                    .padding(.vertical, 8)
            }
            .shadow(color: glow ? tone.color.opacity(0.45) : .clear, radius: glow ? 14 : 0)
    }
}

extension View {
    func card(_ tone: Tone, glow: Bool = false) -> some View { modifier(ToneCard(tone: tone, glow: glow)) }

    /// A list on the void, its rows on no system card.
    func voidList() -> some View {
        listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(Palette.void)
    }

    /// A row of a void list: clear behind it, a hairline between.
    func voidRow() -> some View {
        listRowBackground(Color.clear).listRowSeparatorTint(Palette.raised)
    }
}

/// A section's label, as the swarm's: small mono capitals, dim.
struct Caps: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text.uppercased())
            .font(Face.mono(11, "Medium"))
            .tracking(1)
            .foregroundStyle(Palette.dim)
    }
}

/// The app's bars in the theme: the void behind them, titles in the display
/// face (large) and the text face (inline).
@MainActor func applyBarAppearance() {
    let void = UIColor(Palette.void)
    let fg = UIColor(Palette.fg)
    let nav = UINavigationBarAppearance()
    nav.configureWithOpaqueBackground()
    nav.backgroundColor = void
    nav.shadowColor = UIColor(Palette.raised)
    nav.largeTitleTextAttributes = [
        .font: Face.uiFont("BigShouldersDisplay-ExtraBold", 38, fallback: .heavy), .foregroundColor: fg, .kern: 0.4,
    ]
    nav.titleTextAttributes = [.font: Face.uiFont("InstrumentSans-SemiBold", 17, fallback: .semibold), .foregroundColor: fg]
    UINavigationBar.appearance().standardAppearance = nav
    UINavigationBar.appearance().scrollEdgeAppearance = nav
    UINavigationBar.appearance().compactAppearance = nav

    let tabs = UITabBarAppearance()
    tabs.configureWithOpaqueBackground()
    tabs.backgroundColor = void
    tabs.shadowColor = UIColor(Palette.raised)
    UITabBar.appearance().standardAppearance = tabs
    UITabBar.appearance().scrollEdgeAppearance = tabs
}
