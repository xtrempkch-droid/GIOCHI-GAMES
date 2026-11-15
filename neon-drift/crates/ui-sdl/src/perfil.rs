//! Overlay de desempenho (F1) e contabilidade do quadro.
//!
//! ## Por que o profiler da engine, e não um contador no laço
//!
//! O jogo já teve um contador de desenhos incrementado à mão, e ele estava
//! errado por um: um `draw_instanced` não incrementava. O [`Profiler`] da
//! engine mede no ponto em que o backend emite a chamada, então o número do
//! overlay é o mesmo que o driver recebeu.
//!
//! ## Custo
//!
//! A feature `profiling` já vem ligada no `Cargo.toml` do jogo. Com ela, o
//! profiler custa alguns `Instant::now()` por quadro; sem ela, os métodos são
//! corpos vazios e o overlay continua compilando — só mostra zeros. O overlay
//! em si só é **desenhado** quando o jogador aperta F1.
//!
//! O desenho usa a fonte 5×7 do núcleo, pela cena 3D, porque ADR-037 proíbe
//! misturar o canvas 2D com o contexto 3D: o `DebugOverlay` da engine desenha
//! em `giochi_engine::render`, que este jogo não inicializa.

use std::time::{Duration, Instant};

use giochi_engine::perf::{FrameCounters, Profiler};

/// Estado do overlay e a janela móvel de tempos.
pub struct Perfil {
    profiler: Profiler,
    visivel: bool,
    inicio_quadro: Instant,
    linhas: Vec<String>,
    escala: Option<f32>,
    rede: String,
}

impl Perfil {
    /// Overlay começando oculto.
    pub fn novo() -> Self {
        Self {
            profiler: Profiler::new(),
            visivel: false,
            inicio_quadro: Instant::now(),
            linhas: Vec::new(),
            escala: None,
            rede: String::new(),
        }
    }

    /// Liga/desliga (F1).
    pub fn alternar(&mut self) {
        self.visivel = !self.visivel;
        self.anunciar();
    }

    /// Liga/desliga pelo estado, para o botão `X` do controle e para o menu.
    ///
    /// Separado de [`Perfil::alternar`] porque o menu **sabe** o valor que
    /// quer (ele guarda `perfil: bool`) e alternar duas vezes por engano
    /// deixaria a tela dizendo DESLIGADO com o overlay desenhado.
    pub fn definir_visivel(&mut self, visivel: bool) {
        if self.visivel != visivel {
            self.visivel = visivel;
            self.anunciar();
        }
    }

    fn anunciar(&self) {
        eprintln!(
            "neon-drift: overlay de desempenho {}",
            if self.visivel { "ligado" } else { "desligado" }
        );
    }

    /// `true` quando o F1 está ligado.
    pub fn visivel(&self) -> bool {
        self.visivel
    }

    /// Registra a escala de renderização em vigor, para o overlay não mentir.
    pub fn definir_escala(&mut self, escala: Option<f32>) {
        self.escala = escala;
    }

    /// Texto curto da rede, atualizado pelo módulo `rede`.
    pub fn definir_rede(&mut self, texto: String) {
        self.rede = texto;
    }

    /// Marca o começo de um quadro.
    pub fn inicio_quadro(&mut self) {
        self.inicio_quadro = Instant::now();
        self.profiler.begin_frame();
    }

    /// Fecha o quadro e recalcula as linhas do overlay.
    pub fn fim_quadro(&mut self, counters: FrameCounters) {
        let agora = Instant::now();
        let duracao = agora.duration_since(self.inicio_quadro);
        let stats = self
            .profiler
            .end_frame(duracao.max(Duration::from_nanos(1)), counters);
        let resumo = self.profiler.summary();

        self.linhas.clear();
        self.linhas.push(format!(
            "FPS {:.0}  QUADRO {:.1}MS  P99 {:.1}MS",
            stats.fps,
            stats.frame_time_ms(),
            resumo.p99_ms()
        ));
        self.linhas.push(format!(
            "CPU {:.1}MS  DESENHOS {}  MALHAS {}",
            stats.cpu_time_ms(),
            stats.draw_calls,
            stats.meshes_drawn
        ));
        self.linhas.push(format!(
            "VERTICES {}  ESCALA {:.2}",
            stats.vertices_uploaded,
            self.escala.unwrap_or(1.0)
        ));
        if !self.rede.is_empty() {
            self.linhas.push(self.rede.to_uppercase());
        }
    }

    /// Linhas prontas para a cena desenhar. Vazio com o overlay desligado.
    pub fn linhas(&self) -> &[String] {
        if self.visivel {
            &self.linhas
        } else {
            &[]
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use giochi_engine::perf::FrameCounters;

    #[test]
    fn o_overlay_comeca_oculto_e_o_f1_alterna() {
        let mut p = Perfil::novo();
        assert!(!p.visivel());
        assert!(p.linhas().is_empty());
        p.alternar();
        assert!(p.visivel());
        p.alternar();
        assert!(!p.visivel());
    }

    #[test]
    fn o_overlay_so_entrega_linhas_quando_visivel() {
        let mut p = Perfil::novo();
        p.inicio_quadro();
        p.fim_quadro(FrameCounters::new());
        assert!(p.linhas().is_empty(), "desligado não desenha");
        p.alternar();
        assert!(!p.linhas().is_empty());
        assert!(p.linhas()[0].starts_with("FPS "));
    }

    #[test]
    fn a_escala_aparece_sem_mentir() {
        let mut p = Perfil::novo();
        p.definir_escala(Some(0.75));
        p.alternar();
        p.inicio_quadro();
        p.fim_quadro(FrameCounters::new());
        assert!(
            p.linhas().iter().any(|l| l.contains("ESCALA 0.75")),
            "linhas: {:?}",
            p.linhas()
        );
    }

    #[test]
    fn a_rede_entra_em_caixa_alta_para_a_fonte_5x7() {
        let mut p = Perfil::novo();
        p.definir_rede("host 2 clientes".into());
        p.alternar();
        p.inicio_quadro();
        p.fim_quadro(FrameCounters::new());
        assert!(p.linhas().iter().any(|l| l.contains("HOST 2 CLIENTES")));
    }
}
