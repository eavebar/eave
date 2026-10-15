// How bright it is behind each glass surface that asked (set_report): a grid of cells over the
// surface, drawn from its frosted backdrop, read back without stalling (a pixel pack buffer and a
// fence, polled from a timer) and sent as a backdrop event when it changed. At most every
// REPORT_EVERY, and once more after the last change, so eave hears where things settled.

#include "Glass.hpp"

#include <hyprland/src/render/Renderer.hpp>
#include <hyprland/src/render/gl/GLFramebuffer.hpp>
#include <hyprland/src/managers/eventLoop/EventLoopManager.hpp>
#include <hyprland/src/managers/eventLoop/EventLoopTimer.hpp>

#include "shaders.inc"

using namespace Render;
using namespace Render::GL;

namespace Glass {
    GLuint link(const std::string& vert, const std::string& frag);
    GLuint quadVao();

    constexpr auto    REPORT_EVERY = std::chrono::milliseconds(100);
    // How much an island's average brightness must move (of 255) for a new report. eave only
    // needs to know when one crosses its thresholds, and per island, not per cell: text scrolling
    // behind the bar moves cells a lot and islands little.
    constexpr float NOTICEABLE = 16.F;

    static struct {
        GLuint              program = 0;
        GLint               src, cell, fbOrigin, toFb, bdOrigin, bdScale, bdLimit;
        SP<CEventLoopTimer> timer;
    } g_shared;

    static void arm(std::chrono::milliseconds in) {
        if (g_shared.timer && (!g_shared.timer->armed() || g_shared.timer->leftUs() > in.count() * 1000.F))
            g_shared.timer->updateTimeout(in);
    }

    void SReport::release() {
        if (fence)
            glDeleteSync(fence);
        if (pbo)
            glDeleteBuffers(1, &pbo);
        fence = nullptr;
        pbo   = 0;
        grid.reset();
    }

    // The average of `cells` under `region`, cells `cell` logical px square.
    static float under(const SRegion& region, const std::vector<uint8_t>& cells, uint32_t cell, uint32_t cols, uint32_t rows) {
        const auto span = [cell](int32_t from, int32_t size, uint32_t count) {
            const auto first = std::clamp<int64_t>(from / sc<int64_t>(cell), 0, count);
            const auto last  = std::clamp<int64_t>((sc<int64_t>(from) + size + cell - 1) / cell, 0, count);
            return std::pair{first, last};
        };
        const auto [x0, x1] = span(region.x, region.w, cols);
        const auto [y0, y1] = span(region.y, region.h, rows);
        float      sum = 0.F, n = 0.F;
        for (auto y = y0; y < y1; y++) {
            for (auto x = x0; x < x1; x++) {
                sum += cells[y * cols + x];
                n += 1.F;
            }
        }
        return n > 0.F ? sum / n : 0.F;
    }

    // Whether a read is worth sending: the grid changed shape, or an island's average moved.
    static bool noticeable(const CGlassSurface& glass, const std::vector<uint8_t>& cells) {
        const auto& r = glass.m_report;
        if (r.sentCell != r.cell || r.sentCols != r.cols || r.sentRows != r.rows || r.sent.size() != cells.size())
            return true;
        for (auto const& region : glass.m_current.regions) {
            if (region.kind == EAVE_GLASS_SURFACE_V1_KIND_SOLID)
                continue;
            if (std::abs(under(region, cells, r.cell, r.cols, r.rows) - under(region, r.sent, r.cell, r.cols, r.rows)) >= NOTICEABLE)
                return true;
        }
        return false;
    }

    // One surface's read, if it's done: sent when it's new enough to matter.
    static bool collect(CGlassSurface& glass) {
        auto& r = glass.m_report;
        if (!r.fence)
            return true;
        const GLenum status = glClientWaitSync(r.fence, 0, 0);
        if (status != GL_ALREADY_SIGNALED && status != GL_CONDITION_SATISFIED)
            return false;
        glDeleteSync(r.fence);
        r.fence = nullptr;

        const size_t n = sc<size_t>(r.cols) * r.rows;
        std::vector<uint8_t> cells;
        glBindBuffer(GL_PIXEL_PACK_BUFFER, r.pbo);
        if (const auto* px = sc<const uint8_t*>(glMapBufferRange(GL_PIXEL_PACK_BUFFER, 0, n * 4, GL_MAP_READ_BIT))) {
            cells.resize(n);
            for (size_t i = 0; i < n; i++)
                cells[i] = px[i * 4];
            glUnmapBuffer(GL_PIXEL_PACK_BUFFER);
        }
        glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
        if (!cells.empty() && noticeable(glass, cells)) {
            glass.sendBackdrop(r.cell, r.cols, r.rows, cells);
            r.sent     = std::move(cells);
            r.sentCell = r.cell;
            r.sentCols = r.cols;
            r.sentRows = r.rows;
            r.posted++;
        }
        return true;
    }

    static void onTimer() {
        try {
            g_pHyprOpenGL->makeEGLCurrent();
            const auto now     = Clock::now();
            bool       waiting = false;
            for (auto* glass : allGlass()) {
                auto& r = glass->m_report;
                if (!collect(*glass)) {
                    waiting = true;
                    continue;
                }
                if (r.missed) {
                    const auto wait = std::chrono::duration_cast<std::chrono::milliseconds>(r.last + REPORT_EVERY - now);
                    if (wait.count() > 0)
                        arm(wait);
                    else if (const auto surface = glass->m_surface.lock()) {
                        // The glass redraws, and reads again.
                        r.missed = false;
                        auto hl  = Desktop::View::CWLSurface::fromResource(surface);
                        if (const auto box = hl ? hl->getSurfaceBoxGlobal() : std::nullopt)
                            g_pHyprRenderer->damageBox(*box);
                    }
                }
            }
            if (waiting)
                arm(std::chrono::milliseconds(4));
        } catch (const std::exception& e) { log(std::format("report: {}", e.what())); }
    }

    // In the backdrop step, once it's frosted: the grid of the surface's cells, read back later.
    void reportBackdrop(CGlassSurface& glass, const SJob& job) {
        auto&      r   = glass.m_report;
        const auto now = Clock::now();
        if (r.fence || now - r.last < REPORT_EVERY) {
            r.missed = true;
            arm(std::chrono::milliseconds(4));
            return;
        }
        if (!g_shared.program) {
            g_shared.program = link(SHADER_pass_vert, SHADER_grid_frag);
            if (!g_shared.program)
                return;
            auto at           = [](const char* name) { return glGetUniformLocation(g_shared.program, name); };
            g_shared.src      = at("src");
            g_shared.cell     = at("cell");
            g_shared.fbOrigin = at("fbOrigin");
            g_shared.toFb     = at("toFb");
            g_shared.bdOrigin = at("bdOrigin");
            g_shared.bdScale  = at("bdScale");
            g_shared.bdLimit  = at("bdLimit");
        }
        if (!g_shared.timer) {
            g_shared.timer = makeShared<CEventLoopTimer>(std::nullopt, [](SP<CEventLoopTimer>, void*) { onTimer(); }, nullptr);
            g_pEventLoopManager->addTimer(g_shared.timer);
        }

        // Cells as small as eave asked, or bigger, so the report fits in one message.
        uint32_t cellPx = job.state.reportCell;
        int      cols = 0, rows = 0;
        for (;; cellPx *= 2) {
            const float cell = cellPx * job.scale;
            cols             = std::ceil(job.box.w / cell);
            rows             = std::ceil(job.box.h / cell);
            if (cols <= 0 || rows <= 0)
                return;
            if (sc<size_t>(cols) * rows <= MAX_REPORT_CELLS)
                break;
        }
        const float cell = cellPx * job.scale;
        // Always 8 bits a channel, whatever the frame's format, so it reads back as bytes.
        if (!r.grid)
            r.grid = g_pHyprRenderer->createFB("hypr-eave report");
        if (!r.grid->isAllocated() || r.grid->m_size.x < cols || r.grid->m_size.y < rows) {
            r.grid->release();
            if (!r.grid->alloc(std::max<int>(cols, r.grid->m_size.x), std::max<int>(rows, r.grid->m_size.y))) {
                fail("couldn't allocate a buffer");
                return;
            }
        }
        if (!r.pbo)
            glGenBuffers(1, &r.pbo);

        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, GLFB(r.grid)->getFBID());
        g_pHyprOpenGL->setViewport(0, 0, cols, rows);
        g_pHyprOpenGL->blend(false);
        glBindVertexArray(quadVao());
        glUseProgram(g_shared.program);
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D, job.sampled->getTexture()->m_texID);
        glUniform1i(g_shared.src, 0);
        glUniform1f(g_shared.cell, cell);
        glUniform2f(g_shared.fbOrigin, job.fbOrigin.x, job.fbOrigin.y);
        glUniformMatrix2fv(g_shared.toFb, 1, GL_FALSE, job.toFb);
        glUniform2f(g_shared.bdOrigin, job.copy.x, job.copy.y);
        glUniform2f(g_shared.bdScale, job.level.x / job.sampled->m_size.x, job.level.y / job.sampled->m_size.y);
        glUniform2f(g_shared.bdLimit, job.level.x * job.copy.w / job.sampled->m_size.x, job.level.y * job.copy.h / job.sampled->m_size.y);
        glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);

        glBindFramebuffer(GL_READ_FRAMEBUFFER, GLFB(r.grid)->getFBID());
        glBindBuffer(GL_PIXEL_PACK_BUFFER, r.pbo);
        glBufferData(GL_PIXEL_PACK_BUFFER, sc<GLsizeiptr>(cols) * rows * 4, nullptr, GL_STREAM_READ);
        glReadPixels(0, 0, cols, rows, GL_RGBA, GL_UNSIGNED_BYTE, nullptr);
        glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);

        r.fence = glFenceSync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0);
        r.cell  = cellPx;
        r.cols  = cols;
        r.rows  = rows;
        r.last  = now;
        r.reads++;
        arm(std::chrono::milliseconds(4));
    }

    void freeReports() {
        if (g_shared.timer) {
            g_shared.timer->cancel();
            g_pEventLoopManager->removeTimer(g_shared.timer);
        }
        if (g_shared.program)
            glDeleteProgram(g_shared.program);
        g_shared = {};
    }
}
