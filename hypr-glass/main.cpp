// Spike: liquid-glass refraction behind eave's layer surfaces.
//
// Before the top and overlay layers draw, this copies what's on screen under every `eave*`
// layer, draws the layer's own pixels into a mask, and draws the backdrop refracted through the
// mask's edges. eave's translucent islands then draw on top as usual. The shape comes only from
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
    float frost    = 1.5F;  // radius of a light blur inside the glass (0 = clear)
} g_params;

static struct {
    CHyprSignalListener stage, monitorRemoved;
    SP<SHyprCtlCommand> command;
} g_hooks;

struct SMonitorGlass {
    SP<IFramebuffer> backdrop, mask;
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
    static const char* NAMES[] = {"copy", "mask", "refract", "frame", "sync"};
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

static GLuint g_prog = 0, g_vao = 0, g_vbo = 0;
static GLint  u_backdrop, u_mask, u_fbSize, u_band, u_strength, u_chroma, u_rim, u_frost, u_rect;

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

// Both textures are laid out like the framebuffer, so gl_FragCoord indexes all three alike.
// Framebuffer row 0 is the top of the screen, so "up" is -y.
static const char* FRAG = R"#(#version 300 es
precision highp float;
uniform sampler2D backdrop;
uniform sampler2D mask;
uniform vec2 fbSize;
uniform float band;     // px
uniform float strength; // px
uniform float chroma;
uniform float rim;
uniform float frost;    // px
out vec4 fragColor;

const float THRESHOLD = 0.08; // the islands are translucent: any alpha above this is glass

float cover(vec2 px) {
    return clamp(texture(mask, px / fbSize).a / THRESHOLD, 0.0, 1.0);
}

vec3 look(vec2 px) {
    if (frost <= 0.0)
        return texture(backdrop, px / fbSize).rgb;
    vec3 c = texture(backdrop, px / fbSize).rgb * 0.25;
    c += texture(backdrop, (px + vec2( frost,  frost)) / fbSize).rgb * 0.1875;
    c += texture(backdrop, (px + vec2(-frost,  frost)) / fbSize).rgb * 0.1875;
    c += texture(backdrop, (px + vec2( frost, -frost)) / fbSize).rgb * 0.1875;
    c += texture(backdrop, (px + vec2(-frost, -frost)) / fbSize).rgb * 0.1875;
    return c;
}

void main() {
    vec2  p = gl_FragCoord.xy;
    float a = cover(p);
    if (a <= 0.0)
        discard;

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

static GLuint compile(GLenum type, const char* src) {
    GLuint s = glCreateShader(type);
    glShaderSource(s, 1, &src, nullptr);
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

static bool ensureProgram() {
    if (g_prog)
        return true;

    GLuint vs = compile(GL_VERTEX_SHADER, VERT), fs = compile(GL_FRAGMENT_SHADER, FRAG);
    if (!vs || !fs)
        return false;

    g_prog = glCreateProgram();
    glAttachShader(g_prog, vs);
    glAttachShader(g_prog, fs);
    glLinkProgram(g_prog);
    glDeleteShader(vs);
    glDeleteShader(fs);
    GLint ok = 0;
    glGetProgramiv(g_prog, GL_LINK_STATUS, &ok);
    if (!ok) {
        char log[2048];
        glGetProgramInfoLog(g_prog, sizeof(log), nullptr, log);
        Log::logger->log(Log::ERR, "[eave-glass] link: {}", log);
        glDeleteProgram(g_prog);
        g_prog = 0;
        return false;
    }

    u_backdrop = glGetUniformLocation(g_prog, "backdrop");
    u_mask     = glGetUniformLocation(g_prog, "mask");
    u_fbSize   = glGetUniformLocation(g_prog, "fbSize");
    u_band     = glGetUniformLocation(g_prog, "band");
    u_strength = glGetUniformLocation(g_prog, "strength");
    u_chroma   = glGetUniformLocation(g_prog, "chroma");
    u_rim      = glGetUniformLocation(g_prog, "rim");
    u_frost    = glGetUniformLocation(g_prog, "frost");
    u_rect     = glGetUniformLocation(g_prog, "rect");

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
    return true;
}

static bool isGlass(const PHLLS& ls) {
    return ls && ls->m_mapped && ls->visible() && ls->m_namespace.starts_with("eave");
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
    return (g_params.strength * (1.F + g_params.chroma) + g_params.frost + 2.F) * mon->m_scale;
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

// `format` 0 copies the frame's format: the backdrop must match it to be blitted into.
static void ensureFB(SP<IFramebuffer>& fb, const SP<IFramebuffer>& like, const char* name, DRMFormat format = 0) {
    format = format ? format : like->m_drmFormat;
    if (!fb)
        fb = g_pHyprRenderer->createFB(name);
    if (fb->m_size != like->m_size || fb->m_drmFormat != format) {
        fb->release();
        fb->alloc(like->m_size.x, like->m_size.y, format);
    }
    // Drawing into it converts colors to its description; without one every draw logs an error.
    fb->setImageDescription(like->imageDescription());
}

static void drawGlass(const PHLMONITOR& mon) {
    auto& rd = g_pHyprRenderer->m_renderData;
    if (!g_params.enabled || !mon || !rd.currentFB || !ensureProgram())
        return;

    const auto layers = glassLayers(mon);
    if (layers.empty())
        return;


    const auto  target = rd.currentFB;
    const auto  fbSize = target->m_size;
    const float REACH  = reach(mon);
    auto&       fbs    = g_fbs[mon.get()];
    ensureFB(fbs.backdrop, target, "eave-glass backdrop");
    ensureFB(fbs.mask, target, "eave-glass mask", DRM_FORMAT_ARGB8888); // the frame may have no alpha; the mask is all alpha

    // Only what the damage asks for (onBegin made sure that's all of a layer or none of it).
    std::vector<std::pair<PHLLS, CBox>> todo;
    for (auto const& ls : layers) {
        const auto box = pixelBox(ls, mon);
        if (!rd.damage.copy().intersect(box).empty())
            todo.emplace_back(ls, box);
    }
    if (todo.empty())
        return;

    const bool scissorWas = g_pHyprOpenGL->m_capStatus[CHyprOpenGLImpl::CAP_STATUS_SCISSOR_TEST];
    g_pHyprOpenGL->setCapStatus(GL_SCISSOR_TEST, false);

    GLint oldRead = 0;
    glGetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &oldRead);

    const bool sync = g_timing.mode == g_timing.SYNC;
    if (sync)
        glFinish();
    const auto syncStart = std::chrono::steady_clock::now();

    // 1. Copy the backdrop, as far around each layer as the lookups reach.
    timeBegin(T_COPY);
    glBindFramebuffer(GL_READ_FRAMEBUFFER, GLFB(target)->getFBID());
    glBindFramebuffer(GL_DRAW_FRAMEBUFFER, GLFB(fbs.backdrop)->getFBID());
    for (auto const& [ls, box] : todo) {
        CBox r = box.copy().expand(REACH).intersection(CBox{{}, fbSize}).round();
        glBlitFramebuffer(r.x, r.y, r.x + r.w, r.y + r.h, r.x, r.y, r.x + r.w, r.y + r.h, GL_COLOR_BUFFER_BIT, GL_NEAREST);
    }
    glBindFramebuffer(GL_READ_FRAMEBUFFER, oldRead);
    timeEnd();

    // 2. Draw each layer's own pixels into the mask, the way Hyprland is about to draw them.
    timeBegin(T_MASK);
    {
        auto guard                               = g_pHyprRenderer->bindTempFB(fbs.mask);
        g_pHyprRenderer->m_bBlockSurfaceFeedback = true;
        for (auto const& [ls, box] : todo) {
            CRegion region{box.copy().expand(REACH).intersection(CBox{{}, fbSize})};
            g_pHyprRenderer->draw(CClearPassElement::SClearData{{0.F, 0.F, 0.F, 0.F}}, region);

            CSurfacePassElement::SRenderData data = {mon, Time::steadyNow(), ls->position(Desktop::View::IGeometric::GEOMETRIC_CURRENT)};
            const auto                       size = ls->size(Desktop::View::IGeometric::GEOMETRIC_CURRENT);
            data.fadeAlpha                        = ls->alpha()[Desktop::View::LS_ALPHA_FADE]->value();
            data.w                                = size.x;
            data.h                                = size.y;
            data.pLS                              = ls;
            data.decorate                         = false;
            data.clipBox                          = CBox{{}, fbSize};
            auto surf                             = ls->wlSurface()->resource();
            if (!surf || !surf->m_current.texture)
                continue;
            data.surface     = surf;
            data.texture     = surf->m_current.texture;
            data.mainSurface = true;
            g_pHyprRenderer->draw(data, region);
        }
        g_pHyprRenderer->m_bBlockSurfaceFeedback = false;
    }
    timeEnd();

    // 3. Refract the backdrop through the mask, into the frame.
    timeBegin(T_REFRACT);
    GLint oldProg = 0, oldVao = 0, oldActive = 0, oldTex0 = 0, oldTex1 = 0;
    glGetIntegerv(GL_CURRENT_PROGRAM, &oldProg);
    glGetIntegerv(GL_VERTEX_ARRAY_BINDING, &oldVao);
    glGetIntegerv(GL_ACTIVE_TEXTURE, &oldActive);
    glActiveTexture(GL_TEXTURE1);
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &oldTex1);
    glBindTexture(GL_TEXTURE_2D, fbs.mask->getTexture()->m_texID);
    glActiveTexture(GL_TEXTURE0);
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &oldTex0);
    glBindTexture(GL_TEXTURE_2D, fbs.backdrop->getTexture()->m_texID);

    target->bind();
    g_pHyprOpenGL->blend(true);
    glBlendFunc(GL_ONE, GL_ONE_MINUS_SRC_ALPHA);

    const float S = mon->m_scale;
    glUseProgram(g_prog);
    glUniform1i(u_backdrop, 0);
    glUniform1i(u_mask, 1);
    glUniform2f(u_fbSize, fbSize.x, fbSize.y);
    glUniform1f(u_band, g_params.band * S);
    glUniform1f(u_strength, g_params.strength * S);
    glUniform1f(u_chroma, g_params.chroma);
    glUniform1f(u_rim, g_params.rim);
    glUniform1f(u_frost, g_params.frost * S);
    glBindVertexArray(g_vao);
    for (auto const& [ls, box] : todo) {
        glUniform4f(u_rect, box.x, box.y, box.w, box.h);
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        g_timing.pixels += box.w * box.h;
    }
    timeEnd();

    glBindVertexArray(oldVao);
    glUseProgram(oldProg);
    glActiveTexture(GL_TEXTURE1);
    glBindTexture(GL_TEXTURE_2D, oldTex1);
    glActiveTexture(GL_TEXTURE0);
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
            for (auto const& fb : {fbs.backdrop, fbs.mask}) {
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
        return std::format("enabled {}\nband {}\nstrength {}\nchroma {}\nrim {}\nfrost {}\ntiming {}\n", g_params.enabled, g_params.band, g_params.strength, g_params.chroma,
                           g_params.rim, g_params.frost, sc<int>(g_timing.mode));

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
    if (g_prog)
        glDeleteProgram(g_prog);
    if (g_vao)
        glDeleteVertexArrays(1, &g_vao);
    if (g_vbo)
        glDeleteBuffers(1, &g_vbo);
}
