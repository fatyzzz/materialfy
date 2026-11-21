.pragma library

const styles = {
    displayLarge:   { size: 57, line: 64, weight: 400, tracking: -0.25 },
    displayMedium:  { size: 45, line: 52, weight: 400, tracking: 0 },
    displaySmall:   { size: 36, line: 44, weight: 400, tracking: 0 },
    headlineLarge:  { size: 32, line: 40, weight: 400, tracking: 0 },
    headlineMedium: { size: 28, line: 36, weight: 400, tracking: 0 },
    headlineSmall:  { size: 24, line: 32, weight: 400, tracking: 0 },
    titleLarge:     { size: 22, line: 28, weight: 400, tracking: 0 },
    titleMedium:    { size: 16, line: 24, weight: 500, tracking: 0.15 },
    titleSmall:     { size: 14, line: 20, weight: 500, tracking: 0.1 },
    bodyLarge:      { size: 16, line: 24, weight: 400, tracking: 0.5 },
    bodyMedium:     { size: 14, line: 20, weight: 400, tracking: 0.25 },
    bodySmall:      { size: 12, line: 16, weight: 400, tracking: 0.4 },
    labelLarge:     { size: 14, line: 20, weight: 500, tracking: 0.1 },
    labelMedium:    { size: 12, line: 16, weight: 500, tracking: 0.5 },
    labelSmall:     { size: 11, line: 16, weight: 500, tracking: 0.5 }
}

// Android supplies an sp conversion per size (nonlinear at large font scales).
// Keep each style's original line/letter-spacing proportions, independent of dp layout.
function scaled(styles, convert) {
    const result = {}
    for (const name in styles) {
        const style = styles[name]
        const size = convert(style.size)
        const ratio = size / style.size
        result[name] = { size: size, line: style.line * ratio,
                         weight: style.weight, tracking: style.tracking * ratio }
    }
    return result
}
