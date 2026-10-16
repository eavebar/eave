#version 300 es
EXTENSION
// The bend's field, first step: the surface's coverage, averaged over each of the field's
// texels, so an edge moving between two texels moves the field smoothly instead of in steps.
// Each island's coverage goes in its own channel, so a neighbour's doesn't bend it.
precision highp float;
uniform SAMPLER src;  // the surface's buffer, premultiplied, linearly filtered
uniform vec2 texel;   // one field texel, in the buffer's uv
uniform float offset; // taps along each axis, 1 to 4
uniform vec2 surfSize; // s px
uniform float margin;  // s px

// eave's records, as in glass.frag: boxes in s px, and each island's channel, -1 for solid.
// Solid records don't cut the shape here: content on an island is part of it.
const int MAX = 64;
uniform int  count;
uniform vec4 boxes[MAX];
uniform int  chans[MAX];

in vec2 uv;
out vec4 fragColor;

// The channel of the island s belongs to, as glass.frag's `region` finds it (keep them alike):
// the nearest within `margin`, and of those it's inside, the smallest. -1 for none.
int owner(vec2 s) {
    int   found   = -1;
    float nearest = margin;
    float area    = 0.0;
    for (int i = 0; i < MAX; i++) {
        if (i >= count)
            break;
        if (chans[i] < 0)
            continue; // solid
        vec4  b = boxes[i];
        float d = length(max(max(b.xy - s, s - b.zw), vec2(0.0)));
        float a = (b.z - b.x) * (b.w - b.y);
        if (d < nearest || (d <= 0.0 && found >= 0 && nearest <= 0.0 && a < area)) {
            nearest = d;
            area    = a;
            found   = i;
        }
    }
    return found < 0 ? -1 : chans[found];
}

void main() {
    int  n   = int(offset);
    vec4 sum = vec4(0.0);
    for (int y = 0; y < 4; y++) {
        if (y >= n)
            break;
        for (int x = 0; x < 4; x++) {
            if (x >= n)
                break;
            vec2 u = uv + ((vec2(float(x), float(y)) + 0.5) / float(n) - 0.5) * texel;
            if (any(lessThan(u, vec2(0.0))) || any(greaterThan(u, vec2(1.0))))
                continue;
            int c = owner(u * surfSize);
            if (c >= 0)
                sum += vec4(equal(ivec4(0, 1, 2, 3), ivec4(c))) * texture(src, u).a;
        }
    }
    fragColor = sum / float(n * n);
}
