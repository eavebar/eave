// The three things hypr-eave needs that Hyprland keeps private, and nothing else. Reached through
// explicit template instantiation, where the language doesn't check access: a member that's
// renamed or retyped fails to compile here, instead of silently reading the wrong memory the way
// `#define private public` would on a layout change.
//
// Each is an ask for upstream (docs/decisions.md): a pass element that reads what's behind it
// needs its whole region damaged in this frame's record, and needs to sit right before and after
// the surface it belongs to.
#pragma once

#include <hyprland/src/output/DamageRing.hpp>
#include <hyprland/src/render/pass/Pass.hpp>

namespace Internals {
    template <typename Tag, auto Member>
    struct SRob {
        friend constexpr auto get(Tag) {
            return Member;
        }
    };

    struct SDamagePrevious {
        friend constexpr auto get(SDamagePrevious);
    };
    struct SDamagePreviousIdx {
        friend constexpr auto get(SDamagePreviousIdx);
    };
    struct SPassElements {
        friend constexpr auto get(SPassElements);
    };

    template struct SRob<SDamagePrevious, &Monitor::CDamageRing::m_previous>;
    template struct SRob<SDamagePreviousIdx, &Monitor::CDamageRing::m_previousIdx>;
    template struct SRob<SPassElements, &Render::CRenderPass::m_passElements>;

    // This frame's record in the ring: what older buffers replay to catch up.
    inline CRegion& thisFrameDamage(Monitor::CDamageRing& ring) {
        return (ring.*get(SDamagePrevious{}))[ring.*get(SDamagePreviousIdx{})];
    }

    // The pass's elements, in drawing order.
    inline auto& passElements(Render::CRenderPass& pass) {
        return pass.*get(SPassElements{});
    }
}
