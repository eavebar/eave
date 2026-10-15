#version 300 es
EXTENSION
// The glass, keyed in over a surface that just drew: where the surface is the fill color of an
// island eave said may turn to glass, the frosted backdrop shows through, bent at the edges.
//
// Everything here thinks in `s`: framebuffer px, upright, from the surface's top left, with "up"
// as -y. The framebuffer may be turned (a rotated monitor): `toFb` and `fbOrigin` take s there.
precision highp float;

uniform SAMPLER mask;        // the surface's buffer this frame, premultiplied, top row first
uniform vec2 surfSize;       // s px
uniform vec2 fbOrigin;       // the surface's top left, framebuffer px
uniform mat2 toFb;
uniform mat2 toSurf;

uniform sampler2D backdrop;  // the frosted copy of the frame around the surface
uniform vec2 bdOrigin;       // where the copy starts in the framebuffer, px
uniform vec2 bdScale;        // framebuffer px from bdOrigin -> uv in it
uniform vec2 bdLimit;        // the part of it that holds content, uv
uniform sampler2D wide;      // the most blurred level, for choosing the ink's color
uniform vec2 wideScale;
uniform vec2 wideLimit;
uniform float wideSize;      // s px between its taps

uniform float fade;          // the surface's fade-in or -out
uniform float band;          // s px
uniform float strength;      // s px
uniform float chroma;
uniform float rim;
uniform float tint;
uniform int   ink;           // 0 its own, 1 the opposite of the glass behind, 2 that, made to read
uniform float contrast;
uniform float margin;        // s px

// What eave said about this frame: boxes in s px (x0, y0, x1, y1), and each one's fill color
// with its kind in w (0 a dark island, 1 a light one, 2 solid content).
const int MAX = 64;
uniform int  count;
uniform vec4 boxes[MAX];
uniform vec4 fills[MAX];

out vec4 fragColor;

const vec3 W = vec3(0.2126, 0.7152, 0.0722); // relative luminance, of linear light

float luma(vec3 c) {
    return dot(c, W);
}

float chromaOf(vec3 c) {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

vec4 layer(vec2 s) {
    vec2 uv = s / surfSize;
    if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0))))
        return vec4(0.0);
    return texture(mask, uv);
}

float cover(vec2 s) {
    return layer(s).a;
}

vec3 look(vec2 s) {
    return texture(backdrop, clamp((fbOrigin + toFb * s - bdOrigin) * bdScale, vec2(0.0), bdLimit)).rgb;
}

// The glass behind s, much more blurred than the glass itself, and without its bend: what the
// ink's color is chosen against, so it changes slowly along the bar instead of under each letter.
vec3 wideLook(vec2 s) {
    vec3 sum = vec3(0.0);
    for (int i = 0; i < 9; i++) {
        vec2 o  = vec2(float(i % 3) - 1.0, float(i / 3) - 1.0) * wideSize;
        vec2 uv = clamp((fbOrigin + toFb * (s + o) - bdOrigin) * wideScale, vec2(0.0), wideLimit);
        sum += texture(wide, uv).rgb;
    }
    return sum / 9.0;
}

// Which island s belongs to: the nearest one eave said may turn to glass, within `margin` of it
// (an island's jelly and squish reach a little past the shape it reports), and its fill. -1 for
// none, and 2 inside solid content, whatever it's over.
int region(vec2 s, out vec3 fill) {
    int   kind    = -1;
    float nearest = margin;
    fill          = vec3(0.0);
    for (int i = 0; i < MAX; i++) {
        if (i >= count)
            break;
        vec4  b = boxes[i];
        int   k = int(fills[i].w + 0.5);
        float d = length(max(max(b.xy - s, s - b.zw), vec2(0.0)));
        if (k == 2) {
            if (d <= 0.0)
                return 2;
            continue;
        }
        if (d <= nearest) {
            nearest = d;
            kind    = k;
            fill    = fills[i].rgb;
        }
    }
    return kind;
}

// `color`, made just light (`lighter`) or just dark enough to stand `contrast` to one against
// `bg` (WCAG's ratio of relative luminances). Lighter mixes toward white and darker scales toward
// black, in linear light, so the hue stays. A color already past it is left alone.
vec3 readable(vec3 color, vec3 bg, bool lighter) {
    vec3  c    = pow(color, vec3(2.2));
    float yb   = dot(pow(bg, vec3(2.2)), W);
    float yc   = dot(c, W);
    float up   = contrast * (yb + 0.05) - 0.05; // the darkest a lighter ink may be
    float down = (yb + 0.05) / contrast - 0.05; // the lightest a darker ink may be
    if (lighter && yc < up)
        c += (vec3(1.0) - c) * clamp((up - yc) / max(1.0 - yc, 1e-4), 0.0, 1.0);
    else if (!lighter && yc > down)
        c *= max(down, 0.0) / max(yc, 1e-4);
    return pow(c, vec3(1.0 / 2.2));
}

// The glass, given way just enough that ink can reach `contrast` on it: no lighter than white ink
// allows on a dark island, no darker than black ink allows on a light one. `y` is the luminance
// that decides it (the wide blur's, so the glass dims and brightens smoothly), `c` what's moved.
vec3 makeRoom(vec3 c, float y, bool lighterInk) {
    vec3 l = pow(c, vec3(2.2));
    if (lighterInk) {
        float most = 1.05 / contrast - 0.05;
        if (y > most)
            l *= most / y;
    } else {
        float least = contrast * 0.05 - 0.05;
        if (y < least)
            l += (vec3(1.0) - l) * (least - y) / max(1.0 - y, 1e-4);
    }
    return pow(l, vec3(1.0 / 2.2));
}

// The glass's own color: what's behind it, tinted richer by itself.
vec3 body(vec3 glass) {
    vec3 rich = clamp(mix(vec3(luma(glass)), glass, 1.6), 0.0, 1.0);
    return mix(glass, rich, tint);
}

// Replaces the surface's pixel, which is already drawn. A dark island's fill becomes glass by a
// screen, so lighter ink stays; a light one's by a multiply, so darker ink stays.
void put(vec2 s, vec3 glass, float a, vec3 fill, bool light) {
    vec4 t      = layer(s);
    vec3 c      = t.rgb / max(t.a, 1e-4);
    vec3 b      = body(glass);
    vec3 behind = body(wideLook(s));
    // Which way the ink goes is the island's: lighter on a dark one, darker on a light one (eave
    // turns an island light, with hysteresis, over something bright). Never per pixel, so it can't
    // flip partway through a letter; the glass gives way instead, so the ink can always reach it.
    bool lighterInk = !light;
    if (ink == 2) {
        float y = dot(pow(behind, vec3(2.2)), W);
        b       = makeRoom(b, y, lighterInk);
        behind  = makeRoom(behind, y, lighterInk);
    }

    vec3 col;
    if (light)
        col = clamp(c / max(fill, vec3(1e-4)), 0.0, 1.0) * b;
    else {
        vec3 k = clamp((c - fill) / max(vec3(1.0) - fill, vec3(1e-4)), 0.0, 1.0);
        col    = k + (vec3(1.0) - k) * b;
    }

    if (ink != 0) {
        // How much of the pixel is ink rather than fill, for grey ink only, so edges stay smooth,
        // dimmer ink stays dimmer, and colored content keeps its colors.
        float lf       = luma(fill);
        float amount   = t.a < 0.5 ? 0.0 : clamp(abs(luma(c) - lf) / max(max(lf, 1.0 - lf), 1e-4), 0.0, 1.0);
        float grey     = 1.0 - smoothstep(0.08, 0.25, chromaOf(c));
        vec3  opposite = vec3(1.0) - behind;
        vec3  drawn    = mix(b, ink == 1 ? opposite : readable(opposite, behind, lighterInk), amount);
        col            = mix(col, drawn, grey);
    }
    a *= fade;
    fragColor = vec4(col * a, a);
}

void main() {
    vec2  s = toSurf * (gl_FragCoord.xy - fbOrigin);
    float a = cover(s);
    if (a <= 0.0)
        discard;
    vec3 fill;
    int  kind = region(s, fill);
    if (kind != 0 && kind != 1)
        discard; // not an island eave said may turn to glass, or content over one
    bool light = kind == 1;

    // Deep inside, nothing bends: eight taps a band away settle most of a popover's pixels
    // without the rings below.
    float inside = 1.0;
    for (int i = 0; i < 8; i++) {
        float ang = float(i) * 0.7853982;
        inside    = min(inside, cover(s + vec2(cos(ang), sin(ang)) * band * 1.05));
    }
    if (inside >= 1.0) {
        put(s, look(s), a, fill, light);
        return;
    }

    // Rings of samples around the pixel: their mean coverage says how deep into the glass it is
    // (0.5 on the edge, 1 a full band in), and the empty ones point out of it.
    const int DIRS  = 12;
    const int RINGS = 3;
    float     sum = 0.0, weight = 0.0;
    vec2      out_ = vec2(0.0);
    for (int r = 1; r <= RINGS; r++) {
        float radius = band * float(r) / float(RINGS);
        float w      = 1.0 / float(r);
        for (int i = 0; i < DIRS; i++) {
            float ang = (float(i) + 0.5 * float(r & 1)) * 6.2831853 / float(DIRS);
            vec2  d   = vec2(cos(ang), sin(ang));
            float c   = cover(s + d * radius);
            sum += c * w;
            weight += w;
            out_ += d * (1.0 - c) * w;
        }
    }
    float depth = clamp((sum / weight - 0.5) * 2.0, 0.0, 1.0); // 0 at the edge, 1 inside
    float edge  = 1.0 - depth;
    vec2  n     = length(out_) > 1e-4 ? normalize(out_) : vec2(0.0);

    // A rounded rim: the bend grows quickly near the edge, like the side of a lens.
    float bend   = strength * edge * edge;
    vec2  offset = -n * bend; // look inward: the rim magnifies what's under the middle

    vec3 col;
    col.r = look(s + offset * (1.0 + chroma)).r;
    col.g = look(s + offset).g;
    col.b = look(s + offset * (1.0 - chroma)).b;

    // Light from above and a little left; a softer bounce on the far side.
    vec2  sun = normalize(vec2(-0.35, -1.0));
    float lit = max(dot(n, sun), 0.0) + 0.35 * max(dot(n, -sun), 0.0);
    col += rim * lit * pow(edge, 3.0);

    put(s, col, a, fill, light);
}
