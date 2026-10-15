// Spike: liquid-glass refraction behind eave's layer surfaces.
//
// Before the top and overlay layers draw, this copies what's on screen around every `eave*`
// layer and draws it refracted through the edges of the layer's own buffer. eave's translucent islands then draw on top as usual. The shape comes only from
// the surface's alpha in the frame being drawn, so the glass can't lag a morph.
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
    int   pid      = 0;     // only this eave's layers (0 = every eave)
    int   skip     = 0;     // for measuring: 1 skips the copy, 2 the refraction
} g_params;

static struct {
    CHyprSignalListener stage, monitorRemoved;
    SP<SHyprCtlCommand> command;
} g_hooks;

struct SMonitorGlass {
    SP<IFramebuffer> backdrop;   // the frame around the glass, full size
    SP<IFramebuffer> half, quarter; // blurred down from it, for frost
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
    GLint  backdrop, mask, fbSize, bdOrigin, bdScale, surf, fade, band, strength, chroma, rim, rect;
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
out vec4 fragColor;

const float THRESHOLD = 0.08; // the islands are translucent: any alpha above this is glass

float cover(vec2 px) {
    vec2 uv = (px - surf.xy) / surf.zw;
    if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0))))
        return 0.0;
    return clamp(texture(mask, uv).a * fade / THRESHOLD, 0.0, 1.0);
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
        fragColor = vec4(look(p) * a, a);
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

    fragColor = vec4(col * a, a);
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

// The layer's box on the monitor, in framebuffer pixels.
static CBox pixelBox(const PHLLS& ls, const PHLMONITOR& mon) {
    const auto pos  = ls->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
    const auto size = ls->size(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
    return CBox{pos - mon->m_position, size}.scale(mon->m_scale).round();
}

static float reach(const PHLMONITOR& mon) {
    return (g_params.strength * (1.F + g_params.chroma) + g_params.frost * 4.F + 2.F) * mon->m_scale;
}

// The glass reads around every pixel it draws, so a change anywhere it can see redraws all of
// it. Added to this frame's record in the damage ring too, so older buffers catch up.
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

static void drawGlass(const PHLMONITOR& mon) {
    auto& rd = g_pHyprRenderer->m_renderData;
    if (!g_params.enabled || !mon || !rd.currentFB)
        return;

    const auto  target = rd.currentFB;
    const auto  fbSize = target->m_size;
    const float REACH  = reach(mon);

    // Only what the damage asks for (onBegin made sure that's all of a layer or none of it).
    struct SJob {
        PHLLS        ls;
        CBox         box;
        SP<ITexture> tex;
    };
    std::vector<SJob> todo;
    CBox              copy; // everything the lookups can reach
    for (auto const& ls : glassLayers(mon)) {
        const auto box  = pixelBox(ls, mon);
        auto       surf = ls->wlSurface()->resource();
        if (rd.damage.copy().intersect(box).empty() || !surf || !surf->m_current.texture || !surf->m_current.texture->ok())
            continue;
        todo.push_back({ls, box, surf->m_current.texture});
        const auto r = box.copy().expand(REACH).intersection(CBox{{}, fbSize});
        copy         = copy.empty() ? r : CRegion{copy}.add(r).getExtents();
    }
    if (todo.empty())
        return;
    copy.round();

    auto& fbs = g_fbs[mon.get()];
    ensureBackdrop(fbs.backdrop, target, copy.size());

    const bool scissorWas = g_pHyprOpenGL->m_capStatus[CHyprOpenGLImpl::CAP_STATUS_SCISSOR_TEST];
    g_pHyprOpenGL->setCapStatus(GL_SCISSOR_TEST, false);

    const bool sync = g_timing.mode == g_timing.SYNC;
    if (sync)
        glFinish();
    const auto syncStart = std::chrono::steady_clock::now();

    // 1. Copy the backdrop around the glass into the corner of our buffer.
    timeBegin(T_COPY);
    GLint oldRead = 0;
    glGetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &oldRead);
    glBindFramebuffer(GL_READ_FRAMEBUFFER, GLFB(target)->getFBID());
    glBindFramebuffer(GL_DRAW_FRAMEBUFFER, GLFB(fbs.backdrop)->getFBID());
    if (g_params.skip != 1)
        glBlitFramebuffer(copy.x, copy.y, copy.x + copy.w, copy.y + copy.h, 0, 0, copy.w, copy.h, GL_COLOR_BUFFER_BIT, GL_NEAREST);
    glBindFramebuffer(GL_READ_FRAMEBUFFER, oldRead);
    timeEnd();

    // 2. Frost it: blur down to a quarter and back up to a half.
    GLint oldProg = 0, oldVao = 0, oldActive = 0, oldTex0 = 0;
    glGetIntegerv(GL_CURRENT_PROGRAM, &oldProg);
    glGetIntegerv(GL_VERTEX_ARRAY_BINDING, &oldVao);
    glGetIntegerv(GL_ACTIVE_TEXTURE, &oldActive);
    glActiveTexture(GL_TEXTURE0);
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &oldTex0);

    SP<IFramebuffer> sampled = fbs.backdrop;
    Vector2D         level   = {1, 1}; // the sampled texture's size over the frame's
    const bool       frost   = g_params.frost > 0 && ensurePass(g_down, DOWN_FRAG) && ensurePass(g_up, UP_FRAG) && ensureProgram(false);
    if (frost) {
        timeBegin(T_MASK);
        const Vector2D half = {std::ceil(copy.w / 2.0), std::ceil(copy.h / 2.0)}, quarter = {std::ceil(copy.w / 4.0), std::ceil(copy.h / 4.0)};
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
        pass(g_down, fbs.backdrop, copy.size(), fbs.half, half);
        pass(g_down, fbs.half, half, fbs.quarter, quarter);
        pass(g_up, fbs.quarter, quarter, fbs.half, half);
        sampled = fbs.half;
        level   = {half.x / copy.w, half.y / copy.h};
        timeEnd();
    }

    // 3. Refract it through each layer's own buffer, into the frame.
    timeBegin(T_REFRACT);
    glBindTexture(GL_TEXTURE_2D, sampled->getTexture()->m_texID);

    target->bind(); // and its viewport
    g_pHyprOpenGL->blend(true);
    glBlendFunc(GL_ONE, GL_ONE_MINUS_SRC_ALPHA);
    glBindVertexArray(g_vao);

    const float S = mon->m_scale;
    for (auto const& job : todo) {
        const auto prog = ensureProgram(job.tex->m_type == Render::TEXTURE_EXTERNAL);
        if (!prog)
            continue;
        glBindVertexArray(g_vao); // ensureProgram may have just made it
        glUseProgram(prog->id);
        glUniform1i(prog->backdrop, 0);
        glUniform1i(prog->mask, 1);
        glUniform2f(prog->fbSize, fbSize.x, fbSize.y);
        glUniform2f(prog->bdOrigin, copy.x, copy.y);
        glUniform2f(prog->bdScale, level.x / sampled->m_size.x, level.y / sampled->m_size.y);
        glUniform4f(prog->surf, job.box.x, job.box.y, job.box.w, job.box.h);
        glUniform1f(prog->fade, job.ls->alpha()[Desktop::View::LS_ALPHA_FADE]->value());
        glUniform1f(prog->band, g_params.band * S);
        glUniform1f(prog->strength, g_params.strength * S);
        glUniform1f(prog->chroma, g_params.chroma);
        glUniform1f(prog->rim, g_params.rim);
        glUniform4f(prog->rect, job.box.x, job.box.y, job.box.w, job.box.h);

        glActiveTexture(GL_TEXTURE1);
        job.tex->bind();
        if (g_params.skip != 2)
            glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        job.tex->unbind();
        glActiveTexture(GL_TEXTURE0);
        g_timing.pixels += job.box.w * job.box.h;
    }
    timeEnd();

    glBindVertexArray(oldVao);
    glUseProgram(oldProg);
    glBindTexture(GL_TEXTURE_2D, oldTex0);
    glActiveTexture(oldActive);
    g_pHyprOpenGL->setCapStatus(GL_SCISSOR_TEST, scissorWas);

    if (sync) {
        glFinish();
        const double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - syncStart).count();
        g_timing.sum[T_SYNC] += ms;
        g_timing.worst[T_SYNC] = std::max(g_timing.worst[T_SYNC], ms);
        g_timing.n[T_SYNC]++;
    }
}

class CGlassElement : public IPassElement {
  public:
    CGlassElement(PHLMONITORREF mon) : m_mon(mon) {}

    std::vector<UP<IPassElement>> draw() override {
        drawGlass(m_mon.lock());
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
    PHLMONITORREF m_mon;
};

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
        return std::format("enabled {}\nband {}\nstrength {}\nchroma {}\nrim {}\nfrost {}\npid {}\ntiming {}\n", g_params.enabled, g_params.band, g_params.strength, g_params.chroma,
                           g_params.rim, g_params.frost, g_params.pid, sc<int>(g_timing.mode));

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
        else if (stage == RENDER_LAST_MOMENT)
            timeBegin(T_FRAME);
        else if (stage == RENDER_POST) {
            timeEnd();
            timePoll();
        }
        else if (stage == RENDER_POST_WINDOWS && g_params.enabled)
            g_pHyprRenderer->addPassElement(makeUnique<CGlassElement>(g_pHyprRenderer->m_renderData.pMonitor));
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
