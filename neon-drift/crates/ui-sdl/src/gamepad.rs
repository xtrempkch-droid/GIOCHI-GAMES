//! Gamepad pelo subsistema de controle do SDL2.
//!
//! O `evdev` continua sendo o caminho do aparelho (lê o controle cru, sem
//! depender do mapeamento do SDL). Este módulo é o caminho **portátil**: no PC,
//! no R35S com SDL atualizado e em qualquer controle com mapeamento conhecido,
//! o SDL entrega eixos e botões já normalizados.
//!
//! ## Por que ler o estado a cada quadro, e não por evento
//!
//! Corrida precisa de "quanto" e não de "aconteceu": o gatilho direito é uma
//! aceleração contínua, e o analógico é um ângulo. Eventos de eixo chegam numa
//! taxa que não tem relação com o quadro; ler o estado uma vez por quadro é o
//! que mantém a simulação determinística — o mesmo `Entrada` por `dt` fixo.
//!
//! Botões e direcional também são lidos por estado, pelo mesmo motivo. Um
//! `ButtonDown` que chega entre dois passos do timestep fixo seria perdido.

use std::time::{Duration, Instant};

use giochi_engine::platform::sdl::Platform;
use sdl2::controller::{Axis, Button, GameController};

use crate::entrada_jogo::Analogico;

/// Abaixo disto o analógico é ruído de repouso.
///
/// Controles baratos entregam 300–1500 num eixo de ±32768 parado. Sem a zona
/// morta o carro "puxa" para um lado sozinho, e o sintoma parece defeito de
/// física.
pub const ZONA_MORTA: f32 = 0.16;

/// Tempo entre tentativas de abrir um controle que não estava presente.
const INTERVALO_REABRIR: Duration = Duration::from_secs(2);

/// Um controle aberto, ou a razão de não haver nenhum.
pub struct Gamepad {
    subsystem: Option<sdl2::GameControllerSubsystem>,
    controle: Option<GameController>,
    nome: Option<String>,
    ultima_tentativa: Instant,
    bits: u32,
    analogico: Analogico,
}

impl Gamepad {
    /// Abre o primeiro controle disponível, se houver subsistema.
    ///
    /// Falha em qualquer passo é **silenciosa do ponto de vista do jogo**: sem
    /// controle, teclado e evdev continuam funcionando. Só o diagnóstico diz o
    /// que aconteceu.
    pub fn iniciar(plataforma: &Platform) -> Self {
        let subsystem = match plataforma.game_controller_subsystem() {
            Ok(s) => Some(s.clone()),
            Err(erro) => {
                eprintln!("neon-drift: gamepad indisponível ({erro})");
                None
            }
        };
        let mut gamepad = Self {
            subsystem,
            controle: None,
            nome: None,
            ultima_tentativa: Instant::now() - INTERVALO_REABRIR,
            bits: 0,
            analogico: Analogico::default(),
        };
        gamepad.tentar_abrir();
        gamepad
    }

    /// Nome do controle aberto, para o log de inicialização.
    pub fn nome(&self) -> Option<&str> {
        self.nome.as_deref()
    }

    /// `true` quando há um controle aberto e ainda conectado.
    pub fn ativo(&self) -> bool {
        self.controle.is_some()
    }

    /// Máscara de botões, no mesmo vocabulário do `evdev`.
    pub fn bits(&self) -> u32 {
        self.bits
    }

    /// Eixos analógicos já com zona morta aplicada.
    pub fn analogico(&self) -> Analogico {
        self.analogico
    }

    /// Relê botões e eixos. Chamar uma vez por quadro, antes de `combinar`.
    pub fn atualizar(&mut self) {
        if self.controle.as_ref().is_some_and(|c| !c.attached()) {
            self.controle = None;
            self.nome = None;
            self.bits = 0;
            self.analogico = Analogico::default();
        }
        if self.controle.is_none() && self.ultima_tentativa.elapsed() >= INTERVALO_REABRIR {
            self.tentar_abrir();
        }
        let Some(c) = self.controle.as_ref() else {
            self.bits = 0;
            self.analogico = Analogico::default();
            return;
        };

        use crate::evdev as b;
        let mut bits = 0;
        // O mapa é o **mesmo** do evdev (`evdev::bit_do_codigo`), botão por
        // botão. Até a revisão de controles o caminho do SDL tinha um mapa
        // próprio — `A` era freio de mão aqui e acelerar no aparelho —, e a
        // mesma tecla fazia coisas diferentes conforme quem abrisse o controle.
        //
        // Os gatilhos analógicos entram como eixos (`Analogico`), então o `A`
        // acelera **e** aceita, igual ao aparelho.
        if c.button(Button::A) {
            bits |= b::ACELERAR | b::ACEITAR;
        }
        if c.button(Button::B) {
            bits |= b::FREAR;
        }
        if c.button(Button::X) {
            bits |= b::PERFIL;
        }
        if c.button(Button::Y) || c.button(Button::Back) {
            bits |= b::VOLTAR;
        }
        if c.button(Button::LeftShoulder) {
            bits |= b::FREIO_MAO;
        }
        if c.button(Button::RightShoulder) {
            bits |= b::NITRO;
        }
        if c.button(Button::Start) {
            bits |= b::INICIAR;
        }
        // Direcional: o valor contínuo da navegação sai em `bits` e o volante
        // analógico em `Analogico`.
        if c.button(Button::DPadLeft) {
            bits |= b::ESQUERDA;
        }
        if c.button(Button::DPadRight) {
            bits |= b::DIREITA;
        }
        if c.button(Button::DPadUp) {
            bits |= b::CIMA;
        }
        if c.button(Button::DPadDown) {
            bits |= b::BAIXO;
        }
        self.bits = bits;

        let eixo = |v: i16| (f32::from(v) / 32767.0).clamp(-1.0, 1.0);
        let gatilho = |v: i16| (f32::from(v) / 32767.0).clamp(0.0, 1.0);
        let virar = com_zona_morta(eixo(c.axis(Axis::LeftX)), ZONA_MORTA);
        let acelerar = gatilho(c.axis(Axis::TriggerRight));
        let frear = gatilho(c.axis(Axis::TriggerLeft));
        self.analogico = Analogico {
            virar,
            acelerar: acima_do_limiar(acelerar),
            frear: acima_do_limiar(frear),
        };
    }

    fn tentar_abrir(&mut self) {
        self.ultima_tentativa = Instant::now();
        let Some(subsystem) = self.subsystem.as_ref() else {
            return;
        };
        // `num_joysticks` é a única chamada que não custa abrir o dispositivo.
        let quantidade = match subsystem.num_joysticks() {
            Ok(n) => n,
            Err(_) => return,
        };
        for indice in 0..quantidade {
            if !subsystem.is_game_controller(indice) {
                continue;
            }
            match subsystem.open(indice) {
                Ok(controle) => {
                    self.nome = Some(controle.name());
                    eprintln!("neon-drift: gamepad \"{}\"", self.nome.as_deref().unwrap_or("?"));
                    self.controle = Some(controle);
                    return;
                }
                Err(erro) => {
                    eprintln!("neon-drift: controle {indice} não abriu ({erro})");
                }
            }
        }
    }
}

/// Aplica zona morta e reescala o resto para `[-1, 1]`.
///
/// Reescalar importa: sem isso, o primeiro milímetro do analógico depois da
/// zona morta salta de 0 para 0,16, e a direção fica "quadrada" nas correções
/// finas de drift.
pub fn com_zona_morta(valor: f32, zona: f32) -> f32 {
    let valor = valor.clamp(-1.0, 1.0);
    let zona = zona.clamp(0.0, 0.95);
    let modulo = valor.abs();
    if modulo <= zona {
        return 0.0;
    }
    let sinal = valor.signum();
    sinal * ((modulo - zona) / (1.0 - zona)).clamp(0.0, 1.0)
}

/// Gatilho com curso curto no início: o primeiro 5% é folga mecânica.
fn acima_do_limiar(valor: f32) -> f32 {
    if valor < 0.05 {
        0.0
    } else {
        valor
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_zona_morta_zera_o_repouso() {
        assert_eq!(com_zona_morta(0.0, ZONA_MORTA), 0.0);
        assert_eq!(com_zona_morta(0.10, ZONA_MORTA), 0.0);
        assert_eq!(com_zona_morta(-0.15, ZONA_MORTA), 0.0);
    }

    #[test]
    fn depois_da_zona_morta_o_curso_e_continuo() {
        // A emenda tem de ser contínua: 0,1601 não pode saltar para 0,19.
        let logo_acima = com_zona_morta(ZONA_MORTA + 0.0001, ZONA_MORTA);
        assert!(logo_acima < 0.001, "salto na emenda: {logo_acima}");
        let fim = com_zona_morta(1.0, ZONA_MORTA);
        assert!((fim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn o_sinal_e_preservado() {
        assert!(com_zona_morta(-0.8, ZONA_MORTA) < -0.7);
        assert!(com_zona_morta(0.8, ZONA_MORTA) > 0.7);
    }

    #[test]
    fn o_gatilho_em_repouso_nao_acelera() {
        assert_eq!(acima_do_limiar(0.0), 0.0);
        assert_eq!(acima_do_limiar(0.04), 0.0);
        assert!((acima_do_limiar(0.6) - 0.6).abs() < 1e-6);
    }
}
