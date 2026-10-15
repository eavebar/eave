#version 300 es
// The whole target, with `uv` over the part of the source texture that holds content.
precision highp float;
uniform vec2 srcScale;
layout(location = 0) in vec2 pos;
out vec2 uv;
void main() {
    uv = pos * srcScale;
    gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
}
