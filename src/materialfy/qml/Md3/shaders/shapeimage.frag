#version 440
// Обложка в фигуре MD3 Expressive (MorphImage): одна текстура (сама Image) и аналитическая маска.
// Раньше: Image → слой-маска (offscreen) → MultiEffect (ещё один offscreen) на каждую обложку.
layout(location = 0) in vec2 qt_TexCoord0;
layout(location = 0) out vec4 fragColor;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    vec4 a0; vec4 a1; vec4 a2; vec4 a3;     // фигура A
    vec4 b0; vec4 b1; vec4 b2; vec4 b3;     // фигура B
    vec4 c0; vec4 c1; vec4 c2; vec4 c3;     // фигура C (цель морфа)
    vec4 morph;         // (s: A→B, t: →C, поворот контура, рад, pulse)
    vec4 misc;          // (time для кляксы, ширина обводки px, готовность картинки, -)
    vec4 box;           // (sx, sy, cropX, cropY): q = (uv·2 − 1)·box.xy — эффект чуть больше элемента под перелёт
    vec4 fill;          // заливка / подложка картинки
    vec4 stroke;        // цвет обводки
};
layout(binding = 1) uniform sampler2D source;

// Радиус полярной фигуры MD3 (как Shapes.radius): p0 = (kind, n, depth, p), p1 = (поворот, aspect, масштаб, фаза3),
// p2 = (k1, amp1, фаза1, k2), p3 = (amp2, фаза2, k3, amp3). kind: 0 круг, 1 суперэллипс, 2 печенька, 3 цветок, 4 клякса.
float shapeR(vec4 p0, vec4 p1, vec4 p2, vec4 p3, float th, float time) {
    float t = th - p1.x;
    float r = 1.0;
    if (p0.x > 3.5) {
        r = 1.0 + p2.y * cos(p2.x * t + p2.z + time * 0.6) + p3.x * cos(p2.w * t + p3.y + time * 0.95)
                + p3.w * cos(p3.z * t + p1.w + time * 1.3);
    } else if (p0.x > 2.5) {
        r = 1.0 - p0.z + p0.z * pow(max(abs(cos(p0.y * t * 0.5)), 1e-4), p0.w);
    } else if (p0.x > 1.5) {
        r = 1.0 - p0.z * (0.5 - 0.5 * cos(p0.y * t));
    } else if (p0.x > 0.5) {
        float c = pow(max(abs(cos(t)) / p1.y, 1e-4), p0.w);
        float s = pow(max(abs(sin(t)), 1e-4), p0.w);
        r = pow(c + s, -1.0 / p0.w);
    }
    return r * p1.z;
}

// Морф: from = mix(A, B, s), затем mix(from, C, t). Возвращает f = rho - r (меньше нуля — внутри).
float shapeField(vec2 q) {
    float th = atan(q.y, q.x) - morph.z;
    float r = shapeR(a0, a1, a2, a3, th, misc.x);
    if (morph.x != 0.0)
        r = mix(r, shapeR(b0, b1, b2, b3, th, misc.x), morph.x);
    if (morph.y != 0.0)
        r = mix(r, shapeR(c0, c1, c2, c3, th, misc.x), morph.y);
    float pulse = morph.w;
    // Дыхание: амплитуда контура растёт с pulse, основание чуть поджимается, чтобы пик
    // не вылезал за границы item. Фактор непрерывен по pulse — затухание до тишины
    // не даёт скачка масштаба (раньше у idle был резкий шаг к 0.97).
    if (pulse != 0.0)
        r = (1.0 - (1.0 - r) * (1.0 + 2.2 * pulse)) * (1.0 - 0.03 * pulse);
    return length(q) - r;
}

void main() {
    vec2 q = (qt_TexCoord0 * 2.0 - 1.0) * box.xy;
    float f = shapeField(q);
    float d = f / max(length(vec2(dFdx(f), dFdy(f))), 1e-6);
    float inside = clamp(0.5 - d, 0.0, 1.0);
    // PreserveAspectCrop: box.zw — доля текстуры по осям
    vec4 img = texture(source, 0.5 + q * 0.5 * box.zw);
    vec4 col = mix(vec4(fill.rgb * fill.a, fill.a), img, misc.z);
    fragColor = col * inside * qt_Opacity;
}
