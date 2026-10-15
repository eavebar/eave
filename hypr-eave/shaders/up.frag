#version 300 es
// Frost, the way up: a dual Kawase step to twice the size.
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
