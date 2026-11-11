import QtQuick

// Текст по шкале MD3: Label { type: "titleLarge" }. Шрифт — вариативный Google Sans:
// вес и оптический размер задаются осями, а не только font.weight.
Text {
    id: root

    property string type: "bodyMedium"
    // fade: не влезший текст уходит в прозрачность у края вместо «…». Слой с шейдером — только пока
    // текст длиннее строки (одна строка, без переноса).
    property bool fade: false
    // ширина всей строки без «…»: implicitWidth при включённом elide бывает уже обрезанной
    readonly property real naturalWidth: fade ? _metrics.advanceWidth : implicitWidth
    readonly property bool overflowing: fade && wrapMode === Text.NoWrap && width > 0 && naturalWidth > width + 0.5
    property real fadeLength: 28
    // Затухание — не шире ~15% строки: в узкой плитке фиксированные 28 px съедали
    // половину слова, и весь заголовок выглядел размытым, а не только концевой край.
    readonly property real fadeWidth: Math.min(fadeLength, Math.max(0, width) * 0.15)
    readonly property var _t: Theme.type[type] ?? Theme.type.bodyMedium
    property real weight: _t.weight      // 400..700, ось wght

    color: Theme.fgSurface
    font.family: Theme.fontFamily
    font.pixelSize: _t.size
    // С вариативным шрифтом вес — только осью: font.weight заставляет Qt брать ближайший
    // статический файл семейства (600 → Medium), и промежуточные веса пропадают.
    font.weight: Theme.variableFont ? Font.Normal : Math.round(weight)
    font.letterSpacing: _t.tracking
    font.variableAxes: Theme.fontAxes(_t.size, weight)
    lineHeight: _t.line
    lineHeightMode: Text.FixedHeight
    verticalAlignment: Text.AlignVCenter
    elide: overflowing ? Text.ElideNone : Text.ElideRight
    textFormat: Text.PlainText

    TextMetrics {
        id: _metrics
        font: root.font
        text: root.fade ? root.text : ""
    }

    layer.enabled: overflowing
    layer.effect: ShaderEffect {
        readonly property real fadeFrom: Math.max(0, 1 - root.fadeWidth / Math.max(1, root.width))
        fragmentShader: Qt.resolvedUrl("shaders/fadeedge.frag.qsb")
    }
}
