#version 300 es
// The bend's field, then: a gaussian along one axis, run once across and once down. Outside the
// surface counts as empty.
precision highp float;
uniform sampler2D src;
uniform vec2 srcScale; // the part of src that holds content, uv
uniform vec2 texel;    // one source texel along the blur, uv
uniform float offset;  // the gaussian's sigma, texels, at most 4
in vec2 uv;
out vec4 fragColor;
void main() {
    int   r   = int(ceil(offset * 3.0));
    float k   = -0.5 / (offset * offset);
    float sum = 0.0, weight = 0.0;
    for (int i = -12; i <= 12; i++) {
        if (i < -r || i > r)
            continue;
        float w = exp(k * float(i * i));
        vec2  u = uv + texel * float(i);
        if (all(greaterThanEqual(u, vec2(0.0))) && all(lessThanEqual(u, srcScale)))
            sum += texture(src, u).r * w;
        weight += w;
    }
    fragColor = vec4(sum / weight);
}
