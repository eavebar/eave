#version 300 es
// A quad over `rect`, framebuffer px.
precision highp float;
uniform vec2 fbSize;
uniform vec4 rect;
layout(location = 0) in vec2 pos;
void main() {
    vec2 px = rect.xy + pos * rect.zw;
    gl_Position = vec4(px / fbSize * 2.0 - 1.0, 0.0, 1.0);
}
