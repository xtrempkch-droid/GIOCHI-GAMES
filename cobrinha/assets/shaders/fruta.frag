#version 100
// Blinn-Phong texturizado. Funciona também como prova do hot reload: edite este
// arquivo com o jogo aberto (debug) e o programa é recompilado no mesmo handle.
precision mediump float;

uniform sampler2D u_texture;
uniform vec4 u_tint;
uniform vec3 u_light_direction;
uniform vec3 u_light_color;
uniform float u_ambient_strength;
uniform float u_specular_strength;
uniform float u_shininess;
uniform vec3 u_camera_position;

varying vec2 v_uv;
varying vec3 v_normal;
varying vec3 v_world;

void main() {
    vec4 amostra = texture2D(u_texture, v_uv);
    vec3 normal = normalize(v_normal);
    vec3 to_light = normalize(-u_light_direction);
    vec3 to_eye = normalize(u_camera_position - v_world);

    float diffuse = max(dot(normal, to_light), 0.0);
    vec3 half_vector = normalize(to_light + to_eye);
    float specular = diffuse > 0.0
        ? pow(max(dot(normal, half_vector), 0.0), u_shininess)
        : 0.0;

    vec3 cor = amostra.rgb * u_tint.rgb;
    vec3 ambiente = cor * u_ambient_strength;
    vec3 direto = cor * u_light_color * diffuse;
    vec3 brilho = u_light_color * specular * u_specular_strength * 0.55;

    gl_FragColor = vec4(ambiente + direto + brilho, amostra.a * u_tint.a);
}
