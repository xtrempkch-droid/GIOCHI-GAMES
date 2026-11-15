//! HUD sobre a cena 3D: o `GlesHud` (egui pintado em GLES) da engine 0.5.
//!
//! O placar 3D de cubos continua a existir para builds sem a feature `hud`; com
//! o HUD ligado é ele que mostra pontos, recorde, estado e diagnóstico, com texto
//! de verdade em vez de sete segmentos.
//!
//! O HUD é de leitura: a engine não traduz input por ele (ADR-059), então as
//! teclas continuam a chegar ao jogo como antes.

use giochi_cobrinha_core::Estado;

/// Tudo o que o HUD mostra num quadro. Os campos de diagnóstico só aparecem
/// quando o jogador aperta F1.
#[allow(dead_code)]
pub struct InfoQuadro<'a> {
    pub pontos: u32,
    pub recorde: u32,
    pub estado: Estado,
    pub gles: u8,
    pub instancing: bool,
    pub janela: (u32, u32),
    pub alvo: (u32, u32),
    pub escala: Option<f32>,
    pub draws: u32,
    pub vertices: u64,
    pub particulas: usize,
    pub detalhado: bool,
    pub recarga: &'a str,
}

#[cfg(feature = "hud")]
mod com_hud {
    use super::InfoQuadro;
    use egui::{Align2, Color32, RichText};
    use giochi_cobrinha_core::Estado;
    use giochi_engine_3d::ui::GlesHud;
    use giochi_engine_3d::RenderContext3D;

    const AMBARELO: Color32 = Color32::from_rgb(255, 184, 61);
    const VERDE: Color32 = Color32::from_rgb(89, 255, 199);
    const VERMELHO: Color32 = Color32::from_rgb(255, 96, 108);
    const APAGADO: Color32 = Color32::from_rgb(148, 172, 198);
    const CLARO: Color32 = Color32::from_rgb(232, 241, 250);

    pub struct Hud {
        painter: GlesHud,
        fps: f32,
        aquecido: bool,
    }

    impl Hud {
        pub fn novo(render: &mut RenderContext3D) -> Result<Self, Box<dyn std::error::Error>> {
            let painter = GlesHud::new(render.glow())?;
            let ctx = painter.context();
            let mut estilo = (*ctx.style()).clone();
            estilo.visuals.window_fill = Color32::from_rgba_unmultiplied(7, 13, 27, 215);
            estilo.visuals.window_stroke =
                egui::Stroke::new(1.0_f32, Color32::from_rgb(48, 112, 150));
            estilo.visuals.override_text_color = Some(CLARO);
            ctx.set_style(estilo);
            Ok(Self {
                painter,
                fps: 0.0,
                // O primeiro `run` do egui constrói e envia o atlas de fontes; sem
                // ele o `tessellate` devolve zero primitivas. Um quadro vazio de
                // aquecimento evita que o HUD pisque ausente no primeiro quadro (e
                // que uma captura `--quadro` de um quadro só saia sem HUD).
                aquecido: false,
            })
        }

        pub fn desenhar(
            &mut self,
            render: &mut RenderContext3D,
            info: &InfoQuadro<'_>,
            delta: f32,
        ) -> Result<u32, Box<dyn std::error::Error>> {
            if delta > 0.0 {
                // O primeiro `delta` do laço é de microssegundos; sem o teto o
                // HUD mostraria "FPS 12000" por um quadro.
                let instantaneo = (1.0 / delta).min(999.0);
                self.fps = if self.fps <= 0.0 {
                    instantaneo
                } else {
                    self.fps * 0.9 + instantaneo * 0.1
                };
            }
            let fps = self.fps;
            let alvo = info.alvo;
            let mut interface = |ctx: &egui::Context| {
                egui::Window::new("placar")
                    .anchor(Align2::LEFT_TOP, egui::vec2(12.0, 10.0))
                    .title_bar(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{:03}", info.pontos))
                                    .size(34.0)
                                    .color(AMBARELO)
                                    .monospace(),
                            );
                            ui.vertical(|ui| {
                                ui.label(RichText::new("PONTOS").size(11.0).color(APAGADO));
                                ui.label(
                                    RichText::new(format!("RECORDE {:03}", info.recorde))
                                        .size(13.0)
                                        .color(CLARO),
                                );
                            });
                        });
                        ui.separator();
                        let (mensagem, cor) = match info.estado {
                            Estado::Pronto => {
                                ("PRONTO - A / START começa", Color32::from_rgb(56, 245, 122))
                            }
                            Estado::Jogando => ("JOGANDO", VERDE),
                            Estado::Perdido => ("FIM - A / START recomeça", VERMELHO),
                        };
                        ui.label(RichText::new(mensagem).size(14.0).color(cor));
                        if let Some(escala) = info.escala {
                            ui.label(
                                RichText::new(format!(
                                    "{:.0}x  {}x{} -> {}x{}",
                                    escala, info.alvo.0, info.alvo.1, info.janela.0, info.janela.1
                                ))
                                .size(11.0)
                                .color(APAGADO),
                            );
                        }
                    });

                egui::Window::new("diagnostico")
                    .anchor(Align2::RIGHT_TOP, egui::vec2(12.0, 10.0))
                    .title_bar(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label(
                            RichText::new(format!("FPS {fps:.0}"))
                                .size(16.0)
                                .color(VERDE)
                                .monospace(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "GLES {} | {}",
                                info.gles,
                                if info.instancing {
                                    "instancing"
                                } else {
                                    "por malha"
                                }
                            ))
                            .size(11.0)
                            .color(APAGADO)
                            .monospace(),
                        );
                        if info.detalhado {
                            ui.separator();
                            ui.label(
                                RichText::new(format!(
                                    "draws {}\nvértices {}\npartículas {}\n{}",
                                    info.draws, info.vertices, info.particulas, info.recarga
                                ))
                                .size(11.0)
                                .color(APAGADO)
                                .monospace(),
                            );
                        } else {
                            ui.label(RichText::new("F1 detalhes").size(10.0).color(APAGADO));
                        }
                    });
            };
            let mut desenho =
                self.painter
                    .draw(render.glow(), alvo, delta.max(0.0), &mut interface)?;
            // O primeiro `run` de egui de um contexto recém-criado monta o atlas de
            // fontes e ainda não pinta as janelas: o `tessellate` devolve zero
            // primitivas. Um segundo passe no mesmo quadro resolve — e é o que faz
            // uma captura `--quadro` de um quadro só já sair com o HUD.
            if !self.aquecido {
                self.aquecido = true;
                desenho = self
                    .painter
                    .draw(render.glow(), alvo, delta.max(0.0), &mut interface)?;
            }
            Ok(u32::try_from(desenho).unwrap_or(u32::MAX))
        }

        pub fn destruir(self, render: &RenderContext3D) {
            self.painter.destroy(render.glow());
        }
    }
}

#[cfg(not(feature = "hud"))]
mod sem_hud {
    use super::InfoQuadro;
    use giochi_engine_3d::RenderContext3D;

    /// Build sem egui: o jogo usa o placar 3D de cubos e não paga pelo HUD.
    pub struct Hud;

    impl Hud {
        pub fn novo(_render: &mut RenderContext3D) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(Self)
        }

        pub fn desenhar(
            &mut self,
            _render: &mut RenderContext3D,
            _info: &InfoQuadro<'_>,
            _delta: f32,
        ) -> Result<u32, Box<dyn std::error::Error>> {
            Ok(0)
        }

        pub fn destruir(self, _render: &RenderContext3D) {}
    }
}

#[cfg(feature = "hud")]
pub use com_hud::Hud;
#[cfg(not(feature = "hud"))]
pub use sem_hud::Hud;

/// O HUD está compilado neste binário?
pub const ATIVO: bool = cfg!(feature = "hud");
