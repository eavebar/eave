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

uniform sampler2D field;     // each island's coverage, in its channel, blurred by a gaussian a band wide
uniform vec2 fieldScale;     // s px -> uv in it
uniform vec4 fieldLimit;     // its first and last texel centers with content, uv
uniform float fieldStep;     // s px between its texels
uniform float sigma;         // the field's gaussian, s px
uniform float band;          // s px: the bevel, on an island wide enough for it

uniform float fade;          // the surface's fade-in or -out
uniform float thickness;     // s px
uniform float gap;           // s px between the glass's underside and what's behind it
uniform float chroma;
uniform float rim;
uniform float tint;
uniform int   ink;           // 0 its own, 1 the opposite of the glass behind, 2 that, made to read
uniform float contrast;
uniform float margin;        // s px

// What eave said about this frame: boxes in s px (x0, y0, x1, y1), and each one's fill color
// with, in w, -1 for solid content, or for an island which way its ink goes: 0 lighter, 1 darker;
// and each island's channel of the field and its bevel, s px: the band, or less on an island too
// narrow for two (Render.cpp's `bevelOf`).
const int MAX = 64;
uniform int   count;
uniform vec4  boxes[MAX];
uniform vec4  fills[MAX];
uniform int   chans[MAX];
uniform float bevels[MAX];

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

// How much of the area around s is the island's glass, weighted by a gaussian a band wide: `chan`
// picks the island's channel, so its neighbours don't count.
float height(vec2 s, vec4 chan) {
    return dot(texture(field, clamp(s * fieldScale, fieldLimit.xy, fieldLimit.zw)), chan);
}

// The inverse error function, single precision (Giles, 2010), for |x| up to 0.995.
float erfinv(float x) {
    float w = -log((1.0 - x) * (1.0 + x)) - 2.5;
    float p = 2.81022636e-08;
    p       = 3.43273939e-07 + p * w;
    p       = -3.5233877e-06 + p * w;
    p       = -4.39150654e-06 + p * w;
    p       = 0.00021858087 + p * w;
    p       = -0.00125372503 + p * w;
    p       = -0.00417768164 + p * w;
    p       = 0.246640727 + p * w;
    p       = 1.50140941 + p * w;
    return p * x;
}

// The glass is a slab lying on the backdrop, `thickness` deep, whose edge curves down to nothing
// over a bevel `b` wide: a squircle's quarter, steep at the very edge and flat `b` in. Narrower
// than the band, the slab is thinner by as much, so the island is the same glass made smaller:
// its two sides meet in the middle flat, as one smooth round. Its height, and its slope, for a
// pixel `d` s px in from the edge. The slope is taken half a pixel in at least, where the edge's
// outermost pixel is, so it's never vertical: an error in `d` there leaves a steep edge, not a
// flat strip all bent the same way.
float profile(float d, float b) {
    float e = 1.0 - clamp(d / b, 0.0, 1.0);
    return thickness * b / band * pow(1.0 - e * e * e * e, 0.25);
}

float steepness(float d, float b) {
    float e = 1.0 - clamp(max(d, 0.5) / b, 0.0, 1.0);
    return min(thickness / band * e * e * e * pow(1.0 - e * e * e * e, -0.75), 8.0);
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
// (an island's jelly and squish reach a little past the shape it reports), and of those it's
// inside, the smallest; with its fill, which way its ink goes, its channel of the field and its
// bevel. False
// for none, and inside solid content, whatever it's over. shape.frag's `owner` finds the same
// island: keep them alike.
bool region(vec2 s, out vec3 fill, out float darker, out vec4 chan, out float bevel) {
    int   found   = -1;
    float nearest = margin;
    float area    = 0.0;
    for (int i = 0; i < MAX; i++) {
        if (i >= count)
            break;
        vec4  b = boxes[i];
        float d = length(max(max(b.xy - s, s - b.zw), vec2(0.0)));
        if (fills[i].w < 0.0) {
            if (d <= 0.0)
                return false;
            continue;
        }
        float a = (b.z - b.x) * (b.w - b.y);
        if (d < nearest || (d <= 0.0 && found >= 0 && nearest <= 0.0 && a < area)) {
            nearest = d;
            area    = a;
            found   = i;
        }
    }
    fill   = vec3(0.0);
    darker = 0.0;
    chan   = vec4(0.0);
    bevel  = band;
    if (found < 0)
        return false;
    fill   = fills[found].rgb;
    darker = fills[found].w;
    chan   = vec4(equal(ivec4(0, 1, 2, 3), ivec4(chans[found])));
    bevel  = max(bevels[found], 1.0);
    return true;
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

// What the surface's pixel, already drawn, becomes, with the ink going lighter or darker. Which
// way is the island's (eave decides it over what's behind the whole island, with hysteresis), never
// the pixel's, so it can't flip partway through a letter; the glass gives way instead, so the ink
// can always reach it.
vec3 shade(vec2 s, vec3 glass, vec3 fill, bool lighterInk) {
    vec4 t      = layer(s);
    vec3 c      = t.rgb / max(t.a, 1e-4);
    // Grey ink follows the glass; colored content keeps its colors.
    float grey  = 1.0 - smoothstep(0.08, 0.25, chromaOf(c));
    vec3 b      = body(glass);
    vec3 behind = body(wideLook(s));
    if (ink == 2) {
        float y = dot(pow(behind, vec3(2.2)), W);
        b       = makeRoom(b, y, lighterInk);
        behind  = makeRoom(behind, y, lighterInk);
    }

    // A dark fill becomes glass by a screen or by laying what's on it over, a light one by a
    // multiply: what's drawn on it stays.
    vec3 col;
    if (luma(fill) >= 0.5)
        col = clamp(c / max(fill, vec3(1e-4)), 0.0, 1.0) * b;
    else {
        vec3 k = clamp((c - fill) / max(vec3(1.0) - fill, vec3(1e-4)), 0.0, 1.0);
        // Screened on, colors would wash into the glass's (orange over blue turns pink): laid
        // over it by how much of the pixel they cover instead, they keep theirs.
        float cover = max(k.r, max(k.g, k.b));
        vec3  over  = mix(b, k / max(cover, 1e-4), cover);
        // Grey ink is screened on dark glass, so it stays light whatever is behind.
        col = lighterInk ? mix(over, k + (vec3(1.0) - k) * b, grey) : over;
    }

    if (ink != 0) {
        // How much of the pixel is ink rather than fill, for grey ink only, so edges stay smooth,
        // dimmer ink stays dimmer, and colored content keeps its colors.
        float lf       = luma(fill);
        float amount   = t.a < 0.5 ? 0.0 : clamp(abs(luma(c) - lf) / max(max(lf, 1.0 - lf), 1e-4), 0.0, 1.0);
        vec3  opposite = vec3(1.0) - behind;
        vec3  drawn    = mix(b, ink == 1 ? opposite : readable(opposite, behind, lighterInk), amount);
        col            = mix(col, drawn, grey);
    }
    return col;
}

// Replaces the surface's pixel: shaded once, or both ways while the island's ink turns.
void put(vec2 s, vec3 glass, float a, vec3 fill, float darker) {
    // Its own ink goes the way it's drawn.
    if (ink == 0)
        darker = luma(fill) >= 0.5 ? 1.0 : 0.0;
    vec3 col;
    if (darker <= 0.0)
        col = shade(s, glass, fill, true);
    else if (darker >= 1.0)
        col = shade(s, glass, fill, false);
    else
        col = mix(shade(s, glass, fill, true), shade(s, glass, fill, false), darker);
    a *= fade;
    fragColor = vec4(col * a, a);
}

void main() {
    vec2  s = toSurf * (gl_FragCoord.xy - fbOrigin);
    float a = cover(s);
    if (a <= 0.0)
        discard;
    vec3  fill;
    float darker;
    vec4  chan;
    float b;
    if (!region(s, fill, darker, chan, b))
        discard; // not an island eave said may turn to glass, or content over one

    // How deep into the glass the pixel is, from the island's field: 0.5 on the edge, all but 1 a
    // band in. Its slope points out of the glass. The field is smooth, so both change smoothly
    // from pixel to pixel, around corners too. By a straight edge the field is the gaussian's
    // integral, so it gives back the distance in.
    float h = height(s, chan);
    if (h >= 0.995) {
        put(s, look(s), a, fill, darker); // deep inside, where the glass is flat
        return;
    }
    vec2  e     = vec2(fieldStep, 0.0);
    float hr = height(s + e.xy, chan), hl = height(s - e.xy, chan);
    float hd = height(s + e.yx, chan), hu = height(s - e.yx, chan);
    vec2  slope = vec2(hr - hl, hd - hu);
    float d     = sigma * 1.41421356 * erfinv(clamp(h * 2.0 - 1.0, -0.995, 0.995)); // s px in
    vec2  n     = length(slope) > 1e-6 ? -normalize(slope) : vec2(0.0);

    // Round a corner, the blur takes more of the shape away than by a straight edge, so the field
    // there is lower and its half-way line runs inside the real edge, by about sigma^2 times the
    // edge's curvature over 2. Read straight, the pixels between them would all be at the edge,
    // bent and lit the most, the same: a flat white patch. So the depth is moved out by that much,
    // from the curvature of the field's line through the pixel, taken back to the edge's (a line
    // d in from an edge of radius R has radius R - d). Concave corners move it the other way.
    // It fades out toward the bevel's flat top: in the middle of a narrow island the field's
    // slope is all but nothing, its curvature noise, and moved by it the middle would crease.
    float hdr = height(s + e.xx, chan), hul = height(s - e.xx, chan);
    float hur = height(s + vec2(e.x, -e.x), chan), hdl = height(s + vec2(-e.x, e.x), chan);
    float gx = slope.x / (2.0 * e.x), gy = slope.y / (2.0 * e.x);
    float hxx = (hr + hl - 2.0 * h) / (e.x * e.x), hyy = (hd + hu - 2.0 * h) / (e.x * e.x);
    float hxy = (hdr + hul - hur - hdl) / (4.0 * e.x * e.x);
    float g2  = gx * gx + gy * gy;
    float k   = g2 > 1e-12 ? -(hxx * gy * gy - 2.0 * hxy * gx * gy + hyy * gx * gx) / (g2 * sqrt(g2)) : 0.0;
    k         = k / max(1.0 + k * max(d, 0.0), 0.1); // the edge's, not the line's
    d += clamp(0.5 * sigma * sigma * k, -0.5 * b, 0.5 * b) * clamp(1.0 - d / b, 0.0, 1.0);

    // The glass's surface there, facing up (+z, at the viewer) and, down the bevel, out. The view
    // ray goes straight down into it, bends by Snell's law (glass, 1.5), crosses the slab and the
    // gap under it, and meets the backdrop: where it does is what the pixel shows. A convex edge
    // bends it in, so what's under the bevel is magnified, and near the rim turned over, as in a
    // lens. The wider the gap, the farther a bent ray drifts, and the more turns over.
    vec3 N      = normalize(vec3(n * steepness(d, b), 1.0));
    vec3 R      = refract(vec3(0.0, 0.0, -1.0), N, 1.0 / 1.5);
    vec2 offset = R.xy / max(-R.z, 1e-3) * (profile(d, b) + gap);
    // Red bends a little less than blue.
    vec3 col;
    col.r = look(s + offset * (1.0 - chroma)).r;
    col.g = look(s + offset).g;
    col.b = look(s + offset * (1.0 + chroma)).b;

    // The light off the same surface: what it reflects, by Schlick's Fresnel, from none straight
    // on to all of it at a grazing angle, so a thin line runs all around the steep edge; and a
    // highlight on the side that faces the light, above and a little left. A steep edge faces
    // out, so what it reflects is what's around the island, a little lighter: a dark rim over
    // dark, a bright one over bright, never a white outline on everything. Both are what they'd
    // be on the flat top less what they are there (the 4% glass reflects straight on is in the
    // frost), so they fade out where the bevel does.
    vec3  around = mix(look(s + n * (max(d, 0.0) + 0.25 * b)), vec3(1.0), 0.35);
    vec3  sun    = normalize(vec3(normalize(vec2(-0.35, -1.0)) * 0.8, 0.6));
    vec3  mid    = normalize(sun + vec3(0.0, 0.0, 1.0));
    float f      = 0.96 * pow(1.0 - N.z, 5.0);
    float shine  = max(pow(max(dot(N, mid), 0.0), 80.0) - pow(mid.z, 80.0), 0.0);
    col          = mix(col, around, clamp(rim * f, 0.0, 1.0));
    col += rim * 0.4 * shine;

    put(s, col, a, fill, darker);
}
