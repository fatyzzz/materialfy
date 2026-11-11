.pragma library

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
