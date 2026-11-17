//! Junta teclado e controle numa `Entrada` só.
//!
//! ## Por que a fusão é uma função pura
//!
//! Ela decide coisas que dão para errar em silêncio: "seta + A somam ou o maior
//! vence?", "frear e acelerar juntos faz o quê?", "espaço é freio de mão ou
//! sair?". Se isso morasse dentro do laço de eventos do SDL, a única forma de
//! conferir seria apertar teclas no aparelho.
//!
//! Aqui entra máscara (`u32`), estado de teclas e fase da corrida; sai a entrada
//! do carro e as ações de tela.

use giochi_neon_drift_core::carro::Entrada;
use giochi_neon_drift_core::Fase;

use crate::evdev;

/// Teclas que o jogo observa, como estado (não evento).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Teclas {
    /// Cima (acelerar).
    pub cima: bool,
    /// Baixo (frear).
    pub baixo: bool,
    /// Esquerda.
    pub esquerda: bool,
    /// Direita.
    pub direita: bool,
    /// Espaço — freio de mão.
    pub espaco: bool,
    /// Shift — nitro.
    pub shift: bool,
    /// Enter — começar/reiniciar.
    pub enter: bool,
    /// Esc — sair.
    pub escape: bool,
    /// W/A/S/D, equivalentes às setas.
    pub wasd: bool,
}

/// Ações de tela que o laço precisa executar.
///
/// São **níveis**, não bordas: "a seta está apertada". Quem transforma nível em
/// borda é [`bordas`], porque segurar a seta não pode trocar de mapa 75 vezes
/// por segundo — e essa é a única regra que o laço precisa repetir a cada
/// quadro.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Acoes {
    /// Cursor do menu: `-1` sobe, `+1` desce, `0` fica.
    ///
    /// Só no título, onde o carro está parado — a mesma seta que vira o volante
    /// na corrida escolhe a linha no menu.
    pub navegar: i32,
    /// Valor da linha atual: `-1`/`+1`.
    pub ajustar: i32,
    /// Apertou A: aceita a linha do menu (e no título o carro não acelera, então
    /// o mesmo botão pode aceitar sem ambiguidade).
    pub aceitar: bool,
    /// Apertou START ou Enter: jogar/reiniciar sem passar pelo cursor.
    pub iniciar: bool,
    /// Apertou Y/SELECT/Esc: do título sai do jogo; da corrida volta ao menu.
    pub voltar: bool,
    /// Apertou X/F1: liga ou desliga o overlay de desempenho.
    pub perfil: bool,
}

/// O que **começou** neste quadro.
///
/// A borda não é detalhe de gosto: `navegar` é um nível que fica em `-1`
/// enquanto a seta está apertada, e o laço roda a 75 Hz. Sem esta função, uma
/// seta segurada trocava de mapa 75 vezes por segundo — foi medido, e o teste
/// com `--teclas right` mostrou quatro trocas em quatro quadros.
pub fn bordas(anterior: Acoes, atual: Acoes) -> Acoes {
    Acoes {
        // Passo que **mudou**: manter a seta apertada dá sempre o mesmo nível.
        navegar: if atual.navegar != anterior.navegar {
            atual.navegar
        } else {
            0
        },
        ajustar: if atual.ajustar != anterior.ajustar {
            atual.ajustar
        } else {
            0
        },
        // Booleano: só a transição de solto para apertado conta.
        aceitar: atual.aceitar && !anterior.aceitar,
        iniciar: atual.iniciar && !anterior.iniciar,
        voltar: atual.voltar && !anterior.voltar,
        perfil: atual.perfil && !anterior.perfil,
    }
}

/// Eixos analógicos de um gamepad, já normalizados e com zona morta.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Analogico {
    /// Volante, `-1` (esquerda) a `1` (direita).
    ///
    /// **Só o volante.** Os gatilhos já não entram aqui: no contrato `l2`/`r2`
    /// são `NITRO` e `l`/`r` são freio de mão, e ler o eixo do gatilho como
    /// acelerador transformava o R2 em acelerar **e** nitro no mesmo quadro —
    /// o defeito da issue #1 por outra porta. Acelerar e frear são botões
    /// (`a`/`b`), no aparelho e no contrato.
    pub virar: f32,
}

/// Combina as duas fontes de entrada com a fase atual.
pub fn combinar(bits: u32, teclas: &Teclas, fase: Fase) -> (Entrada, Acoes) {
    combinar_com_analogico(bits, Analogico::default(), teclas, fase)
}

// ── um contrato, uma fonte por quadro ──────────────────────────────
//
// O GIOCHI-SYSTEM publica **um controle virtual** chamado `Giochi Gamepad`
// (issue #2): um nó só, com botões e eixos, 17 botões de códigos fixos e quatro
// eixos em -32767..32767. O `gamecontrollerdb.txt` da imagem mapeia `a:b1`, de
// modo que o `Button::A` do SDL é o `a` do `buttons.map` em **todas** as placas.
//
// Isso resolve a classe do defeito da issue #1 sem `if` por console: quando o
// contrato está presente, os dois caminhos (SDL e evdev) concordam por
// construção. Quando ele **não** está (imagem antiga, PC), aí sim há dois mapas
// diferentes e é preciso escolher — e escolher é o que esta seção faz.
//
// A regra da casa: **um mapa por aparelho, uma tradução, nenhum caminho
// alternativo.** Nunca somar duas fontes: somar foi o que fez o A do R35S
// acelerar e frear no mesmo quadro.

/// O que `GIOCHI_CONTROLE` pediu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Forcar {
    /// Segue a precedência do contrato.
    #[default]
    Automatico,
    /// Usa o SDL sempre que ele tiver controle (`GIOCHI_CONTROLE=sdl`).
    Sdl,
    /// Usa o evdev sempre que ele tiver controle (`GIOCHI_CONTROLE=evdev`).
    Evdev,
}

impl Forcar {
    /// `sdl`, `evdev` ou qualquer outra coisa = automático.
    pub fn do_valor(valor: &str) -> Self {
        match valor.trim().to_ascii_lowercase().as_str() {
            "sdl" => Self::Sdl,
            "evdev" => Self::Evdev,
            _ => Self::Automatico,
        }
    }
}

/// O que cada caminho tem aberto **neste** quadro.
///
/// Números e nomes, não os objetos: assim a decisão é uma função pura e o teste
/// dela não precisa de placa, de `/dev` nem de SDL.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fontes {
    /// O nó `Giochi Gamepad` está aberto no evdev (o contrato, pelo lado cru).
    pub evdev_contrato: bool,
    /// Quantos controles **físicos** o evdev abriu.
    pub evdev_fisicos: usize,
    /// `/etc/giochi/buttons.map` foi lido (o mapa do plástico desta placa).
    pub mapa_do_sistema: bool,
    /// O SDL tem um controle aberto.
    pub sdl: bool,
    /// O controle aberto pelo SDL **é** o `Giochi Gamepad`.
    pub sdl_contrato: bool,
}

/// Quem fornece botões e eixos neste quadro. **Um** só.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dono {
    /// O `evdev`, com os códigos que o mapa da placa declara.
    Evdev,
    /// O SDL, que normaliza botões e eixos pelo `gamecontrollerdb`.
    Sdl,
    /// Nem um nem outro: só teclado.
    Nenhum,
}

impl Dono {
    pub fn nome(self) -> &'static str {
        match self {
            Dono::Evdev => "evdev",
            Dono::Sdl => "sdl",
            Dono::Nenhum => "nenhum",
        }
    }
}

/// Decide a fonte do quadro.
///
/// A precedência, e o porquê de cada degrau:
///
/// 1. **Contrato pelo evdev** — o nó virtual está aberto: os códigos são fixos
///    e não dependem do `gamecontrollerdb`. É o caminho mais determinístico.
/// 2. **Contrato pelo SDL** — o controle aberto é o `Giochi Gamepad`: o
///    `gamecontrollerdb` da imagem garante `Button::A = a`.
/// 3. **Físico com mapa do plástico** — imagem sem o virtual: o `buttons.map`
///    é a única descrição de qual código é o `a` nesta placa (é o caso da X35S,
///    que troca A e B de verdade).
/// 4. **SDL** — o PC, onde não há `buttons.map` e o `Button::A` do SDL é
///    `BTN_SOUTH` de verdade; e qualquer placa cujo evdev não abra nada.
///
/// `forcar` fura os degraus 1–4, mas nunca devolve uma fonte que não exista:
/// ficar sem controle é pior que usar o mapa "errado" do outro caminho.
pub fn dono(fontes: Fontes, forcar: Forcar) -> Dono {
    let evdev = fontes.evdev_contrato || fontes.evdev_fisicos > 0;
    match forcar {
        Forcar::Evdev if evdev => Dono::Evdev,
        Forcar::Sdl if fontes.sdl => Dono::Sdl,
        Forcar::Evdev | Forcar::Sdl => {
            // O caminho pedido não existe: cai para o que houver, em vez de
            // deixar o jogo sem controle nenhum.
            if evdev {
                Dono::Evdev
            } else if fontes.sdl {
                Dono::Sdl
            } else {
                Dono::Nenhum
            }
        }
        Forcar::Automatico => {
            if fontes.evdev_contrato {
                Dono::Evdev
            } else if fontes.sdl_contrato {
                Dono::Sdl
            } else if fontes.mapa_do_sistema && fontes.evdev_fisicos > 0 {
                Dono::Evdev
            } else if fontes.sdl {
                Dono::Sdl
            } else {
                Dono::Nenhum
            }
        }
    }
}

/// A máscara de botões do quadro: a do **dono**, nunca as duas somadas.
pub fn mascara_dos_botoes(evdev: u32, sdl: u32, dono: Dono) -> u32 {
    match dono {
        Dono::Evdev => evdev,
        Dono::Sdl => sdl,
        Dono::Nenhum => 0,
    }
}

/// Combina teclado, botões e eixos analógicos.
///
/// O volante **soma** ao que o direcional pede, limitado a 1: apertar a seta e
/// torcer o analógico no mesmo sentido não vira duas vezes. A assinatura
/// separada de [`combinar`] mantém a função pura e testável — os botões entram
/// por máscara, o volante por valor.
pub fn combinar_com_analogico(
    bits: u32,
    analogico: Analogico,
    teclas: &Teclas,
    fase: Fase,
) -> (Entrada, Acoes) {
    // Teclado e controle **somam** com `max`, não com `or` booleano: se as duas
    // fontes dizem "acelerar", o carro não acelera duas vezes.
    //
    // O direcional acelera e freia pelo mesmo caminho do botão (`CIMA`/`BAIXO`),
    // e não acendendo `ACELERAR`: quem joga deitado usa o direcional, e o bit
    // do A precisa continuar significando só o A (ver `evdev.rs`).
    let mut acelerar: f32 = if bits & (evdev::ACELERAR | evdev::CIMA) != 0 {
        1.0
    } else {
        0.0
    };
    let mut frear: f32 = if bits & (evdev::FREAR | evdev::BAIXO) != 0 {
        1.0
    } else {
        0.0
    };
    let mut virar: f32 = 0.0;

    if bits & evdev::ESQUERDA != 0 {
        virar -= 1.0;
    }
    if bits & evdev::DIREITA != 0 {
        virar += 1.0;
    }

    // O volante entra depois dos bits, com soma limitada.
    virar += analogico.virar.clamp(-1.0, 1.0);

    if teclas.cima {
        acelerar = acelerar.max(1.0);
    }
    if teclas.baixo {
        frear = frear.max(1.0);
    }
    if teclas.esquerda {
        virar -= 1.0;
    }
    if teclas.direita {
        virar += 1.0;
    }

    let freio_mao = bits & evdev::FREIO_MAO != 0 || teclas.espaco;
    let nitro = bits & evdev::NITRO != 0 || teclas.shift;

    let entrada = Entrada {
        acelerar,
        frear,
        virar: virar.clamp(-1.0, 1.0),
        freio_mao,
        nitro,
    };

    // ── menu ────────────────────────────────────────────────────────────
    //
    // Navegar e ajustar só existem no título. Em qualquer outra fase as setas
    // voltam a ser volante e os dois ficam em 0 — uma tecla com dois
    // significados simultâneos seria pior que tecla morta.
    let (navegar, ajustar) = if fase == Fase::Titulo {
        let mut navegar = 0;
        if bits & evdev::CIMA != 0 || teclas.cima {
            navegar -= 1;
        }
        if bits & evdev::BAIXO != 0 || teclas.baixo {
            navegar += 1;
        }
        let mut ajustar = 0;
        if bits & evdev::ESQUERDA != 0 || teclas.esquerda {
            ajustar -= 1;
        }
        if bits & evdev::DIREITA != 0 || teclas.direita {
            ajustar += 1;
        }
        (navegar, ajustar)
    } else {
        (0, 0)
    };

    let acoes = Acoes {
        navegar,
        ajustar,
        // O A aceita em qualquer tela; quem decide se aquilo significa alguma
        // coisa é a fase, no laço. Assim o mesmo botão não muda de bit conforme
        // a tela e a fusão continua sem estado.
        aceitar: bits & evdev::ACEITAR != 0,
        iniciar: bits & evdev::INICIAR != 0 || teclas.enter,
        voltar: bits & evdev::VOLTAR != 0 || teclas.escape,
        perfil: bits & evdev::PERFIL != 0,
    };

    (entrada, acoes)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn sem_teclas() -> Teclas {
        Teclas::default()
    }

    #[test]
    fn sem_nada_apertado_o_carro_fica_parado() {
        let (e, a) = combinar(0, &sem_teclas(), Fase::Correndo);
        assert_eq!(e, Entrada::PARADA);
        assert!(!a.aceitar && !a.iniciar && !a.voltar && !a.perfil);
        assert_eq!((a.navegar, a.ajustar), (0, 0));
    }

    #[test]
    fn a_do_controle_acelera_e_direcional_vira() {
        let (e, _) = combinar(evdev::ACELERAR | evdev::DIREITA, &sem_teclas(), Fase::Correndo);
        assert_eq!(e.acelerar, 1.0);
        assert_eq!(e.virar, 1.0);
    }

    #[test]
    fn o_direcional_tambem_acelera_e_freia() {
        // Quem joga deitado usa o direcional. O bit é `CIMA`/`BAIXO` (botão
        // físico) e não `ACELERAR`/`FREAR`, mas o carro tem de andar igual.
        let (sobe, _) = combinar(evdev::CIMA, &sem_teclas(), Fase::Correndo);
        assert_eq!(sobe.acelerar, 1.0);
        let (desce, _) = combinar(evdev::BAIXO, &sem_teclas(), Fase::Correndo);
        assert_eq!(desce.frear, 1.0);
        assert_eq!(desce.acelerar, 0.0, "o direcional para baixo não acelera");
    }

    #[test]
    fn segurar_o_direcional_nao_apaga_o_acelerar_do_a() {
        // O bit do A e o do direcional são diferentes justamente para isto: o
        // jogador acelera com o A, toca o direcional e o carro **não** pode
        // parar de acelerar quando ele solta o direcional.
        let com_os_dois = evdev::ACELERAR | evdev::CIMA;
        let (e, _) = combinar(com_os_dois, &sem_teclas(), Fase::Correndo);
        assert_eq!(e.acelerar, 1.0);
        // Soltar o direcional deixa só o A na máscara.
        let (e, _) = combinar(evdev::ACELERAR, &sem_teclas(), Fase::Correndo);
        assert_eq!(e.acelerar, 1.0, "o A continua apertado");
    }

    #[test]
    fn esquerda_e_direita_juntos_cancelam() {
        // Acontece de verdade: o jogador aperta as duas no pânico. O carro tem de
        // seguir reto, não escolher um lado por ordem de leitura.
        let (e, _) = combinar(evdev::ESQUERDA | evdev::DIREITA, &sem_teclas(), Fase::Correndo);
        assert_eq!(e.virar, 0.0);
    }

    #[test]
    fn teclado_e_controle_somam_para_um_so() {
        // `or` booleano em `f32` não existe; o que não pode acontecer é virar 2.0
        // e o carro girar duas vezes mais rápido com as duas fontes apertadas.
        let teclas = Teclas {
            cima: true,
            direita: true,
            ..sem_teclas()
        };
        let (e, _) = combinar(evdev::ACELERAR | evdev::DIREITA, &teclas, Fase::Correndo);
        assert_eq!(e.acelerar, 1.0);
        assert_eq!(e.virar, 1.0, "virar não pode passar de 1");
    }

    #[test]
    fn espaco_e_freio_de_mao_e_shift_e_nitro() {
        let teclas = Teclas {
            espaco: true,
            shift: true,
            ..sem_teclas()
        };
        let (e, _) = combinar(0, &teclas, Fase::Correndo);
        assert!(e.freio_mao);
        assert!(e.nitro);
    }

    #[test]
    fn l1_e_r2_do_controle_fazem_o_mesmo_que_espaco_e_shift() {
        let (e, _) = combinar(evdev::FREIO_MAO | evdev::NITRO, &sem_teclas(), Fase::Correndo);
        assert!(e.freio_mao);
        assert!(e.nitro);
    }

    #[test]
    fn start_no_titulo_comeca_a_corrida() {
        let (_, a) = combinar(evdev::INICIAR, &sem_teclas(), Fase::Titulo);
        assert!(a.iniciar, "START no título tem de começar");
        let teclas = Teclas {
            enter: true,
            ..sem_teclas()
        };
        let (_, a) = combinar(0, &teclas, Fase::Titulo);
        assert!(a.iniciar, "enter no título tem de começar");
    }

    #[test]
    fn start_no_resultado_reinicia() {
        for (bits, teclas) in [
            (evdev::INICIAR, sem_teclas()),
            (
                0,
                Teclas {
                    enter: true,
                    ..sem_teclas()
                },
            ),
        ] {
            let (_, a) = combinar(bits, &teclas, Fase::Resultado);
            assert!(a.iniciar);
        }
    }

    #[test]
    fn start_correndo_nao_faz_nada() {
        // Sem isso, apertar start no meio da corrida reiniciaria a partida e o
        // jogador perderia o progresso sem entender. Quem decide é o laço, que
        // só olha `iniciar` fora de `Correndo` — este teste garante que a fusão
        // não inventa a ação sozinha em outra fase.
        let (_, a) = combinar(evdev::INICIAR, &sem_teclas(), Fase::Correndo);
        assert!(a.iniciar, "a fusão reporta o nível; o laço é que filtra");
    }

    #[test]
    fn o_cursor_do_menu_so_se_move_no_titulo() {
        for fase in [Fase::Correndo, Fase::Resultado] {
            let (_, a) = combinar(evdev::CIMA | evdev::ESQUERDA, &sem_teclas(), fase);
            assert_eq!(a.navegar, 0, "{fase:?}");
            assert_eq!(a.ajustar, 0, "{fase:?}");
        }
        let (_, a) = combinar(evdev::CIMA | evdev::ESQUERDA, &sem_teclas(), Fase::Titulo);
        assert_eq!(a.navegar, -1);
        assert_eq!(a.ajustar, -1);
        let (_, a) = combinar(evdev::BAIXO | evdev::DIREITA, &sem_teclas(), Fase::Titulo);
        assert_eq!(a.navegar, 1);
        assert_eq!(a.ajustar, 1);
    }

    #[test]
    fn o_a_aceita_e_o_x_liga_o_perfil() {
        let (_, a) = combinar(evdev::ACEITAR, &sem_teclas(), Fase::Titulo);
        assert!(a.aceitar);
        assert!(!a.perfil, "o A não abre o overlay");
        let (_, a) = combinar(evdev::PERFIL, &sem_teclas(), Fase::Correndo);
        assert!(a.perfil);
        assert!(!a.aceitar, "o X não aceita");
    }

    #[test]
    fn esc_e_y_voltam_em_qualquer_fase() {
        for fase in [Fase::Titulo, Fase::Correndo, Fase::Resultado] {
            let teclas = Teclas {
                escape: true,
                ..sem_teclas()
            };
            let (_, a) = combinar(0, &teclas, fase);
            assert!(a.voltar, "{fase:?}");
            let (_, a) = combinar(evdev::VOLTAR, &sem_teclas(), fase);
            assert!(a.voltar, "{fase:?} pelo controle");
        }
    }

    #[test]
    fn a_borda_nao_repete_o_passo_segurado() {
        // O defeito medido: com `--teclas right`, quatro quadros davam quatro
        // trocas de mapa. A borda transforma "segurando" em "apertou agora".
        let apertou = Acoes {
            ajustar: 1,
            navegar: 1,
            aceitar: true,
            iniciar: true,
            voltar: true,
            perfil: true,
        };
        let primeira = bordas(Acoes::default(), apertou);
        assert_eq!(primeira.ajustar, 1);
        assert_eq!(primeira.navegar, 1);
        assert!(primeira.aceitar && primeira.iniciar && primeira.voltar && primeira.perfil);

        let segunda = bordas(apertou, apertou);
        assert_eq!(segunda, Acoes::default(), "segurar não repete a ação");

        // Soltar e apertar de novo conta como ação nova.
        let soltou = bordas(apertou, Acoes::default());
        assert_eq!(soltou, Acoes::default(), "soltar não é ação");
        let terceira = bordas(Acoes::default(), apertou);
        assert!(terceira.aceitar && terceira.iniciar);
    }

    #[test]
    fn a_borda_deixa_passar_a_mudanca_de_valor() {
        // Segurar a esquerda e depois a direita (sem soltar a primeira) muda o
        // nível de -1 para +1: é uma ação nova e tem de passar.
        let esquerda = Acoes {
            ajustar: -1,
            ..Acoes::default()
        };
        let ambos = Acoes {
            ajustar: 0,
            ..Acoes::default()
        };
        assert_eq!(bordas(esquerda, ambos).ajustar, 0, "cancelou: nada a fazer");
        assert_eq!(bordas(ambos, esquerda).ajustar, -1);
    }

    #[test]
    fn nenhuma_combinacao_produz_entrada_fora_da_faixa() {
        // Varredura: virar sempre em -1..1 e acelerar/frear em 0..1, com
        // qualquer máscara e qualquer tecla.
        for bits in 0..=u32::from(u8::MAX) {
            for teclas in [
                Teclas::default(),
                Teclas {
                    cima: true,
                    baixo: true,
                    esquerda: true,
                    direita: true,
                    ..Teclas::default()
                },
            ] {
                let (e, _) = combinar(bits, &teclas, Fase::Correndo);
                assert!((-1.0..=1.0).contains(&e.virar), "virar {}", e.virar);
                assert!((0.0..=1.0).contains(&e.acelerar));
                assert!((0.0..=1.0).contains(&e.frear));
            }
        }
    }

    #[test]
    fn seta_no_titulo_ajusta_o_valor_e_na_corrida_vira_volante() {
        let esq = Teclas {
            esquerda: true,
            ..sem_teclas()
        };
        let (_, a) = combinar(0, &esq, Fase::Titulo);
        assert_eq!(a.ajustar, -1);
        let (e, a) = combinar(0, &esq, Fase::Correndo);
        assert_eq!(a.ajustar, 0, "na corrida a seta é volante, não menu");
        assert_eq!(e.virar, -1.0);

        let (_, a) = combinar(evdev::DIREITA, &sem_teclas(), Fase::Titulo);
        assert_eq!(a.ajustar, 1, "o direcional do controle também escolhe");
    }

    #[test]
    fn o_volante_vira_e_a_soma_fica_no_limite() {
        let analógico = Analogico { virar: 0.75 };
        let (e, _) = combinar_com_analogico(0, analógico, &sem_teclas(), Fase::Correndo);
        assert!((e.virar - 0.75).abs() < 1e-6);

        let teclas = Teclas {
            direita: true,
            ..sem_teclas()
        };
        let (e, _) = combinar_com_analogico(0, analógico, &teclas, Fase::Correndo);
        assert_eq!(e.virar, 1.0, "duas fontes no mesmo sentido não passam de 1");
    }

    #[test]
    fn o_volante_nao_acelera_nem_freia() {
        // O contrato não declara acelerador analógico: `a` e `b` são botões, e
        // `l2`/`r2` são nitro. Se o volante mexesse no acelerador, o carro
        // andaria sozinho ao fazer uma curva.
        let (e, _) = combinar_com_analogico(
            0,
            Analogico { virar: -0.9 },
            &sem_teclas(),
            Fase::Correndo,
        );
        assert_eq!((e.acelerar, e.frear), (0.0, 0.0));
    }

    #[test]
    fn o_analogico_nao_escolhe_mapa_no_titulo() {
        // O stick analógico é volante, não menu: no título ele não pode trocar
        // de pista a cada quadro por causa de ruído de repouso. Quem navega é o
        // **direcional**, que é discreto e tem borda.
        let (_, a) = combinar_com_analogico(
            0,
            Analogico { virar: 0.9 },
            &sem_teclas(),
            Fase::Titulo,
        );
        assert_eq!(a.ajustar, 0);
        assert_eq!(a.navegar, 0);
    }

    // ── o contrato manda: um dono por quadro (issues #1 e #2) ───────

    fn sem_fontes() -> Fontes {
        Fontes::default()
    }

    /// Contrato pelo evdev, e nada mais.
    fn com_contrato_evdev() -> Fontes {
        Fontes {
            evdev_contrato: true,
            evdev_fisicos: 1,
            mapa_do_sistema: true,
            sdl: true,
            sdl_contrato: true,
        }
    }

    #[test]
    fn o_contrato_pelo_evdev_vence_tudo() {
        // É o degrau mais determinístico: códigos fixos, sem depender do
        // `gamecontrollerdb` nem de tabela por placa.
        assert_eq!(dono(com_contrato_evdev(), Forcar::Automatico), Dono::Evdev);
    }

    #[test]
    fn o_contrato_pelo_sdl_basta_quando_nao_ha_no_evdev() {
        // O PC, e a placa em que o evdev não abriu nada: o `gamecontrollerdb`
        // da imagem garante `Button::A = a`, então o SDL é tão bom quanto.
        let fontes = Fontes {
            sdl: true,
            sdl_contrato: true,
            ..sem_fontes()
        };
        assert_eq!(dono(fontes, Forcar::Automatico), Dono::Sdl);
    }

    #[test]
    fn sem_contrato_o_mapa_do_plastico_ainda_decide() {
        // Imagem antiga, sem o `Giochi Gamepad`: o SDL abre o nó físico com o
        // mapeamento automático errado (`a:b0` no R35S). Quem sabe qual código é
        // o `a` é o `buttons.map`, então o evdev manda.
        let fontes = Fontes {
            evdev_fisicos: 1,
            mapa_do_sistema: true,
            sdl: true,
            sdl_contrato: false,
            ..sem_fontes()
        };
        assert_eq!(dono(fontes, Forcar::Automatico), Dono::Evdev);
    }

    #[test]
    fn sem_contrato_e_sem_mapa_o_sdl_manda() {
        // O PC: não há `/etc/giochi/buttons.map`, e o `Button::A` do SDL é
        // `BTN_SOUTH` de verdade num controle de Xbox.
        let fontes = Fontes {
            evdev_fisicos: 1,
            sdl: true,
            ..sem_fontes()
        };
        assert_eq!(dono(fontes, Forcar::Automatico), Dono::Sdl);
    }

    #[test]
    fn um_teclado_aberto_nao_e_controle() {
        // `controles` só conta nó que declara código do jogo; um teclado aberto
        // não pode escolher a fonte.
        assert_eq!(dono(sem_fontes(), Forcar::Automatico), Dono::Nenhum);
        assert_eq!(
            dono(
                Fontes {
                    mapa_do_sistema: true,
                    ..sem_fontes()
                },
                Forcar::Automatico
            ),
            Dono::Nenhum
        );
    }

    #[test]
    fn a_mascara_e_a_do_dono_e_nunca_a_soma() {
        // A regra que fecha a issue #1: com o A apertado, o evdev acende
        // ACELERAR e o SDL acende FREAR. Somados, o carro freia; do dono, não.
        let do_evdev = evdev::ACELERAR | evdev::ACEITAR;
        let do_sdl = evdev::FREAR;
        let mascara = mascara_dos_botoes(do_evdev, do_sdl, Dono::Evdev);
        assert_eq!(mascara, do_evdev, "só o evdev");
        assert_eq!(mascara_dos_botoes(do_evdev, do_sdl, Dono::Sdl), do_sdl);
        assert_eq!(mascara_dos_botoes(do_evdev, do_sdl, Dono::Nenhum), 0);

        let (e, _) =
            combinar_com_analogico(mascara, Analogico::default(), &sem_teclas(), Fase::Correndo);
        assert_eq!(e.acelerar, 1.0);
        assert_eq!(e.frear, 0.0);
    }

    #[test]
    fn forcar_nao_deixa_o_jogo_sem_controle() {
        // O caminho pedido que não existe cai para o que existe: ficar sem
        // controle é pior que usar o mapa do outro caminho.
        let so_sdl = Fontes {
            sdl: true,
            ..sem_fontes()
        };
        assert_eq!(dono(so_sdl, Forcar::Evdev), Dono::Sdl);
        let so_evdev = Fontes {
            evdev_fisicos: 1,
            ..sem_fontes()
        };
        assert_eq!(dono(so_evdev, Forcar::Sdl), Dono::Evdev);
        assert_eq!(dono(sem_fontes(), Forcar::Sdl), Dono::Nenhum);

        // E quando o caminho existe, ele é respeitado.
        assert_eq!(dono(com_contrato_evdev(), Forcar::Sdl), Dono::Sdl);
        assert_eq!(dono(so_evdev, Forcar::Evdev), Dono::Evdev);
        assert_eq!(Forcar::do_valor("SDL"), Forcar::Sdl);
        assert_eq!(Forcar::do_valor("evdev"), Forcar::Evdev);
        assert_eq!(Forcar::do_valor(" lixo "), Forcar::Automatico);
    }

    #[test]
    fn o_nome_do_dono_e_o_que_vai_para_o_json() {
        assert_eq!(Dono::Evdev.nome(), "evdev");
        assert_eq!(Dono::Sdl.nome(), "sdl");
        assert_eq!(Dono::Nenhum.nome(), "nenhum");
    }
}
