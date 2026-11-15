//! Regressão do bug gráfico do supersampling.
//!
//! O passe de upscale desliga `glDepthMask` para desenhar o quad e, na versão da
//! engine que o jogo vendorizava (0.2.2), não o religava. Com escala > 1 o
//! `glClear(GL_DEPTH_BUFFER_BIT)` do quadro seguinte era mascarado, o depth
//! buffer guardava a geometria do quadro anterior e objetos mais distantes
//! desapareciam a partir do **segundo** quadro. O primeiro quadro saía perfeito,
//! que é o motivo de uma captura `--quadro` isolada não pegar o defeito.
//!
//! ```text
//! SDL_VIDEODRIVER=offscreen cargo test -p giochi-cobrinha --test upscale_segundo_quadro
//! ```

use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Transform3D, Vec3};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::{DirectionalLight, RenderContext3D};

const LARGURA: u32 = 160;
const ALTURA: u32 = 120;

fn contexto_headless() -> Option<(Platform, Window)> {
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "offscreen");
    }
    let platform = Platform::new().ok()?;
    context::configure_attributes(&platform, GlesVersion::Gles2).ok()?;
    let window = Window::new_opengl(
        &platform,
        WindowConfig::new(LARGURA, ALTURA),
        "cobrinha-upscale",
    )
    .ok()?;
    Some((platform, window))
}

fn pixels_acesos(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[0] > 8 || px[1] > 8 || px[2] > 8)
        .count()
}

/// Desenha um cubo e devolve a imagem **apresentada** (já com o upscale).
fn quadro_com_cubo(
    render: &mut RenderContext3D,
    window: &Window,
    cubo: u64,
    shader: u64,
    z: f32,
) -> Vec<u8> {
    let camera = Camera3D::from_parts(Vec3::new(0.0, 0.0, 3.0), Vec3::ZERO, Vec3::UP)
        .with_clip_planes(0.1, 100.0);
    render.begin_frame(window);
    render
        .draw_mesh_colored(
            cubo,
            shader,
            &Transform3D::from_translation(Vec3::new(0.0, 0.0, z)),
            &camera,
            Color3D::new(0.9, 0.9, 0.9, 1.0),
        )
        .expect("desenho do cubo");
    render
        .read_pixels(LARGURA, ALTURA)
        .expect("leitura do framebuffer")
}

#[test]
fn o_segundo_quadro_supersampleado_limpa_a_profundidade() {
    let Some((_platform, window)) = contexto_headless() else {
        eprintln!("saltado: sem driver GLES (use SDL_VIDEODRIVER=offscreen)");
        return;
    };
    let mut render = RenderContext3D::new(context::create(&window).expect("contexto GL"));
    let cubo = render.create_cube_mesh(2.5).expect("cubo");
    let shader = render
        .create_builtin_shader(BuiltinShader::Phong)
        .expect("shader phong");
    render.set_clear_color(Color3D::BLACK);
    render.set_light(DirectionalLight::sun());
    // 1,25 é o padrão do jogo num painel 640x480 (alvo 800x600), não um valor
    // escolhido para o teste passar: é o caminho que o jogador usa.
    render.set_render_scale(Some(1.25));

    let perto = quadro_com_cubo(&mut render, &window, cubo, shader, 1.0);
    assert!(
        pixels_acesos(&perto) > 2_000,
        "o quadro de referência está vazio"
    );

    // O segundo quadro desenha o cubo mais longe, numa profundidade que a
    // geometria do primeiro quadro já cobria. Sem o depth clear (ou com o depth
    // mask desligado pelo passe de upscale) todo fragmento falha em GL_LEQUAL e
    // o quadro fica só na cor de limpeza.
    let longe = quadro_com_cubo(&mut render, &window, cubo, shader, -2.0);
    assert!(
        pixels_acesos(&longe) > 300,
        "o segundo quadro escalado está vazio: a profundidade não foi limpa/escrita"
    );
    assert_eq!(render.take_gl_error(), 0, "erro GLES no caminho de upscale");
}
