#version 300 es
// One fragment per cell of the report: how bright the frosted backdrop is there, from four taps.
precision highp float;
uniform sampler2D src;
uniform float cell;     // s px
uniform vec2 fbOrigin;  // see glass.frag
uniform mat2 toFb;
uniform vec2 bdOrigin;
uniform vec2 bdScale;
uniform vec2 bdLimit;
out vec4 fragColor;
void main() {
    vec2  c = gl_FragCoord.xy * cell; // the cell's middle, s px from the surface's top left
    float l = 0.0;
    for (int i = 0; i < 4; i++) {
        vec2 o  = (vec2(float(i & 1), float(i >> 1)) - 0.5) * 0.5 * cell;
        vec2 uv = clamp((fbOrigin + toFb * (c + o) - bdOrigin) * bdScale, vec2(0.0), bdLimit);
        l += dot(texture(src, uv).rgb, vec3(0.2126, 0.7152, 0.0722));
    }
    fragColor = vec4(l / 4.0, 0.0, 0.0, 1.0);
}
