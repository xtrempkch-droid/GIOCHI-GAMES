#version 100
// Shader da fruta: o mesmo layout de atributos/uniformes da engine.
precision highp float;

uniform mat4 u_view_proj;
uniform mat4 u_model;
uniform mat4 u_normal_matrix;

attribute vec3 a_position;
attribute vec3 a_normal;
attribute vec2 a_uv;

varying vec2 v_uv;
varying vec3 v_normal;
varying vec3 v_world;

void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    v_world = world.xyz;
    v_normal = mat3(u_normal_matrix) * a_normal;
    v_uv = a_uv;
    gl_Position = u_view_proj * world;
}
