//! Neon Drift: laço principal, GLES e diagnóstico.
//!
//! ## O que este arquivo **não** faz
//!
//! Regra de corrida, física, pontuação e formatação vivem no núcleo
//! (`giochi-neon-drift-core`). Aqui ficam só as três coisas que precisam do
//! aparelho: janela/GLES, entrada e o laço com teto de quadros. É o que permite
//! testar "duas voltas terminam em vitória" sem abrir tela.
//!
//! ## Diagnóstico sem dedo no aparelho
//!
//! `--quadro`, `--quadros`, `--tamanho`, `--teclas`, `--passos` e `--json`
//! existem porque este projeto já pagou caro por ferramenta que só funcionava
//! com alguém olhando a tela: o `--teclas` do launcher passou 223 testes verdes
//! imprimindo um estado que não tinha mudado.

#![forbid(unsafe_code)]

mod atlas;
mod audio;
mod cena3d;
mod efeitos;
mod entrada_jogo;
mod evdev;
mod gamepad;
mod malha;
mod menu;
mod modelos;
mod perfil;
mod rede;
mod texturas;
mod vetor;
mod video;

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use audio::{EstadoSom, Som};
use cena3d::Cena3D;
use entrada_jogo::Teclas;
use gamepad::Gamepad;
use giochi_engine::platform::sdl::Platform;
use giochi_engine::platform::window::{Window, WindowConfig};
use giochi_engine_3d::context::{self, GlesVersion};
use giochi_engine_3d::RenderContext3D;
use giochi_neon_drift_core::carro::Entrada;
use giochi_neon_drift_core::corrida::{Corrida, Evento, Fase};
use giochi_neon_drift_core::dificuldade::Dificuldade;
use giochi_neon_drift_core::relogio::{ALVO_FPS, MAX_FPS, TIQUE};
use giochi_neon_drift_core::{hud, Relogio};
use perfil::Perfil;
use rede::{Papel, Rede};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::surface::Surface;
use video::Rotacao;

const VERSAO: &str = env!("CARGO_PKG_VERSION");

/// Opções de linha de comando.
#[derive(Debug)]
struct Opcoes {
    quadro: Option<String>,
    /// Grava a folha das texturas em BMP e sai, sem abrir janela.
    texturas: Option<String>,
    /// Grava o atlas de sprites em BMP e sai, sem abrir janela.
    atlas: Option<String>,
    /// Grava a prova de rotação e sai (`--prova-rotacao arquivo.bmp`).
    ///
    /// Três barras em posições conhecidas do **projeto**, com predição numérica do
    /// lugar de cada uma no **painel**. Existe porque "está de ponta a cabeça" e
    /// "as informações na vertical" não dizem qual peça girou errado, e comparar
    /// aparelho com PC não separa os casos quando os dois erram igual.
    prova_rotacao: Option<String>,
    tamanho: Option<(u32, u32)>,
    quadros: Option<u32>,
    passos: u32,
    /// `s` onde o carro começa, em metros (`--em`).
    em: Option<f32>,
    teclas: Teclas,
    /// Toques simulados, um por dois quadros (aperta, solta).
    teclas_ciclo: Option<Vec<Teclas>>,
    /// Botões de controle segurados, como máscara (`--botoes`).
    botoes: u32,
    /// Toques de botão, um por dois quadros (`--botoes-ciclo "a;baixo;a"`).
    botoes_ciclo: Option<Vec<u32>>,
    tela: Option<Fase>,
    fps: u32,
    json: bool,
    sem_som: bool,
    piloto: bool,
    so: Option<String>,
    mapa: Option<String>,
    /// Modo de dificuldade por id (`facil`, `normal`, `dificil`, `desafiante`).
    dificuldade: Option<String>,
    sem_vsync: bool,
    escala: f32,
    /// Sobe um host de corrida LAN.
    lan_host: bool,
    /// Entra num host LAN no endereço dado (`ip[:porta]`).
    lan_entrar: Option<String>,
    /// Procura um lobby por mDNS antes de abrir a corrida.
    lan_procurar: bool,
    /// Porta do host LAN (padrão: `giochi_net::PORTA_PADRAO`).
    lan_porta: Option<u16>,
    ajuda: bool,
    versao: bool,
}

impl Default for Opcoes {
    fn default() -> Self {
        Self {
            quadro: None,
            atlas: None,
            prova_rotacao: None,
            tamanho: None,
            quadros: None,
            passos: 0,
            em: None,
            texturas: None,
            teclas: Teclas::default(),
            teclas_ciclo: None,
            botoes: 0,
            botoes_ciclo: None,
            tela: None,
            fps: ALVO_FPS,
            json: false,
            sem_som: false,
            piloto: false,
            so: None,
            mapa: None,
            dificuldade: None,
            sem_vsync: std::env::var_os("GIOCHI_VSYNC").is_some_and(|v| v == "0"),
            escala: std::env::var("GIOCHI_ESCALA")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.0),
            lan_host: false,
            lan_entrar: None,
            lan_procurar: false,
            lan_porta: None,
            ajuda: false,
            versao: false,
        }
    }
}

fn opcoes(argv: Vec<String>) -> Result<Opcoes, String> {
    let mut o = Opcoes::default();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--quadro" => {
                o.quadro = Some(argv.get(i + 1).ok_or("--quadro sem caminho")?.clone());
                i += 2;
            }
            // Grava a folha das texturas procedurais: é o molde que o artista
            // usa para pintar a versão dele. Sem isto, "que densidade tem o
            // asfalto?" só se responde lendo o código.
            "--texturas" => {
                o.texturas = Some(argv.get(i + 1).ok_or("--texturas sem caminho")?.clone());
                i += 2;
            }
            "--prova-rotacao" => {
                o.prova_rotacao = Some(
                    argv.get(i + 1)
                        .ok_or("--prova-rotacao sem caminho")?
                        .clone(),
                );
                // Sem este avanço o laço relê a mesma flag para sempre: o
                // processo gira em silêncio, sem imprimir nada e sem abrir
                // janela. Custou uma rodada de diagnóstico.
                i += 2;
            }
            "--atlas" => {
                o.atlas = Some(argv.get(i + 1).ok_or("--atlas sem caminho")?.clone());
                i += 2;
            }
            "--tamanho" => {
                let valor = argv.get(i + 1).ok_or("--tamanho sem valor")?;
                let (l, a) = valor
                    .split_once('x')
                    .ok_or("--tamanho espera LARGURAxALTURA")?;
                let tamanho = (
                    l.parse().map_err(|_| "largura inválida")?,
                    a.parse().map_err(|_| "altura inválida")?,
                );
                if tamanho.0 == 0 || tamanho.1 == 0 {
                    return Err("--tamanho exige dimensões maiores que zero".into());
                }
                o.tamanho = Some(tamanho);
                i += 2;
            }
            "--quadros" | "--frames" => {
                let n: u32 = argv
                    .get(i + 1)
                    .ok_or("--quadros sem valor")?
                    .parse()
                    .map_err(|_| "--quadros espera um número")?;
                if n == 0 {
                    return Err("--quadros precisa ser maior que zero".into());
                }
                o.quadros = Some(n);
                i += 2;
            }
            // Posiciona o carro em `s` metros da volta, já andando na velocidade
            // de cruzeiro. Existe para testar rampa: chegar ao salto a 30 s de
            // volta transforma cada tentativa de ajuste em meio minuto.
            "--em" => {
                o.em = Some(
                    argv.get(i + 1)
                        .ok_or("--em sem valor")?
                        .parse()
                        .map_err(|_| "--em espera metros")?,
                );
                i += 2;
            }
            "--passos" => {
                o.passos = argv
                    .get(i + 1)
                    .ok_or("--passos sem valor")?
                    .parse()
                    .map_err(|_| "--passos espera um número")?;
                i += 2;
            }
            "--teclas" => {
                let valor = argv.get(i + 1).ok_or("--teclas sem valor")?;
                o.teclas = teclas(valor)?;
                i += 2;
            }
            // `--teclas` deixa a tecla **segurando** para sempre, e o laço trata
            // cada ação como borda: segurar move o cursor **uma** linha. Para
            // provar "apertei cinco vezes" (navegar o menu, confirmar a LAN) é
            // preciso soltar entre os toques, que é o que este ciclo faz.
            //
            // A lista é separada por `;` e cada pedaço é **um** toque:
            // `--teclas-ciclo "down;down;a"` desce duas linhas e confirma.
            // Botões do **controle**, não do teclado. Existem porque o teclado
            // não é mais o caminho principal: sem isto, provar por script que o
            // A do controle navega o menu seria impossível — e `--teclas a`
            // significa "acelerar" (seta para cima), não "botão A".
            "--botoes" => {
                let valor = argv.get(i + 1).ok_or("--botoes sem valor")?;
                o.botoes = botoes_de_lista(valor)?;
                i += 2;
            }
            "--botoes-ciclo" => {
                let valor = argv.get(i + 1).ok_or("--botoes-ciclo sem valor")?;
                let mut toques = Vec::new();
                for pedaco in valor.split(';') {
                    toques.push(botoes_de_lista(pedaco)?);
                }
                o.botoes_ciclo = Some(toques);
                i += 2;
            }
            "--teclas-ciclo" => {
                let valor = argv.get(i + 1).ok_or("--teclas-ciclo sem valor")?;
                let mut toques = Vec::new();
                for pedaco in valor.split(';') {
                    toques.push(teclas(pedaco)?);
                }
                o.teclas_ciclo = Some(toques);
                i += 2;
            }
            "--tela" => {
                let valor = argv.get(i + 1).ok_or("--tela sem valor")?;
                o.tela = Some(match valor.as_str() {
                    "titulo" | "title" => Fase::Titulo,
                    "corrida" | "race" => Fase::Correndo,
                    "resultado" | "result" => Fase::Resultado,
                    outro => return Err(format!("--tela desconhecida: {outro}")),
                });
                i += 2;
            }
            "--fps-teto" => {
                let fps: u32 = argv
                    .get(i + 1)
                    .ok_or("--fps-teto sem valor")?
                    .parse()
                    .map_err(|_| "--fps-teto espera um número")?;
                if !(1..=MAX_FPS).contains(&fps) {
                    return Err(format!("--fps-teto precisa ficar entre 1 e {MAX_FPS}"));
                }
                o.fps = fps;
                i += 2;
            }
            "--lan-host" => {
                o.lan_host = true;
                i += 1;
            }
            "--lan-entrar" => {
                o.lan_entrar = Some(argv.get(i + 1).ok_or("--lan-entrar sem endereço")?.clone());
                i += 2;
            }
            "--lan-procurar" => {
                o.lan_procurar = true;
                i += 1;
            }
            "--lan-porta" => {
                let porta: u16 = argv
                    .get(i + 1)
                    .ok_or("--lan-porta sem valor")?
                    .parse()
                    .map_err(|_| "--lan-porta espera um número de 1 a 65534")?;
                if porta == 0 {
                    return Err("--lan-porta não aceita 0".into());
                }
                o.lan_porta = Some(porta);
                i += 2;
            }
            "--json" => {
                o.json = true;
                i += 1;
            }
            "--piloto" => {
                o.piloto = true;
                i += 1;
            }
            "--mapa" => {
                o.mapa = Some(argv.get(i + 1).ok_or("--mapa sem nome")?.clone());
                i += 2;
            }
            "--dificuldade" => {
                o.dificuldade = Some(argv.get(i + 1).ok_or("--dificuldade sem nome")?.clone());
                i += 2;
            }
            "--so" => {
                o.so = Some(argv.get(i + 1).ok_or("--so sem nome de malha")?.clone());
                i += 1;
                i += 1;
            }
            "--sem-som" => {
                o.sem_som = true;
                i += 1;
            }
            "--sem-vsync" => {
                o.sem_vsync = true;
                i += 1;
            }
            "--escala" => {
                let v = argv.get(i + 1).ok_or("--escala sem valor")?;
                o.escala = v
                    .parse::<f32>()
                    .map_err(|_| format!("--escala precisa de um número, veio {v}"))?;
                i += 2;
            }
            "-h" | "--ajuda" => {
                o.ajuda = true;
                i += 1;
            }
            "--versao" => {
                o.versao = true;
                i += 1;
            }
            outro => return Err(format!("argumento desconhecido: {outro}")),
        }
    }
    Ok(o)
}

/// Converte `a,right,space` em teclas mantidas apertadas.
fn teclas(valor: &str) -> Result<Teclas, String> {
    let mut t = Teclas::default();
    for nome in valor.split(',') {
        match nome.trim() {
            "" => {}
            "a" | "acelerar" | "up" => t.cima = true,
            "b" | "frear" | "down" => t.baixo = true,
            "left" | "esquerda" => t.esquerda = true,
            "right" | "direita" => t.direita = true,
            "space" | "espaco" | "drift" => t.espaco = true,
            "shift" | "nitro" => t.shift = true,
            "enter" | "start" => t.enter = true,
            "esc" | "escape" => t.escape = true,
            outro => return Err(format!("tecla desconhecida: {outro}")),
        }
    }
    Ok(t)
}

/// Botões de controle a partir de uma lista (`a,b,start`), para diagnóstico.
///
/// O vocabulário é o do plástico, não o do bit: quem escreve o script pensa "o
/// A", não "ACEITAR". `a` acende os dois bits do botão A (acelerar e aceitar),
/// exatamente como o `evdev` faz.
fn botoes_de_lista(valor: &str) -> Result<u32, String> {
    use evdev as b;
    let mut mascara = 0;
    for nome in valor.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        mascara |= match nome {
            "a" | "acelerar" | "aceitar" => b::ACELERAR | b::ACEITAR,
            "b" | "frear" => b::FREAR,
            "x" | "perfil" => b::PERFIL,
            "y" | "select" | "voltar" => b::VOLTAR,
            "start" | "iniciar" => b::INICIAR,
            "l1" | "r1" | "derrapar" => b::FREIO_MAO,
            "l2" | "r2" | "nitro" => b::NITRO,
            "cima" => b::CIMA,
            "baixo" => b::BAIXO,
            "esq" | "esquerda" => b::ESQUERDA,
            "dir" | "direita" => b::DIREITA,
            outro => return Err(format!("botão desconhecido: {outro}")),
        };
    }
    Ok(mascara)
}

/// Estado da corrida em uma linha, para o `--json`.
///
/// Existe porque `--passos` sem número de retorno é fé: rodar 900 passos e
/// receber `"fase":"Correndo"` não diz se o carro andou 4 m ou 500 m.
fn diagnostico(corrida: &Corrida, cena: &Cena3D, render: &RenderContext3D) -> String {
    // Contadores da **engine**, não do jogo. O jogo tinha um `desenhos` próprio,
    // incrementado à mão em cada ponto de desenho, e ele já estava errado por um:
    // os riscos de velocidade emitiam `draw_instanced` sem incrementar. Um
    // contador que o autor precisa lembrar de atualizar mede intenção; este conta
    // no ponto em que o backend emite a chamada.
    let contadores = render.frame_counters();
    let s = corrida.carro.pos;
    let (cx, cy, cz, fov) = cena.camera();
    // Quanto o melhor adversário está à frente do jogador, em metros (negativo =
    // o jogador lidera). Existe porque `--dificuldade` sem número mensurável é
    // fé: `posicao` vale 1 nas quatro dificuldades com o piloto ao volante, e
    // "1º" não distingue ganhar por 200 m de ganhar por 2.
    let rival = corrida
        .adversarios
        .iter()
        .map(|a| a.progresso(&corrida.pista))
        .fold(f32::NEG_INFINITY, f32::max);
    let gap = rival - corrida.progresso_jogador();
    format!(
        "\"velocidade\":{:.1},\"volta\":{},\"posicao\":{},\"tempo\":{:.2},\"drift\":{},\"s\":{:.1},\"x\":{:.1},\"z\":{:.1},\"altitude\":{:.2},\"cam\":[{:.1},{:.1},{:.1}],\"fov\":{:.2},\"mapa\":\"{}\",\"dificuldade\":\"{}\",\"gap\":{:.1},\"posicao\":{},\"rendendo\":{},\"impacto\":{:.2},\"fragmentos\":{},\"largou\":{},\"progresso\":{:.1},\"no_ar\":{},\"v_vertical\":{:.2},\"altura_solo\":{:.2},\"nitro\":{:.1},\"nitro_usando\":{},\"derrapando\":{},\"malhas\":{},\"vertices\":{}",
        corrida.carro.velocidade,
        corrida.volta,
        corrida.posicao,
        corrida.tempo,
        corrida.drift.pontos,
        corrida.carro.s,
        s.x,
        s.z,
        corrida.carro.altura,
        cx,
        cy,
        cz,
        fov,
        corrida.mapa.id,
        corrida.dificuldade.id(),
        gap,
        corrida.posicao,
        corrida.carro.raspando,
        corrida.carro.impacto,
        cena.fragmentos(),
        // `largou` é o que separa "volta 1" de "corrida começada": sem ele, um
        // `--passos 250` que responde `volta=1` pode ser uma corrida que não
        // largou ou uma que largou e ainda não fechou a volta. O diagnóstico
        // existe para não ter de adivinhar entre as duas.
        corrida.largou(),
        corrida.progresso_jogador(),
        // `no_ar` e `v_vertical` são o salto em números: sem eles, "o carro
        // pulou" só se prova olhando a tela, e um salto que não acontece é
        // indistinguível de uma rampa que não existe.
        corrida.carro.no_ar,
        corrida.carro.velocidade_vertical,
        corrida.pista.altura_em(corrida.carro.s),
        // O nitro é a economia do jogo: `gasto 30/s` contra `recarga 8/s` dá
        // 21 % de ciclo ativo. Sem expor os dois, "o carro passa dos 200 km/h
        // direto" é indistinguível de "o tanque está infinito".
        corrida.carro.nitro,
        corrida.carro.nitro_usando,
        corrida.carro.derrapando,
        contadores.meshes_drawn,
        contadores.vertices_uploaded
    )
}

fn ajuda() {
    println!(
        "giochi-neon-drift {VERSAO}

Uso:
  giochi-neon-drift                        jogar
  giochi-neon-drift --quadro <arq.bmp>      desenhar um quadro e sair
  giochi-neon-drift --tamanho 640x480       painel simulado
  giochi-neon-drift --quadros 300           desenhar N quadros e sair
  giochi-neon-drift --passos 2000           adiantar a simulação
  giochi-neon-drift --em 1240 --teclas up   começar o carro em 1 240 m da volta
  giochi-neon-drift --piloto                o piloto artificial dirige (nos --passos e nos quadros)
  giochi-neon-drift --teclas a,right,shift  teclas seguradas
  giochi-neon-drift --teclas-ciclo down;down;a
                                            toques de tecla (; = um toque)
  giochi-neon-drift --botoes a,start        botões de controle segurados
  giochi-neon-drift --botoes-ciclo baixo;baixo;a
                                            toques de botão do controle
  giochi-neon-drift --tela resultado        forçar uma tela
  giochi-neon-drift --fps-teto 75           limite: 1 a 75 (é também o padrão)
  giochi-neon-drift --sem-som               sem áudio
  giochi-neon-drift --mapa boavista         escolhe o mapa (avenida, boavista)
  giochi-neon-drift --so asfalto            desenha uma malha só (diagnóstico)
  giochi-neon-drift --texturas t.bmp        grava a folha das 9 texturas e sai
  giochi-neon-drift --atlas a.bmp           grava o atlas de sprites e sai
  giochi-neon-drift --prova-rotacao r.bmp   grava a prova de orientação e sai
  GIOCHI_SPRITES=x.bmp GIOCHI_SPRITES_COLUNAS=8
                                            usa a sua folha de sprites (BMP)
  giochi-neon-drift --frames 300             idem `--quadros` (nome de smoke test)
  giochi-neon-drift --lan-host               hospeda uma corrida LAN (mDNS)
  giochi-neon-drift --lan-entrar 192.168.0.9 entra num host (porta padrão 47721)
  giochi-neon-drift --lan-procurar           descobre um lobby por mDNS e entra
  giochi-neon-drift --lan-porta 47721        porta do host (controle = porta+1)
  giochi-neon-drift --json                  saída de máquina (stdout é só o JSON)
  giochi-neon-drift --versao

Controles (o gamepad e o caminho principal; o teclado e o atalho):
  A/gatilho direito acelera, B/gatilho esquerdo freia, analogico/direcional vira,
  L1/R1 freio de mao (drift), L2/R2 nitro, X overlay, START jogar/reiniciar,
  Y/SELECT volta ao menu (no menu, sai).
Menu do titulo: direcional cima/baixo escolhe a linha, esq/dir muda o valor, A aceita.
  MAPA, DIFICULDADE, QUALIDADE (escala de render), PERFIL, LAN, JOGAR.
Teclado: WASD/setas, ESPACO freio de mao, SHIFT nitro, ENTER jogar, ESC sair;
  H hospeda a LAN, J entra, F3 escala, F1 overlay, -/= mudam o valor do menu.
GIOCHI_GL_VERSION=2 solicita GLES 2.0; o padrão pede 3.0 com queda para 2.0."
    );
}

fn painel(plataforma: &Platform, o: &Opcoes) -> Result<(u32, u32), String> {
    if let Some(t) = o.tamanho {
        return Ok(t);
    }
    plataforma
        .video_subsystem()
        .desktop_display_mode(0)
        .map(|m| (m.w as u32, m.h as u32))
        .map_err(|e| format!("não consegui consultar o painel: {e}"))
}

fn criar_render(
    plataforma: &Platform,
    tamanho: (u32, u32),
    tela_cheia: bool,
    versao: GlesVersion,
    sem_vsync: bool,
) -> Result<(Window, RenderContext3D), Box<dyn std::error::Error>> {
    context::configure_attributes(plataforma, versao)?;
    let config = WindowConfig::new(tamanho.0, tamanho.1).fullscreen(tela_cheia);
    let janela = Window::new_opengl(plataforma, config, "Giochi Neon Drift")?;
    // `--sem-vsync` existe porque o KMSDRM desta imagem trava o swap em 60 Hz: com
    // vsync, todo quadro mais barato que 16,7 ms mede "59,9 fps" e não dá para
    // saber se sobrou folga ou se faltou 1 ms. Sem vsync o número passa a ser o
    // custo real do quadro — é o que permite decidir se um efeito novo cabe.
    let render = RenderContext3D::new(context::create(&janela)?);
    if sem_vsync {
        // O vsync pertence ao contexto GL, e o `sdl2` 0.38 expõe a troca em
        // `VideoSubsystem` — não na janela. Chamar a função C crua exigiria
        // `unsafe`, proibido no jogo (`#![forbid(unsafe_code)]`).
        if let Err(erro) = plataforma.video_subsystem().gl_set_swap_interval(0) {
            eprintln!("aviso: não consegui desligar o vsync ({erro})");
        }
    }
    Ok((janela, render))
}

/// Grava o back buffer em BMP.
///
/// `read_pixels` devolve as linhas de baixo para cima (convenção do GL); o BMP
/// espera de cima para baixo. Sem a inversão o quadro sai virado e a comparação
/// com a referência mede o espelho, não o desenho.
fn gravar_quadro(
    render: &RenderContext3D,
    caminho: &str,
) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    let (largura, altura) = render.drawable_size();
    let mut pixels = render.read_pixels(largura, altura)?;
    let pitch = largura as usize * 4;
    for y in 0..altura as usize / 2 {
        let outro = altura as usize - 1 - y;
        let (inicio, fim) = pixels.split_at_mut(outro * pitch);
        inicio[y * pitch..(y + 1) * pitch].swap_with_slice(&mut fim[..pitch]);
    }
    let superficie = Surface::from_data(
        &mut pixels,
        largura,
        altura,
        largura * PixelFormatEnum::RGBA32.byte_size_per_pixel() as u32,
        PixelFormatEnum::RGBA32,
    )?;
    superficie.save_bmp(caminho)?;
    Ok((largura, altura))
}

/// Grava a folha das texturas procedurais em BMP, sem abrir janela.
///
/// Uma tira horizontal com as nove texturas lado a lado, na ordem de
/// `texturas::todas()`, com uma faixa escura de separação. Serve de **molde**:
/// quem quiser desenhar a própria textura pinta em cima destas células, com o
/// mesmo tamanho e a mesma densidade, e o resultado entra no jogo sem ajuste.
///
/// A folha é o **molde**: quem pinta a versão dele põe o arquivo em
/// `GIOCHI_TEXTURAS=<dir>` com o nome do material (`asfalto.png`, …) e o jogo o
/// carrega por `render.create_texture`. O que não existir continua vindo da
/// fórmula, e o log diz material por material qual foi o caminho.
fn gravar_texturas(caminho: &str) -> Result<(), Box<dyn std::error::Error>> {
    let lista = texturas::todas();
    let lado = texturas::LADO;
    let separacao = 4u32;
    let largura = lista.len() as u32 * (lado + separacao) + separacao;
    let altura = lado + separacao * 2;
    let mut pixels = vec![0u8; (largura * altura * 4) as usize];
    // Fundo: cinza escuro, para a borda de cada célula ser visível.
    for i in 0..(largura * altura) as usize {
        pixels[i * 4] = 24;
        pixels[i * 4 + 1] = 24;
        pixels[i * 4 + 2] = 32;
        pixels[i * 4 + 3] = 255;
    }
    for (indice, textura) in lista.iter().enumerate() {
        let x0 = separacao + indice as u32 * (lado + separacao);
        let y0 = separacao;
        for y in 0..lado {
            for x in 0..lado {
                let origem = ((y * lado + x) * 4) as usize;
                let destino = (((y + y0) * largura + (x + x0)) * 4) as usize;
                pixels[destino..destino + 4].copy_from_slice(&textura.pixels[origem..origem + 4]);
            }
        }
    }
    let superficie = Surface::from_data(
        &mut pixels,
        largura,
        altura,
        largura * PixelFormatEnum::RGBA32.byte_size_per_pixel() as u32,
        PixelFormatEnum::RGBA32,
    )?;
    superficie.save_bmp(caminho)?;
    eprintln!(
        "neon-drift: {largura}x{altura}, {} texturas de {lado}x{lado} ({}): {}",
        lista.len(),
        lista.iter().map(|t| t.nome).collect::<Vec<_>>().join(", "),
        caminho
    );
    Ok(())
}

/// Grava o atlas de sprites em BMP, **sem abrir janela nem contexto GL**.
///
/// O atlas é uma função pura (`atlas::gerar`), mas "a função roda" não é a mesma
/// afirmação que "a forma está certa": uma gaussiana centrada errada continua
/// gerando bytes válidos e desenha a nuvem fora do quad. Este arquivo é a única
/// forma de olhar para o que as partículas vão amostrar.
///
/// O fundo sai **magenta**, e não preto: o alfa 0 de uma célula vazia é
/// indistinguível de um sprite preto sobre preto, e o quadro mentiria sobre ter
/// desenhado.
fn gravar_atlas(caminho: &str) -> Result<(), Box<dyn std::error::Error>> {
    let lado = atlas::LADO;
    // Respeita `GIOCHI_SPRITES` como o jogo: uma ferramenta que mostra o atlas
    // procedural enquanto o jogo desenha a folha do usuário mede outra coisa.
    let colunas = std::env::var("GIOCHI_SPRITES_COLUNAS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(atlas::COLUNAS);
    let caminho_folha = std::env::var("GIOCHI_SPRITES").ok();
    let (origem, fonte, problema) = atlas::carregar_ou_gerar(caminho_folha.as_deref(), colunas);
    if let Some(motivo) = problema {
        eprintln!("neon-drift: folha de sprites ignorada — {motivo}");
    }
    let mut pixels = vec![0u8; (lado * lado * 4) as usize];
    for i in 0..(lado * lado) as usize {
        let a = f32::from(origem[i * 4 + 3]) / 255.0;
        // Fundo magenta fixo; o branco do atlas entra por cima ponderado pelo alfa.
        let fundo = [255.0f32, 0.0, 255.0];
        for canal in 0..3 {
            let cor = f32::from(origem[i * 4 + canal]);
            pixels[i * 4 + canal] = (fundo[canal] * (1.0 - a) + cor * a).round() as u8;
        }
        pixels[i * 4 + 3] = 255;
    }
    let superficie = Surface::from_data(
        &mut pixels,
        lado,
        lado,
        lado * PixelFormatEnum::RGBA32.byte_size_per_pixel() as u32,
        PixelFormatEnum::RGBA32,
    )?;
    superficie.save_bmp(caminho)?;
    eprintln!(
        "neon-drift: atlas {}x{} com {} formas ({}) gravado em {caminho}",
        lado,
        lado,
        atlas::Forma::TODAS.len(),
        fonte.nome()
    );
    Ok(())
}

/// Estado do som a partir da corrida.
fn estado_som(corrida: &Corrida) -> EstadoSom {
    let carro = &corrida.carro;
    EstadoSom {
        rpm: carro.rpm(),
        marcha: carro.marcha(),
        acelerando: carro.velocidade > 0.5,
        deslize: if carro.derrapando {
            carro.derrapagem.abs()
        } else {
            0.0
        },
        nitro: carro.nitro_usando,
        kmh: carro.kmh(),
        ativo: corrida.fase == Fase::Correndo && corrida.contagem <= 0.0,
    }
}

/// Sobe a sessão de rede pedida na linha de comando, se houver.
fn criar_rede(o: &Opcoes) -> Result<Rede, String> {
    if o.lan_host {
        return Rede::hospedar(o.lan_porta);
    }
    if let Some(texto) = &o.lan_entrar {
        let endereco = endereco_lan(texto)?;
        return Rede::entrar(endereco);
    }
    if o.lan_procurar {
        let endereco = Rede::procurar(Duration::from_secs(3))?;
        return Rede::entrar(endereco);
    }
    Ok(Rede::off())
}

/// Aceita `ip`, `ip:porta` ou um nome resolvível.
fn endereco_lan(texto: &str) -> Result<SocketAddr, String> {
    let texto = texto.trim();
    if let Ok(endereco) = texto.parse::<SocketAddr>() {
        return Ok(endereco);
    }
    if let Ok(ip) = texto.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, giochi_net::PORTA_PADRAO));
    }
    // Nome de máquina: resolve uma vez, no boot, e não no laço.
    use std::net::ToSocketAddrs;
    (texto, giochi_net::PORTA_PADRAO)
        .to_socket_addrs()
        .map_err(|e| format!("--lan-entrar '{texto}': {e}"))?
        .next()
        .ok_or_else(|| format!("--lan-entrar '{texto}': nenhum endereço"))
}

/// Aplica a escala de renderização no renderer e no overlay.
fn definir_escala(render: &mut RenderContext3D, perfil: &mut Perfil, escala: Option<f32>) {
    render.set_render_scale(escala);
    perfil.definir_escala(escala);
    let (rw, rh) = render.render_size();
    eprintln!(
        "neon-drift: escala {:.2} — alvo interno {rw}x{rh}",
        escala.unwrap_or(1.0)
    );
}

/// Rodapé do HUD: o estado da rede (quando há) e a escala em vigor.
///
/// A escala aparece aqui porque é o número que muda o que foi desenhado: sem
/// ele, o jogador aperta o botão da qualidade e não vê nada acontecer além do
/// quadro ficar mais rápido.
fn aviso_hud(rede: &Rede, menu: &menu::Menu) -> String {
    let mut texto = rede.aviso().to_string();
    if !texto.is_empty() {
        texto.push_str("  ");
    }
    texto.push_str(&format!(
        "ESCALA {}  X PERFIL  START MENU",
        menu::rotulo_escala(menu.escala())
    ));
    texto
}

/// Monta as linhas do menu do título e a dica da linha do cursor.
///
/// Existe para o laço e o `--quadro` desenharem **o mesmo** menu: duas
/// montagens diferentes dariam uma captura de tela que não é o que o jogador
/// vê, e foi assim que um `--quadro` já mentiu sobre o HUD.
fn montar_menu(
    menu: &menu::Menu,
    corrida: &Corrida,
    rede: &Rede,
) -> (Vec<(String, bool)>, String) {
    // `resumo_curto`: o `resumo` completo tem 58 caracteres e não cabe no
    // painel de 640×480 (ele sai pelas duas bordas, porque o texto é centrado).
    // O completo continua no overlay F1, que é onde o diagnóstico mora.
    let resumo = rede.resumo_curto();
    let ctx = menu::Contexto {
        mapa: corrida.mapa.nome,
        dificuldade: corrida.dificuldade.nome(),
        dificuldade_dica: corrida.dificuldade.descricao(),
        rede: &resumo,
    };
    let dica = menu.dica(&ctx, rede.ativo());
    (menu.linhas(&ctx), dica)
}

/// Troca de mapa: o mapa é **construtivo** (define pista, relevo, voltas,
/// cenário e até as malhas), então trocar é reconstruir a corrida e a cena.
///
/// `release_all`/`destruir` antes liberam as malhas e texturas do mapa anterior;
/// sem isso cada troca deixaria um mundo pendurado na GPU.
fn trocar_mapa(
    passo: i32,
    indice_mapa: &mut usize,
    corrida: &mut Corrida,
    cena: &mut Cena3D,
    render: &mut RenderContext3D,
    rotacao: Rotacao,
    o: &Opcoes,
) -> Result<(), Box<dyn std::error::Error>> {
    let total = giochi_neon_drift_core::mapa::TODOS.len() as i32;
    let novo = ((*indice_mapa as i32 + passo).rem_euclid(total)) as usize;
    if novo == *indice_mapa {
        return Ok(());
    }
    *indice_mapa = novo;
    let mapa = giochi_neon_drift_core::mapa::TODOS[novo];
    *corrida = Corrida::com_mapa_e_dificuldade(mapa, corrida.dificuldade);
    // Destruição explícita do mundo anterior: cada textura e malha tem de passar
    // pelos `destroy_*`, não só pelo `release_all`.
    cena.destruir(render);
    // O buffer de instâncias também foi junto. Sem recriar, o mundo novo desenha
    // carros e partículas **empilhados na origem** — medido antes desta linha:
    // 50 desenhos por quadro antes da troca, 30 depois, porque `desenhar_carros`
    // e `desenhar_particulas` desistem sem instancing.
    render.rebuild_instancing();
    *cena = Cena3D::nova(render, &corrida.pista, rotacao, mapa.cenario)?;
    cena.so(o.so.clone());
    if !o.json {
        eprintln!(
            "neon-drift: mapa \"{}\" ({}) — pista {:.0} m, desnível {:.0} m",
            mapa.nome,
            mapa.id,
            corrida.pista.comprimento(),
            mapa.desnivel()
        );
    }
    Ok(())
}

/// Abre um lobby ou entra num, pelo mesmo caminho do menu e das teclas.
fn abrir_lan(acao: menu::AcaoLan, porta: Option<u16>) -> Result<Rede, String> {
    match acao {
        menu::AcaoLan::Nada => Ok(Rede::off()),
        menu::AcaoLan::Hospedar => Rede::hospedar(porta),
        // O `procurar` bloqueia por até 3 s: é o preço de não ter teclado para
        // digitar um IP, e é o que o mDNS compra.
        menu::AcaoLan::Entrar => Rede::procurar(Duration::from_secs(3)).and_then(Rede::entrar),
    }
}

/// Aplica **um** pedido do menu. É o único lugar que muda o jogo por escolha do
/// jogador — teclado, controle e `--passos` passam todos por aqui.
#[allow(clippy::too_many_arguments)]
fn aplicar_pedido(
    pedido: menu::Pedido,
    menu: &mut menu::Menu,
    perfil: &mut Perfil,
    render: &mut RenderContext3D,
    corrida: &mut Corrida,
    indice_mapa: &mut usize,
    cena: &mut Cena3D,
    rotacao: Rotacao,
    o: &Opcoes,
    rede: &mut Rede,
) -> Result<(), Box<dyn std::error::Error>> {
    match pedido {
        menu::Pedido::Nada => {}
        menu::Pedido::Jogar => corrida.comecar(),
        menu::Pedido::Escala(escala) => {
            menu.definir_escala(escala);
            definir_escala(render, perfil, escala);
        }
        menu::Pedido::Perfil(ligado) => {
            menu.definir_perfil(ligado);
            perfil.definir_visivel(ligado);
        }
        // Mapa e dificuldade não se mexem com a LAN no ar: num host-autoritativo
        // a pista é do host, e trocá-la no cliente desenharia outro mundo.
        menu::Pedido::Mapa(passo) => {
            if corrida.fase == Fase::Titulo && !rede.ativo() {
                trocar_mapa(passo, indice_mapa, corrida, cena, render, rotacao, o)?;
            }
        }
        menu::Pedido::Dificuldade(passo) => {
            if corrida.fase == Fase::Titulo && !rede.ativo() {
                let mut nova = corrida.dificuldade;
                for _ in 0..passo.abs() {
                    nova = if passo > 0 {
                        nova.proxima()
                    } else {
                        nova.anterior()
                    };
                }
                if nova != corrida.dificuldade {
                    corrida.definir_dificuldade(nova);
                    if !o.json {
                        eprintln!("neon-drift: dificuldade {} ({})", nova.nome(), nova.descricao());
                    }
                }
            }
        }
        menu::Pedido::Hospedar | menu::Pedido::Entrar => {
            if corrida.fase != Fase::Titulo || rede.ativo() {
                return Ok(());
            }
            let acao = if pedido == menu::Pedido::Hospedar {
                menu::AcaoLan::Hospedar
            } else {
                menu::AcaoLan::Entrar
            };
            match abrir_lan(acao, o.lan_porta) {
                Ok(nova) => {
                    *rede = nova;
                    perfil.definir_rede(rede.resumo());
                }
                Err(erro) => eprintln!("neon-drift: LAN {acao:?} falhou: {erro}"),
            }
        }
        menu::Pedido::DesligarLan => {
            *rede = Rede::off();
            perfil.definir_rede(rede.resumo());
            cena.definir_carros_rede(Vec::new(), true);
            if !o.json {
                eprintln!("neon-drift: LAN desligada");
            }
        }
    }
    Ok(())
}

fn principal() -> Result<i32, Box<dyn std::error::Error>> {
    let o = opcoes(std::env::args().skip(1).collect())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    if o.ajuda {
        ajuda();
        return Ok(0);
    }
    if o.versao {
        if o.json {
            println!("{{\"nome\":\"giochi-neon-drift\",\"versao\":\"{VERSAO}\"}}");
        } else {
            println!("giochi-neon-drift {VERSAO}");
        }
        return Ok(0);
    }

    if let Some(caminho) = &o.texturas {
        gravar_texturas(caminho)?;
        return Ok(0);
    }
    if let Some(caminho) = &o.atlas {
        gravar_atlas(caminho)?;
        return Ok(0);
    }

    // GLES 3 por padrão, com queda para 2 — o piso dos aparelhos do projeto.
    if std::env::var_os("GIOCHI_GL_VERSION").is_none() {
        std::env::set_var("GIOCHI_GL_VERSION", "3");
    }

    let plataforma = Platform::new()?;
    let tamanho_painel = painel(&plataforma, &o).map_err(std::io::Error::other)?;
    let (rotacao, fonte_rotacao) =
        video::resolver(tamanho_painel.0, tamanho_painel.1, o.tamanho.is_none());

    let versao_pedida = GlesVersion::from_env();
    let tela_cheia = o.tamanho.is_none();
    let (janela, mut render) =
        match criar_render(&plataforma, tamanho_painel, tela_cheia, versao_pedida, o.sem_vsync) {
            Ok(par) => par,
            Err(erro) if versao_pedida == GlesVersion::Gles3 => {
                eprintln!("aviso: GLES 3.0 indisponível ({erro}); tentando GLES 2.0");
                std::env::set_var("GIOCHI_GL_VERSION", "2");
                criar_render(&plataforma, tamanho_painel, tela_cheia, GlesVersion::Gles2, o.sem_vsync)?
            }
            Err(erro) => return Err(error_de_video(erro.to_string()).into()),
        };

    let (largura, altura) = janela.drawable_size();
    render.resize(largura, altura);
    // `--tamanho` é um painel **simulado**: no PC os drivers `offscreen` e
    // `dummy` obedecem, mas no KMSDRM o aparelho continua no modo dele. Aí o
    // projeto fica 640×480 num drawable 720×1280, a rotação vem de
    // `deduzir(pedido)` em vez do sistema, e o BMP sai com o conteúdo no canto.
    // Já custou uma rodada inteira de medição errada; avisa em vez de mentir.
    //
    // A comparação é com o **drawable de verdade**, não com o nome do driver:
    // assim vale para qualquer backend, sem lista de exceções.
    if o.tamanho.is_some() && (largura, altura) != tamanho_painel {
        eprintln!(
            "neon-drift: aviso --tamanho {}x{} mas o drawable saiu {largura}x{altura}: \
             o driver ignorou o pedido, a rotação veio de `deduzir` e a captura sai \
             inconsistente — no aparelho, capture sem --tamanho",
            tamanho_painel.0, tamanho_painel.1
        );
    }
    // O `Upscaler` da engine: desenha num alvo menor e estica com bilinear. Num
    // painel de 720×1280 são 921 mil pixels por quadro, e o Mali-G52 gasta
    // proporcionalmente ao **pixel**, não ao triângulo — é o botão que troca
    // nitidez por quadro em qualquer aparelho, com um número só.
    // A escala vive no `menu`: é ele que desenha `QUALIDADE [ 0.75 ]` na tela e
    // é ele que o direcional ajusta. Duas cópias do mesmo número divergiriam no
    // primeiro caminho que esquecesse de sincronizar.
    let mut menu = menu::Menu::com_escala(if o.escala < 0.999 {
        Some(o.escala.clamp(0.25, 1.0))
    } else {
        None
    });
    if let Some(escala) = menu.escala() {
        render.set_render_scale(Some(escala));
        let (rw, rh) = render.render_size();
        eprintln!(
            "neon-drift: escala {escala:.2} — desenhando em {rw}x{rh} e esticando para {largura}x{altura}"
        );
    }

    // O mapa define traçado, relevo, voltas, tempo limite e cenário. Default: o
    // primeiro da lista, para `--mapa` ausente não mudar o comportamento de
    // ninguém que já usava o binário.
    let mapa = match &o.mapa {
        Some(id) => match giochi_neon_drift_core::mapa::por_id(id) {
            Some(m) => m,
            None => {
                let nomes: Vec<&str> = giochi_neon_drift_core::mapa::TODOS
                    .iter()
                    .map(|m| m.id)
                    .collect();
                return Err(format!("--mapa '{id}' não existe (use: {})", nomes.join(", ")).into());
            }
        },
        None => giochi_neon_drift_core::mapa::PADRAO,
    };
    let dificuldade = match &o.dificuldade {
        Some(id) => match Dificuldade::parse(id) {
            Some(d) => d,
            None => {
                let nomes: Vec<&str> = Dificuldade::todos().iter().map(|d| d.id()).collect();
                return Err(
                    format!("--dificuldade '{id}' não existe (use: {})", nomes.join(", ")).into(),
                );
            }
        },
        None => Dificuldade::Normal,
    };
    let mut corrida = Corrida::com_mapa_e_dificuldade(mapa, dificuldade);
    eprintln!(
        "neon-drift: dificuldade {} ({}) — jogador {:.0}%, adversários {:.0}%",
        dificuldade.nome(),
        dificuldade.descricao(),
        dificuldade.fator_jogador() * 100.0,
        dificuldade.fator_ia() * 100.0
    );
    if let Some(em) = o.em {
        // `--em` implica corrida em andamento: posicionar na tela de título
        // mediria a pista errada.
        if corrida.fase == Fase::Titulo {
            corrida.comecar();
            corrida.contagem = 0.0;
        }
        let total = corrida.pista.comprimento();
        let s = em.rem_euclid(total.max(1.0));
        corrida.carro.posicionar(&corrida.pista, s, 0.0);
        corrida.carro.velocidade = 48.0;
        // A largada é dada por cruzada: começar em cima da pista sem marcar
        // `largou` deixaria a volta e o progresso negativos.
        corrida.marcar_largou();
        eprintln!("--em: carro em {s:.0} m de {total:.0} m, a 48 m/s");
    }
    if let Some(tela) = o.tela {
        match tela {
            Fase::Correndo => corrida.comecar(),
            Fase::Resultado => {
                // Resultado sem ninguém correndo: usa "tempo esgotado", que é o
                // desfecho honesto dessa situação.
                corrida.comecar();
                corrida.contagem = 0.0;
                corrida.tempo = corrida.tempo_limite;
                corrida.passo(&Entrada::PARADA, 1.0 / TIQUE);
                corrida.fase = Fase::Resultado;
            }
            Fase::Titulo => corrida.titulo(),
        }
    }

    let mut cena = Cena3D::nova(&mut render, &corrida.pista, rotacao, corrida.mapa.cenario)?;
    cena.so(o.so.clone());
    let mut indice_mapa = giochi_neon_drift_core::mapa::TODOS
        .iter()
        .position(|m| std::ptr::eq(*m, mapa))
        .unwrap_or(0);

    if !o.json {
        eprintln!(
            "neon-drift: painel {largura}x{altura}, rotação {} ({fonte_rotacao}), GLES {} em {}",
            rotacao.nome(),
            render.gles_version().major(),
            render.adapter()
        );
        eprintln!(
            "neon-drift: mapa \"{}\" ({}) — pista {:.0} m, desnível {:.0} m",
            mapa.nome,
            mapa.id,
            corrida.pista.comprimento(),
            mapa.desnivel()
        );
        eprintln!(
            "neon-drift: instancing {}, pista {:.0} m, {} voltas, limite {} fps",
            if cena.usa_instancing() {
                "ativo"
            } else {
                "indisponível"
            },
            corrida.pista.comprimento(),
            corrida.total_voltas,
            o.fps
        );
    }

    let mut botoes = evdev::Botoes::iniciar();
    if !o.json {
        eprintln!("neon-drift: {} dispositivo(s) de entrada", botoes.abertos());
    }
    let mut som = if o.sem_som {
        Som::sem_dispositivo()
    } else {
        Som::iniciar(&plataforma)
    };
    let mut rede = criar_rede(&o).map_err(std::io::Error::other)?;
    let mut gamepad = Gamepad::iniciar(&plataforma);
    let mut perfil = Perfil::novo();
    // `GIOCHI_PERFIL=1` liga o overlay já no boot: é o que permite capturar
    // um `--quadro` com o F1 ligado sem dedo no aparelho (e sem tecla no PC).
    if std::env::var_os("GIOCHI_PERFIL").is_some() {
        perfil.alternar();
    }
    perfil.definir_escala(menu.escala());
    perfil.definir_rede(rede.resumo());
    if gamepad.ativo() {
        eprintln!(
            "neon-drift: gamepad aberto: {}",
            gamepad.nome().unwrap_or("?")
        );
    }
    if !som.is_available() {
        eprintln!("neon-drift: áudio indisponível — drift e turbo ficam mudos");
    }

    // `--passos` antes do laço: é o que permite capturar um quadro com a corrida
    // em andamento (drift, nitro, HUD cheio) sem dedo no aparelho.
    if o.passos > 0 {
        let (entrada, acoes) = entrada_jogo::combinar(0, &o.teclas, corrida.fase);
        if acoes.iniciar {
            corrida.comecar();
        }
        // `--piloto` dirige no lugar das teclas fixas. Sem ele, `--passos 900`
        // com "acelerar" apertado só leva o carro de encontro ao primeiro muro —
        // medido: 2,6 m/s encostado na borda, e o quadro capturado mostrava a
        // linha de largada como se a corrida nem tivesse começado.
        for i in 0..o.passos {
            let entrada = if o.piloto {
                giochi_neon_drift_core::piloto::entrada(&corrida)
            } else {
                entrada
            };
            corrida.passo(&entrada, 1.0 / TIQUE);
            cena.seguir(&corrida, 1.0 / TIQUE);
            if corrida.fase == Fase::Resultado {
                break;
            }
            let _ = i;
        }
    }

    let mut teclas = o.teclas;
    let mut sair = false;
    // Ações do quadro anterior, para transformar nível em borda.
    let mut ultimas_acoes = entrada_jogo::Acoes::default();
    // O que as teclas e o controle pediram **neste** quadro. Aplicado depois de
    // acumular, porque aplicar um pedido de mapa reconstrói a cena inteira.
    let mut pedidos: Vec<menu::Pedido> = Vec::with_capacity(4);
    let mut avisou_erro_rede = false;
    let mut relogio = Relogio::com_ritmo(TIQUE, o.fps);

    // Prova de rotação: desenha três barras e sai. Roda antes de tudo porque não
    // depende de estado de partida nenhum — é o teste da **geometria** da tela.
    if let Some(caminho) = &o.prova_rotacao {
        cena.preparar(&mut render);
        render.begin_frame(&janela);
        cena.desenhar_prova_rotacao(&mut render)?;
        let (w, h) = gravar_quadro(&render, caminho)?;
        render.end_frame(&janela);
        cena.destruir(&mut render);
        if o.json {
            println!(
                "{{\"prova\":\"gravada\",\"largura\":{w},\"altura\":{h},\"rotacao\":\"{}\",\"fonte\":\"{}\"}}",
                rotacao.nome(),
                fonte_rotacao
            );
        }
        return Ok(0);
    }

    // Quadro único: captura e sai, sem laço de eventos. Com `--quadros N` a
    // captura é a do último quadro do laço (ver o fim do laço), e não esta.
    if let Some(caminho) = o.quadro.as_ref().filter(|_| o.quadros.is_none()) {
        // O overlay e o aviso também entram na captura: é o que permite
        // conferir o F1 e o rodapé de LAN num `--quadro` headless, sem tecla.
        if perfil.visivel() {
            perfil.inicio_quadro();
            perfil.fim_quadro(render.frame_counters());
        }
        cena.definir_perfil(perfil.linhas().to_vec());
        cena.definir_aviso_rede(Some(aviso_hud(&rede, &menu)));
        let (linhas_menu, dica) = montar_menu(&menu, &corrida, &rede);
        cena.definir_menu(linhas_menu, dica);
        cena.preparar(&mut render);
        render.begin_frame(&janela);
        cena.desenhar(&mut render, &corrida, 1.0 / TIQUE)?;
        let (w, h) = gravar_quadro(&render, caminho)?;
        render.end_frame(&janela);
        let erro = render.take_gl_error();
        if erro != 0 {
            return Err(format!("erro GLES 0x{erro:04x} ao desenhar o quadro").into());
        }
        if o.json {
            println!(
                "{{\"quadro\":\"gravado\",\"largura\":{w},\"altura\":{h},\"gles\":{},\"instancing\":{},\"escala\":{:.2},\"lan\":\"{:?}\",\"veu\":{:.2},\"batches\":{},\"desenhos\":{},\"fase\":\"{:?}\",\"efeitos\":{{{}}},{}}}",
                render.gles_version().major(),
                cena.usa_instancing(),
                menu::rotulo_escala(menu.escala()).parse::<f32>().unwrap_or(1.0),
                rede.papel(),
                cena.veu(),
                cena.batches(),
                render.frame_counters().draw_calls,
                corrida.fase,
                cena.efeitos_json(),
                diagnostico(&corrida, &cena, &render)
            );
        } else {
            eprintln!("neon-drift: quadro {w}x{h} gravado em {caminho}");
        }
        cena.destruir(&mut render);
        return Ok(0);
    }

    let inicio = Instant::now();
    let mut quadros = 0u32;
    let mut passos_totais = 0u32;
    let mut ultimo = Instant::now();
    // Soma do tempo de CPU por quadro, sem o swap (diagnóstico de `--json`).
    let mut ms_cpu = 0.0f64;

    while !sair {
        for evento in plataforma.poll_events() {
            match evento {
                Event::Quit { .. } => sair = true,
                Event::KeyDown {
                    keycode: Some(tecla),
                    repeat: false,
                    ..
                } => {
                    // O teclado continua sendo um caminho completo, com as
                    // mesmas ações do menu — mas ele **não** é mais o caminho
                    // principal: num handheld não existe teclado, e cada tecla
                    // daqui tem de ter um botão equivalente na tela.
                    //
                    // O pedido é **acumulado** e aplicado depois do laço de
                    // eventos: aplicar no meio exigiria emprestar a cena, o
                    // renderer e a corrida de dentro do `poll_events`, e a
                    // primeira tecla que reconstruísse a pista deixaria o resto
                    // do laço com um empréstimo aberto.
                    match tecla {
                        Keycode::F1 => {
                            let ligado = menu.alternar_perfil();
                            perfil.definir_visivel(ligado);
                        }
                        Keycode::F3 => {
                            let nova = menu::proxima_escala(menu.escala());
                            pedidos.push(menu::Pedido::Escala(nova));
                        }
                        Keycode::Minus if corrida.fase == Fase::Titulo => {
                            pedidos.push(menu.ajustar(-1));
                        }
                        Keycode::Equals if corrida.fase == Fase::Titulo => {
                            pedidos.push(menu.ajustar(1));
                        }
                        // H e J continuam: são o caminho curto de quem tem
                        // teclado, e o equivalente do controle é a linha LAN do
                        // menu (escolher com o direcional, executar com A).
                        Keycode::H if corrida.fase == Fase::Titulo && !rede.ativo() => {
                            pedidos.push(menu::Pedido::Hospedar);
                        }
                        Keycode::J if corrida.fase == Fase::Titulo && !rede.ativo() => {
                            pedidos.push(menu::Pedido::Entrar);
                        }
                        _ => aplicar_tecla(&mut teclas, tecla, true),
                    }
                }
                Event::KeyUp {
                    keycode: Some(tecla), ..
                } => {
                    aplicar_tecla(&mut teclas, tecla, false);
                }
                _ => {}
            }
        }
        if sair {
            break;
        }

        // Toque simulado: o ciclo aperta/solta a cada quadro. Só existe para o
        // diagnóstico headless — sem soltar, toda ação de borda acontece uma vez
        // só e "navegar o menu" não teria como ser provado por script.
        if let Some(toques) = &o.teclas_ciclo {
            let toque = (quadros / 2) as usize;
            teclas = match toques.get(toque) {
                Some(t) if quadros % 2 == 0 => *t,
                _ => Teclas::default(),
            };
        }
        let mut botoes_simulados = o.botoes;
        if let Some(toques) = &o.botoes_ciclo {
            let toque = (quadros / 2) as usize;
            botoes_simulados = match toques.get(toque) {
                Some(m) if quadros % 2 == 0 => *m,
                _ => 0,
            };
        }
        // A máscara é lida **uma** vez. Lendo duas (uma para decidir, outra
        // para qualquer uso posterior), uma borda de pressão pode cair entre as
        // duas leituras e o quadro age sobre um estado que já mudou.
        gamepad.atualizar();
        let bits_agora = botoes.bits() | gamepad.bits() | botoes_simulados;
        let (mut entrada, acoes) = entrada_jogo::combinar_com_analogico(
            bits_agora,
            gamepad.analogico(),
            &teclas,
            corrida.fase,
        );
        // `--piloto` também dirige **nos quadros**, não só nos `--passos`.
        //
        // Antes ele só era consultado no laço de `--passos`, então
        // `--quadros 1400 --piloto` mostrava o carro parado na grelha: o piloto
        // avançava a corrida até onde `--passos` mandasse e o laço de quadros
        // seguia com a entrada dos dispositivos, que num aparelho headless é
        // nenhuma. O sintoma era um `--json` com `largou: false` e `s` na
        // posição de grade depois de 1400 quadros — e a impressão de que a
        // simulação não estava rodando.
        if o.piloto && corrida.fase == Fase::Correndo {
            entrada = giochi_neon_drift_core::piloto::entrada(&corrida);
        }
        // Nível → borda. Segurar o direcional não pode andar seis linhas de menu
        // por quadro, e é a única regra que o laço precisa repetir por quadro.
        let bordas = entrada_jogo::bordas(ultimas_acoes, acoes);
        ultimas_acoes = acoes;
        // ── o menu, e as teclas que o atalham ───────────────────────
        //
        // Tudo o que muda mapa, dificuldade, escala, overlay ou LAN passa por
        // `Pedido`. É o que garante que o botão do controle e a tecla façam a
        // mesma coisa: existe **um** lugar que aplica, não um por dispositivo.
        if bordas.navegar != 0 {
            menu.navegar(bordas.navegar);
        }
        if bordas.ajustar != 0 {
            pedidos.push(menu.ajustar(bordas.ajustar));
        }
        if bordas.aceitar && corrida.fase == Fase::Titulo {
            pedidos.push(menu.aceitar(rede.ativo()));
        }
        if bordas.perfil {
            let ligado = menu.alternar_perfil();
            perfil.definir_visivel(ligado);
        }
        // START e Enter: jogar/reiniciar sem passar pelo cursor. É o caminho
        // rápido de quem só quer correr — e o único que funciona no resultado,
        // onde o menu não está na tela.
        if bordas.iniciar {
            match corrida.fase {
                Fase::Titulo => pedidos.push(menu::Pedido::Jogar),
                Fase::Resultado => {
                    corrida.reiniciar();
                    cena.limpar_efeitos();
                }
                Fase::Correndo => {}
            }
        }
        // O A no resultado também reinicia: é o botão que o dedo procura.
        if bordas.aceitar && corrida.fase == Fase::Resultado {
            corrida.reiniciar();
            cena.limpar_efeitos();
        }
        // Voltar: no título fecha o jogo; na corrida e no resultado devolve ao
        // menu. Antes o Y matava o jogo em qualquer tela — num handheld isso é
        // um dedo escorregado longe do botão de voltar.
        if bordas.voltar {
            match corrida.fase {
                Fase::Titulo => sair = true,
                Fase::Correndo | Fase::Resultado => {
                    // `reiniciar` **antes** de `titulo`: o título desenha o
                    // mundo atrás do menu, e voltar com o carro parado dentro de
                    // uma mureta a 400 m da largada parece defeito. `reiniciar`
                    // põe o carro na linha e `titulo` troca a tela.
                    corrida.reiniciar();
                    corrida.titulo();
                    cena.limpar_efeitos();
                    if !o.json {
                        eprintln!("neon-drift: de volta ao menu");
                    }
                }
            }
        }

        // Aplicados depois de acumular: um pedido de mapa reconstrói a cena e
        // não pode acontecer no meio do empréstimo de outro.
        for pedido in pedidos.drain(..) {
            aplicar_pedido(
                pedido,
                &mut menu,
                &mut perfil,
                &mut render,
                &mut corrida,
                &mut indice_mapa,
                &mut cena,
                rotacao,
                &o,
                &mut rede,
            )?;
        }
        if sair {
            break;
        }

        // A rede é drenada uma vez por quadro, antes dos passos. No host isso
        // atualiza o input de cada cliente; no cliente aplica largada/volta/
        // resultado que chegaram pelo canal confiável.
        rede.processar(&mut corrida);

        let agora = inicio.elapsed().as_secs_f64();
        let planejado = relogio.avancar(agora);
        for _ in 0..planejado.ticks {
            passos_totais += 1;
            // Cliente LAN não simula o próprio carro: a posição vem do host.
            // A física que sobra é só a do host, dentro do mesmo passo fixo.
            if !rede.simula_localmente() {
                continue;
            }
            let evento = corrida.passo(&entrada, 1.0 / TIQUE);
            // Só a batida precisa avisar a cena; o resto ela lê do estado.
            if evento == Evento::Bateu {
                cena.bater();
            }
            rede.passo_host(&mut corrida, 1.0 / TIQUE);
        }

        let dt = ultimo.elapsed().as_secs_f32().min(0.1);
        ultimo = Instant::now();
        som.atualizar(&estado_som(&corrida), dt);

        match rede.papel() {
            Papel::Cliente => {
                let carros = rede.aplicar_cliente(&mut corrida, &entrada, dt);
                cena.definir_carros_rede(carros, false);
            }
            Papel::Host => {
                let carros = rede.publicar_host(&mut corrida);
                cena.definir_carros_rede(carros, true);
            }
            Papel::Off => cena.definir_carros_rede(Vec::new(), true),
        }
        if let Some(erro) = rede.erro() {
            if !avisou_erro_rede {
                eprintln!("neon-drift: rede — {erro}");
                avisou_erro_rede = true;
            }
        }
        perfil.definir_rede(rede.resumo());
        let linhas_perfil = if perfil.visivel() {
            perfil.linhas().to_vec()
        } else {
            Vec::new()
        };
        cena.definir_perfil(linhas_perfil);
        cena.definir_aviso_rede(Some(aviso_hud(&rede, &menu)));
        let (linhas_menu, dica) = montar_menu(&menu, &corrida, &rede);
        cena.definir_menu(linhas_menu, dica);

        perfil.inicio_quadro();
        cena.preparar(&mut render);
        let t_cpu = Instant::now();
        render.begin_frame(&janela);
        cena.desenhar(&mut render, &corrida, dt)?;
        // Antes do `end_frame`: ele faz o swap e **bloqueia no vblank**, então
        // incluí-lo transformaria toda medida em "tempo de vsync" e não diria
        // nada sobre o custo do quadro.
        ms_cpu += t_cpu.elapsed().as_secs_f64() * 1000.0;
        perfil.fim_quadro(render.frame_counters());
        // `--quadro` sozinho é um quadro só (ver acima); junto com `--quadros N`
        // ele captura o **último** dos N, que é o que permite provar por imagem
        // o que um cliente LAN desenhou depois de a corrida começar.
        if let Some(caminho) = &o.quadro {
            if o.quadros.is_some_and(|limite| quadros + 1 >= limite) {
                let (w, h) = gravar_quadro(&render, caminho)?;
                if !o.json {
                    eprintln!("neon-drift: quadro {w}x{h} gravado em {caminho}");
                }
            }
        }
        render.end_frame(&janela);
        quadros += 1;

        if let Some(limite) = o.quadros {
            if quadros >= limite {
                break;
            }
        }

        std::thread::sleep(planejado.dormir);
    }

    let decorrido = inicio.elapsed().as_secs_f64();
    let fps_medio = if decorrido > 0.0 {
        (quadros.saturating_sub(1)) as f64 / decorrido
    } else {
        0.0
    };
    // Destruição explícita **antes** do relatório: os contadores de `recursos`
    // têm de refletir o estado final. Liberar depois daria sempre zero mesmo
    // com recurso vazado.
    cena.destruir(&mut render);

    if o.json {
        // Stdout é **só dado** neste ramo: as linhas `neon-drift:` (texturas,
        // painel, mapa, atlas) vão para stderr. Antes elas dividiam o stdout com
        // o JSON e `--json | jq` falhava com "Expecting value: line 1 column 1" —
        // o consumidor lia a prosa do diagnóstico como se fosse o objeto.
        println!(
            // `posicao`, `volta` e `drift` **não** aparecem aqui: `diagnostico`
            // já os emite, e a versão anterior do JSON trazia os dois — chave
            // duplicada, com o último valor vencendo em silêncio. Duas fontes do
            // mesmo número envelhecem diferente; esta é a que não tem a corrida
            // em mãos.
            "{{\"quadros\":{quadros},\"passos\":{passos_totais},\"segundos\":{decorrido:.3},\"fps\":{fps_medio:.1},\"fps_teto\":{},\"ms_cpu\":{:.2},\"particulas\":{},\"desenhos\":{},\"efeitos\":{{{}}},\"gles\":{},\"instancing\":{},\"escala\":{:.2},\"lan\":\"{:?}\",\"menu\":\"{}\",\"menu_linha\":{},\"perfil\":{},\"lan_acao\":\"{}\",\"veu\":{:.2},\"batches\":{},\"recursos\":{{\"malhas\":{},\"texturas\":{},\"shaders\":{}}},\"fase\":\"{:?}\",{}}}",
            o.fps,
            if quadros > 0 { ms_cpu / quadros as f64 } else { 0.0 },
            cena.particulas_vivas(),
            render.frame_counters().draw_calls,
            cena.efeitos_json(),
            render.gles_version().major(),
            cena.usa_instancing(),
            menu::rotulo_escala(menu.escala()).parse::<f32>().unwrap_or(1.0),
            rede.papel(),
            // O menu no `--json` é o que permite provar, de um script, que o
            // controle **navegou**: sem estes campos o estado do menu só existe
            // na tela, e "o direcional funciona" volta a ser fé.
            menu.linha().nome(),
            menu.indice(),
            menu.perfil(),
            menu.lan().rotulo(),
            cena.veu(),
            cena.batches(),
            // Contagem **depois** do `destruir`: tem de ser (0, 0, 0). Um
            // número diferente de zero é vazamento de GPU, e é este o número
            // que o smoke test confere.
            render.mesh_count(),
            render.texture_count(),
            render.shader_count(),
            corrida.fase,
            diagnostico(&corrida, &cena, &render)
        );
    } else {
        eprintln!(
            "neon-drift: fim — {} quadros em {:.2} s ({:.1} fps; teto {}) · drift {} · {}",
            quadros,
            decorrido,
            fps_medio,
            o.fps,
            hud::pontos(corrida.drift.total()),
            corrida.desfecho.map(|d| d.titulo()).unwrap_or("em andamento")
        );
    }
    som.encerrar();
    Ok(0)
}

/// Liga ou desliga uma tecla do estado.
fn aplicar_tecla(teclas: &mut Teclas, tecla: Keycode, apertou: bool) {
    let alvo = match tecla {
        Keycode::Up | Keycode::W => &mut teclas.cima,
        Keycode::Down | Keycode::S => &mut teclas.baixo,
        Keycode::Left | Keycode::A => &mut teclas.esquerda,
        Keycode::Right | Keycode::D => &mut teclas.direita,
        Keycode::Space => &mut teclas.espaco,
        Keycode::LShift | Keycode::RShift => &mut teclas.shift,
        Keycode::Return | Keycode::KpEnter => &mut teclas.enter,
        Keycode::Escape | Keycode::Q => &mut teclas.escape,
        _ => return,
    };
    *alvo = apertou;
}

/// Mensagem de erro de vídeo com dica do que fazer.
fn error_de_video(mensagem: String) -> std::io::Error {
    let dica = if mensagem.contains("kmsdrm") {
        "  outro processo está com o DRM (o launcher?). Feche-o antes.\n"
    } else {
        ""
    };
    std::io::Error::other(format!("{mensagem}\n{dica}"))
}

fn main() {
    match principal() {
        Ok(codigo) => std::process::exit(codigo),
        Err(erro) => {
            eprintln!("erro: {erro}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn teclas_da_cli_viram_estado() {
        let t = teclas("a,right,shift").unwrap();
        assert!(t.cima);
        assert!(t.direita);
        assert!(t.shift);
        assert!(!t.esquerda);
    }

    #[test]
    fn tecla_desconhecida_e_erro_com_nome() {
        let erro = teclas("a,voar").unwrap_err();
        assert!(erro.contains("voar"), "{erro}");
    }

    #[test]
    fn o_teto_de_fps_recusa_acima_de_75() {
        let erro = opcoes(vec!["--fps-teto".into(), "76".into()]).unwrap_err();
        assert!(erro.contains("1 e 75"), "{erro}");
    }

    #[test]
    fn o_teto_padrao_e_75() {
        assert_eq!(opcoes(vec![]).unwrap().fps, 75);
        assert!(opcoes(vec!["--fps-teto".into(), "0".into()]).is_err());
    }

    #[test]
    fn tamanho_invalido_e_erro() {
        assert!(opcoes(vec!["--tamanho".into(), "640".into()]).is_err());
        assert!(opcoes(vec!["--tamanho".into(), "0x480".into()]).is_err());
        let o = opcoes(vec!["--tamanho".into(), "800x600".into()]).unwrap();
        assert_eq!(o.tamanho, Some((800, 600)));
    }

    #[test]
    fn tela_desconhecida_e_erro() {
        // Toda flag que consome um valor tem de avançar o índice. Faltar o
        // `i += 2` num braço não dá erro de compilação nem mensagem: o laço relê
        // a mesma flag para sempre, e o sintoma é um processo que não imprime
        // nada e não termina. Aconteceu com `--prova-rotacao`.
        for flag in [
            "--quadro",
            "--texturas",
            "--atlas",
            "--prova-rotacao",
            "--tamanho",
            "--mapa",
            "--config",
            "--classe",
            "--perfil",
            "--em",
            "--teclas",
            "--tela",
            "--fps-teto",
            "--escala",
        ] {
            let valor = if flag == "--tamanho" {
                "640x480"
            } else if flag == "--fps-teto" {
                "60"
            } else if flag == "--escala" {
                "0.5"
            } else if flag == "--em" {
                "100"
            } else if flag == "--tela" {
                "titulo"
            } else if flag == "--mapa" {
                "avenida"
            } else if flag == "--classe" {
                "soldado"
            } else if flag == "--perfil" {
                "balanceado"
            } else {
                "x"
            };
            // Se o braço não avançar, esta chamada nunca retorna — por isso ela
            // roda com um valor que o braço precisa consumir.
            let r = opcoes(vec![flag.into(), valor.into()]);
            assert!(r.is_ok() || r.is_err(), "{flag} não terminou");
        }
        assert!(opcoes(vec!["--tela".into(), "inexistente".into()]).is_err());
        assert_eq!(
            opcoes(vec!["--tela".into(), "resultado".into()]).unwrap().tela,
            Some(Fase::Resultado)
        );
    }

    #[test]
    fn argumento_desconhecido_nao_e_ignorado() {
        assert!(opcoes(vec!["--turbo".into()]).unwrap_err().contains("--turbo"));
    }

    #[test]
    fn quadros_zero_e_recusado() {
        assert!(opcoes(vec!["--quadros".into(), "0".into()]).is_err());
    }

    #[test]
    fn as_teclas_do_jogo_mapeiam_para_os_campos_certos() {
        let mut t = Teclas::default();
        aplicar_tecla(&mut t, Keycode::Up, true);
        aplicar_tecla(&mut t, Keycode::Space, true);
        aplicar_tecla(&mut t, Keycode::LShift, true);
        assert!(t.cima && t.espaco && t.shift);
        aplicar_tecla(&mut t, Keycode::Up, false);
        assert!(!t.cima, "soltar tem de desligar");
    }

    #[test]
    fn o_endereco_lan_aceita_ip_e_ip_com_porta() {
        let so_ip = endereco_lan("192.168.0.9").expect("ip");
        assert_eq!(so_ip.port(), giochi_net::PORTA_PADRAO);
        let com_porta = endereco_lan("192.168.0.9:47900").expect("ip:porta");
        assert_eq!(com_porta.port(), 47900);
        assert!(endereco_lan("nao e um endereco !!!").is_err());
    }

    #[test]
    fn o_alias_frames_faz_o_mesmo_que_quadros() {
        // Smoke test headless usa `--frames N`; o nome antigo continua.
        let a = opcoes(vec!["--frames".into(), "5".into()]).expect("--frames");
        let b = opcoes(vec!["--quadros".into(), "5".into()]).expect("--quadros");
        assert_eq!(a.quadros, Some(5));
        assert_eq!(a.quadros, b.quadros);
    }

    #[test]
    fn as_flags_de_lan_entram_no_estado() {
        let o = opcoes(vec![
            "--lan-host".into(),
            "--lan-porta".into(),
            "47901".into(),
        ])
        .expect("lan");
        assert!(o.lan_host);
        assert_eq!(o.lan_porta, Some(47901));
        let c = opcoes(vec!["--lan-entrar".into(), "10.0.0.2".into()]).expect("entrar");
        assert_eq!(c.lan_entrar.as_deref(), Some("10.0.0.2"));
        assert!(opcoes(vec!["--lan-porta".into(), "0".into()]).is_err());
    }

    #[test]
    fn as_teclas_wasd_fazem_o_mesmo_que_as_setas() {
        let mut com_seta = Teclas::default();
        aplicar_tecla(&mut com_seta, Keycode::Left, true);
        let mut com_wasd = Teclas::default();
        aplicar_tecla(&mut com_wasd, Keycode::A, true);
        assert_eq!(com_seta, com_wasd);
    }
}
