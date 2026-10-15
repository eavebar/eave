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
#include <hyprland/src/debug/log/Logger.hpp>
#include <hyprland/protocols/wlr-layer-shell-unstable-v1.hpp>
#undef private

using namespace Render;
using namespace Render::GL;

static HANDLE PHANDLE = nullptr;

// Logical pixels unless said otherwise; scaled by the monitor's scale when drawn.
static struct {
    bool  enabled  = true;
    float band     = 10.F;  // how far in from the edge the glass bends
    float strength = 14.F;  // the most a pixel's lookup moves, at the very edge
    float chroma   = 0.12F; // spread between red and blue lookups, as a fraction of the bend
    float rim      = 0.35F; // brightness of the lit edge
    float frost    = 1.5F;  // spread of each blur step (0 = clear glass)
    float clear    = 0.7F;  // how much of the backdrop shows through the island's color
    bool  light    = false; // white islands: multiply the glass in, instead of screening it
    int   pid      = 0;     // only this eave's layers (0 = every eave)
    int   skip     = 0;     // for measuring: 1 skips the copy, 2 the refraction
} g_params;

static struct {
    CHyprSignalListener stage, monitorRemoved;
    SP<SHyprCtlCommand> command;
} g_hooks;

struct SMonitorGlass {
    SP<IFramebuffer> backdrop;      // the frame around the glass, full size
    SP<IFramebuffer> half, quarter; // blurred down from it, for frost
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
    GLint  backdrop, mask, fbSize, bdOrigin, bdScale, surf, fade, band, strength, chroma, rim, clear, light, rect;
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
uniform bool  light;
out vec4 fragColor;

float cover(vec2 px) {
    vec2 uv = (px - surf.xy) / surf.zw;
    if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0))))
        return 0.0;
    return texture(mask, uv).a;
}

// Over the layer, which is already drawn. Dark: screen, so black island becomes glass and white
// ink stays white. Light: multiply, so white island becomes glass and black ink stays black. The
// blend function does the other half.
void put(vec3 glass, float a) {
    a *= fade;
    if (light)
        fragColor = vec4(mix(vec3(1.0), vec3(1.0 - clear) + clear * glass, a), 1.0);
    else
        fragColor = vec4(glass * clear * a, 1.0);
}

vec3 look(vec2 px) {
    return texture(backdrop, (px - bdOrigin) * bdScale).rgb;
}

void main() {
    vec2  p = gl_FragCoord.xy;
    float a = cover(p);
    if (a <= 0.0)
        discard;

    // Deep inside, nothing bends: eight taps a band away settle most of a popover's pixels
    // without the rings below.
    float inside = 1.0;
    for (int i = 0; i < 8; i++) {
        float ang = float(i) * 0.7853982;
        inside = min(inside, cover(p + vec2(cos(ang), sin(ang)) * band * 1.05));
    }
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
    vec2  light = normalize(vec2(-0.35, -1.0));
    float lit   = max(dot(n, light), 0.0) + 0.35 * max(dot(n, -light), 0.0);
    col += rim * lit * pow(edge, 3.0);

    put(col, a);
}
)#";

// Frost: a dual Kawase blur. Two steps down to a quarter, one back up to a half, each with
// linear filtering doing half the work, so it's smooth where single taps at a distance were
// grainy, and it runs on a sixteenth of the pixels.
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
    p.light    = at("light");
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
    return ls && ls->m_mapped && ls->visible() && ls->m_namespace.starts_with("eave") && (!g_params.pid || ls->getPID() == g_params.pid);
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
    return (g_params.strength * (1.F + g_params.chroma) + g_params.frost * 4.F + 2.F) * mon->m_scale;
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
};

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

    // 2. Frost it: blur down to a quarter and back up to a half.
    job.sampled    = fbs.backdrop;
    job.level      = {1, 1};
    const bool frost = g_params.frost > 0 && ensurePass(g_down, DOWN_FRAG) && ensurePass(g_up, UP_FRAG) && ensureProgram(false);
    if (frost) {
        timeBegin(T_MASK);
        const Vector2D half = {std::ceil(c.w / 2.0), std::ceil(c.h / 2.0)}, quarter = {std::ceil(c.w / 4.0), std::ceil(c.h / 4.0)};
        ensureBackdrop(fbs.half, target, half);
        ensureBackdrop(fbs.quarter, target, quarter);
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
        pass(g_down, fbs.backdrop, c.size(), fbs.half, half);
        pass(g_down, fbs.half, half, fbs.quarter, quarter);
        pass(g_up, fbs.quarter, quarter, fbs.half, half);
        job.sampled = fbs.half;
        job.level   = {half.x / c.w, half.y / c.h};
        g_pHyprOpenGL->blend(true);
        timeEnd();
    }

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

    SGLState gl;
    gl.save();
    const auto start = syncBegin();

    timeBegin(T_REFRACT);
    glBindTexture(GL_TEXTURE_2D, job.sampled->getTexture()->m_texID);
    g_pHyprOpenGL->blend(true);
    if (g_params.light)
        glBlendFuncSeparate(GL_ZERO, GL_SRC_COLOR, GL_ZERO, GL_ONE);
    else
        glBlendFuncSeparate(GL_ONE_MINUS_DST_COLOR, GL_ONE, GL_ZERO, GL_ONE);
    glBindVertexArray(g_vao);

    const float S = mon->m_scale;
    glUseProgram(prog->id);
    glUniform1i(prog->backdrop, 0);
    glUniform1i(prog->mask, 1);
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
    glUniform1i(prog->light, g_params.light);
    glUniform4f(prog->rect, job.box.x, job.box.y, job.box.w, job.box.h);

    glActiveTexture(GL_TEXTURE1);
    job.tex->bind();
    if (g_params.skip != 2)
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
    job.tex->unbind();
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
        drawn.add(job->box);

        elements.insert(elements.begin() + i, {.element = makeUnique<CGlassElement>(job, CGlassElement::BACKDROP)});
        elements.insert(elements.begin() + i + 2, {.element = makeUnique<CGlassElement>(job, CGlassElement::KEY)});
        i += 2;
    }
    g_fbs[mon.get()].drawn = drawn;
}

static std::string onCommand(eHyprCtlOutputFormat, std::string args) {
    std::istringstream in(args);
    std::string        cmd, key, value;
    in >> cmd >> key >> value;

    if (key == "buffers") {
        std::string out;
        for (auto const& [mon, fbs] : g_fbs) {
            for (auto const& fb : {fbs.backdrop, fbs.half, fbs.quarter}) {
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
        return std::format("enabled {}\nband {}\nstrength {}\nchroma {}\nrim {}\nfrost {}\nclear {}\nlight {}\npid {}\ntiming {}\n", g_params.enabled, g_params.band, g_params.strength,
                           g_params.chroma, g_params.rim, g_params.frost, g_params.clear, g_params.light, g_params.pid, sc<int>(g_timing.mode));

    if (key == "stats")
        return timingStats();

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
        else if (key == "clear")
            g_params.clear = std::stof(value);
        else if (key == "light")
            setBool(g_params.light);
        else if (key == "skip")
            g_params.skip = std::stoi(value);
        else if (key == "pid")
            g_params.pid = std::stoi(value);
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
    for (auto& p : g_progs) {
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
