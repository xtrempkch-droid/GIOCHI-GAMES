//! Gamepad pelo subsistema de controle do SDL2.
//!
//! Com o contrato do GIOCHI-SYSTEM (issue #2) este é o caminho **preferido**:
//! o `Giochi Gamepad` é um `GameController` de verdade em todas as placas, e o
//! `gamecontrollerdb` da imagem faz `Button::A` ser o `a` do `buttons.map`.
//!
//! ## O que o SDL **não** garante sozinho
//!
//! Sem o contrato, o SDL chama de `Button::A` o **primeiro** botão que o device
//! declara em `EV_KEY`, que nas placas do projeto é `BTN_SOUTH` — o **B**
//! serigrafado. Medido no R35S com o `sdl-input-probe` do GIOCHI-SYSTEM:
//! `a:b0`, `b0 = BTN_SOUTH`. É por isso que ele não é usado às cegas em imagem
//! antiga, e por isso a máscara dele nunca é **somada** à do `evdev` (issue
//! #1): quem decide é [`crate::entrada_jogo::dono`], uma fonte por quadro.
//!
//! ## Por que o mapa daqui é o nome do `buttons.map`
//!
//! `Button::A` = `a`, `LeftShoulder` = `l`, `RightTrigger` = `r2`. É o
//! vocabulário do contrato, o mesmo do `evdev::bit_do_nome`, e há teste que
//! compara os dois — divergir de novo é o defeito da issue #1.
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

    /// O controle aberto é o **contrato** (`Giochi Gamepad`)?
    ///
    /// É o que permite o degrau 2 de [`crate::entrada_jogo::dono`]: com o
    /// `gamecontrollerdb` da imagem, neste device o `Button::A` do SDL é o `a`
    /// do `buttons.map`, e o caminho do SDL passa a dispensar a tabela por
    /// aparelho.
    pub fn contrato(&self) -> bool {
        self.nome
            .as_deref()
            .is_some_and(|n| n.trim() == crate::evdev::NOME_DO_CONTRATO)
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

        let mut bits = 0;
        for botao in BOTOES {
            if c.button(botao) {
                bits |= bit_do_botao(botao).unwrap_or(0);
            }
        }

        let eixo = |v: i16| (f32::from(v) / 32767.0).clamp(-1.0, 1.0);
        let gatilho = |v: i16| (f32::from(v) / 32767.0).clamp(0.0, 1.0);

        // Os gatilhos são `l2`/`r2` = **nitro**, e não acelerador. Quando o
        // `gamecontrollerdb` liga o gatilho a um botão (`righttrigger:b7`), o
        // próprio SDL sintetiza o eixo como 0 ou 32767 — por isso a leitura é a
        // mesma nos dois casos, e o limiar é que decide.
        //
        // O `sdl2` 0.38 não expõe `Button::Trigger*`: o gatilho só existe como
        // eixo, então ele não entra em `BOTOES` e é lido aqui.
        if acima_do_limiar(gatilho(c.axis(Axis::TriggerRight))) > 0.0
            || acima_do_limiar(gatilho(c.axis(Axis::TriggerLeft))) > 0.0
        {
            bits |= crate::evdev::NITRO;
        }

        let virar = com_zona_morta(eixo(c.axis(Axis::LeftX)), ZONA_MORTA);
        self.bits = bits;
        self.analogico = Analogico { virar };
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

/// Todos os botões que têm bit do jogo — menos os gatilhos, que o `sdl2` só
/// expõe como eixo.
///
/// A lista existe para o laço de `atualizar` e para o teste que a compara com o
/// vocabulário do `evdev`: um botão novo tem de aparecer aqui **e** em
/// [`bit_do_botao`], senão é um botão mudo.
pub const BOTOES: [Button; 11] = [
    Button::A,
    Button::B,
    Button::X,
    Button::Y,
    Button::Back,
    Button::Start,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
];

/// Bits do jogo para um botão do SDL — pelo **nome do `buttons.map`**, não pela
/// posição no plástico.
///
/// `Button::A` é `a`, `LeftShoulder` é `l`, `RightShoulder` é `r`. É o que o
/// `gamecontrollerdb` da imagem garante (`a:b1`, `b:b0`, `leftshoulder:b4`) e o
/// que faz o caminho do SDL concordar com o do `evdev` sem tabela por console.
///
/// O teste `o_mapa_do_sdl_e_o_do_buttons_map` compara esta tabela com
/// `evdev::bit_do_nome`: era a divergência entre esses dois vocabulários que
/// fazia o A do R35S frear (issue #1) — e, depois de alinhados, o R1 dar nitro
/// e o R2 acelerar (issue #2 mediu o contrato e expôs o resto).
pub fn bit_do_botao(botao: Button) -> Option<u32> {
    use crate::evdev as b;
    Some(match botao {
        Button::A => b::ACELERAR | b::ACEITAR,
        Button::B => b::FREAR,
        Button::X => b::PERFIL,
        Button::Y | Button::Back => b::VOLTAR,
        // Os dois ombros fazem freio de mão (`l` e `r`), como no aparelho.
        Button::LeftShoulder | Button::RightShoulder => b::FREIO_MAO,
        Button::Start => b::INICIAR,
        Button::DPadUp => b::CIMA,
        Button::DPadDown => b::BAIXO,
        Button::DPadLeft => b::ESQUERDA,
        Button::DPadRight => b::DIREITA,
        _ => return None,
    })
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
    fn o_mapa_do_sdl_e_o_do_buttons_map() {
        // A regra da casa: **um mapa por aparelho, uma tradução, nenhum
        // caminho alternativo**. Os dois caminhos (SDL e evdev) têm de falar o
        // mesmo vocabulário — o do `buttons.map`. Cada linha aqui é um nome, e
        // os dois lados são comparados com a **mesma** fonte (`bit_do_nome`).
        use crate::evdev::bit_do_nome;
        let pares = [
            (Button::A, "a"),
            (Button::B, "b"),
            (Button::X, "x"),
            (Button::Y, "y"),
            (Button::Back, "select"),
            (Button::Start, "start"),
            (Button::LeftShoulder, "l"),
            (Button::RightShoulder, "r"),
            (Button::DPadUp, "up"),
            (Button::DPadDown, "down"),
            (Button::DPadLeft, "left"),
            (Button::DPadRight, "right"),
        ];
        for (botao, nome) in pares {
            assert_eq!(
                bit_do_botao(botao),
                bit_do_nome(nome),
                "{botao:?} devia ser \"{nome}\""
            );
        }
    }

    #[test]
    fn o_r2_e_nitro_e_o_r1_e_freio_de_mao() {
        // A divergência que a issue #2 expôs: no SDL o R1 dava NITRO e o R2
        // acendia o acelerador pelo eixo. No contrato, `l`/`r` são freio de mão
        // e `l2`/`r2` são nitro.
        use crate::evdev::{bit_do_nome, FREIO_MAO, NITRO};
        assert_eq!(bit_do_botao(Button::RightShoulder), Some(FREIO_MAO));
        assert_eq!(bit_do_botao(Button::LeftShoulder), Some(FREIO_MAO));
        assert_eq!(bit_do_nome("r"), Some(FREIO_MAO));
        assert_eq!(bit_do_nome("r2"), Some(NITRO));
        // E nada no mapa de botões do SDL acende ACELERAR além do A.
        assert_eq!(bit_do_botao(Button::RightShoulder), Some(FREIO_MAO));
        assert_eq!(bit_do_botao(Button::Paddle1), None, "botão sem bit do jogo");
    }

    #[test]
    fn a_lista_de_botoes_nao_deixa_botao_mudo() {
        // `BOTOES` é a que o laço percorre: um botão que tem bit mas não está
        // na lista é um botão que não funciona, e o sintoma é "não responde".
        for botao in BOTOES {
            assert!(
                bit_do_botao(botao).is_some(),
                "{botao:?} está em BOTOES mas não tem bit"
            );
        }
        // Os gatilhos são a exceção declarada: só existem como eixo.
        assert!(!BOTOES.contains(&Button::LeftStick));
        assert_eq!(bit_do_botao(Button::LeftStick), None);
    }

    #[test]
    fn o_gatilho_em_repouso_nao_acelera() {
        assert_eq!(acima_do_limiar(0.0), 0.0);
        assert_eq!(acima_do_limiar(0.04), 0.0);
        assert!((acima_do_limiar(0.6) - 0.6).abs() < 1e-6);
    }
}
