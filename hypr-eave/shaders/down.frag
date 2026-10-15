#version 300 es
// Frost, the way down: a dual Kawase step to half the size. Linear filtering does half the work,
// so it's smooth where single taps at a distance were grainy.
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
