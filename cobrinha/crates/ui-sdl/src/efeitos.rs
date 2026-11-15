//! Partículas da engine: o estouro da fruta comida e o da cobra que morreu.
//!
//! Usa o `BillboardParticleSystem` da engine 0.5 — pool fixo, RNG determinístico
//! e desenho instanciado (com o fallback da engine nos drivers sem extensão).
//! Cada sistema tem a sua cor; o estouro da fruta é quente e o da morte é
//! vermelho, e como são pools separados um não tinge o outro.

use giochi_engine_3d::context::GlState;
use giochi_engine_3d::math3d::{Camera3D, Color3D, Vec3};
use giochi_engine_3d::mesh::Vertex;
use giochi_engine_3d::particles::BillboardParticleSystem;
use giochi_engine_3d::render3d::{MeshHandle, ShaderHandle};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::RenderContext3D;

const COR_COMIDA: Color3D = Color3D::new(1.0, 0.38, 0.26, 0.95);
const COR_MORTE: Color3D = Color3D::new(1.0, 0.16, 0.22, 0.95);

pub struct Efeitos {
    comida: BillboardParticleSystem,
    morte: BillboardParticleSystem,
    quad: MeshHandle,
    shader: ShaderHandle,
}

impl Efeitos {
    pub fn novo(render: &mut RenderContext3D) -> Result<Self, Box<dyn std::error::Error>> {
        // Quad centrado em XY com a normal local em +Z: é o formato que o billboard
        // da engine espera (ele roda o quad para a câmara).
        let quad = render.create_mesh(
            &[
                Vertex::flat([-0.5, -0.5, 0.0], [0.0, 0.0, 1.0]),
                Vertex::flat([0.5, -0.5, 0.0], [0.0, 0.0, 1.0]),
                Vertex::flat([0.5, 0.5, 0.0], [0.0, 0.0, 1.0]),
                Vertex::flat([-0.5, 0.5, 0.0], [0.0, 0.0, 1.0]),
            ],
            &[0, 1, 2, 0, 2, 3],
        )?;
        let shader = render.create_builtin_shader(BuiltinShader::Instanced)?;

        // A gravidade é suave porque a câmara está a ~26 unidades: 5 u/s² dá um
        // arco visível sem as partículas caírem fora da tela em meio segundo.
        let comida = BillboardParticleSystem::new(384, Vec3::new(0.0, -5.0, 0.0), 0xC0_11_A1)?;
        let morte = BillboardParticleSystem::new(512, Vec3::new(0.0, -7.0, 0.0), 0xDE_AD_01)?;
        Ok(Self {
            comida,
            morte,
            quad,
            shader,
        })
    }

    /// Estouro curto da fruta comida.
    pub fn comer(&mut self, posicao: Vec3) -> Result<(), Box<dyn std::error::Error>> {
        self.comida.emit_burst(posicao, 64, 3.4, 0.8, 0.17)?;
        Ok(())
    }

    /// Estouro maior no ponto em que a cobra morreu.
    pub fn morrer(&mut self, posicao: Vec3) -> Result<(), Box<dyn std::error::Error>> {
        self.morte.emit_burst(posicao, 160, 4.6, 1.5, 0.24)?;
        Ok(())
    }

    /// Um brilho solto em volta da fruta, emitido a cada poucos quadros.
    pub fn brilhar(&mut self, posicao: Vec3) -> Result<(), Box<dyn std::error::Error>> {
        self.comida
            .emit(posicao, Vec3::new(0.0, 0.45, 0.0), 0.9, 0.09)?;
        Ok(())
    }

    pub fn atualizar(&mut self, delta: f32) -> Result<(), Box<dyn std::error::Error>> {
        self.comida.update(delta)?;
        self.morte.update(delta)?;
        Ok(())
    }

    pub fn ativos(&self) -> usize {
        self.comida.len() + self.morte.len()
    }

    /// Desenha os dois pools e devolve as draw calls emitidas.
    ///
    /// O estado de partículas (blend ligado, cull desligado) é aplicado só durante
    /// o desenho e o estado anterior é reposto — a cena opaca continua a ser
    /// desenhada com cull e sem blend, que é o caminho barato num Mali.
    pub fn desenhar(
        &mut self,
        render: &mut RenderContext3D,
        camera: &Camera3D,
    ) -> Result<u32, Box<dyn std::error::Error>> {
        let anterior = render.state();
        let estado = GlState {
            blend: true,
            cull_face: false,
            ..anterior
        };
        render.set_state(estado);
        render.state().apply(render.glow());

        let mut draws = self
            .comida
            .render(render, self.quad, self.shader, camera, COR_COMIDA)?;
        draws += self
            .morte
            .render(render, self.quad, self.shader, camera, COR_MORTE)?;

        render.set_state(anterior);
        render.state().apply(render.glow());
        Ok(draws)
    }
}
