#version 300 es
EXTENSION
// The bend's field, first step: the surface's coverage, averaged over each of the field's
// texels, so an edge moving between two texels moves the field smoothly instead of in steps.
precision highp float;
uniform SAMPLER src;  // the surface's buffer, premultiplied, linearly filtered
uniform vec2 texel;   // one field texel, in the buffer's uv
uniform float offset; // taps along each axis, 1 to 4
in vec2 uv;
out vec4 fragColor;
void main() {
    int   n   = int(offset);
    float sum = 0.0;
    for (int y = 0; y < 4; y++) {
        if (y >= n)
            break;
        for (int x = 0; x < 4; x++) {
            if (x >= n)
                break;
            vec2 u = uv + ((vec2(float(x), float(y)) + 0.5) / float(n) - 0.5) * texel;
            if (all(greaterThanEqual(u, vec2(0.0))) && all(lessThanEqual(u, vec2(1.0))))
                sum += texture(src, u).a;
        }
    }
    fragColor = vec4(sum / float(n * n));
}
