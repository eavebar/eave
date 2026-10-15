// eave_glass_v1, the compositor's side: a global, and per wl_surface what eave says about its
// frames, latched on the surface's commit like ext-background-effect-v1 does.
//
// Unloading can't take the objects clients hold away from them: a request on one would call into
// code that's gone, and destroying them under the client is a protocol error the next time it uses
// them. So the plugin stays mapped after it's unloaded (RTLD_NODELETE, Plugin.cpp), and everything
// made before the unload turns inert: it answers nothing, does nothing, and goes away only when the
// client destroys it. A new load starts a new generation with a new global.

#include "Glass.hpp"

#include <cstring>
#include <memory>

#include <hyprland/src/protocols/WaylandProtocol.hpp>
#include <hyprland/src/protocols/core/Compositor.hpp>
#include <hyprland/src/desktop/view/WLSurface.hpp>
#include <hyprland/src/render/Renderer.hpp>

using namespace Render::GL;

namespace Glass {
    // Each load's objects remember theirs; an unload moves on, which makes them all inert.
    static uint32_t g_generation = 1;

    class CProtocol;
    // Leaked on unload with everything it holds, so no destructor of ours runs at exit.
    static CProtocol* g_protocol = nullptr;

    template <typename F>
    static void guarded(const char* what, F&& fn) {
        try {
            fn();
        } catch (const std::exception& e) { log(std::format("{}: {}", what, e.what())); } catch (...) {
            log(std::format("{}: unknown exception", what));
        }
    }

    class CProtocol : public IWaylandProtocol {
      public:
        CProtocol() : IWaylandProtocol(&eave_glass_manager_v1_interface, 1, "EaveGlass") {}

        void bindManager(wl_client* client, void*, uint32_t ver, uint32_t id) override {
            guarded("bind", [&] {
                auto manager = makeShared<CEaveGlassManagerV1>(client, ver, id);
                if (!manager->resource()) {
                    wl_client_post_no_memory(client);
                    return;
                }
                const uint32_t generation = m_generation;
                manager->setOnDestroy([this, generation](CEaveGlassManagerV1* m) {
                    if (generation == g_generation)
                        std::erase_if(m_managers, [m](const auto& o) { return o.get() == m; });
                });
                manager->setDestroy([this, generation](CEaveGlassManagerV1* m) {
                    if (generation != g_generation) {
                        wl_resource_destroy(m->resource());
                        return;
                    }
                    std::erase_if(m_managers, [m](const auto& o) { return o.get() == m; });
                });
                manager->setGetGlass([this, generation](CEaveGlassManagerV1* m, uint32_t id, wl_resource* surface) {
                    guarded("get_glass", [&] { getGlass(m, id, surface, generation); });
                });
                m_managers.emplace_back(std::move(manager));
            });
        }

        void getGlass(CEaveGlassManagerV1* manager, uint32_t id, wl_resource* surfaceResource, uint32_t generation) {
            auto resource = makeShared<CEaveGlassSurfaceV1>(manager->client(), manager->version(), id);
            if (!resource->resource()) {
                manager->noMemory();
                return;
            }
            if (generation != g_generation) {
                // Bound before an unload: an object that does nothing, until the client lets go.
                resource->setDestroy([](CEaveGlassSurfaceV1* r) { wl_resource_destroy(r->resource()); });
                m_inert.emplace_back(std::move(resource));
                return;
            }
            auto surface = CWLSurfaceResource::fromResource(surfaceResource);
            if (!surface)
                return; // libwayland checked it's a wl_surface: it's one going away
            if (m_surfaces.contains(surface.get())) {
                manager->error(EAVE_GLASS_MANAGER_V1_ERROR_ALREADY_EXISTS, "the surface already has glass");
                return;
            }
            auto* key = surface.get();
            m_surfaces.emplace(key, std::make_unique<CGlassSurface>(resource, surface));
        }

        void forget(const CWLSurfaceResource* surface) {
            m_surfaces.erase(surface);
        }

        CGlassSurface* glassOf(const CWLSurfaceResource* surface) {
            const auto it = m_surfaces.find(surface);
            return it == m_surfaces.end() ? nullptr : it->second.get();
        }

        std::vector<CGlassSurface*> all() {
            std::vector<CGlassSurface*> out;
            for (auto const& [_, glass] : m_surfaces)
                out.push_back(glass.get());
            return out;
        }

        // Unloading: every surface hears its glass is gone, and every object turns inert.
        void stop() {
            for (auto& [_, glass] : m_surfaces) {
                glass->sendActive(false);
                glass->makeInert();
                (void)glass.release(); // leaked: its resource lives until the client lets go
            }
            m_surfaces.clear();
            removeGlobal();
            g_generation++;
        }

        uint32_t m_generation = g_generation;

      private:
        std::vector<SP<CEaveGlassManagerV1>>                                m_managers;
        std::unordered_map<const CWLSurfaceResource*, std::unique_ptr<CGlassSurface>> m_surfaces;
        std::vector<SP<CEaveGlassSurfaceV1>>                                m_inert;
    };

    CGlassSurface::CGlassSurface(SP<CEaveGlassSurfaceV1> resource, SP<CWLSurfaceResource> surface) : m_resource(std::move(resource)), m_surface(surface) {
        const uint32_t generation = g_generation;
        const auto*    key        = surface.get();

        m_resource->setDestroy([key, generation](CEaveGlassSurfaceV1* r) {
            if (generation != g_generation) {
                wl_resource_destroy(r->resource());
                return;
            }
            // The glass goes on the surface's next commit, so the frame without it lands whole.
            if (auto* glass = Glass::glassOf(key)) {
                glass->m_pending  = {};
                glass->m_resource = nullptr; // destroys the wl_resource
            }
        });
        m_resource->setOnDestroy([key, generation](CEaveGlassSurfaceV1*) {
            if (generation != g_generation)
                return;
            if (auto* glass = Glass::glassOf(key)) {
                glass->m_pending = {};
                glass->m_resource.reset();
            }
        });
        m_resource->setSetRegions([key, generation](CEaveGlassSurfaceV1*, wl_array* array) {
            if (generation != g_generation)
                return;
            auto* glass = Glass::glassOf(key);
            if (!glass)
                return;
            constexpr size_t RECORD = 6 * sizeof(int32_t);
            const size_t     n      = std::min(array->size / RECORD, MAX_REGIONS);
            const auto*      data   = sc<const int32_t*>(array->data);
            auto&            out    = glass->m_pending.regions;
            out.clear();
            for (size_t i = 0; i < n; i++) {
                const int32_t* r = data + i * 6;
                if (r[2] <= 0 || r[3] <= 0 || sc<uint32_t>(r[4]) > EAVE_GLASS_SURFACE_V1_KIND_SOLID)
                    continue;
                out.push_back({.x = r[0], .y = r[1], .w = r[2], .h = r[3], .kind = sc<uint32_t>(r[4]), .fill = sc<uint32_t>(r[5])});
            }
        });
        m_resource->setSetLook([key, generation](CEaveGlassSurfaceV1*, wl_fixed_t band, wl_fixed_t strength, wl_fixed_t chroma, wl_fixed_t rim, wl_fixed_t frost, uint32_t depth,
                                                 wl_fixed_t tint, eaveGlassSurfaceV1Ink ink, wl_fixed_t contrast, wl_fixed_t inkBlur) {
            if (generation != g_generation)
                return;
            auto* glass = Glass::glassOf(key);
            if (!glass)
                return;
            const auto f = [](wl_fixed_t v, float lo, float hi) { return std::clamp(sc<float>(wl_fixed_to_double(v)), lo, hi); };
            glass->m_pending.look = {
                .band     = f(band, 0.F, 64.F),
                .strength = f(strength, 0.F, 128.F),
                .chroma   = f(chroma, 0.F, 1.F),
                .rim      = f(rim, 0.F, 2.F),
                .frost    = f(frost, 0.F, 8.F),
                .depth    = std::clamp(sc<int>(depth), 1, 6),
                .tint     = f(tint, 0.F, 1.F),
                .ink      = std::min(sc<int>(ink), sc<int>(EAVE_GLASS_SURFACE_V1_INK_READABLE)),
                .contrast = f(contrast, 1.F, 21.F),
                .inkBlur  = f(inkBlur, 0.F, 64.F),
            };
        });
        m_resource->setSetReport([key, generation](CEaveGlassSurfaceV1*, uint32_t cell) {
            if (generation != g_generation)
                return;
            if (auto* glass = Glass::glassOf(key))
                glass->m_pending.reportCell = cell == 0 ? 0 : std::clamp(cell, 4U, 256U);
        });

        m_listeners.commit = surface->m_events.commit.listen([this, key] {
            guarded("commit", [&] {
                const bool changed = m_current != m_pending;
                m_current          = m_pending;
                // Stopped asking for reports: say there's nothing to report, once.
                if (!m_current.reportCell && !m_report.sent.empty()) {
                    sendBackdrop(0, 0, 0, {});
                    m_report.sent.clear();
                    m_report.sentCell = m_report.sentCols = m_report.sentRows = 0;
                }
                if (!m_resource) {
                    // Destroyed: this commit is the first frame without glass.
                    g_protocol->forget(key); // destroys this
                    return;
                }
                // The glass reads around every pixel it draws, so a change to what eave says is
                // a change to all of it. The frame's own damage is grown in onRenderBegin.
                if (!changed)
                    return;
                auto hl = Desktop::View::CWLSurface::fromResource(m_surface.lock());
                if (const auto box = hl ? hl->getSurfaceBoxGlobal() : std::nullopt)
                    g_pHyprRenderer->damageBox(*box);
            });
        });
        m_listeners.destroy = surface->m_events.destroy.listen([key] {
            // The surface is gone: the object is inert until the client destroys it.
            guarded("surface destroy", [&] {
                if (auto* glass = Glass::glassOf(key)) {
                    glass->makeInert();
                    g_protocol->forget(key);
                }
            });
        });

        sendActive(enabled());
    }

    CGlassSurface::~CGlassSurface() {
        if (g_pHyprOpenGL)
            g_pHyprOpenGL->makeEGLCurrent();
        m_report.release();
    }

    bool CGlassSurface::good() const {
        return m_resource && m_resource->resource();
    }

    void CGlassSurface::sendActive(bool active) {
        if (good() && active != m_active) {
            m_resource->sendActive(active);
            m_active = active;
        }
    }

    void CGlassSurface::sendBackdrop(uint32_t cell, uint32_t cols, uint32_t rows, const std::vector<uint8_t>& cells) {
        // Never past what a message holds: it would cut eave's connection, the whole bar.
        if (!good() || cells.size() > MAX_REPORT_CELLS)
            return;
        wl_array array;
        wl_array_init(&array);
        if (!cells.empty()) {
            void* data = wl_array_add(&array, cells.size());
            if (!data) {
                wl_array_release(&array);
                return;
            }
            std::memcpy(data, cells.data(), cells.size());
        }
        m_resource->sendBackdrop(cell, cols, rows, &array);
        wl_array_release(&array);
    }

    void CGlassSurface::makeInert() {
        m_listeners = {};
        if (m_resource) {
            // The resource now only answers destroy, and leaves the C++ object behind: a few bytes.
            m_resource->setDestroy([](CEaveGlassSurfaceV1* r) { wl_resource_destroy(r->resource()); });
            m_resource->setOnDestroy([](CEaveGlassSurfaceV1*) {});
            m_resource->setSetRegions([](CEaveGlassSurfaceV1*, wl_array*) {});
            m_resource->setSetLook([](CEaveGlassSurfaceV1*, wl_fixed_t, wl_fixed_t, wl_fixed_t, wl_fixed_t, wl_fixed_t, uint32_t, wl_fixed_t, eaveGlassSurfaceV1Ink, wl_fixed_t,
                                      wl_fixed_t) {});
            m_resource->setSetReport([](CEaveGlassSurfaceV1*, uint32_t) {});
            new SP<CEaveGlassSurfaceV1>(m_resource); // leaked with it
            m_resource.reset();
        }
    }

    CGlassSurface* glassOf(const CWLSurfaceResource* surface) {
        return g_protocol ? g_protocol->glassOf(surface) : nullptr;
    }

    std::vector<CGlassSurface*> allGlass() {
        return g_protocol ? g_protocol->all() : std::vector<CGlassSurface*>{};
    }

    void startProtocol() {
        g_protocol = new CProtocol();
    }

    void stopProtocol() {
        if (!g_protocol)
            return;
        g_protocol->stop();
        g_protocol = nullptr; // leaked, with the global it removed
    }
}
