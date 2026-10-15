// Spike: liquid-glass refraction behind eave's layer surfaces.
//
// Right before an `eave*` layer draws, this copies what's on screen around it. Right after, it
// lets that through the layer's islands, refracted at their edges. eave draws its islands opaque,
// as without the plugin, and the plugin keys the glass in by the island's color: black (or white,
// in the light scheme) becomes glass, content stays. Fades cover content with the island's color,
// so they fade to glass with no change in eave. The shape comes only from the buffer drawn this
// frame, at the box it's drawn at, so the glass can't lag a morph or a resize.
//
// Throwaway: no config, no transforms (rotated monitors), knobs through `hyprctl eaveglass`.

// Standard headers first: they must not see the define below.
#include <any>
#include <chrono>
#include <format>
#include <sstream>
#include <unordered_map>

// The damage ring keeps the record of each frame's damage private, and we must add to it.
#define private public
#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/render/Renderer.hpp>
#include <hyprland/src/render/OpenGL.hpp>
#include <hyprland/src/render/gl/GLFramebuffer.hpp>
#include <hyprland/src/render/pass/SurfacePassElement.hpp>
#include <hyprland/src/output/Monitor.hpp>
#include <hyprland/src/desktop/view/LayerSurface.hpp>
#include <hyprland/src/desktop/view/WLSurface.hpp>
#include <hyprland/src/protocols/core/Compositor.hpp>
#include <hyprland/src/state/MonitorState.hpp>
#include <hyprland/src/event/EventBus.hpp>
#include <hyprland/src/managers/EventManager.hpp>
#include <hyprland/src/managers/eventLoop/EventLoopManager.hpp>
#include <hyprland/src/managers/eventLoop/EventLoopTimer.hpp>
#include <hyprland/src/debug/log/Logger.hpp>
#include <hyprland/protocols/wlr-layer-shell-unstable-v1.hpp>
#undef private

using namespace Render;
using namespace Render::GL;

static HANDLE PHANDLE = nullptr;

// Logical pixels unless said otherwise; scaled by the monitor's scale when drawn.
static struct {
    bool  enabled  = true;
    float band     = 12.F;  // how far in from the edge the glass bends
    float strength = 22.F;  // the most a pixel's lookup moves, at the very edge
    float chroma   = 0.18F; // spread between red and blue lookups, as a fraction of the bend
    float rim      = 0.7F;  // brightness of the lit edge
    float frost    = 2.F;   // spread of each blur step (0 = clear glass)
    int   depth    = 3;     // how many times the blur halves the backdrop: each doubles its reach
    float clear    = 0.7F;  // how much of the backdrop shows through the island's color
    int   ink      = 2;     // grey ink: 0 as eave drew it, 1 the opposite of the glass behind, 2 that, as light or dark as it takes to read
    float contrast = 4.5F;  // ink 2: the contrast ratio it reaches (any background allows up to about 4.58)
    bool  colored  = true;  // the glass tinted by what's behind it instead of darkened
    float tint     = 0.35F; // how strong that tint is (0 = clear glass)
    bool  report   = true;  // tell eave how bright it is behind each surface, so islands can turn light
    std::vector<int> pids;  // only these eaves' layers (none = every eave)
    int   skip     = 0;     // for measuring: 1 skips the copy, 2 the refraction
} g_params;

static struct {
    CHyprSignalListener stage, monitorRemoved;
    SP<SHyprCtlCommand> command;
} g_hooks;

struct SMonitorGlass {
    SP<IFramebuffer> backdrop;      // the frame around the glass, full size
    std::vector<SP<IFramebuffer>> levels; // blurred down from it, for frost: a half, a quarter...
    SP<IFramebuffer> attached;      // a row: how far down each column is attached to the top
    CRegion          drawn;         // where the glass was drawn last, framebuffer pixels
};
static std::unordered_map<Monitor::CMonitor*, SMonitorGlass> g_fbs;

// GPU time, from timer queries read back a few frames later, so measuring doesn't stall anything.
// `timing stages` times our three steps; `timing frame` times Hyprland's whole frame (the pass
// and the copy to the screen), to compare with the glass on and off. They can't run at once:
// timer queries don't nest.
#define GL_TIME_ELAPSED_EXT 0x88BF
#define GL_GPU_DISJOINT_EXT 0x8FBB

enum eTimed : uint8_t {
    T_COPY = 0,
    T_MASK,
    T_REFRACT,
    T_FRAME,
    T_SYNC,
    T_LAST
};

static struct {
    enum : uint8_t {
        OFF,
        STAGES,
        FRAME,
        SYNC // glFinish before and after our work: stalls, but the only clock Asahi gets right
    } mode = OFF;
    double                                 sum[T_LAST] = {}, worst[T_LAST] = {};
    uint64_t                               n[T_LAST]   = {};
    uint64_t                               rendered = 0, glassed = 0; // frames drawn, and those that redrew glass
    double                                 pixels   = 0;              // glass pixels drawn
    std::vector<std::pair<GLuint, eTimed>> pending;
    GLuint                                 open = 0;
} g_timing;

static void timeBegin(eTimed what) {
    const bool want = what == T_FRAME ? g_timing.mode == g_timing.FRAME : g_timing.mode == g_timing.STAGES;
    if (!want || g_timing.open)
        return;
    GLuint q;
    glGenQueries(1, &q);
    glBeginQuery(GL_TIME_ELAPSED_EXT, q);
    g_timing.open = q;
    g_timing.pending.emplace_back(q, what);
}

static void timeEnd() {
    if (!g_timing.open)
        return;
    glEndQuery(GL_TIME_ELAPSED_EXT);
    g_timing.open = 0;
}

static void timePoll() {
    GLint disjoint = 0;
    glGetIntegerv(GL_GPU_DISJOINT_EXT, &disjoint);
    std::erase_if(g_timing.pending, [disjoint](auto& p) {
        if (p.first == g_timing.open)
            return false;
        GLuint ready = 0;
        glGetQueryObjectuiv(p.first, GL_QUERY_RESULT_AVAILABLE, &ready);
        if (!ready)
            return false;
        GLuint ns = 0;
        glGetQueryObjectuiv(p.first, GL_QUERY_RESULT, &ns);
        glDeleteQueries(1, &p.first);
        if (!disjoint) {
            const double ms = ns / 1e6;
            g_timing.sum[p.second] += ms;
            g_timing.worst[p.second] = std::max(g_timing.worst[p.second], ms);
            g_timing.n[p.second]++;
        }
        return true;
    });
}

static std::string timingStats() {
    static const char* NAMES[] = {"copy", "frost", "refract", "frame", "sync"};
    std::string        out     = std::format("monitor frames {}, {} redrew glass, {:.0f} glass px per redraw\n", g_timing.rendered, g_timing.glassed,
                                             g_timing.glassed ? g_timing.pixels / g_timing.glassed : 0.0);
    for (int i = 0; i < T_LAST; i++) {
        if (!g_timing.n[i])
            continue;
        out += std::format("{:8} n {:5} mean {:.3f} ms worst {:.3f} ms\n", NAMES[i], g_timing.n[i], g_timing.sum[i] / g_timing.n[i], g_timing.worst[i]);
        g_timing.sum[i] = g_timing.worst[i] = 0;
        g_timing.n[i]                       = 0;
    }
    g_timing.rendered = g_timing.glassed = 0;
    g_timing.pixels                      = 0;
    return out;
}

// One program per kind of surface texture: a dmabuf can come in as an external image, which
// needs its own sampler type.
struct SProgram {
    GLuint id = 0;
    GLint  backdrop, mask, attached, fbSize, bdOrigin, bdScale, surf, fade, band, strength, chroma, rim, clear, inkMode, contrast, mixed, colored, tint, rect;
};
static SProgram g_progs[2]; // [0] sampler2D, [1] samplerExternalOES
static GLuint   g_vao = 0, g_vbo = 0;

static const char* VERT = R"#(#version 300 es
precision highp float;
uniform vec2 fbSize;
uniform vec4 rect; // x, y, w, h in framebuffer pixels
layout(location = 0) in vec2 pos;
void main() {
    vec2 px = rect.xy + pos * rect.zw;
    gl_Position = vec4(px / fbSize * 2.0 - 1.0, 0.0, 1.0);
}
)#";

// gl_FragCoord is in framebuffer pixels, whose row 0 is the top of the screen ("up" is -y).
// The backdrop is a copy of the part of the frame at bdOrigin; the mask is the layer's own
// buffer, which covers `surf`. Both are stored top row first, like the frame.
static const char* FRAG = R"#(#version 300 es
EXTENSION
precision highp float;
uniform sampler2D backdrop;
uniform SAMPLER mask;
uniform vec2 fbSize;
uniform vec2 bdOrigin;  // where the copy starts in the frame, px
uniform vec2 bdScale;   // frame px from bdOrigin -> uv in the backdrop texture
uniform vec4 surf;      // the layer, px
uniform float fade;     // the layer's fade-in or -out
uniform float band;     // px
uniform float strength; // px
uniform float chroma;
uniform float rim;
uniform float clear;    // how much of the backdrop the island's color lets through
uniform int   inkMode;  // grey ink: 0 its own, 1 the opposite of the glass behind, 2 that, made to read
uniform float contrast; // 2: the contrast ratio it reaches
uniform bool  mixed;    // light islands may show (eave hears how bright it is behind)
uniform bool  colored;  // the glass tinted by what's behind it, not by the island's color
uniform float tint;     // how strong that tint is
uniform sampler2D attached; // per column of the surface: how far down it's attached to the top
out vec4 fragColor;

vec4 layer(vec2 px) {
    vec2 uv = (px - surf.xy) / surf.zw;
    if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0))))
        return vec4(0.0);
    return texture(mask, uv); // premultiplied
}

float cover(vec2 px) {
    return layer(px).a;
}

float luma(vec3 c) {
    return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

// How light the island around p is: 0 for a black one, 1 for a white one (eave turns an island
// light over something bright). Most of what's a band away is the island's own color, not ink.
float lightness;

// `color`, made just light or just dark enough to stand `contrast` to one against `bg` (WCAG's
// ratio of relative luminances): lighter if it's lighter than bg, darker if darker, unless only the
// other way can get there. Lighter mixes toward white and darker scales toward black, in linear
// light, so the hue stays. Every background allows a ratio of about 4.58 one way or the other.
vec3 readable(vec3 color, vec3 bg) {
    const vec3 W = vec3(0.2126, 0.7152, 0.0722);
    vec3  c  = pow(color, vec3(2.2));
    float yb = dot(pow(bg, vec3(2.2)), W);
    float yc = dot(c, W);
    float up   = contrast * (yb + 0.05) - 0.05; // the darkest a lighter ink may be
    float down = (yb + 0.05) / contrast - 0.05; // the lightest a darker ink may be
    bool  lighter = yc > yb;
    if (lighter && up > 1.0)
        lighter = false;
    if (!lighter && down < 0.0)
        lighter = true;
    if (lighter && yc < up)
        c += (vec3(1.0) - c) * clamp((up - yc) / max(1.0 - yc, 1e-4), 0.0, 1.0);
    else if (!lighter && yc > down)
        c *= max(down, 0.0) / max(yc, 1e-4);
    return pow(c, vec3(1.0 / 2.2));
}

// How much of the pixel at px is grey ink, rather than its island's color.
float inkAt(vec2 px) {
    vec4 t = layer(px);
    if (t.a < 0.5)
        return 0.0;
    vec3 c = t.rgb / t.a;
    float grey = 1.0 - smoothstep(0.08, 0.25, max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b)));
    return abs(luma(c) - lightness) * grey;
}

// Replaces the layer's pixel, which is already drawn. A black island becomes glass by a screen,
// so white ink stays white; a white island by a multiply, so black ink stays black.
void put(vec3 glass, float a) {
    vec4 t = layer(gl_FragCoord.xy);
    vec3 ink = t.rgb / max(t.a, 1e-4);
    vec3 dark  = clear * glass;
    vec3 light = vec3(1.0 - clear) + clear * glass;
    if (colored) {
        // Tinted by what's behind instead of by black or white: the same color, richer, over it.
        vec3 rich = clamp(mix(vec3(luma(glass)), glass, 1.6), 0.0, 1.0);
        dark = light = mix(glass, rich, tint);
    }
    vec3 col = mix(ink + (1.0 - ink) * dark, ink * light, lightness);
    if (inkMode != 0) {
        // How much of the pixel is ink rather than island, for the grey ink only, so edges stay
        // smooth and colored content keeps its colors.
        vec3  behind = mix(dark, light, lightness);
        float amount = inkAt(gl_FragCoord.xy);
        float grey   = 1.0 - smoothstep(0.08, 0.25, max(ink.r, max(ink.g, ink.b)) - min(ink.r, min(ink.g, ink.b)));
        vec3  opposite = vec3(1.0) - behind;
        vec3  drawn    = mix(behind, inkMode == 1 ? opposite : readable(opposite, behind), amount);
        col = mix(col, drawn, grey);
    }
    a *= fade;
    fragColor = vec4(col * a, a);
}

vec3 look(vec2 px) {
    return texture(backdrop, (px - bdOrigin) * bdScale).rgb;
}

void main() {
    vec2  p = gl_FragCoord.xy;
    float a = cover(p);
    if (a <= 0.0)
        discard;
    // What hangs from the screen's top edge (the notch, the grown edge, the islands attached to it)
    // stands for the hardware around the picture: it stays the layer's own black.
    vec4 at = texelFetch(attached, ivec2(int(p.x - surf.x), 0), 0);
    if (p.y - surf.y < round(at.r * 255.0) * 256.0 + round(at.g * 255.0))
        discard;

    // Deep inside, nothing bends: eight taps a band away settle most of a popover's pixels
    // without the rings below.
    float inside = 1.0, bright = 0.0, counted = 0.0;
    for (int i = 0; i < 16; i++) {
        float ang = float(i) * 0.7853982 + (i < 8 ? 0.0 : 0.3926991);
        vec4  t   = layer(p + vec2(cos(ang), sin(ang)) * band * (i < 8 ? 1.05 : 2.1));
        if (i < 8)
            inside = min(inside, t.a);
        if (t.a > 0.5) {
            bright += step(0.5, luma(t.rgb / t.a));
            counted += 1.0;
        }
    }
    // Ink is never most of what's around, so it takes a clear majority to call an island light:
    // a split vote would mark the island's own pixels as half ink, and speckle them.
    lightness = !mixed || counted == 0.0 ? 0.0 : smoothstep(0.6, 0.8, bright / counted);
    if (inside >= 1.0) {
        put(look(p), a);
        return;
    }

    // Rings of samples around the pixel: their mean coverage says how deep into the glass it is
    // (0.5 on the edge, 1 a full band in), and the empty ones point out of it.
    const int   DIRS  = 12;
    const int   RINGS = 3;
    float sum = 0.0, weight = 0.0;
    vec2  out_ = vec2(0.0);
    for (int r = 1; r <= RINGS; r++) {
        float radius = band * float(r) / float(RINGS);
        float w      = 1.0 / float(r);
        for (int i = 0; i < DIRS; i++) {
            float ang = (float(i) + 0.5 * float(r & 1)) * 6.2831853 / float(DIRS);
            vec2  d   = vec2(cos(ang), sin(ang));
            float c   = cover(p + d * radius);
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
    col.r = look(p + offset * (1.0 + chroma)).r;
    col.g = look(p + offset).g;
    col.b = look(p + offset * (1.0 - chroma)).b;

    // Light from above and a little left; a softer bounce on the far side.
    vec2  sun = normalize(vec2(-0.35, -1.0));
    float lit = max(dot(n, sun), 0.0) + 0.35 * max(dot(n, -sun), 0.0);
    col += rim * lit * pow(edge, 3.0);

    put(col, a);
}
)#";

// Frost: a dual Kawase blur. `depth` steps down, each to half the size, and back up to a half,
// each with linear filtering doing half the work, so it's smooth where single taps at a distance
// were grainy. Each step down doubles how far it reaches, at a quarter of the pixels.
static const char* PASS_VERT = R"#(#version 300 es
precision highp float;
uniform vec2 srcScale; // the part of the source texture that holds content, as uv
layout(location = 0) in vec2 pos;
out vec2 uv;
void main() {
    uv = pos * srcScale;
    gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
}
)#";

static const char* DOWN_FRAG = R"#(#version 300 es
precision highp float;
uniform sampler2D src;
uniform vec2 srcScale;
uniform vec2 texel; // one source texel, uv
uniform float offset;
in vec2 uv;
out vec4 fragColor;
vec4 at(vec2 u) { return texture(src, clamp(u, texel * 0.5, srcScale - texel * 0.5)); }
void main() {
    vec2 h = texel * 0.5 * offset;
    vec4 c = at(uv) * 4.0;
    c += at(uv - h);
    c += at(uv + h);
    c += at(uv + vec2(h.x, -h.y));
    c += at(uv - vec2(h.x, -h.y));
    fragColor = c / 8.0;
}
)#";

static const char* UP_FRAG = R"#(#version 300 es
precision highp float;
uniform sampler2D src;
uniform vec2 srcScale;
uniform vec2 texel;
uniform float offset;
in vec2 uv;
out vec4 fragColor;
vec4 at(vec2 u) { return texture(src, clamp(u, texel * 0.5, srcScale - texel * 0.5)); }
void main() {
    vec2 h = texel * 0.5 * offset;
    vec4 c = at(uv + vec2(-h.x * 2.0, 0.0));
    c += at(uv + vec2(-h.x, h.y)) * 2.0;
    c += at(uv + vec2(0.0, h.y * 2.0));
    c += at(uv + vec2(h.x, h.y)) * 2.0;
    c += at(uv + vec2(h.x * 2.0, 0.0));
    c += at(uv + vec2(h.x, -h.y)) * 2.0;
    c += at(uv + vec2(0.0, -h.y * 2.0));
    c += at(uv + vec2(-h.x, -h.y)) * 2.0;
    fragColor = c / 12.0;
}
)#";

struct SPassProgram {
    GLuint id = 0;
    GLint  src, srcScale, texel, offset;
};
static SPassProgram g_down, g_up;

// One fragment per column of the surface: walks down from its top while the surface is covered,
// and writes how far it got, in px, over two 8-bit channels.
static const char* ATTACH_FRAG = R"#(#version 300 es
EXTENSION
precision highp float;
uniform SAMPLER mask;
uniform vec2 size; // the surface, px
in vec2 uv;
out vec4 fragColor;
void main() {
    float d = size.y;
    for (float y = 0.5; y < size.y; y += 1.0) {
        if (texture(mask, vec2(uv.x, y / size.y)).a < 0.5) {
            d = y - 0.5;
            break;
        }
    }
    fragColor = vec4(floor(d / 256.0) / 255.0, mod(d, 256.0) / 255.0, 0.0, 1.0);
}
)#";

struct SAttachProgram {
    GLuint id = 0;
    GLint  mask, size, srcScale;
};
static SAttachProgram g_attach[2]; // [0] sampler2D, [1] samplerExternalOES

static GLuint compile(GLenum type, const std::string& src);

static GLuint link(GLuint vs, GLuint fs) {
    if (!vs || !fs)
        return 0;
    GLuint id = glCreateProgram();
    glAttachShader(id, vs);
    glAttachShader(id, fs);
    glLinkProgram(id);
    glDeleteShader(vs);
    glDeleteShader(fs);
    GLint ok = 0;
    glGetProgramiv(id, GL_LINK_STATUS, &ok);
    if (!ok) {
        char log[2048];
        glGetProgramInfoLog(id, sizeof(log), nullptr, log);
        Log::logger->log(Log::ERR, "[eave-glass] link: {}", log);
        glDeleteProgram(id);
        return 0;
    }
    return id;
}

static std::string replaced(std::string s, const std::string& what, const std::string& with);

static SAttachProgram* ensureAttach(bool external) {
    auto& p = g_attach[external];
    if (p.id)
        return &p;
    auto frag = replaced(replaced(ATTACH_FRAG, "EXTENSION", external ? "#extension GL_OES_EGL_image_external_essl3 : require" : ""), "SAMPLER",
                         external ? "samplerExternalOES" : "sampler2D");
    p.id = link(compile(GL_VERTEX_SHADER, PASS_VERT), compile(GL_FRAGMENT_SHADER, frag));
    if (!p.id)
        return nullptr;
    p.mask     = glGetUniformLocation(p.id, "mask");
    p.size     = glGetUniformLocation(p.id, "size");
    p.srcScale = glGetUniformLocation(p.id, "srcScale");
    return &p;
}

static bool ensurePass(SPassProgram& p, const char* frag) {
    if (p.id)
        return true;
    p.id = link(compile(GL_VERTEX_SHADER, PASS_VERT), compile(GL_FRAGMENT_SHADER, frag));
    if (!p.id)
        return false;
    p.src      = glGetUniformLocation(p.id, "src");
    p.srcScale = glGetUniformLocation(p.id, "srcScale");
    p.texel    = glGetUniformLocation(p.id, "texel");
    p.offset   = glGetUniformLocation(p.id, "offset");
    return true;
}

static GLuint compile(GLenum type, const std::string& src) {
    GLuint      s = glCreateShader(type);
    const char* c = src.c_str();
    glShaderSource(s, 1, &c, nullptr);
    glCompileShader(s);
    GLint ok = 0;
    glGetShaderiv(s, GL_COMPILE_STATUS, &ok);
    if (!ok) {
        char log[2048];
        glGetShaderInfoLog(s, sizeof(log), nullptr, log);
        Log::logger->log(Log::ERR, "[eave-glass] shader: {}", log);
        glDeleteShader(s);
        return 0;
    }
    return s;
}

static std::string replaced(std::string s, const std::string& what, const std::string& with) {
    s.replace(s.find(what), what.size(), with);
    return s;
}

static SProgram* ensureProgram(bool external) {
    auto& p = g_progs[external];
    if (p.id)
        return &p;

    auto frag = replaced(replaced(FRAG, "EXTENSION", external ? "#extension GL_OES_EGL_image_external_essl3 : require" : ""), "SAMPLER",
                         external ? "samplerExternalOES" : "sampler2D");
    GLuint vs = compile(GL_VERTEX_SHADER, VERT), fs = compile(GL_FRAGMENT_SHADER, frag);
    if (!vs || !fs)
        return nullptr;

    p.id = glCreateProgram();
    glAttachShader(p.id, vs);
    glAttachShader(p.id, fs);
    glLinkProgram(p.id);
    glDeleteShader(vs);
    glDeleteShader(fs);
    GLint ok = 0;
    glGetProgramiv(p.id, GL_LINK_STATUS, &ok);
    if (!ok) {
        char log[2048];
        glGetProgramInfoLog(p.id, sizeof(log), nullptr, log);
        Log::logger->log(Log::ERR, "[eave-glass] link: {}", log);
        glDeleteProgram(p.id);
        p.id = 0;
        return nullptr;
    }

    auto at    = [&](const char* name) { return glGetUniformLocation(p.id, name); };
    p.backdrop = at("backdrop");
    p.mask     = at("mask");
    p.fbSize   = at("fbSize");
    p.bdOrigin = at("bdOrigin");
    p.bdScale  = at("bdScale");
    p.surf     = at("surf");
    p.fade     = at("fade");
    p.band     = at("band");
    p.strength = at("strength");
    p.chroma   = at("chroma");
    p.rim      = at("rim");
    p.clear    = at("clear");
    p.inkMode  = at("inkMode");
    p.contrast = at("contrast");
    p.mixed    = at("mixed");
    p.colored  = at("colored");
    p.tint     = at("tint");
    p.attached = at("attached");
    p.rect     = at("rect");

    if (!g_vao) {
        static const float QUAD[] = {0, 0, 1, 0, 0, 1, 1, 1};
        glGenVertexArrays(1, &g_vao);
        glGenBuffers(1, &g_vbo);
        GLint oldVao = 0, oldVbo = 0;
        glGetIntegerv(GL_VERTEX_ARRAY_BINDING, &oldVao);
        glGetIntegerv(GL_ARRAY_BUFFER_BINDING, &oldVbo);
        glBindVertexArray(g_vao);
        glBindBuffer(GL_ARRAY_BUFFER, g_vbo);
        glBufferData(GL_ARRAY_BUFFER, sizeof(QUAD), QUAD, GL_STATIC_DRAW);
        glEnableVertexAttribArray(0);
        glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, nullptr);
        glBindVertexArray(oldVao);
        glBindBuffer(GL_ARRAY_BUFFER, oldVbo);
    }
    return &p;
}

static bool isGlass(const PHLLS& ls) {
    return ls && ls->m_mapped && ls->visible() && ls->m_namespace.starts_with("eave") && (g_params.pids.empty() || std::ranges::contains(g_params.pids, ls->getPID()));
}

static std::vector<PHLLS> glassLayers(const PHLMONITOR& mon) {
    std::vector<PHLLS> out;
    for (auto layer : {ZWLR_LAYER_SHELL_V1_LAYER_TOP, ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY}) {
        for (auto const& ref : mon->m_layerSurfaceLayers[layer]) {
            auto ls = ref.lock();
            if (isGlass(ls))
                out.emplace_back(ls);
        }
    }
    return out;
}

// The layer's box on the monitor, in framebuffer pixels, as arranged. What's drawn can differ for
// a frame while it resizes, so the drawing goes by the pass instead (see SJob).
static CBox pixelBox(const PHLLS& ls, const PHLMONITOR& mon) {
    const auto pos  = ls->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
    const auto size = ls->size(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
    return CBox{pos - mon->m_position, size}.scale(mon->m_scale).round();
}

static float reach(const PHLMONITOR& mon) {
    return (g_params.strength * (1.F + g_params.chroma) + g_params.frost * (1 << g_params.depth) + 2.F) * mon->m_scale;
}

// The glass reads around every pixel it draws, so a change anywhere it can see redraws all of
// it. Added to this frame's record in the damage ring too, so older buffers catch up. Where the
// glass was last drawn counts too: a layer that just shrank leaves it there otherwise.
static void onBegin() {
    g_timing.rendered++;
    if (!g_params.enabled)
        return;

    auto& rd  = g_pHyprRenderer->m_renderData;
    auto  mon = rd.pMonitor.lock();
    if (!mon)
        return;

    CRegion extra;
    for (auto const& ls : glassLayers(mon)) {
        const auto box = pixelBox(ls, mon);
        if (!rd.damage.copy().intersect(box.copy().expand(reach(mon))).empty())
            extra.add(box);
    }
    auto& drawn = g_fbs[mon.get()].drawn;
    if (!extra.empty() || !rd.damage.copy().intersect(drawn.copy().expand(reach(mon))).empty())
        extra.add(drawn);
    if (extra.empty())
        return;

    g_timing.glassed++;
    rd.damage.add(extra);
    rd.finalDamage.add(extra);
    auto& ring = mon->m_damage;
    ring.m_previous[ring.m_previousIdx].add(extra);
}

// The backdrop copy only needs to hold the strips around the glass. It grows to the largest
// asked for and stays, so a popover sliding open doesn't reallocate every frame.
static void ensureBackdrop(SP<IFramebuffer>& fb, const SP<IFramebuffer>& like, const Vector2D& need) {
    if (!fb)
        fb = g_pHyprRenderer->createFB("eave-glass");
    const Vector2D size = {std::max(need.x, fb->m_size.x), std::max(need.y, fb->m_size.y)};
    if (!fb->isAllocated() || fb->m_size.x < need.x || fb->m_size.y < need.y || fb->m_drmFormat != like->m_drmFormat) {
        fb->release();
        fb->alloc(size.x, size.y, like->m_drmFormat); // must match the frame to be blitted into
    }
    fb->setImageDescription(like->imageDescription());
}

// One glass layer's surface in this frame's pass: the backdrop is taken just before it draws,
// and the glass keyed in just after, both at the box and with the buffer it draws.
struct SJob {
    PHLMONITORREF    mon;
    CBox             box; // framebuffer pixels
    SP<ITexture>     tex;
    float            fade = 1.F;
    CBox             copy; // the part of the frame the backdrop holds
    SP<IFramebuffer> sampled;
    Vector2D         level; // the sampled texture's size over the frame's
    bool             ready = false;
    int              pid   = 0;
    CBox             global; // the layer, logical px on the whole desktop
};

// How bright it is behind each glass surface, for eave: a grid of cells over the surface, read
// back without stalling (a pixel pack buffer and a fence, polled from a timer) and sent as a
// Hyprland custom event when it changed. At most every REPORT_EVERY, and once more after the
// last change, so eave hears where things settled.
static const char* GRID_FRAG = R"#(#version 300 es
precision highp float;
uniform sampler2D src;
uniform vec2 origin;   // the surface's top left, frame px
uniform float cell;    // frame px
uniform vec2 bdOrigin; // as in the glass
uniform vec2 bdScale;
uniform vec2 limit;    // the part of src that holds content, uv
out vec4 fragColor;
void main() {
    vec2  c = origin + gl_FragCoord.xy * cell; // the cell's middle
    float l = 0.0;
    for (int i = 0; i < 4; i++) {
        vec2 o  = (vec2(float(i & 1), float(i >> 1)) - 0.5) * 0.5 * cell;
        vec2 uv = clamp((c + o - bdOrigin) * bdScale, vec2(0.0), limit);
        l += dot(texture(src, uv).rgb, vec3(0.2126, 0.7152, 0.0722));
    }
    fragColor = vec4(l / 4.0, 0.0, 0.0, 1.0);
}
)#";

constexpr float REPORT_CELL  = 16.F; // logical px
constexpr auto  REPORT_EVERY = std::chrono::milliseconds(100);

static struct {
    GLuint                                program = 0;
    GLint                                 src, origin, cell, bdOrigin, bdScale, limit;
    SP<IFramebuffer>                      grid;
    GLuint                                pbo   = 0;
    GLsync                                fence = nullptr;
    int                                   pid = 0, cols = 0, rows = 0;
    std::chrono::steady_clock::time_point last;
    bool                                  missed = false; // a change came too soon after the last
    CBox                                  missedBox;      // global logical, to redraw for it
    std::unordered_map<int, std::string>  sent;           // by pid
    SP<CEventLoopTimer>                   timer;
    uint64_t                              read = 0, posted = 0; // for `stats`
} g_report;

static void armReport(std::chrono::milliseconds in) {
    if (g_report.timer && (!g_report.timer->armed() || g_report.timer->leftUs() > in.count() * 1000.F))
        g_report.timer->updateTimeout(in);
}

static bool differs(const std::string& a, const std::string& b) {
    if (a.size() != b.size())
        return true;
    for (size_t i = 0; i < a.size(); i++) {
        if (std::abs(std::stoi(std::string{a[i]}, nullptr, 16) - std::stoi(std::string{b[i]}, nullptr, 16)) >= 2)
            return true;
    }
    return false;
}

static void post(int pid, const std::string& cells, int cols, int rows) {
    g_pEventManager->postEvent(SHyprIPCEvent{.event = "custom", .data = std::format("eave-glass {} {} {} {} {}", pid, REPORT_CELL, cols, rows, cells)});
}

static void onReportTimer() {
    const auto now = std::chrono::steady_clock::now();
    if (g_report.fence) {
        g_pHyprOpenGL->makeEGLCurrent();
        const GLenum status = glClientWaitSync(g_report.fence, 0, 0);
        if (status != GL_ALREADY_SIGNALED && status != GL_CONDITION_SATISFIED) {
            armReport(std::chrono::milliseconds(4));
            return;
        }
        glDeleteSync(g_report.fence);
        g_report.fence = nullptr;
        const size_t n = sc<size_t>(g_report.cols) * g_report.rows;
        glBindBuffer(GL_PIXEL_PACK_BUFFER, g_report.pbo);
        const auto* px = sc<const uint8_t*>(glMapBufferRange(GL_PIXEL_PACK_BUFFER, 0, n * 4, GL_MAP_READ_BIT));
        std::string cells;
        if (px) {
            cells.reserve(n);
            for (size_t i = 0; i < n; i++)
                cells += "0123456789abcdef"[(px[i * 4] * 15 + 127) / 255];
            glUnmapBuffer(GL_PIXEL_PACK_BUFFER);
        }
        glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
        auto& sent = g_report.sent[g_report.pid];
        if (!cells.empty() && differs(cells, sent)) {
            sent = cells;
            g_report.posted++;
            post(g_report.pid, cells, g_report.cols, g_report.rows);
        }
    }
    if (g_report.missed) {
        const auto wait = std::chrono::duration_cast<std::chrono::milliseconds>(g_report.last + REPORT_EVERY - now);
        if (wait.count() > 0)
            armReport(wait);
        else {
            g_report.missed = false;
            g_pHyprRenderer->damageBox(g_report.missedBox); // the glass redraws, and reads again
        }
    }
}

// In the backdrop step, once it's frosted: the grid of the surface's cells, read back later.
static void reportBackdrop(const SJob& job, const PHLMONITOR& mon, const SP<IFramebuffer>& target) {
    const auto now = std::chrono::steady_clock::now();
    if (g_report.fence || now - g_report.last < REPORT_EVERY) {
        g_report.missed    = true;
        g_report.missedBox = job.global;
        armReport(std::chrono::milliseconds(4));
        return;
    }
    if (!g_report.program) {
        g_report.program = link(compile(GL_VERTEX_SHADER, PASS_VERT), compile(GL_FRAGMENT_SHADER, GRID_FRAG));
        if (!g_report.program)
            return;
        auto at             = [](const char* name) { return glGetUniformLocation(g_report.program, name); };
        g_report.src        = at("src");
        g_report.origin     = at("origin");
        g_report.cell       = at("cell");
        g_report.bdOrigin   = at("bdOrigin");
        g_report.bdScale    = at("bdScale");
        g_report.limit      = at("limit");
        g_report.timer      = makeShared<CEventLoopTimer>(std::nullopt, [](SP<CEventLoopTimer>, void*) { onReportTimer(); }, nullptr);
        g_pEventLoopManager->addTimer(g_report.timer);
        glGenBuffers(1, &g_report.pbo);
    }
    const float cell = REPORT_CELL * mon->m_scale;
    const int   cols = std::ceil(job.box.w / cell), rows = std::ceil(job.box.h / cell);
    if (cols <= 0 || rows <= 0)
        return;

    ensureBackdrop(g_report.grid, target, Vector2D{sc<double>(cols), sc<double>(rows)});
    glBindFramebuffer(GL_DRAW_FRAMEBUFFER, GLFB(g_report.grid)->getFBID());
    g_pHyprOpenGL->setViewport(0, 0, cols, rows);
    glBindVertexArray(g_vao);
    glUseProgram(g_report.program);
    glBindTexture(GL_TEXTURE_2D, job.sampled->getTexture()->m_texID);
    glUniform1i(g_report.src, 0);
    glUniform2f(g_report.origin, job.box.x, job.box.y);
    glUniform1f(g_report.cell, cell);
    glUniform2f(g_report.bdOrigin, job.copy.x, job.copy.y);
    glUniform2f(g_report.bdScale, job.level.x / job.sampled->m_size.x, job.level.y / job.sampled->m_size.y);
    glUniform2f(g_report.limit, job.level.x * job.copy.w / job.sampled->m_size.x, job.level.y * job.copy.h / job.sampled->m_size.y);
    glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);

    GLint oldRead = 0;
    glGetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &oldRead);
    glBindFramebuffer(GL_READ_FRAMEBUFFER, GLFB(g_report.grid)->getFBID());
    glBindBuffer(GL_PIXEL_PACK_BUFFER, g_report.pbo);
    glBufferData(GL_PIXEL_PACK_BUFFER, sc<GLsizeiptr>(cols) * rows * 4, nullptr, GL_STREAM_READ);
    glReadPixels(0, 0, cols, rows, GL_RGBA, GL_UNSIGNED_BYTE, nullptr);
    glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
    glBindFramebuffer(GL_READ_FRAMEBUFFER, oldRead);

    g_report.fence = glFenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0);
    g_report.pid   = job.pid;
    g_report.cols  = cols;
    g_report.rows  = rows;
    g_report.last  = now;
    g_report.read++;
    armReport(std::chrono::milliseconds(4));
}

struct SGLState {
    GLint prog = 0, vao = 0, active = 0, tex0 = 0;
    bool  scissor = false;
    void  save() {
        glGetIntegerv(GL_CURRENT_PROGRAM, &prog);
        glGetIntegerv(GL_VERTEX_ARRAY_BINDING, &vao);
        glGetIntegerv(GL_ACTIVE_TEXTURE, &active);
        glActiveTexture(GL_TEXTURE0);
        glGetIntegerv(GL_TEXTURE_BINDING_2D, &tex0);
        scissor = g_pHyprOpenGL->m_capStatus[CHyprOpenGLImpl::CAP_STATUS_SCISSOR_TEST];
        g_pHyprOpenGL->setCapStatus(GL_SCISSOR_TEST, false);
    }
    void restore() {
        glBindVertexArray(vao);
        glUseProgram(prog);
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D, tex0);
        glActiveTexture(active);
        g_pHyprOpenGL->setCapStatus(GL_SCISSOR_TEST, scissor);
    }
};

static std::chrono::steady_clock::time_point syncBegin() {
    if (g_timing.mode == g_timing.SYNC)
        glFinish();
    return std::chrono::steady_clock::now();
}

static void syncEnd(std::chrono::steady_clock::time_point start) {
    if (g_timing.mode != g_timing.SYNC)
        return;
    glFinish();
    const double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - start).count();
    g_timing.sum[T_SYNC] += ms;
    g_timing.worst[T_SYNC] = std::max(g_timing.worst[T_SYNC], ms);
    g_timing.n[T_SYNC]++;
}

// Before the layer: copy what's under and around it, and frost it.
static void takeBackdrop(SJob& job) {
    auto& rd  = g_pHyprRenderer->m_renderData;
    auto  mon = job.mon.lock();
    if (!g_params.enabled || !mon || !rd.currentFB || rd.damage.copy().intersect(job.box).empty())
        return;

    const auto target = rd.currentFB;
    job.copy          = job.box.copy().expand(reach(mon)).intersection(CBox{{}, target->m_size}).round();
    if (job.copy.empty())
        return;

    auto& fbs = g_fbs[mon.get()];
    ensureBackdrop(fbs.backdrop, target, job.copy.size());

    SGLState gl;
    gl.save();
    const auto start = syncBegin();

    // 1. Copy the backdrop around the glass into the corner of our buffer.
    timeBegin(T_COPY);
    GLint oldRead = 0;
    glGetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &oldRead);
    glBindFramebuffer(GL_READ_FRAMEBUFFER, GLFB(target)->getFBID());
    glBindFramebuffer(GL_DRAW_FRAMEBUFFER, GLFB(fbs.backdrop)->getFBID());
    const auto& c = job.copy;
    if (g_params.skip != 1)
        glBlitFramebuffer(c.x, c.y, c.x + c.w, c.y + c.h, 0, 0, c.w, c.h, GL_COLOR_BUFFER_BIT, GL_NEAREST);
    glBindFramebuffer(GL_READ_FRAMEBUFFER, oldRead);
    timeEnd();

    // 2. Frost it: blur down `depth` halvings and back up to a half.
    job.sampled    = fbs.backdrop;
    job.level      = {1, 1};
    const int  depth = std::clamp(g_params.depth, 1, 6);
    const bool frost = g_params.frost > 0 && ensurePass(g_down, DOWN_FRAG) && ensurePass(g_up, UP_FRAG) && ensureProgram(false);
    if (frost) {
        timeBegin(T_MASK);
        fbs.levels.resize(std::max<size_t>(fbs.levels.size(), depth));
        std::vector<Vector2D> sizes;
        for (int i = 0; i < depth; i++) {
            const double div = 2 << i;
            sizes.push_back({std::ceil(c.w / div), std::ceil(c.h / div)});
            ensureBackdrop(fbs.levels[i], target, sizes.back());
        }
        glBindVertexArray(g_vao);
        g_pHyprOpenGL->blend(false);

        auto pass = [&](SPassProgram& prog, const SP<IFramebuffer>& src, const Vector2D& srcContent, const SP<IFramebuffer>& dst, const Vector2D& dstContent) {
            glBindFramebuffer(GL_DRAW_FRAMEBUFFER, GLFB(dst)->getFBID());
            g_pHyprOpenGL->setViewport(0, 0, dstContent.x, dstContent.y); // through its cache, which bind() later trusts
            glUseProgram(prog.id);
            glBindTexture(GL_TEXTURE_2D, src->getTexture()->m_texID);
            glUniform1i(prog.src, 0);
            glUniform2f(prog.srcScale, srcContent.x / src->m_size.x, srcContent.y / src->m_size.y);
            glUniform2f(prog.texel, 1.0 / src->m_size.x, 1.0 / src->m_size.y);
            glUniform1f(prog.offset, g_params.frost);
            glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        };
        pass(g_down, fbs.backdrop, c.size(), fbs.levels[0], sizes[0]);
        for (int i = 1; i < depth; i++)
            pass(g_down, fbs.levels[i - 1], sizes[i - 1], fbs.levels[i], sizes[i]);
        for (int i = depth - 1; i > 0; i--)
            pass(g_up, fbs.levels[i], sizes[i], fbs.levels[i - 1], sizes[i - 1]);
        job.sampled = fbs.levels[0];
        job.level   = {sizes[0].x / c.w, sizes[0].y / c.h};
        timeEnd();
    }
    if (g_params.report)
        reportBackdrop(job, mon, target);
    g_pHyprOpenGL->blend(true);

    target->bind(); // and its viewport, for the layer that draws next
    gl.restore();
    syncEnd(start);
    job.ready = true;
}

// After the layer: key the glass in where its islands are, refracted at their edges.
static void keyGlass(SJob& job) {
    auto mon = job.mon.lock();
    if (!job.ready || !mon)
        return;
    const auto prog = ensureProgram(job.tex->m_type == Render::TEXTURE_EXTERNAL);
    if (!prog)
        return;

    auto&      rd     = g_pHyprRenderer->m_renderData;
    const auto fbSize = rd.currentFB->m_size;

    const bool external = job.tex->m_type == Render::TEXTURE_EXTERNAL;
    const auto attach   = ensureAttach(external);
    if (!attach)
        return;

    SGLState gl;
    gl.save();
    const auto start = syncBegin();

    timeBegin(T_REFRACT);
    // What hangs from the top edge, column by column, into a row of its own.
    auto& fbs = g_fbs[mon.get()];
    ensureBackdrop(fbs.attached, rd.currentFB, Vector2D{job.box.w, 1.0});
    g_pHyprOpenGL->blend(false);
    glBindFramebuffer(GL_DRAW_FRAMEBUFFER, GLFB(fbs.attached)->getFBID());
    g_pHyprOpenGL->setViewport(0, 0, job.box.w, 1);
    glBindVertexArray(g_vao);
    glUseProgram(attach->id);
    glUniform1i(attach->mask, 1);
    glUniform2f(attach->size, job.box.w, job.box.h);
    glUniform2f(attach->srcScale, 1, 1);
    glActiveTexture(GL_TEXTURE1);
    job.tex->bind();
    glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
    rd.currentFB->bind();

    glActiveTexture(GL_TEXTURE2);
    glBindTexture(GL_TEXTURE_2D, fbs.attached->getTexture()->m_texID);
    glActiveTexture(GL_TEXTURE0);
    glBindTexture(GL_TEXTURE_2D, job.sampled->getTexture()->m_texID);
    g_pHyprOpenGL->blend(true);
    glBindVertexArray(g_vao);

    const float S = mon->m_scale;
    glUseProgram(prog->id);
    glUniform1i(prog->backdrop, 0);
    glUniform1i(prog->mask, 1);
    glUniform1i(prog->attached, 2);
    glUniform2f(prog->fbSize, fbSize.x, fbSize.y);
    glUniform2f(prog->bdOrigin, job.copy.x, job.copy.y);
    glUniform2f(prog->bdScale, job.level.x / job.sampled->m_size.x, job.level.y / job.sampled->m_size.y);
    glUniform4f(prog->surf, job.box.x, job.box.y, job.box.w, job.box.h);
    glUniform1f(prog->fade, job.fade);
    glUniform1f(prog->band, g_params.band * S);
    glUniform1f(prog->strength, g_params.strength * S);
    glUniform1f(prog->chroma, g_params.chroma);
    glUniform1f(prog->rim, g_params.rim);
    glUniform1f(prog->clear, g_params.clear);
    glUniform1i(prog->inkMode, g_params.ink);
    glUniform1f(prog->contrast, g_params.contrast);
    glUniform1i(prog->mixed, g_params.report);
    glUniform1i(prog->colored, g_params.colored);
    glUniform1f(prog->tint, g_params.tint);
    glUniform4f(prog->rect, job.box.x, job.box.y, job.box.w, job.box.h);

    glActiveTexture(GL_TEXTURE1);
    job.tex->bind();
    if (g_params.skip != 2)
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
    job.tex->unbind();
    glActiveTexture(GL_TEXTURE2);
    glBindTexture(GL_TEXTURE_2D, 0);
    g_timing.pixels += job.box.w * job.box.h;
    timeEnd();

    g_pHyprOpenGL->blend(true); // back to Hyprland's premultiplied blending
    gl.restore();
    syncEnd(start);
}

class CGlassElement : public IPassElement {
  public:
    enum eStep : uint8_t {
        BACKDROP,
        KEY
    };
    CGlassElement(SP<SJob> job, eStep step) : m_job(std::move(job)), m_step(step) {}

    std::vector<UP<IPassElement>> draw() override {
        if (m_step == BACKDROP)
            takeBackdrop(*m_job);
        else
            keyGlass(*m_job);
        return {};
    }
    bool needsLiveBlur() override {
        return false;
    }
    bool needsPrecomputeBlur() override {
        return false;
    }
    const char* passName() override {
        return "CEaveGlassElement";
    }
    ePassElementType type() override {
        return EK_CUSTOM;
    }

  private:
    SP<SJob> m_job;
    eStep    m_step;
};

// Wrap each glass layer's own surface in the pass with the two steps, now that the pass is built.
static void wrapLayers() {
    auto& rd  = g_pHyprRenderer->m_renderData;
    auto  mon = rd.pMonitor.lock();
    if (!g_params.enabled || !mon)
        return;

    auto&   elements = g_pHyprRenderer->m_renderPass.m_passElements;
    CRegion drawn;
    for (size_t i = 0; i < elements.size(); i++) {
        if (elements[i].element->type() != EK_SURFACE)
            continue;
        auto* surf = sc<CSurfacePassElement*>(elements[i].element.get());
        auto& d    = surf->m_data;
        if (!d.mainSurface || d.popup || !isGlass(d.pLS) || !d.texture || !d.texture->ok())
            continue;

        auto job  = makeShared<SJob>();
        job->mon  = mon;
        job->box  = surf->getTexBox().scale(mon->m_scale).round();
        job->tex  = d.texture;
        job->fade = d.alpha * d.fadeAlpha;
        job->pid  = d.pLS->getPID();
        job->global = CBox{d.pLS->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT), d.pLS->size(Desktop::View::IGeometric::GEOMETRIC_CURRENT)};
        drawn.add(job->box);

        elements.insert(elements.begin() + i, {.element = makeUnique<CGlassElement>(job, CGlassElement::BACKDROP)});
        elements.insert(elements.begin() + i + 2, {.element = makeUnique<CGlassElement>(job, CGlassElement::KEY)});
        i += 2;
    }
    g_fbs[mon.get()].drawn = drawn;
}

static std::string pidList() {
    std::string out;
    for (const int pid : g_params.pids)
        out += std::format("{}{}", out.empty() ? "" : ",", pid);
    return out.empty() ? "0" : out;
}

static std::string onCommand(eHyprCtlOutputFormat, std::string args) {
    std::istringstream in(args);
    std::string        cmd, key, value;
    in >> cmd >> key >> value;

    if (key == "buffers") {
        std::string out;
        for (auto const& [mon, fbs] : g_fbs) {
            std::vector<SP<IFramebuffer>> all = {fbs.backdrop, fbs.attached};
            all.insert(all.end(), fbs.levels.begin(), fbs.levels.end());
            for (auto const& fb : all) {
                if (!fb)
                    continue;
                const auto name = NFormatUtils::drmFormatName(fb->m_drmFormat);
                const int  bpp  = name.contains("16") ? 8 : 4; // 16 bits a channel, or 8
                out += std::format("{} {} {}x{} {} bytes/px = {:.1f} MiB\n", mon->m_name, name, fb->m_size.x, fb->m_size.y, bpp, fb->m_size.x * fb->m_size.y * bpp / 1048576.0);
            }
        }
        return out.empty() ? "none yet\n" : out;
    }

    if (key.empty() || key == "show")
        return std::format("enabled {}\nband {}\nstrength {}\nchroma {}\nrim {}\nfrost {}\ndepth {}\nclear {}\ncolored {}\ntint {}\nink {}\ncontrast {}\nreport {}\npid {}\ntiming {}\n", g_params.enabled, g_params.band, g_params.strength,
                           g_params.chroma, g_params.rim, g_params.frost, g_params.depth, g_params.clear, g_params.colored, g_params.tint, g_params.ink, g_params.contrast, g_params.report, pidList(), sc<int>(g_timing.mode));

    if (key == "stats")
        return timingStats() + std::format("reports read {}, posted {}, pending {}\n", g_report.read, g_report.posted, g_report.fence != nullptr);

    auto setBool = [&](bool& b) {
        b = value == "1" || value == "on" || value == "true";
    };

    try {
        if (key == "enabled")
            setBool(g_params.enabled);
        else if (key == "timing")
            g_timing.mode = value == "stages" ? g_timing.STAGES : value == "frame" ? g_timing.FRAME : value == "sync" ? g_timing.SYNC : g_timing.OFF;
        else if (key == "band")
            g_params.band = std::stof(value);
        else if (key == "strength")
            g_params.strength = std::stof(value);
        else if (key == "chroma")
            g_params.chroma = std::stof(value);
        else if (key == "rim")
            g_params.rim = std::stof(value);
        else if (key == "frost")
            g_params.frost = std::stof(value);
        else if (key == "depth")
            g_params.depth = std::stoi(value);
        else if (key == "clear")
            g_params.clear = std::stof(value);
        else if (key == "colored")
            setBool(g_params.colored);
        else if (key == "tint")
            g_params.tint = std::stof(value);
        else if (key == "ink")
            g_params.ink = value == "off" ? 0 : value == "invert" ? 1 : value == "readable" ? 2 : std::stoi(value);
        else if (key == "contrast")
            g_params.contrast = std::stof(value);
        else if (key == "report") {
            setBool(g_params.report);
            if (!g_params.report) {
                for (auto const& [pid, cells] : g_report.sent)
                    post(pid, "", 0, 0);
                g_report.sent.clear();
            }
        }
        else if (key == "skip")
            g_params.skip = std::stoi(value);
        else if (key == "pid") {
            g_params.pids.clear();
            std::istringstream list(value);
            for (std::string pid; std::getline(list, pid, ',');) {
                if (const int n = std::stoi(pid))
                    g_params.pids.push_back(n);
            }
        }
        else
            return "unknown key\n";
    } catch (...) { return "bad value\n"; }

    for (auto const& m : State::monitorState()->monitors())
        g_pHyprRenderer->damageMonitor(m);
    return "ok\n";
}

APICALL EXPORT std::string PLUGIN_API_VERSION() {
    return HYPRLAND_API_VERSION;
}

APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE handle) {
    PHANDLE = handle;

    if (std::string{__hyprland_api_get_hash()} != __hyprland_api_get_client_hash())
        throw std::runtime_error("[eave-glass] built for a different Hyprland");

    g_hooks.stage = Event::bus()->m_events.render.stage.listen([](eRenderStage stage) {
        if (stage == RENDER_BEGIN)
            onBegin();
        else if (stage == RENDER_LAST_MOMENT) {
            wrapLayers();
            timeBegin(T_FRAME);
        }
        else if (stage == RENDER_POST) {
            timeEnd();
            timePoll();
        }
    });
    g_hooks.monitorRemoved = Event::bus()->m_events.monitor.destroyMon.listen([](PHLMONITOR mon) { g_fbs.erase(mon.get()); });
    g_hooks.command        = HyprlandAPI::registerHyprCtlCommand(PHANDLE, SHyprCtlCommand{.name = "eaveglass", .exact = false, .fn = onCommand});

    return {"eave-glass", "Refraction behind eave's surfaces (spike)", "eave", "0.0.1"};
}

APICALL EXPORT void PLUGIN_EXIT() {
    // A pass is cleared at the start of the next frame: an element left in it would be destroyed
    // by code that's no longer loaded.
    g_pHyprRenderer->m_renderPass.removeAllOfType("CEaveGlassElement");
    g_hooks = {};
    g_fbs.clear();
    if (g_report.timer)
        g_pEventLoopManager->removeTimer(g_report.timer);
    if (g_report.fence)
        glDeleteSync(g_report.fence);
    if (g_report.pbo)
        glDeleteBuffers(1, &g_report.pbo);
    if (g_report.program)
        glDeleteProgram(g_report.program);
    // Islands that turned light go back to dark.
    for (auto const& [pid, cells] : g_report.sent)
        post(pid, "", 0, 0);
    g_report = {};
    for (auto& p : g_progs) {
        if (p.id)
            glDeleteProgram(p.id);
    }
    for (auto& p : g_attach) {
        if (p.id)
            glDeleteProgram(p.id);
    }
    for (auto* p : {&g_down, &g_up}) {
        if (p->id)
            glDeleteProgram(p->id);
    }
    if (g_vao)
        glDeleteVertexArrays(1, &g_vao);
    if (g_vbo)
        glDeleteBuffers(1, &g_vbo);
}
