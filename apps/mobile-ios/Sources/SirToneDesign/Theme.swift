// The phone's theme over the generated tokens (DESIGN.md §2–§6, `docs/mobile/README.md` §4):
// the neutral set of the current scheme plus the three active tokens of the person's colour
// (`primary*` are neutral since design 2.1, D-155).
// Components read only `Palette`; nothing outside the tokens (DESIGN.md §12).

import CoreText
import SwiftUI

public struct Palette: Sendable {
    public let dark: Bool
    public let bgBase, bgLayer, bgRaised, bgFlyout, bgSelected: Color
    public let inactive, border, separator: Color
    public let textPrimary, textSecondary, textTertiary, icon: Color
    public let artworkPlaceholder, artworkScrim, danger, onPrimary: Color
    /// `accent-fill`, `accent-text`, `accent-on`, `primary`, `primary-text`.
    public let accent: SirToneTokens.Accent

    /// `highContrast` — the system setting «Увеличить контрастность» (`lightHC` / `darkHC`, FR-LOOK-001).
    public static func make(dark: Bool, accent slug: String, highContrast: Bool = false) -> Palette {
        let pair = SirToneTokens.accents[slug] ?? SirToneTokens.accents["sirkov"]!
        if dark && highContrast {
            typealias T = SirToneTokens.DarkHC
            return Palette(
                dark: true, bgBase: T.bgBase, bgLayer: T.bgLayer, bgRaised: T.bgRaised,
                bgFlyout: T.bgFlyout, bgSelected: T.bgSelected, inactive: T.inactive,
                border: T.border, separator: T.separator, textPrimary: T.textPrimary,
                textSecondary: T.textSecondary, textTertiary: T.textTertiary, icon: T.icon,
                artworkPlaceholder: T.artworkPlaceholder, artworkScrim: T.artworkScrim,
                danger: T.danger, onPrimary: T.onPrimary, accent: pair.dark)
        }
        if !dark && highContrast {
            typealias T = SirToneTokens.LightHC
            return Palette(
                dark: false, bgBase: T.bgBase, bgLayer: T.bgLayer, bgRaised: T.bgRaised,
                bgFlyout: T.bgFlyout, bgSelected: T.bgSelected, inactive: T.inactive,
                border: T.border, separator: T.separator, textPrimary: T.textPrimary,
                textSecondary: T.textSecondary, textTertiary: T.textTertiary, icon: T.icon,
                artworkPlaceholder: T.artworkPlaceholder, artworkScrim: T.artworkScrim,
                danger: T.danger, onPrimary: T.onPrimary, accent: pair.light)
        }
        if dark {
            typealias T = SirToneTokens.Dark
            return Palette(
                dark: true, bgBase: T.bgBase, bgLayer: T.bgLayer, bgRaised: T.bgRaised,
                bgFlyout: T.bgFlyout, bgSelected: T.bgSelected, inactive: T.inactive,
                border: T.border, separator: T.separator, textPrimary: T.textPrimary,
                textSecondary: T.textSecondary, textTertiary: T.textTertiary, icon: T.icon,
                artworkPlaceholder: T.artworkPlaceholder, artworkScrim: T.artworkScrim,
                danger: T.danger, onPrimary: T.onPrimary, accent: pair.dark)
        }
        typealias T = SirToneTokens.Light
        return Palette(
            dark: false, bgBase: T.bgBase, bgLayer: T.bgLayer, bgRaised: T.bgRaised,
            bgFlyout: T.bgFlyout, bgSelected: T.bgSelected, inactive: T.inactive,
            border: T.border, separator: T.separator, textPrimary: T.textPrimary,
            textSecondary: T.textSecondary, textTertiary: T.textTertiary, icon: T.icon,
            artworkPlaceholder: T.artworkPlaceholder, artworkScrim: T.artworkScrim,
            danger: T.danger, onPrimary: T.onPrimary, accent: pair.light)
    }

    /// The picker's order (D-158): Sirtone red, then the ten macOS-style colours.
    public static let accentOrder = [
        "sirkov", "blue", "violet", "pink", "red", "orange", "yellow", "green", "gray", "lightblue",
    ]
}

private struct PaletteKey: EnvironmentKey {
    static let defaultValue = Palette.make(dark: true, accent: "sirkov")
}

public extension EnvironmentValues {
    var palette: Palette {
        get { self[PaletteKey.self] }
        set { self[PaletteKey.self] = newValue }
    }
}

// MARK: Geologica (DESIGN.md §5): embedded, never from the network.

public enum Typeface {
    /// Registers the bundled Geologica once; without it the system font stands in.
    public static func register() {
        guard let url = fontURL() else { return }
        CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
    }

    /// Found by name in the app or its resource bundles (SwiftPM `Bundle.module` traps when its
    /// bundle is missing; a missing font must not stop the app).
    private static func fontURL() -> URL? {
        let fm = FileManager.default
        for base in [Bundle.main.resourceURL, Bundle.main.bundleURL].compactMap({ $0 }) {
            let direct = base.appendingPathComponent("Geologica.ttf")
            if fm.fileExists(atPath: direct.path) { return direct }
            let items = (try? fm.contentsOfDirectory(at: base, includingPropertiesForKeys: nil)) ?? []
            for b in items where b.pathExtension == "bundle" {
                for sub in ["Geologica.ttf", "Resources/Geologica.ttf"] {
                    let u = b.appendingPathComponent(sub)
                    if fm.fileExists(atPath: u.path) { return u }
                }
            }
        }
        return nil
    }
}

public extension Font {
    /// A DESIGN.md text style in Geologica, scaled with Dynamic Type.
    static func sirtone(_ s: SirToneTokens.TextStyle, relativeTo rel: Font.TextStyle = .body) -> Font {
        let weight: Font.Weight = switch s.weight {
        case ..<500: .regular
        case 500..<600: .medium
        case 600..<700: .semibold
        default: .bold
        }
        return .custom("Geologica", size: s.size, relativeTo: rel).weight(weight)
    }
}
