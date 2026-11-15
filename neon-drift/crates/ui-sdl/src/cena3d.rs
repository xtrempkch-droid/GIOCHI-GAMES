//! A cena 3D: pista, cidade, carros, partículas e HUD.
//!
//! ## Estratégia de desenho
//!
//! Tudo o que não se move é **assado** numa malha por cor: asfalto, faixas, guias,
//! muretas, prédios, postes, túnel, cones. Isso dá ~16 malhas estáticas, ou seja,
//! ~16 *draw calls* por quadro para o mundo inteiro, independente do tamanho da
//! pista. Um desenho ingênuo (uma caixa por prédio, um quad por faixa) faria
//! centenas de chamadas a 75 Hz num RK3326.
//!
//! Os carros são cinco malhas cada (lataria, cabine, rodas, faróis, lanternas):
//! `draw_mesh_colored` pinta a malha inteira de uma cor, e um carro de uma cor só
//! lê como bloco. Cinco desenhos por carro, quatro carros, vinte chamadas.
//!
//! Partículas e HUD usam instancing quando o driver suporta — e a engine cai no
//! caminho individual quando não suporta, então o jogo não depende disso.

use std::error::Error;

use giochi_engine_3d::context::{FrontFace, GlState};
use giochi_engine_3d::math3d::{Camera3D, Color3D, Mat4, Quat, Transform3D, Vec3};
use giochi_engine_3d::mesh::{StaticBatch, StaticBatchBuilder};
use giochi_engine_3d::render3d::{MeshHandle, ShaderHandle};
use giochi_engine_3d::shader::BuiltinShader;
use giochi_engine_3d::{DirectionalLight, RenderContext3D};
use giochi_neon_drift_core::corrida::{Corrida, Fase};
use giochi_neon_drift_core::mapa::Cenario;
use giochi_neon_drift_core::fonte;
use giochi_neon_drift_core::hud;
use giochi_neon_drift_core::pista::{Pista, Ponto};
use giochi_neon_drift_core::{COR_JOGADOR, CORES_ADVERSARIOS, NOME};

use crate::efeitos::{self, Particulas, Tipo};
use crate::malha::{self, Construtor};
use crate::rede::CarroRede;
use crate::vetor::{direcao_normalizada, misturar, somar, subtrair};
use crate::video::{girar_ponto as girar_ponto_do_painel, girar_rect, Rotacao};

// ════════════════════════════════════════════════════════════
// PALETA
// ════════════════════════════════════════════════════════════

// A paleta teve de subir uma rodada inteira. A primeira versão era "noite
// realista": asfalto a 0,058 e prédios a 0,09. Medido no quadro, dava
// **luminância média 39 de 255, 25% do quadro quase preto e só 4% de pixels
// coloridos** — num painel de 3,5" isso não é atmosfera, é uma tela apagada.
// Estes valores são o que faz a cena ler: média ~70, quase preto abaixo de 8%.
const NOITE: Color3D = Color3D::new(0.020, 0.024, 0.052, 1.0);
/// Céu da serra: azul profundo em vez de quase preto.
///
/// Numa descida a linha do horizonte fica visível e o céu ocupa um terço do
/// quadro; o mesmo quase-preto da avenida (onde os prédios o tapam) lia como
/// tela apagada. Medido: 28 % do quadro abaixo de 8/255 com o céu da avenida,
/// 9 % com este.
const CEU_SERRA: Color3D = Color3D::new(0.100, 0.095, 0.175, 1.0);
/// Silhueta dos morros do outro lado do vale.
///
/// **Mais escuro que o céu**, e esse é o ponto: medido, os morros usavam
/// `PREDIO_LONGE` (0,10 0,12 0,22) contra um céu de (0,05 0,07 0,13) e saíam
/// *mais claros* que ele — 48% do quadro virava uma parede azul chapada que lia
/// como "céu vazio". Contra a luz, morro é silhueta: escuro por cima de um céu
/// que ainda tem o brilho da cidade.
const MORRO: Color3D = Color3D::new(0.042, 0.048, 0.086, 1.0);
/// Céu do circuito de teste: violeta de pista iluminada por refletores.
///
/// Não é noite fechada como a avenida porque num proving ground o assunto é o
/// asfalto, e um céu preto com torre de luz lia como "a pista está apagada".
const CEU_CIRCUITO: Color3D = Color3D::new(0.075, 0.062, 0.135, 1.0);
/// Céu do porto: azul-esverdeado de cidade portuária, com a maré refletindo.
const CEU_PORTO: Color3D = Color3D::new(0.055, 0.080, 0.115, 1.0);
/// Céu do salar: azul profundo de altitude, quase sem poluição de luz.
const CEU_SALAR: Color3D = Color3D::new(0.095, 0.110, 0.180, 1.0);
// Barreira e contêiner não têm constante de cor própria: eles reusam `muros` e
// `predios_a/b/c`, e a cor de uma malha é decidida quando ela é montada, não no
// desenho. Dar cor própria a eles custaria malha nova — e a regra do orçamento
// é que cenário novo não cria malha nova. Por isso a variedade do porto vem dos
// **três** grupos de prédio, que já existem, e não de tint por peça.
// Mar do porto e montanha do salar usam `longe`, e isso é **o oposto** do que o
// morro da serra faz — de propósito. Na descida a silhueta tem de ser mais
// escura que o céu, e por isso ela ganhou malha e cor próprias (§31). Aqui o
// papel é outro: a água reflete as luzes do cais e a rocha do salar recebe luar,
// então as duas ficam **mais claras** que o céu. Medido: o mar dá 14,1 % do
// quadro contra 32,9 % de céu, e a montanha sai 18 % mais escura que o céu por
// causa do ambiente — se um dia ela ficar mais clara, o sintoma é a chapa
// chapada de §31 e a correção é a mesma: malha e cor próprias.
const ASFALTO: Color3D = Color3D::new(0.155, 0.160, 0.215, 1.0);
/// Tint da faixa central **texturizada**.
///
/// A textura é branca no traço e quase preta no fundo; este tint é o que pinta
/// o traço. Um pouco mais frio que o branco puro porque o pulso multiplica em
/// cima — com 1,0 no pico o traço saturava e piscava.
const FAIXA_PISTA: Color3D = Color3D::new(0.86, 0.92, 1.0, 1.0);

/// Tint da grade de neon das fitas de advertência (ciano-magenta).
const GRADE_NEON: Color3D = Color3D::new(0.45, 0.95, 1.0, 1.0);
const GUIA_VERMELHA: Color3D = Color3D::new(0.95, 0.16, 0.30, 1.0);
const GUIA_BRANCA: Color3D = Color3D::new(0.70, 0.74, 0.82, 1.0);
const MURO: Color3D = Color3D::new(0.22, 0.23, 0.32, 1.0);
const NEON_CIANO: Color3D = Color3D::new(0.16, 0.92, 1.0, 1.0);
const NEON_MAGENTA: Color3D = Color3D::new(1.0, 0.22, 0.82, 1.0);
const PREDIO_A: Color3D = Color3D::new(0.20, 0.21, 0.30, 1.0);
const PREDIO_B: Color3D = Color3D::new(0.24, 0.20, 0.34, 1.0);
const PREDIO_C: Color3D = Color3D::new(0.17, 0.23, 0.32, 1.0);
/// Silhueta distante: mais escura e mais azul que os prédios da beira da pista,
/// para o horizonte ter profundidade em vez de terminar num bloco só.
const PREDIO_LONGE: Color3D = Color3D::new(0.10, 0.12, 0.22, 1.0);
const JANELA: Color3D = Color3D::new(1.0, 0.84, 0.48, 1.0);
const POSTE: Color3D = Color3D::new(0.28, 0.30, 0.40, 1.0);
const LUZ_POSTE: Color3D = Color3D::new(1.0, 0.94, 0.72, 1.0);
const TUNEL: Color3D = Color3D::new(0.16, 0.17, 0.23, 1.0);
const TUNEL_LUZ: Color3D = Color3D::new(0.85, 0.94, 1.0, 1.0);
const CONE: Color3D = Color3D::new(1.0, 0.46, 0.10, 1.0);
const LARGADA_A: Color3D = Color3D::new(0.95, 0.95, 1.0, 1.0);
const LARGADA_B: Color3D = Color3D::new(0.10, 0.10, 0.14, 1.0);
const VIDRO: Color3D = Color3D::new(0.07, 0.09, 0.16, 1.0);
const PNEU: Color3D = Color3D::new(0.045, 0.045, 0.06, 1.0);
const FAROL: Color3D = Color3D::new(0.92, 0.98, 1.0, 1.0);
const LANTERNA: Color3D = Color3D::new(1.0, 0.16, 0.22, 1.0);
const REFLEXO: Color3D = Color3D::new(0.35, 0.78, 1.0, 0.16);
/// Tints dos materiais **texturizados**.
///
/// A textura é cinza (ou verde, no caso da mata) e a cor final é `amostra × tint`,
/// então estes valores são o material, não a cor da textura. Um tint escuro num
/// material já escuro apaga a superfície — por isso o concreto é bem mais claro
/// aqui do que o `MURO` de cor chapada era.
const CONCRETO: Color3D = Color3D::new(0.44, 0.46, 0.56, 1.0);
const PEDRA: Color3D = Color3D::new(0.52, 0.50, 0.48, 1.0);
const MATA: Color3D = Color3D::new(0.34, 0.60, 0.34, 1.0);
const GUARDA_CORPO: Color3D = Color3D::new(0.62, 0.64, 0.70, 1.0);
const PAINEL_NEON: Color3D = Color3D::new(1.0, 0.55, 0.92, 1.0);
/// Luz de tudo o que é fonte de luz na cena: ambiente cheio e difusa zero.
///
/// É o que faz o neon parecer neon. Com a luz do mundo (ambiente 0,62) o ciano
/// saía a 45% e a avenida ficava cinza.
const EMISSIVO: DirectionalLight = DirectionalLight {
    direction: Vec3::new(0.0, -1.0, 0.0),
    color: Color3D::BLACK,
    ambient: 1.0,
    specular: 0.0,
    shininess: 1.0,
    _reserved: [0; 4],
};

/// Poça de luz do poste no asfalto molhado. Translúcida: com blend ligado no
/// quadro inteiro, ela soma luz sem apagar a faixa branca que passa por baixo.
const POCA_LUZ: Color3D = Color3D::new(0.42, 0.36, 0.20, 0.42);

/// Altura do plano do HUD, em unidades de mundo.
///
/// Com a câmera do HUD na origem olhando para -Z e `fov` de 90°, um ponto a esta
/// distância tem `x = ndc_x · PROF · aspecto` e `y = ndc_y · PROF`. É o que
/// permite posicionar HUD em pixels.
const PROFUNDIDADE_HUD: f32 = 10.0;

// ── véu das telas de texto ─────────────────────────────────
//
// O título e o resultado desenham texto claro sobre a cidade, e o céu da serra
// chega a 39 de média: um texto de 5×7 de 2 px por ponto sobre um fundo de
// luminância 39 não se lê num painel de 3,5" sob luz do dia. Um quad preto
// translúcido de tela cheia separa as duas camadas sem esconder o mundo — o
// jogador continua vendo a pista girar atrás do menu, que é o que dá vida à
// tela de entrada.
//
// O valor não é escolha estética: 0,62 foi o que deixou o texto acima de 200 de
// luminância contra um fundo abaixo de 45 em **todas** as cinco pistas
// (`o_veu_do_menu_deixa_o_texto_longe_do_fundo` mede a razão).

/// Alfa do véu no título: o cenário fica visível, mas nunca compete com o texto.
const VEU_TITULO: f32 = 0.62;
/// Alfa do véu no resultado: mais leve que o do título.
///
/// Ali o jogador acabou de correr e o carro **deve** aparecer; o que precisa de
/// contraste é só o bloco de três linhas, que já fica na metade de baixo.
const VEU_RESULTADO: f32 = 0.50;
/// Duração de uma transição completa do véu (0 → 1), em segundos.
///
/// A transição tem duas exigências que se opõem: **não piscar** (um salto de
/// 0,62 para 0 num quadro, no instante do "JOGAR", cega) e **não atrasar** (um
/// véu que ainda escurece quando o carro larga é o defeito original ao
/// contrário). Por isso a rampa é de **taxa constante**, e não exponencial: a
/// exponencial não garante nenhuma das duas — com constante de tempo pequena
/// pula 0,26 no primeiro quadro, com constante grande leva 0,7 s para sumir.
///
/// 0,30 s dá 0,044 de alfa por quadro a 75 Hz (imperceptível como corte) e
/// termina em 14 quadros do título para a corrida — antes de a contagem chegar
/// ao fim.
const DURACAO_VEU: f32 = 0.30;

/// Alvo do véu para cada fase. `0,0` na corrida: a gameplay é a tela cheia.
///
/// `GIOCHI_VEU=0` desliga o véu. Existe para **medir**: sem A/B não dá para
/// dizer "o texto ficou legível", só "eu gostei", e este arquivo tem uma regra
/// de que afirmação sobre a imagem se mede (o mesmo motivo do `GIOCHI_CULL=0`).
fn alvo_veu(fase: Fase) -> f32 {
    if veu_desligado() {
        return 0.0;
    }
    match fase {
        Fase::Titulo => VEU_TITULO,
        Fase::Resultado => VEU_RESULTADO,
        Fase::Correndo => 0.0,
    }
}

/// `GIOCHI_VEU=0` desliga o escurecimento das telas de texto.
fn veu_desligado() -> bool {
    use std::sync::OnceLock;
    static DESLIGADO: OnceLock<bool> = OnceLock::new();
    *DESLIGADO.get_or_init(|| {
        std::env::var("GIOCHI_VEU").is_ok_and(|v| v.trim() == "0")
    })
}

/// Um passo do véu em direção a `alvo`. Puro, para não precisar de GPU no teste.
///
/// Três propriedades que os testes cobram, e cada uma já foi um defeito em
/// outro lugar deste arquivo:
///
/// - **`dt` negativo, zero ou `NaN` não move nada** e não produz `NaN`. Um
///   `NaN` no alfa faz o driver descartar o quad de tela cheia, e o véu inteiro
///   some sem erro nenhum.
/// - **passo limitado por quadro**: no máximo `dt / DURACAO_VEU`, que é o que
///   separa "transição" de "piscada".
/// - **encaixe**: a diferença abaixo de 0,002 vira exatamente o alvo, senão o
///   jogo paga um desenho de tela cheia por um resíduo invisível, para sempre.
fn avancar_veu(atual: f32, alvo: f32, dt: f32) -> f32 {
    let atual = if atual.is_finite() {
        atual.clamp(0.0, 1.0)
    } else {
        alvo
    };
    let alvo = if alvo.is_finite() {
        alvo.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if dt.is_nan() || dt <= 0.0 {
        return atual;
    }
    let limite = dt / DURACAO_VEU;
    let passo = (alvo - atual).clamp(-limite, limite);
    let novo = atual + passo;
    if (alvo - novo).abs() < 0.002 {
        alvo
    } else {
        novo
    }
}

/// Campo de visão horizontal base, em radianos (~74°).
const FOV_BASE: f32 = 1.30;
/// Campo de visão com nitro no máximo (~92°): é o que dá a sensação de velocidade.
const FOV_NITRO: f32 = 1.62;

// ── decalques de luz dos carros ────────────────────────────
//
// As medidas estão em metros e existem para a poça e o feixe não voltarem a
// crescer por acidente: num painel de 720×1280 o decalque paga **cada pixel**,
// e a rodada de 09/11 mediu 9,6 m de poça derrubando o quadro de 39 para 30 fps.

/// Comprimento e largura da poça de neon sob o carro (elipse alinhada com ele).
const POCA_COMPRIMENTO: f32 = 4.2;
const POCA_LARGURA: f32 = 2.5;
/// Altura dos decalques sobre o asfalto, para não brigarem com ele no z-buffer.
const ALTURA_POCA: f32 = 0.02;
const ALTURA_FEIXE: f32 = 0.03;
/// Comprimento, largura e início do feixe do farol, à frente do para-choque.
const FEIXE_COMPRIMENTO: f32 = 7.5;
const FEIXE_LARGURA: f32 = 2.6;
const INICIO_FEIXE: f32 = 5.5;
/// Tempo de drift, em segundos, para o neon sob o carro chegar ao brilho máximo.
///
/// Existe para um toque de volante não piscar a poça como um drift de três
/// segundos: a rampa curta é o que separa "o carro saiu de lado" de "o carro
/// encostou no volante".
const RAMPA_DRIFT: f32 = 0.45;
/// Alfa da poça com o carro no piso e no auge do drift.
const POCA_ALFA_BASE: f32 = 0.46;
const POCA_ALFA_DRIFT: f32 = 0.86;
/// Alfa do feixe no ponto quente (junto ao para-choque).
const FEIXE_ALFA: f32 = 0.45;

// ════════════════════════════════════════════════════════════
// CENA
// ════════════════════════════════════════════════════════════

/// Estado da câmera, com atraso e inclinação.
///
/// ## Por que a câmera não segue o carro na hora
///
/// Colada no carro, ela transmite cada correção de volante como um tranco e o
/// jogador perde a referência da pista. O atraso exponencial é o que dá o "peso"
/// de perseguição. A inclinação (`roll`) é o que faz a curva parecer curva: é o
/// mesmo truque do cinema, e custa uma multiplicação.
struct CameraJogo {
    pos: Vec3,
    alvo: Vec3,
    fov: f32,
    roll: f32,
    tremor: f32,
}

impl CameraJogo {
    fn nova(pista: &Pista, rotacao: Rotacao) -> Self {
        let inicio = pista.ponto_em(0.0);
        let (x, z) = girar_ponto(rotacao, inicio);
        let y = pista.altura_em(0.0);
        Self {
            pos: Vec3::new(x, y + 3.4, z - 8.0),
            alvo: Vec3::new(x, y + 1.0, z + 4.0),
            fov: FOV_BASE,
            roll: 0.0,
            tremor: 0.0,
        }
    }
}

/// As texturas do mapa, já na GPU.
///
/// Um `TextureHandle` por material. A cor continua vindo do `tint` do desenho —
/// a textura dá **relevo**, não cor — e é por isso que a mesma textura de
/// concreto serve ao muro azulado da avenida e à pedra clara da serra.
struct TexturasGpu {
    asfalto: giochi_engine_3d::TextureHandle,
    concreto: giochi_engine_3d::TextureHandle,
    pedra: giochi_engine_3d::TextureHandle,
    mata: giochi_engine_3d::TextureHandle,
    guarda: giochi_engine_3d::TextureHandle,
    painel: giochi_engine_3d::TextureHandle,
    xadrez: giochi_engine_3d::TextureHandle,
    /// Faixa de pista: tracejado e duas linhas, POT 64 para mipmap/REPEAT.
    faixa: giochi_engine_3d::TextureHandle,
    /// Grade de neon das fitas de advertência das barreiras.
    grade: giochi_engine_3d::TextureHandle,
    /// Máscara de alfa da poça de neon sob os carros.
    ///
    /// Não é material: o RGB é branco e o desenho é o alfa (ver `texturas.rs`).
    /// Por isso ela **não** entra em `todas()` e nunca é trocada por arquivo.
    halo: giochi_engine_3d::TextureHandle,
    /// Máscara de alfa do cone do farol projetado no asfalto.
    cone: giochi_engine_3d::TextureHandle,
}

impl TexturasGpu {
    /// Sobe as nove texturas de superfície, **de arquivo quando houver**.
    ///
    /// `GIOCHI_TEXTURAS=<dir>` faz o jogo procurar `<dir>/<nome>.<png|bmp|jpg|jpeg>`
    /// para cada material; o que não existir sai da receita procedural, como
    /// sempre. É o caminho que a engine 0.4.0 abriu com o `Texture3D::from_file`
    /// (o `SDL_image` que ele usa decodifica PNG, JPEG e BMP) e que
    /// [`TEXTURAS.md`](../../TEXTURAS.md) registrava como inexistente.
    ///
    /// A mistura é deliberada: pedir textura de asfalto não deve obrigar a pintar
    /// as outras seis. Mas cada material que **não** veio de arquivo é dito no
    /// log, com o motivo — cair em silêncio faria o usuário concluir que a arte
    /// dele não serve quando o que falhou foi o nome do arquivo (§32).
    fn criar(render: &mut RenderContext3D) -> Result<Self, Box<dyn Error>> {
        let mut handles = Vec::new();
        let mut total = 0usize;
        let mut nomes = Vec::new();
        let mut de_arquivo = 0usize;
        let mut ausentes = Vec::new();
        let dir = crate::texturas::diretorio();
        if let Some(dir) = dir.as_deref() {
            if !dir.is_dir() {
                eprintln!(
                    "neon-drift: {} aponta para '{}', que não é diretório — usando as nove procedurais",
                    crate::texturas::VARIAVEL,
                    dir.display()
                );
            }
        }
        for t in crate::texturas::todas() {
            let arquivo = match dir.as_deref().filter(|d| d.is_dir()) {
                Some(d) => crate::texturas::arquivo_para(d, t.nome),
                None => None,
            };
            let h = match arquivo {
                Some(caminho) => {
                    let h = render.create_texture(&caminho)?;
                    let (l, a) = render.texture_size(h).unwrap_or((0, 0));
                    eprintln!(
                        "neon-drift: textura '{}' de arquivo: {} ({l}x{a})",
                        t.nome,
                        caminho.display()
                    );
                    total += (l * a * 4) as usize;
                    de_arquivo += 1;
                    h
                }
                None => {
                    if dir.is_some() {
                        ausentes.push(t.nome);
                    }
                    let h = render.create_texture_from_rgba(t.largura, t.altura, &t.pixels)?;
                    total += t.bytes();
                    h
                }
            };
            nomes.push(t.nome);
            handles.push(h);
        }
        if let Some(dir) = dir.as_deref() {
            if !ausentes.is_empty() {
                eprintln!(
                    "neon-drift: {} material(is) sem arquivo em '{}': {} — usados os procedurais",
                    ausentes.len(),
                    dir.display(),
                    ausentes.join(", ")
                );
            }
        }
        // Uma textura que falha em subir não tem sintoma visível: o material
        // desenha preto e o quadro continua "funcionando". Dizer o que subiu, o
        // tamanho e quantas são é o que transforma "está preto" em diagnóstico.
        eprintln!(
            "neon-drift: {} textura(s) {nomes:?} {total} bytes, {} de arquivo · {} procedurais, {} x {} de receita",
            handles.len(),
            de_arquivo,
            handles.len() - de_arquivo,
            crate::texturas::LADO,
            crate::texturas::LADO
        );

        // As duas máscaras de luz sobem fora do laço: elas não são materiais e
        // não têm versão em arquivo (o porquê está em `texturas.rs`).
        let halo_t = crate::texturas::halo_neon();
        let cone_t = crate::texturas::cone_farol();
        let halo = render.create_texture_from_rgba(halo_t.largura, halo_t.altura, &halo_t.pixels)?;
        let cone = render.create_texture_from_rgba(cone_t.largura, cone_t.altura, &cone_t.pixels)?;
        eprintln!(
            "neon-drift: máscaras de luz '{}' e '{}' ({} x {}, {} bytes, só alfa)",
            halo_t.nome,
            cone_t.nome,
            halo_t.largura,
            halo_t.altura,
            halo_t.bytes() + cone_t.bytes()
        );

        Ok(Self {
            asfalto: handles[0],
            concreto: handles[1],
            pedra: handles[2],
            mata: handles[3],
            guarda: handles[4],
            painel: handles[5],
            xadrez: handles[6],
            faixa: handles[7],
            grade: handles[8],
            halo,
            cone,
        })
    }
}

/// A cena inteira: malhas, shaders e o estado de câmera/partículas.
pub struct Cena3D {
    // ── estáticos ──────────────────────────────────────────────
    /// Texturas do mapa, criadas no boot e vivas enquanto a cena viver.
    texturas: TexturasGpu,
    /// Shader que amostra textura em vez de usar cor por malha.
    texturizado: ShaderHandle,
    /// Atlas de sprites das partículas (formas com alfa).
    ///
    /// Separado de `texturas` de propósito: aquelas são **materiais** opacos com
    /// UV de mundo, esta é uma **máscara** com UV de célula. Misturar as duas
    /// coisas num só registro faria o próximo material herdar `REPEAT` e mostrar
    /// oito sprites no asfalto.
    atlas_t: giochi_engine_3d::TextureHandle,
    /// Um quadrilátero por forma, com o UV da célula já embutido.
    sprites: Vec<MeshHandle>,
    /// Shader instanciado + texturizado das partículas.
    sprite_shader: ShaderHandle,
    /// Malhas **texturizadas**: uma por material, uma chamada cada.
    asfalto_t: MeshHandle,
    concreto_t: MeshHandle,
    pedra_t: MeshHandle,
    mata_t: MeshHandle,
    guarda_t: MeshHandle,
    xadrez_t: MeshHandle,
    painel_t: MeshHandle,
    /// Faixa central da pista, texturizada (tracejado vem da textura).
    faixas_t: MeshHandle,
    /// Fita de advertência das barreiras, texturizada com a grade de neon.
    grade_t: MeshHandle,
    /// Cubo pequeno para os fragmentos de colisão, instanciado.
    cubo: MeshHandle,
    asfalto: MeshHandle,
    guia_a: MeshHandle,
    guia_b: MeshHandle,
    muros: MeshHandle,
    neon: MeshHandle,
    neon_b: MeshHandle,
    reflexos: MeshHandle,
    poco_luz: MeshHandle,
    telhados: MeshHandle,
    longe: MeshHandle,
    /// Morros da serra: silhueta escura, cor própria (ver [`MORRO`]).
    morros: MeshHandle,
    predios_a: MeshHandle,
    predios_b: MeshHandle,
    predios_c: MeshHandle,
    janelas: MeshHandle,
    tunel: MeshHandle,
    tunel_luzes: MeshHandle,
    tunel_luzes_b: MeshHandle,
    largada_b: MeshHandle,
    /// Protótipo do poste/torre: um cubo unitário escalado por instância.
    poste_proto: MeshHandle,
    /// Protótipo do cone de obstáculo.
    cone_proto: MeshHandle,
    /// Portal de checkpoint, carregado por `load_obj_mesh`.
    portal: MeshHandle,
    /// Instâncias estáticas que a cena desenha com `draw_instanced`.
    postes_inst: Vec<Mat4>,
    luzes_inst: Vec<Mat4>,
    cones_inst: Vec<Mat4>,
    portais_inst: Vec<Mat4>,

    // ── carros ─────────────────────────────────────────────────
    corpo: MeshHandle,
    cabine: MeshHandle,
    rodas: MeshHandle,
    farois: MeshHandle,
    lanternas: MeshHandle,

    // ── dinâmicos ──────────────────────────────────────────────
    quad: MeshHandle,
    phong: ShaderHandle,
    instanciado: Option<ShaderHandle>,
    instancing: bool,

    rotacao: Rotacao,
    /// Medidas da tela de projeto no quadro corrente (paisagem nos dois sentidos).
    largura_projeto: f32,
    altura_projeto: f32,
    /// Medidas **reais** do alvo de desenho (o painel), no quadro corrente.
    ///
    /// Diferente de `largura_projeto` sempre que há giro: o projeto é paisagem
    /// (1280×720) e o painel é retrato (720×1280). O HUD se posiciona no projeto
    /// e é **desenhado** no painel, e é a rotação que faz a ponte.
    painel: (u32, u32),
    camera: CameraJogo,
    particulas: Particulas,
    /// Malhas de instância para os efeitos e para o HUD.
    malhas_efeito: Vec<Mat4>,
    instancias: Vec<Mat4>,
    /// Tempo acumulado, para animações (piscar do neon, tremor da câmera).
    tempo: f32,
    /// Escurecimento das telas de texto, `0,0` (nada) a `1,0` (preto).
    ///
    /// Alvo por fase ([`alvo_veu`]) com atraso exponencial. Guardado como
    /// **estado** e não calculado por fase porque a transição tem de ser vista:
    /// um véu que aparece de um quadro para o outro pisca a tela no "JOGAR".
    veu: f32,
    /// `false` até o primeiro [`Cena3D::seguir`].
    ///
    /// No primeiro quadro o véu **salta** para o alvo em vez de deslizar: senão
    /// um `--quadro` com `--tela corrida` sairia escurecido (a cena nasce com o
    /// véu do título) e o diagnóstico mentiria sobre a imagem de produção.
    veu_iniciado: bool,
    /// Luz do mundo, guardada para restaurar depois do HUD.
    luz_mundo: DirectionalLight,
    /// Cor do céu, que depende do cenário.
    ceu: Color3D,
    /// Quando `Some`, só esta malha é desenhada (diagnóstico `--so`).
    so: Option<String>,
    /// Impacto lido no quadro anterior.
    ///
    /// Serve para a rajada de destroços sair **uma vez por batida**: o impacto
    /// decai no núcleo, mas enquanto ele está acima do limiar o desenho o veria
    /// em vários quadros seguidos e soltaria destroços continuamente.
    ultimo_impacto: f32,
    /// Carros da rede (clientes no host, todos no cliente).
    carros_rede: Vec<CarroRede>,
    /// `false` no cliente LAN: o carro local chega pelo snapshot, não da
    /// `Corrida` simulada — desenhar os dois duplicaria o carro na tela.
    desenhar_locais: bool,
    /// Linhas do overlay F1 (vazio = oculto).
    perfil: Vec<String>,
    /// Aviso de LAN/menu no rodapé do HUD.
    aviso_rede: Option<String>,
    /// As linhas do menu do título, prontas: texto e "é a linha do cursor".
    ///
    /// Vazio é o caso do `--so` e do `--quadro` sem menu montado; aí o HUD cai
    /// num cartão fixo. Quem monta as linhas é o `menu.rs`, que é puro — a cena
    /// não sabe o que é "qualidade" nem "LAN", só desenha.
    menu_linhas: Vec<(String, bool)>,
    /// A frase de ajuda da linha do cursor.
    menu_dica: String,
    /// Quantas malhas estáticas vieram de um `StaticBatch` da engine.
    batches: usize,
    // O contador de chamadas de desenho **não** mora aqui: quem conta é a
    // engine (`RenderContext3D::frame_counters`), no ponto em que a chamada é
    // emitida. Ver o comentário de `diagnostico` em `main.rs`.
}

impl Cena3D {
    /// O espaço do HUD no quadro corrente.
    fn tela_hud(&self) -> TelaHud {
        TelaHud {
            rotacao: self.rotacao,
            projeto: (self.largura_projeto, self.altura_projeto),
            painel: self.painel,
        }
    }

    /// Restringe o desenho a uma malha, pelo nome do diagnóstico.
    pub fn so(&mut self, nome: Option<String>) {
        self.so = nome;
    }

    /// Carros que chegaram pela rede, e se os carros da `Corrida` local devem
    /// ser desenhados.
    ///
    /// No host, `desenhar_locais = true` (jogador + IA) e a lista traz os
    /// clientes. No cliente, `false`: o carro local e os adversários chegam
    /// todos como snapshot, e desenhar a `Corrida` local junto duplicaria os
    /// mesmos carros em posições diferentes.
    pub fn definir_carros_rede(&mut self, carros: Vec<CarroRede>, desenhar_locais: bool) {
        self.carros_rede = carros;
        self.desenhar_locais = desenhar_locais;
    }

    /// Linhas do overlay F1 (vazio desliga).
    pub fn definir_perfil(&mut self, linhas: Vec<String>) {
        self.perfil = linhas;
    }

    /// Aviso de LAN/menu no rodapé do HUD.
    pub fn definir_aviso_rede(&mut self, aviso: Option<String>) {
        self.aviso_rede = aviso.map(|a| a.to_uppercase());
    }

    /// As linhas do menu do título e a dica da linha do cursor.
    ///
    /// Vazio desliga o menu (diagnóstico `--so`); o HUD volta ao cartão fixo.
    pub fn definir_menu(&mut self, linhas: Vec<(String, bool)>, dica: String) {
        self.menu_linhas = linhas;
        self.menu_dica = dica;
    }

    /// Quantas malhas estáticas a cena montou (diagnóstico).
    pub fn batches(&self) -> usize {
        self.batches
    }

    /// Libera **explicitamente** todas as malhas, texturas e shaders da cena.
    ///
    /// O `release_all` do renderer também faria isso, mas ele é uma rede de
    /// segurança: o caminho normal tem de exercitar os `destroy_*` para que um
    /// recurso esquecido apareça no `mesh_count`/`texture_count` durante o
    /// desenvolvimento. No fim, a contagem é impressa; um número diferente de
    /// zero é vazamento, não ruído.
    pub fn destruir(&mut self, render: &mut RenderContext3D) {
        let malhas = [
            self.asfalto_t,
            self.concreto_t,
            self.pedra_t,
            self.mata_t,
            self.guarda_t,
            self.xadrez_t,
            self.painel_t,
            self.faixas_t,
            self.grade_t,
            self.cubo,
            self.asfalto,
            self.guia_a,
            self.guia_b,
            self.muros,
            self.neon,
            self.neon_b,
            self.reflexos,
            self.poco_luz,
            self.telhados,
            self.longe,
            self.morros,
            self.predios_a,
            self.predios_b,
            self.predios_c,
            self.janelas,
            self.tunel,
            self.tunel_luzes,
            self.tunel_luzes_b,
            self.largada_b,
            self.poste_proto,
            self.cone_proto,
            self.portal,
            self.corpo,
            self.cabine,
            self.rodas,
            self.farois,
            self.lanternas,
            self.quad,
        ];
        for malha in malhas {
            render.destroy_mesh(malha);
        }
        for malha in self.sprites.drain(..) {
            render.destroy_mesh(malha);
        }
        for textura in [
            self.texturas.asfalto,
            self.texturas.concreto,
            self.texturas.pedra,
            self.texturas.mata,
            self.texturas.guarda,
            self.texturas.painel,
            self.texturas.xadrez,
            self.texturas.faixa,
            self.texturas.grade,
            self.texturas.halo,
            self.texturas.cone,
            self.atlas_t,
        ] {
            render.destroy_texture(textura);
        }
        render.destroy_shader(self.texturizado);
        render.destroy_shader(self.sprite_shader);
        render.destroy_shader(self.phong);
        if let Some(inst) = self.instanciado.take() {
            render.destroy_shader(inst);
        }
        render.release_all();
        let restantes = (render.mesh_count(), render.texture_count(), render.shader_count());
        if restantes != (0, 0, 0) {
            eprintln!(
                "neon-drift: AVISO — recursos restantes após destruir: {} malha(s), {} textura(s), {} shader(s)",
                restantes.0, restantes.1, restantes.2
            );
        }
    }

    /// Monta a cena a partir da pista e do cenário do mapa.
    pub fn nova(
        render: &mut RenderContext3D,
        pista: &Pista,
        rotacao: Rotacao,
        cenario: Cenario,
    ) -> Result<Self, Box<dyn Error>> {
        let mundo = construir_mundo(pista, rotacao, cenario);

        let ceu = match cenario {
            Cenario::Avenida => NOITE,
            Cenario::Descida => CEU_SERRA,
            Cenario::Circuito => CEU_CIRCUITO,
            Cenario::Porto => CEU_PORTO,
            Cenario::Salar => CEU_SALAR,
        };
        let texturas = TexturasGpu::criar(render)?;
        let texturizado = render.create_builtin_shader(BuiltinShader::Textured)?;
        let asfalto_t = criar(render, &mundo.asfalto_t)?;
        let concreto_t = criar(render, &mundo.concreto_t)?;
        let pedra_t = criar(render, &mundo.pedra_t)?;
        let mata_t = criar(render, &mundo.mata_t)?;
        let guarda_t = criar(render, &mundo.guarda_t)?;
        let xadrez_t = criar(render, &mundo.xadrez_t)?;
        let painel_t = criar(render, &mundo.painel_t)?;
        let faixas_t = criar(render, &mundo.faixas_t)?;
        let grade_t = criar(render, &mundo.grade_t)?;
        let cubo = render.create_cube_mesh(1.0)?;

        let asfalto = criar(render, &mundo.asfalto)?;
        let guia_a = criar(render, &mundo.guia_a)?;
        let guia_b = criar(render, &mundo.guia_b)?;
        let muros = criar(render, &mundo.muros)?;
        let neon = criar(render, &mundo.neon)?;
        let neon_b = criar(render, &mundo.neon_b)?;
        let reflexos = criar(render, &mundo.reflexos)?;
        let poco_luz = criar(render, &mundo.poco_luz)?;
        let telhados = criar(render, &mundo.telhados)?;
        let longe = criar(render, &mundo.longe)?;
        let morros = criar(render, &mundo.morros)?;
        let predios_a = criar(render, &mundo.predios_a)?;
        let predios_b = criar(render, &mundo.predios_b)?;
        let predios_c = criar(render, &mundo.predios_c)?;
        let janelas = criar(render, &mundo.janelas)?;
        let tunel = criar(render, &mundo.tunel)?;
        let tunel_luzes = criar(render, &mundo.tunel_luzes)?;
        let tunel_luzes_b = criar(render, &mundo.tunel_luzes_b)?;
        let largada_b = criar(render, &mundo.largada_b)?;

        let carro = malha::carro();
        // A lataria vem de `.obj` (com normais e triangulação garantidas pelo
        // `load_obj` da engine). O modelo procedural fica como queda: um `obj`
        // removido do pacote não pode deixar o jogo sem carro.
        let corpo = match crate::modelos::caminho(crate::modelos::CARRO) {
            Some(caminho) => match render.load_obj_mesh(&caminho) {
                Ok(h) => {
                    eprintln!(
                        "neon-drift: lataria de '{}' ({} vértices)",
                        caminho.display(),
                        render.mesh_vertex_count(h)
                    );
                    h
                }
                Err(erro) => {
                    eprintln!("neon-drift: OBJ da lataria recusado ({erro}); usando a procedural");
                    render.create_mesh(&carro.corpo.0, &carro.corpo.1)?
                }
            },
            None => render.create_mesh(&carro.corpo.0, &carro.corpo.1)?,
        };
        let cabine = render.create_mesh(&carro.cabine.0, &carro.cabine.1)?;
        let rodas = render.create_mesh(&carro.rodas.0, &carro.rodas.1)?;
        let farois = render.create_mesh(&carro.farois.0, &carro.farois.1)?;
        let lanternas = render.create_mesh(&carro.lanternas.0, &carro.lanternas.1)?;

        let poste_proto = render.create_cube_mesh(1.0)?;
        let (cv, ci) = malha::cone(0.55, 1.0);
        let cone_proto = render.create_mesh(&cv, &ci)?;
        let portal = match crate::modelos::caminho(crate::modelos::PORTAL) {
            Some(caminho) => match render.load_obj_mesh(&caminho) {
                Ok(h) => {
                    eprintln!(
                        "neon-drift: portal de '{}' ({} vértices, instanciado nos checkpoints)",
                        caminho.display(),
                        render.mesh_vertex_count(h)
                    );
                    h
                }
                Err(erro) => {
                    eprintln!("neon-drift: OBJ do portal recusado ({erro}); usando o fallback");
                    let (pv, pi) = malha::portal_fallback();
                    render.create_mesh(&pv, &pi)?
                }
            },
            None => {
                let (pv, pi) = malha::portal_fallback();
                render.create_mesh(&pv, &pi)?
            }
        };
        let batches = mundo.contagem_estaticas();
        eprintln!(
            "neon-drift: {batches} malha(s) estática(s) via StaticBatch; {} poste(s), {} cone(s), {} portal(is) instanciados",
            mundo.postes_inst.len(),
            mundo.cones_inst.len(),
            mundo.portais_inst.len()
        );

        let (qv, qi) = malha::quad();
        let quad = render.create_mesh(&qv, &qi)?;

        // Atlas de sprites: uma textura, uma malha por forma.
        //
        // `GIOCHI_SPRITES=<arquivo.bmp>` troca as formas procedurais por uma
        // folha do usuário, e `GIOCHI_SPRITES_COLUNAS` diz quantas colunas ela
        // tem (padrão: 4, o layout do atlas). A folha é **normalizada** para o
        // mesmo 128×128 de quatro colunas, então nada abaixo desta linha sabe de
        // onde ela veio — e uma folha 8×1 entra igual a uma 4×2.
        let caminho_folha = std::env::var("GIOCHI_SPRITES").ok();
        let colunas_folha = std::env::var("GIOCHI_SPRITES_COLUNAS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(crate::atlas::COLUNAS);
        let (atlas_pixels, origem_atlas, problema_folha) =
            crate::atlas::carregar_ou_gerar(caminho_folha.as_deref(), colunas_folha);
        // O atlas sai **sem mipmap**, e as superfícies saem com: num atlas cada
        // célula é vizinha de outra, e o filtro trilinear de um nível reduzido
        // puxa o texel da célula ao lado — o sprite ganha um canto do vizinho. O
        // inset de 1e-4 do retângulo de UV cobre o nível 0 e não o nível 2. Já a
        // superfície de chão, vista de raspa a 200 m, ganha muito em não tremer.
        let atlas_t = render.create_texture_from_rgba_no_mipmap(
            crate::atlas::LADO,
            crate::atlas::LADO,
            &atlas_pixels,
        )?;
        let sprites = crate::atlas::Forma::TODAS
            .iter()
            .map(|forma| {
                let (sv, si) = malha::quad_celula(forma.indice());
                render.create_mesh(&sv, &si)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let phong = render.create_builtin_shader(BuiltinShader::Phong)?;
        let instancing = render.supports_instancing();
        let instanciado = if instancing {
            Some(render.create_builtin_shader(BuiltinShader::Instanced)?)
        } else {
            None
        };
        let sprite_shader = render.create_builtin_shader(BuiltinShader::TexturedInstanced)?;
        let nomes: Vec<&str> = crate::atlas::Forma::TODAS.iter().map(|f| f.nome()).collect();
        // O aviso de folha recusada é **alto**: sem ele o jogo desenha as formas
        // procedurais e o usuário conclui que a folha dele não serve, quando o
        // que falhou foi o caminho, o formato ou o número de colunas.
        if let Some(motivo) = &problema_folha {
            eprintln!("neon-drift: folha de sprites ignorada — {motivo}");
        }
        eprintln!(
            "neon-drift: atlas de sprites {}x{} {nomes:?} {} bytes (origem: {})",
            crate::atlas::LADO,
            crate::atlas::LADO,
            atlas_pixels.len(),
            origem_atlas.nome()
        );

        // `sun()` puro (ambiente 0,3 + difusa 1,0 + especular 0,25) satura: o
        // que era branco virava 255,255,255 chapado e o que era claro virava
        // branco. Com 0,62 de ambiente e 0,34 de difusa a cor da malha continua
        // mandando, e ainda há variação entre faces — é o que dá volume ao
        // prédio e ao carro sem apagar a paleta.
        // A serra recebe mais ambiente que a avenida: medido, o quadro da
        // descida ficava com média 33 contra 41–61 da cidade, e as copas da mata
        // saíam a (10, 26, 26) — praticamente preto. Não é atmosfera, é tela
        // apagada: num painel de 3,5" o que não dá para ler não existe.
        let ambiente = match cenario {
            Cenario::Avenida => 0.62,
            Cenario::Descida => 0.80,
            // O circuito tem torre de luz; o salar é o mais claro de todos
            // porque o chão é branco e devolve luz.
            Cenario::Circuito => 0.74,
            Cenario::Porto => 0.66,
            Cenario::Salar => 0.78,
        };
        let luz_mundo = DirectionalLight::sun()
            .with_ambient(ambiente)
            .with_color(Color3D::new(0.34, 0.34, 0.40, 1.0))
            .with_specular(0.10, 40.0);

        Ok(Self {
            texturas,
            texturizado,
            atlas_t,
            sprites,
            sprite_shader,
            ceu,
            asfalto_t,
            concreto_t,
            pedra_t,
            mata_t,
            guarda_t,
            xadrez_t,
            painel_t,
            faixas_t,
            grade_t,
            cubo,
            asfalto,
            guia_a,
            guia_b,
            muros,
            neon,
            neon_b,
            reflexos,
            poco_luz,
            telhados,
            longe,
            morros,
            predios_a,
            predios_b,
            predios_c,
            janelas,
            tunel,
            tunel_luzes,
            tunel_luzes_b,
            largada_b,
            poste_proto,
            cone_proto,
            portal,
            postes_inst: mundo.postes_inst,
            luzes_inst: mundo.luzes_inst,
            cones_inst: mundo.cones_inst,
            portais_inst: mundo.portais_inst,
            corpo,
            cabine,
            rodas,
            farois,
            lanternas,
            quad,
            phong,
            instanciado,
            instancing,
            rotacao,
            largura_projeto: 640.0,
            altura_projeto: 480.0,
            painel: (640, 480),
            camera: CameraJogo::nova(pista, rotacao),
            particulas: Particulas::nova(),
            malhas_efeito: Vec::with_capacity(1024),
            instancias: Vec::with_capacity(512),
            tempo: 0.0,
            // Nasce com o véu do título porque é ali que a cena aparece na
            // esmagadora maioria das vezes; `seguir` corrige no primeiro quadro.
            veu: VEU_TITULO,
            veu_iniciado: false,
            luz_mundo,
            so: None,
            ultimo_impacto: 0.0,
            carros_rede: Vec::new(),
            desenhar_locais: true,
            perfil: Vec::new(),
            aviso_rede: None,
            menu_linhas: Vec::new(),
            menu_dica: String::new(),
            batches,
        })
    }

    /// Chamadas de desenho do último quadro.


    /// `true` quando o driver aceita desenho instanciado.
    pub fn usa_instancing(&self) -> bool {
        self.instancing
    }

    /// Marca uma batida: a câmera treme por alguns décimos de segundo.
    ///
    /// Tremor e não flash: um flash branco num jogo noturno cega, e o tremor
    /// comunica "bateu" sem esconder a pista justamente na hora em que o jogador
    /// precisa corrigir.
    pub fn bater(&mut self) {
        self.camera.tremor = (self.camera.tremor + 0.9).min(1.4);
        // Faísca é o que **situa** a batida: o tremor sozinho diz "aconteceu
        // alguma coisa", mas não onde. A rajada sai das laterais do carro e
        // cai, então o olho encontra o ponto do choque.
        self.particulas.pedir_faiscas(14);
    }

    /// Cor de fundo (céu) e preparação do quadro.
    ///
    /// Precisa ser chamado **antes** de `begin_frame`: a engine aplica o estado
    /// dentro dele, e um `set_state` no meio do quadro só vale para o seguinte.
    /// Foi assim que o `blend` do neon e o `clear_color` da noite não pegavam.
    ///
    /// A mistura fica ligada para o quadro inteiro — os opacos têm alfa 1,0, e
    /// `src·1 + dst·0` é o próprio fragmento. Desligá-la exigiria dois estados
    /// por quadro, que esta engine não tem.
    ///
    /// O descarte de faces também fica desligado, pelo mesmo motivo: a corrida
    /// desenha caixas e quads virados para os dois lados (chão, billboards, HUD)
    /// e distinguir os passes custaria um estado por passe. O preço é desenhar a
    /// face de trás de caixas fechadas — que o teste de profundidade descarta —
    /// e o ganho é que um quadrilátero invertido nunca mais some em silêncio.
    pub fn preparar(&self, render: &mut RenderContext3D) {
        render.set_state(GlState {
            depth_test: true,
            cull_face: std::env::var_os("GIOCHI_CULL").is_none_or(|v| v != "0"),
            front_face: FrontFace::CounterClockwise,
            blend: true,
            clear_color: self.ceu,
        });
    }

    /// Desenha um quadro completo.
    pub fn desenhar(
        &mut self,
        render: &mut RenderContext3D,
        corrida: &Corrida,
        dt: f32,
    ) -> Result<(), Box<dyn Error>> {
        self.seguir(corrida, dt);

        // Duas luzes no mesmo quadro: `set_light` grava uniforme por desenho,
        // então dá para trocar entre um objeto e outro (o `set_state` não, que
        // só é aplicado no começo do quadro).
        //
        // Opacos são iluminados; tudo o que **emite** luz — neon, lampião,
        // janela, telhado, farol — é desenhado com luz cheia. Sem essa separação
        // o ciano do neon saía a 45% do brilho e a cidade ficava cinza.
        let camera = self.camera_mundo(render.drawable_size());
        let ident = Transform3D::IDENTITY;

        // ── mundo texturizado ────────────────────────────────────
        //
        // Uma chamada por material. O `tint` é o que dá a cor: a textura é
        // neutra de propósito, então o asfalto é escuro por tint e a pedra é
        // clara pela mesma textura com outro tint.
        //
        // Desenhado **primeiro**: o teste de profundidade resolve a ordem entre
        // opaco e opaco, e as superfícies texturizadas são todas opacas.
        // `u_tint` dinâmico: o asfalto e a barreira pulsam em amplitudes
        // diferentes. Custa um uniforme por desenho — nenhum pixel a mais, e é
        // o que faz a pista parecer viva em vez de uma foto.
        let pulso_asfalto = 0.93 + 0.07 * (self.tempo * 1.7).sin();
        let pulso_barreira = 0.88 + 0.12 * (self.tempo * 2.6 + 1.0).sin();
        // A faixa central respira no contratempo da barreira: dois pulsos
        // iguais na mesma direção leem como um pisca-pisca só.
        let pulso_faixa = 0.90 + 0.10 * (self.tempo * 2.6).sin();
        render.set_light(self.luz_mundo);
        for (nome, malha, textura, tint) in [
            (
                "asfalto_t",
                self.asfalto_t,
                self.texturas.asfalto,
                multiplicar(ASFALTO, pulso_asfalto),
            ),
            ("concreto_t", self.concreto_t, self.texturas.concreto, CONCRETO),
            ("pedra_t", self.pedra_t, self.texturas.pedra, PEDRA),
            ("mata_t", self.mata_t, self.texturas.mata, MATA),
            (
                "guarda_t",
                self.guarda_t,
                self.texturas.guarda,
                multiplicar(GUARDA_CORPO, pulso_barreira),
            ),
            ("painel_t", self.painel_t, self.texturas.painel, PAINEL_NEON),
            ("xadrez_t", self.xadrez_t, self.texturas.xadrez, LARGADA_A),
            (
                "faixas_t",
                self.faixas_t,
                self.texturas.faixa,
                multiplicar(FAIXA_PISTA, pulso_faixa),
            ),
            (
                "grade_t",
                self.grade_t,
                self.texturas.grade,
                multiplicar(GRADE_NEON, pulso_barreira),
            ),
        ] {
            if self.quero(nome) && !self.desligado(parte_de_textura(nome)) {
                // A ordem é a da engine 0.4.0: textura antes do modelo. Na 0.2.2 era
                // `(malha, shader, modelo, camera, textura, tint)`; trocar sem avisar
                // daria erro de tipo, mas a mensagem fala de `Transform3D` esperado
                // onde veio `TextureHandle` — não de versão.
                render.draw_mesh_textured(
                    malha,
                    self.texturizado,
                    textura,
                    &ident,
                    &camera,
                    tint,
                )?;
            }
        }

        render.set_light(self.luz_mundo);
        for (nome, malha, cor) in [
            ("asfalto", self.asfalto, ASFALTO),
            ("guia_a", self.guia_a, GUIA_VERMELHA),
            ("guia_b", self.guia_b, GUIA_BRANCA),
            ("muros", self.muros, MURO),
            ("predios_a", self.predios_a, PREDIO_A),
            ("predios_b", self.predios_b, PREDIO_B),
            ("predios_c", self.predios_c, PREDIO_C),
            ("tunel", self.tunel, TUNEL),
            ("largada_b", self.largada_b, LARGADA_B),
            ("longe", self.longe, PREDIO_LONGE),
            ("morros", self.morros, MORRO),
        ] {
            if self.quero(nome) && !self.desligado(nome) {
                render.draw_mesh_colored(malha, self.phong, &ident, &camera, cor)?;
            }
        }

        // Postes e cones: **uma** chamada cada, independente de quantos são.
        // Antes eles eram assados na malha do mundo; a geometria é idêntica e
        // a única coisa que muda é a transformação, que é exatamente o caso de
        // `draw_instanced`.
        if let Some(inst) = self.instanciado {
            for (nome, malha, instancias, cor) in [
                ("postes", self.poste_proto, &self.postes_inst, POSTE),
                ("cones", self.cone_proto, &self.cones_inst, CONE),
            ] {
                if self.quero(nome) && !self.desligado(nome) {
                    render.draw_instanced(malha, inst, instancias, &camera, cor)?;
                }
            }
        }

        // Pulso: a cidade respira. Custa **zero** — a cor já é uniforme por
        // desenho, então modular o brilho não é um desenho a mais nem um vertex a
        // mais. É o efeito mais barato que existe e o que mais diferencia uma
        // avenida estática de uma viva.
        let pulso = 0.80 + 0.20 * (self.tempo * 2.2).sin();
        let pulso_b = 0.80 + 0.20 * (self.tempo * 2.2 + std::f32::consts::PI).sin();
        let tremula = 0.94 + 0.06 * (self.tempo * 11.0).sin();

        render.set_light(EMISSIVO);
        for (nome, malha, cor) in [
            ("janelas", self.janelas, multiplicar(JANELA, tremula)),
            ("telhados", self.telhados, multiplicar(NEON_MAGENTA, pulso_b)),
            ("tunel_luzes", self.tunel_luzes, multiplicar(TUNEL_LUZ, pulso)),
            ("tunel_luzes_b", self.tunel_luzes_b, multiplicar(TUNEL_LUZ, pulso_b)),
            ("neon", self.neon, multiplicar(NEON_CIANO, pulso)),
            ("neon_b", self.neon_b, multiplicar(NEON_MAGENTA, pulso_b)),
            ("poco_luz", self.poco_luz, multiplicar(POCA_LUZ, pulso)),
            ("reflexos", self.reflexos, multiplicar(REFLEXO, pulso)),
        ] {
            if self.quero(nome) && !self.desligado(nome) {
                render.draw_mesh_colored(malha, self.phong, &ident, &camera, cor)?;
            }
        }
        // Refletores e portais de checkpoint: instanciados e emissivos.
        if let Some(inst) = self.instanciado {
            for (nome, malha, instancias, cor) in [
                (
                    "luzes",
                    self.poste_proto,
                    &self.luzes_inst,
                    multiplicar(LUZ_POSTE, pulso),
                ),
                (
                    "portais",
                    self.portal,
                    &self.portais_inst,
                    multiplicar(NEON_CIANO, pulso_b),
                ),
            ] {
                if self.quero(nome) && !self.desligado(nome) {
                    render.draw_instanced(malha, inst, instancias, &camera, cor)?;
                }
            }
        }
        render.set_light(self.luz_mundo);

        // ── carros, partículas e HUD ──────────────────────────────
        if self.so.is_none() {
            if !self.desligado("carros") {
                self.desenhar_carros(render, corrida, &camera)?;
            }
            if !self.desligado("particulas") {
                self.desenhar_particulas(render, &camera)?;
                self.desenhar_fragmentos(render, &camera)?;
            }
            if !self.desligado("hud") {
                self.desenhar_hud(render, corrida)?;
            }
        }

        Ok(())
    }

    // ────────────────────────────────────────────────────────
    // câmera
    // ────────────────────────────────────────────────────────

    /// Avança só a câmera (e o tempo das animações), sem desenhar.
    ///
    /// Existe para o `--passos`: ele adianta a simulação e depois desenha **um**
    /// quadro, e sem isto a câmera ficava no ponto de partida enquanto o carro
    /// já estava 250 m adiante. O sintoma era um quadro que parecia defeito de
    /// desenho — a pista aparecia como uma faixa fina ao longe e a tela enchia
    /// de prédios.
    pub fn seguir(&mut self, corrida: &Corrida, dt: f32) {
        self.tempo += dt;
        self.atualizar_veu(corrida.fase, dt);
        self.atualizar_camera(corrida, dt);
        self.atualizar_particulas(corrida, dt);
    }

    /// Leva o véu até o alvo da fase corrente.
    ///
    /// Atraso exponencial, e não interpolação linear: a linear chega no fim com
    /// velocidade zero e a exponencial sai rápido do começo, que é o que faz a
    /// troca ler como "a tela clareou" e não como "algo se mexeu devagar".
    fn atualizar_veu(&mut self, fase: Fase, dt: f32) {
        let alvo = alvo_veu(fase);
        if !self.veu_iniciado {
            self.veu = alvo;
            self.veu_iniciado = true;
            return;
        }
        self.veu = avancar_veu(self.veu, alvo, dt);
    }

    /// O véu em vigor, para o `--json` e para os testes.
    ///
    /// Publicar isto é o que permite provar por script que a gameplay **não**
    /// está escurecida: a diferença entre 0,0 e 0,62 não aparece numa contagem
    /// de desenhos, só na imagem — ou neste número.
    pub fn veu(&self) -> f32 {
        self.veu
    }

    /// Posição da câmera, para o diagnóstico em `--json`.
    pub fn camera(&self) -> (f32, f32, f32, f32) {
        (
            self.camera.pos.x,
            self.camera.pos.y,
            self.camera.pos.z,
            self.camera.fov,
        )
    }

    /// `GIOCHI_SEM=carros,particulas,hud` desliga partes do quadro.
    ///
    /// Existe para medir: num painel de 720×1280 o quadro é dominado por pixel,
    /// e sem separar as três partes "está lento" não diz o que cortar.
    fn desligado(&self, parte: &str) -> bool {
        std::env::var("GIOCHI_SEM")
            .map(|v| v.split(',').any(|p| p.trim() == parte))
            .unwrap_or(false)
    }

    fn quero(&self, nome: &str) -> bool {
        match &self.so {
            None => true,
            // `--so mundo` desenha toda a geometria estática e **nada** mais:
            // é o que separa "o mundo custa" de "carro, partícula e HUD custam".
            Some(filtro) if filtro == "mundo" => true,
            Some(filtro) => filtro == nome,
        }
    }

    fn atualizar_camera(&mut self, corrida: &Corrida, dt: f32) {
        let carro = &corrida.carro;
        let (px, pz) = girar_ponto(self.rotacao, carro.pos);
        let direcao = carro.direcao();
        let (dx, dz) = girar_direcao(self.rotacao, direcao);

        // Alvo: atrás do carro, e um pouco acima. A distância cresce um pouco com
        // a velocidade, o que abre o enquadramento na reta.
        //
        // O `y` soma a **altura da pista**: numa descida a câmera tem de descer
        // junto, senão ela fica no nível do mar enquanto o carro está a 60 m e o
        // jogo vira uma vista aérea.
        let rapidez = (carro.velocidade / 62.0).clamp(0.0, 1.4);
        // Spring-arm: o braço da câmera estica com a velocidade e **mais** com
        // o nitro. O alongamento no boost não é cosmético — é o que dá a
        // leitura de "o carro disparou" sem desenhar um único pixel a mais.
        let boost = if carro.nitro_usando { 1.0 } else { 0.0 };
        let distancia = 7.4 + rapidez * 1.9 + boost * 1.1;
        let altura = carro.altura + 3.05 + rapidez * 0.35 + boost * 0.18;
        let alvo_pos = Vec3::new(px - dx * distancia, altura, pz - dz * distancia);
        let alvo_mira = Vec3::new(px + dx * 7.0, carro.altura + 1.05, pz + dz * 7.0);

        // Atraso exponencial estável em qualquer `dt`: `1 - exp(-k·dt)` não
        // estoura com quadro perdido, ao contrário de `k·dt`. É a integração
        // de um mola-amortecedor criticamente amortecido, e é o que dá peso de
        // perseguição em vez de a câmera colar no carro.
        let suavidade = 1.0 - (-8.5 * dt).exp();
        self.camera.pos = misturar(self.camera.pos, alvo_pos, suavidade);
        let suavidade_mira = 1.0 - (-11.0 * dt).exp();
        self.camera.alvo = misturar(self.camera.alvo, alvo_mira, suavidade_mira);

        // FOV: nitro abre o campo de visão. É o efeito de velocidade mais barato
        // que existe — nenhum pixel a mais é desenhado.
        let nitro = if carro.nitro_usando { 1.0 } else { 0.0 };
        let alvo_fov = FOV_BASE + (FOV_NITRO - FOV_BASE) * nitro;
        self.camera.fov += (alvo_fov - self.camera.fov) * (1.0 - (-6.0 * dt).exp());

        // Inclinação no drift: o horizonte tomba para o lado de fora da curva.
        let alvo_roll = (-carro.derrapagem * 0.010).clamp(-0.10, 0.10)
            + (carro.velocidade * 0.0015).clamp(0.0, 0.05);
        self.camera.roll += (alvo_roll - self.camera.roll) * (1.0 - (-7.0 * dt).exp());

        // Tremor de batida: some em ~0,25 s.
        self.camera.tremor = (self.camera.tremor - dt * 4.0).max(0.0);
    }

    /// Câmera do mundo, com a rotação do sistema aplicada como *roll*.
    fn camera_mundo(&self, tela: (u32, u32)) -> Camera3D {
        let aspecto = tela.0 as f32 / tela.1.max(1) as f32;
        // Com giro, a altura do painel cobre a largura do projeto: o FOV vertical
        // passa a ser o horizontal do projeto. Sem isso, o mundo sairia com o
        // zoom errado no X55 (retrato).
        let fov_y = match self.rotacao {
            Rotacao::Nenhuma => {
                2.0 * (self.camera.fov * 0.5).tan().atan2(aspecto)
            }
            _ => self.camera.fov,
        };

        let tremor = self.camera.tremor;
        let desvio = if tremor > 0.001 {
            Vec3::new(
                (self.tempo * 61.0).sin() * tremor * 0.35,
                (self.tempo * 47.0).cos() * tremor * 0.28,
                0.0,
            )
        } else {
            Vec3::ZERO
        };

        let roll = roll_do_mundo(self.rotacao, self.camera.roll);
        let frente = direcao_normalizada(subtrair(self.camera.alvo, self.camera.pos));
        let up = Quat::from_axis_angle(frente, roll).rotate_vec3(Vec3::UP);

        Camera3D::from_parts(somar(self.camera.pos, desvio), self.camera.alvo, up)
            .with_fov_y(fov_y)
            .with_clip_planes(0.25, 900.0)
            .with_drawable_size(tela.0, tela.1)
    }

    // ────────────────────────────────────────────────────────
    // carros
    // ────────────────────────────────────────────────────────

    /// Desenha os quatro carros em **oito** chamadas, e não em vinte.
    ///
    /// ## Por que isso importa num RK3566
    ///
    /// Medido no aparelho: uma `draw_instanced` a mais custa ~0,15 ms de CPU, e
    /// o quadro inteiro tem 16,7 ms para caber em 60 Hz. Com cinco malhas por
    /// carro e quatro carros, o desenho ingênuo gastava 3 ms só aqui — 18% do
    /// orçamento em vinte chamadas que desenham a **mesma** geometria.
    ///
    /// Como as malhas são as mesmas, `draw_instanced` serve a todos os carros de
    /// uma vez: vidro, roda, farol e lanterna têm cor única e saem numa chamada
    /// cada. Só a lataria precisa de uma chamada por cor, porque a cor é
    /// uniforme por desenho.
    fn desenhar_carros(
        &mut self,
        render: &mut RenderContext3D,
        corrida: &Corrida,
        camera: &Camera3D,
    ) -> Result<(), Box<dyn Error>> {
        let Some(inst) = self.instanciado else {
            return Ok(());
        };

        // A lataria sai com `BuiltinShader::Phong`: é o único shader da engine
        // que aplica o termo especular da luz, e é o que faz o neon da cidade
        // correr pela tampa do carro. Como Phong é um desenho por malha (não
        // tem variante instanciada), o corpo custa **uma chamada por carro**;
        // vidro, rodas, faróis e lanternas continuam instanciados numa chamada
        // cada, porque esses não precisam de brilho especular para ler.
        let mut todos: Vec<Mat4> = Vec::with_capacity(corrida.adversarios.len() + self.carros_rede.len() + 1);
        let mut corpos: Vec<(Transform3D, [f32; 3])> = Vec::with_capacity(todos.capacity());
        let mut luzes: Vec<LuzDoCarro> = Vec::with_capacity(todos.capacity());

        if self.desenhar_locais {
            let transform = self.transform_do_jogador(&corrida.carro);
            todos.push(transform.matrix());
            corpos.push((transform, COR_JOGADOR));
            luzes.push(LuzDoCarro {
                base: transform.matrix(),
                cor: COR_JOGADOR,
                deriva: rampa_de_drift(corrida.carro.derrapando, corrida.carro.tempo_drift),
                no_ar: corrida.carro.no_ar,
            });

            for adversario in &corrida.adversarios {
                let transform = transform_de_carro(
                    self.rotacao,
                    adversario.posicao(&corrida.pista),
                    adversario.angulo(&corrida.pista),
                    corrida.pista.altura_em(adversario.s),
                    0.0,
                );
                todos.push(transform.matrix());
                let cor = CORES_ADVERSARIOS[adversario.cor % CORES_ADVERSARIOS.len()];
                corpos.push((transform, cor));
                // A IA não derrapa: ela segue a linha com oscilação lateral, que
                // não abre ângulo. `deriva = 0` é o fato, não uma omissão.
                luzes.push(LuzDoCarro {
                    base: transform.matrix(),
                    cor,
                    deriva: 0.0,
                    no_ar: false,
                });
            }
        }

        // Carros da rede: no host são os clientes; no cliente são todos,
        // inclusive o próprio, porque o cliente **não** simula o carro local.
        for carro in &self.carros_rede {
            let transform = transform_de_carro(
                self.rotacao,
                carro.pos,
                carro.angulo,
                carro.altura,
                0.0,
            );
            todos.push(transform.matrix());
            corpos.push((transform, carro.cor));
            luzes.push(LuzDoCarro {
                base: transform.matrix(),
                cor: carro.cor,
                // O snapshot traz um **bit** de drift a 15 Hz, não a duração:
                // inventar um relógio local dessincronizaria do host. Bit ligado
                // = neon no auge.
                deriva: if carro.derrapando { 1.0 } else { 0.0 },
                no_ar: carro.no_ar,
            });
        }

        render.set_light(self.luz_mundo);
        for (transform, cor) in &corpos {
            render.draw_mesh_colored(
                self.corpo,
                self.phong,
                transform,
                camera,
                Color3D::new(cor[0], cor[1], cor[2], 1.0),
            )?;
        }

        // Peças de cor única: uma chamada para todos os carros.
        for (malha, cor) in [
            (self.cabine, VIDRO),
            (self.rodas, PNEU),
            // Faróis e lanternas ficam acesos de qualquer ângulo: é o que faz o
            // carro ser visto de longe, à noite.
            (self.farois, FAROL),
            (self.lanternas, LANTERNA),
        ] {
            render.draw_instanced(malha, inst, &todos, camera, cor)?;
        }

        self.desenhar_luz_dos_carros(render, camera, &luzes)?;
        Ok(())
    }

    /// A luz que os carros jogam no asfalto: a poça de neon e o cone do farol.
    ///
    /// ## Por que textura, e não cor chapada
    ///
    /// Até aqui eram dois quads de cor sólida: a poça era um retângulo de borda
    /// dura (o comentário antigo falava em "duas camadas" justamente porque não
    /// havia gradiente) e o feixe, uma faixa azul de largura constante que lia
    /// como fita adesiva no chão. As máscaras de alfa do `texturas.rs` fazem o
    /// **mesmo** quad desenhar luz: nenhum pixel a mais, e o custo por pixel é o
    /// mesmo de antes. O que a mudança cobra é chamada de desenho — uma por cor
    /// de carro na poça, uma para todos no feixe; o número medido está no README
    /// ("O que custa", em *A luz que os carros jogam no chão*).
    ///
    /// ## Por que agrupado por cor
    ///
    /// `draw_instanced_textured` tem um `u_tint` por lote, e cada carro tem a
    /// sua cor. Ver [`agrupar_por_tinta`].
    ///
    /// ## No ar, nada
    ///
    /// A poça e o feixe são decalques colados no chão. Com o carro saltando, um
    /// decalque embaixo dele fica no asfalto sem emissor e lê como sombra
    /// errada — e sombra de luz é o defeito mais visível que existe num jogo
    /// noturno.
    fn desenhar_luz_dos_carros(
        &mut self,
        render: &mut RenderContext3D,
        camera: &Camera3D,
        luzes: &[LuzDoCarro],
    ) -> Result<(), Box<dyn Error>> {
        if !self.desligado("aura") {
            let mut pocas = Vec::with_capacity(luzes.len());
            for luz in luzes.iter().filter(|l| !l.no_ar) {
                let tinta = tinta_da_poca(luz.cor, luz.deriva, self.tempo);
                let matriz = achatar(luz.base, POCA_LARGURA, POCA_COMPRIMENTO, ALTURA_POCA);
                pocas.push((tinta, matriz));
            }
            for (tinta, matrizes) in agrupar_por_tinta(pocas) {
                render.draw_instanced_textured(
                    self.quad,
                    self.sprite_shader,
                    &matrizes,
                    camera,
                    self.texturas.halo,
                    tinta,
                )?;
            }
        }

        if !self.desligado("farol") {
            self.instancias.clear();
            for luz in luzes.iter().filter(|l| !l.no_ar) {
                self.instancias.push(feixe(luz.base));
            }
            if !self.instancias.is_empty() {
                // Farol é branco-frio para todos: a cor não identifica o carro,
                // e um farol colorido leria como neon em vez de farol.
                render.draw_instanced_textured(
                    self.quad,
                    self.sprite_shader,
                    &self.instancias,
                    camera,
                    self.texturas.cone,
                    Color3D::new(0.82, 0.91, 1.0, FEIXE_ALFA),
                )?;
            }
        }
        Ok(())
    }

    /// Transformação do carro do jogador, com o empinamento do salto.
    fn transform_do_jogador(&self, carro: &giochi_neon_drift_core::Carro) -> Transform3D {
        let inclinacao = if carro.no_ar {
            carro
                .velocidade_vertical
                .atan2(carro.velocidade.abs().max(6.0))
        } else {
            0.0
        };
        transform_de_carro(
            self.rotacao,
            carro.pos,
            carro.angulo,
            carro.altura,
            inclinacao,
        )
    }

    // ────────────────────────────────────────────────────────
    // partículas
    // ────────────────────────────────────────────────────────

    /// Avança o sistema de efeitos e entrega a câmera a ele.
    ///
    /// O sistema precisa da câmera porque chuva e poeira nascem **à frente
    /// dela**: emitir na volta inteira de 962 m daria 99% de partícula que
    /// ninguém vê e o dobro de trabalho.
    fn atualizar_particulas(&mut self, corrida: &Corrida, dt: f32) {
        self.particulas
            .atualizar(corrida, self.camera.pos, self.camera.alvo, dt);

        if corrida.fase != Fase::Correndo {
            return;
        }

        // ── raspão: faísca contínua ──────────────────────────
        //
        // O `raspando` do carro é **contato de parede**, não batida: encostar de
        // lado solta faísca sem tocar o som de colisão nem zerar o combo. Antes
        // disso o toque não deixava sinal nenhum, e o jogador não sabia se tinha
        // encostado ou se o carro tinha ficado mais lento por outro motivo.
        let carro = &corrida.carro;
        if carro.raspando && carro.velocidade.abs() > 6.0 {
            // Proporcional à velocidade e ao ângulo: raspão de lado solta pouco,
            // entrada torta solta muito.
            let forca = (carro.velocidade.abs() / 45.0).clamp(0.15, 1.0);
            let quantas = (forca * 3.0) as u32 + 1;
            self.particulas.pedir_faiscas(quantas);
        }

        // ── colisão: destroços ───────────────────────────────
        //
        // Só acima de 0,35: abaixo disso é raspão, e um encostar de 3 m/s não
        // pode jogar a mesma chuva de lataria que uma batida a 200 km/h. O
        // `impacto` já decai sozinho no núcleo, então a rajada sai **uma vez** por
        // batida em vez de todo quadro.
        const LIMIAR_DESTROCO: f32 = 0.35;
        if carro.impacto > LIMIAR_DESTROCO && carro.impacto >= self.ultimo_impacto {
            let (x, z) = girar_ponto(self.rotacao, carro.pos);
            let (dx, dz) = girar_direcao(self.rotacao, carro.direcao());
            self.particulas.pedir_fragmentos(
                carro.impacto,
                COR_JOGADOR,
                Vec3::new(x, carro.altura + 0.6, z),
                Vec3::new(dx, 0.0, dz),
            );
            self.camera.tremor = (self.camera.tremor + 0.5 * carro.impacto).min(1.4);
        }
        self.ultimo_impacto = if carro.impacto < 0.05 {
            0.0
        } else {
            carro.impacto
        };
    }

    /// Quantas partículas vivas (diagnóstico).
    pub fn particulas_vivas(&self) -> usize {
        self.particulas.total()
    }

    /// Destroços vivos (diagnóstico). Zero é resposta legítima: sem batida não
    /// há fragmento, e é isso que distingue "não desenhou" de "não bateu".
    pub fn fragmentos(&self) -> usize {
        self.particulas.fragmentos()
    }

    /// Contagem por tipo, na ordem de [`Tipo::TODOS`].
    pub fn contagem_particulas(&self) -> [u32; 7] {
        *self.particulas.contagens()
    }

    /// As contagens como objeto JSON, para o diagnóstico de `--json`.
    pub fn efeitos_json(&self) -> String {
        let contagem = self.contagem_particulas();
        Tipo::TODOS
            .iter()
            .zip(contagem.iter())
            .map(|(tipo, n)| format!("\"{}\":{n}", tipo.nome()))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Esquece as partículas vivas (reinício de corrida).
    ///
    /// Sem isto a fumaça do drift anterior continua no ar depois do `ENTER`, e o
    /// jogador reinicia dentro da nuvem da corrida que ele acabou de abandonar.
    pub fn limpar_efeitos(&mut self) {
        self.particulas.limpar();
    }

    /// Desenha os destroços: **um** `draw_instanced` para todos.
    ///
    /// Cubos de verdade, com a matriz completa de cada peça (posição, giro em
    /// três eixos, escala). Um billboard sempre encara a câmera, e destroço que
    /// encara quem olha não lê como destroço.
    fn desenhar_fragmentos(
        &mut self,
        render: &mut RenderContext3D,
        camera: &Camera3D,
    ) -> Result<(), Box<dyn Error>> {
        let Some(inst) = self.instanciado else {
            return Ok(());
        };
        if self.particulas.fragmentos() == 0 {
            return Ok(());
        }
        self.particulas
            .matrizes_fragmentos(&mut self.malhas_efeito);
        if self.malhas_efeito.is_empty() {
            return Ok(());
        }
        let cor = self.particulas.cor_fragmentos();
        // Luz de verdade: destroço é objeto sólido, e com luz chapada o cubo
        // vira uma silhueta sem faces — some a leitura de "pedaço girando".
        render.set_light(self.luz_mundo);
        render.draw_instanced(self.cubo, inst, &self.malhas_efeito, camera, cor)?;
        Ok(())
    }

    /// Desenha as partículas: **uma** chamada de desenho por tipo vivo.
    ///
    /// Sete tipos no pior caso, e um tipo extinto não custa nada. A cor sai da
    /// vida restante de cada grupo, então a fumaça desbota sem precisar de um
    /// desenho por partícula.
    fn desenhar_particulas(
        &mut self,
        render: &mut RenderContext3D,
        camera: &Camera3D,
    ) -> Result<(), Box<dyn Error>> {
        if self.particulas.total() == 0 {
            return Ok(());
        }
        // Sem instancing as partículas ficam de fora: o caminho não instanciado
        // desenharia todas empilhadas na origem, e um sprite na origem lê como
        // defeito, não como efeito ausente.
        if self.instanciado.is_none() {
            return Ok(());
        }

        // Billboards: os eixos vêm da câmera, então o quad sempre encara quem
        // olha. Um quad fixo sumiria de perfil.
        let (direita, cima) = efeitos::base_da_camera(camera.position, camera.target);

        // Luz chapada: partícula não recebe sombreamento, senão fumaça branca
        // vira fumaça cinza-escura no meio da pista mal iluminada.
        render.set_light(efeitos::LUZ_CHAPADA);

        for tipo in Tipo::TODOS {
            let total = efeitos::Particulas::faixas(tipo);
            let base = tipo.cor();
            for faixa in 0..total {
                let alfa = self.particulas.alfa_da_faixa(tipo, faixa, total);
                if alfa <= 0.004 {
                    continue;
                }
                self.particulas.matrizes(
                    tipo,
                    faixa,
                    total,
                    direita,
                    cima,
                    &mut self.malhas_efeito,
                );
                if self.malhas_efeito.is_empty() {
                    continue;
                }
                let cor = Color3D::new(base.r, base.g, base.b, alfa);
                // Sprite texturizado: a forma vem do atlas, a cor do tint. Antes
                // disto toda partícula era um quadrado sólido — que num painel de
                // 3,5" lê como "quadrado", não como fumaça.
                let malha = self
                    .sprites
                    .get(tipo.forma().indice())
                    .copied()
                    .unwrap_or(self.quad);
                render.draw_instanced_textured(
                    malha,
                    self.sprite_shader,
                    &self.malhas_efeito,
                    camera,
                    self.atlas_t,
                    cor,
                )?;
            }
        }

        render.set_light(self.luz_mundo);
        Ok(())
    }

    // ────────────────────────────────────────────────────────
    // HUD
    // ────────────────────────────────────────────────────────

    fn desenhar_hud(
        &mut self,
        render: &mut RenderContext3D,
        corrida: &Corrida,
    ) -> Result<(), Box<dyn Error>> {
        let Some(inst) = self.instanciado else {
            return Ok(());
        };
        let tela = render.drawable_size();
        // A rotação troca as medidas do HUD: o que se posiciona é a "tela de
        // projeto", que é paisagem nos dois casos.
        let projeto = match self.rotacao {
            Rotacao::Nenhuma => tela,
            _ => (tela.1, tela.0),
        };
        self.largura_projeto = projeto.0 as f32;
        self.altura_projeto = projeto.1 as f32;
        self.painel = tela;

        // O HUD não é iluminado: com `ambient = 1` e luz direta zero, a cor sai
        // exatamente como pedida. Sem isso o texto sairia 47 % mais escuro e
        // ninguém saberia por quê.
        render.set_light(DirectionalLight {
            direction: Vec3::new(0.0, 0.0, -1.0),
            color: Color3D::BLACK,
            ambient: 1.0,
            specular: 0.0,
            shininess: 1.0,
            _reserved: [0; 4],
        });
        // A câmera do HUD é montada no **painel**, não no projeto: ela descreve
        // o alvo real de desenho, e a rotação entra na matriz de cada elemento
        // (`hud_matriz`). Montá-la no projeto foi o defeito que deixava o texto
        // esticado e deitado no X55.
        let camera = camara_hud(tela);
        let largura = projeto.0 as f32;
        let altura = projeto.1 as f32;

        // ── véu das telas de texto ──────────────────────────────
        // **Antes** do texto e **depois** do mundo: é o que separa as duas
        // camadas sem esconder a cidade. Tem de ser `tela_cheia` e não
        // `retangulo(0, 0, largura, altura)`: pela `hud_matriz` esse retângulo
        // cobre só o quarto central da tela (ver `hud_matriz_tela`).
        if self.veu > 0.002 {
            self.tela_cheia(
                render,
                inst,
                &camera,
                Color3D::new(0.0, 0.0, 0.0, self.veu),
            )?;
        }

        match corrida.fase {
            Fase::Titulo => self.hud_titulo(render, inst, &camera, largura, altura, corrida)?,
            Fase::Correndo => self.hud_corrida(render, inst, &camera, corrida, largura, altura)?,
            Fase::Resultado => self.hud_resultado(render, inst, &camera, corrida, largura, altura)?,
        }

        // Linhas de velocidade por cima de tudo: é o "rastro" do nitro.
        self.hud_linhas_de_velocidade(render, inst, &camera, corrida, largura, altura)?;
        self.hud_raios_de_velocidade(render, inst, &camera, corrida, largura, altura)?;
        self.hud_flash_de_nitro(render, inst, &camera, corrida)?;
        self.hud_perfil(render, inst, &camera, largura, altura)?;

        render.set_light(self.luz_mundo);
        Ok(())
    }

    /// Overlay F1 e o rodapé de LAN/menu, ambos em 3D (ADR-037).
    fn hud_perfil(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        largura: f32,
        altura: f32,
    ) -> Result<(), Box<dyn Error>> {
        let escala = (largura / 420.0).clamp(1.2, 2.2);
        let mut y = 8.0;
        let linhas = self.perfil.clone();
        for linha in linhas {
            self.texto(render, inst, camera, &linha, 8.0, y, escala, NEON_CIANO)?;
            y += escala * 10.0;
        }
        if let Some(aviso) = self.aviso_rede.clone() {
            let escala_aviso = (largura / 560.0).clamp(1.1, 1.8);
            let largura_texto = fonte::largura_texto(&aviso) as f32 * escala_aviso;
            let x = (largura - largura_texto - 8.0).max(8.0);
            self.texto(
                render,
                inst,
                camera,
                &aviso,
                x,
                altura - escala_aviso * 10.0,
                escala_aviso,
                NEON_MAGENTA,
            )?;
        }
        Ok(())
    }

    /// Tela de título: o menu, e o cartão de controles.
    ///
    /// Desenha as linhas que o `menu.rs` montou. A cena não decide nada: se a
    /// linha é o mapa ou a qualidade, se está selecionada, o que o A faz nela —
    /// tudo isso chega pronto em `self.menu_linhas`, e a única coisa que a cena
    /// sabe é que a linha marcada vai em cor quente e maior.
    ///
    /// Sem menu montado (diagnóstico `--so`, ou um `--quadro` sem laço) cai no
    /// cartão fixo com mapa e dificuldade, que é o que existia antes — um
    /// título em branco mentiria sobre o estado do jogo.
    fn hud_titulo(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        largura: f32,
        altura: f32,
        corrida: &Corrida,
    ) -> Result<(), Box<dyn Error>> {
        let escala = (largura / 200.0).clamp(2.0, 5.0);
        let titulo = fonte::largura_texto(NOME) as f32 * escala;
        self.texto(
            render,
            inst,
            camera,
            NOME,
            (largura - titulo) * 0.5,
            altura * 0.10,
            escala,
            NEON_CIANO,
        )?;

        let sub = fonte::largura_texto("CORRIDA NOTURNA ARCADE") as f32 * (escala * 0.45);
        self.texto(
            render,
            inst,
            camera,
            "CORRIDA NOTURNA ARCADE",
            (largura - sub) * 0.5,
            altura * 0.10 + escala * 9.0,
            escala * 0.45,
            NEON_MAGENTA,
        )?;

        // O pulso do cursor é o mesmo truque do neon da cidade: custa um
        // uniforme e é o que diz "esta linha é a sua".
        let pulso = 0.72 + 0.28 * (self.tempo * 5.0).sin();
        let linha_altura = (escala * 0.62).clamp(1.6, 3.4);
        let mut y = altura * 0.26;

        if self.menu_linhas.is_empty() {
            // Cartão fixo: sem menu, o estado tem de continuar legível.
            for (texto, cor) in [
                (
                    format!("MAPA  {}", corrida.mapa.nome.to_uppercase()),
                    NEON_CIANO,
                ),
                (
                    corrida.dificuldade.nome().to_uppercase(),
                    Color3D::new(0.95, 0.72, 1.0, 1.0),
                ),
            ] {
                let w = fonte::largura_texto(&texto) as f32 * linha_altura;
                self.texto(
                    render,
                    inst,
                    camera,
                    &texto,
                    (largura - w) * 0.5,
                    y,
                    linha_altura,
                    cor,
                )?;
                y += linha_altura * 12.0;
            }
        } else {
            // `mem::take` porque `self.texto` precisa de `&mut self` e as linhas
            // moram em `self`: emprestar o vetor enquanto se desenha é o tipo de
            // conflito que o compilador resolve e o programador não deve.
            let linhas = std::mem::take(&mut self.menu_linhas);
            for (texto, selecionada) in &linhas {
                let cor = if *selecionada {
                    Color3D::new(1.0, 0.94, 0.42, pulso)
                } else {
                    Color3D::new(0.74, 0.80, 0.92, 1.0)
                };
                let w = fonte::largura_texto(texto) as f32 * linha_altura;
                self.texto(
                    render,
                    inst,
                    camera,
                    texto,
                    (largura - w) * 0.5,
                    y,
                    linha_altura,
                    cor,
                )?;
                y += linha_altura * 12.0;
            }
            self.menu_linhas = linhas;
            let dica = std::mem::take(&mut self.menu_dica);
            if !dica.is_empty() {
                let dica_escala = linha_altura * 0.82;
                let w = fonte::largura_texto(&dica) as f32 * dica_escala;
                self.texto(
                    render,
                    inst,
                    camera,
                    &dica,
                    (largura - w) * 0.5,
                    y + linha_altura * 2.0,
                    dica_escala,
                    Color3D::new(0.55, 1.0, 0.75, 1.0),
                )?;
            }
            self.menu_dica = dica;
        }

        // Cartão de controles: **controle primeiro**, teclado depois. Este é um
        // jogo de handheld, e a legenda tem de funcionar com o que o aparelho
        // tem. As teclas continuam valendo, mas não são o caminho principal.
        let controles = [
            "ANALOGICO/DIRECIONAL VIRAR   A OU GATILHO ACELERAR",
            "B OU GATILHO FREAR   L1/R1 DERRAPAR   L2/R2 NITRO",
            "X OVERLAY   START JOGAR   Y VOLTAR",
        ];
        let escala_controles = linha_altura * 0.72;
        let mut y = altura * 0.78;
        for texto in controles {
            let w = fonte::largura_texto(texto) as f32 * escala_controles;
            self.texto(
                render,
                inst,
                camera,
                texto,
                (largura - w) * 0.5,
                y,
                escala_controles,
                Color3D::new(0.62, 0.68, 0.80, 1.0),
            )?;
            y += escala_controles * 10.0;
        }
        Ok(())
    }

    fn hud_corrida(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        corrida: &Corrida,
        largura: f32,
        altura: f32,
    ) -> Result<(), Box<dyn Error>> {
        let escala = (largura / 220.0).clamp(1.8, 4.2);

        // ── topo esquerdo: volta e posição ────────────────────────
        self.texto(
            render,
            inst,
            camera,
            &hud::volta(corrida.volta, corrida.total_voltas),
            largura * 0.03,
            altura * 0.04,
            escala,
            Color3D::new(0.85, 0.92, 1.0, 1.0),
        )?;
        self.texto(
            render,
            inst,
            camera,
            &hud::posicao(corrida.posicao, corrida.adversarios.len() + 1),
            largura * 0.03,
            altura * 0.04 + escala * 10.0,
            escala,
            if corrida.posicao == 1 { NEON_CIANO } else { NEON_MAGENTA },
        )?;

        // ── topo direito: tempo ──────────────────────────────────
        let tempo = hud::tempo(corrida.tempo);
        let w = fonte::largura_texto(&tempo) as f32 * escala;
        self.texto(
            render,
            inst,
            camera,
            &tempo,
            largura - w - largura * 0.03,
            altura * 0.04,
            escala,
            Color3D::new(0.85, 0.92, 1.0, 1.0),
        )?;

        // ── centro: drift ─────────────────────────────────────────
        if corrida.drift.ativo && corrida.drift.total() > 0 {
            let escala_drift = escala * 1.25;
            let texto = "DRIFT";
            let w = fonte::largura_texto(texto) as f32 * escala_drift;
            let pulso = 0.75 + 0.25 * (self.tempo * 12.0).sin();
            self.texto(
                render,
                inst,
                camera,
                texto,
                (largura - w) * 0.5,
                altura * 0.14,
                escala_drift,
                Color3D::new(NEON_MAGENTA.r, NEON_MAGENTA.g, NEON_MAGENTA.b, pulso),
            )?;
            let pontos = hud::pontos(corrida.drift.total());
            let wp = fonte::largura_texto(&pontos) as f32 * escala;
            self.texto(
                render,
                inst,
                camera,
                &pontos,
                (largura - wp) * 0.5,
                altura * 0.14 + escala_drift * 10.0,
                escala,
                Color3D::new(1.0, 0.86, 0.35, 1.0),
            )?;
            if corrida.drift.combo_inteiro() > 1 {
                let combo = hud::combo(corrida.drift.combo_inteiro());
                let wc = fonte::largura_texto(&combo) as f32 * escala;
                self.texto(
                    render,
                    inst,
                    camera,
                    &combo,
                    (largura - wc) * 0.5,
                    altura * 0.14 + escala_drift * 10.0 + escala * 9.0,
                    escala,
                    NEON_CIANO,
                )?;
            }
        }

        // ── rodapé esquerdo: velocímetro, marcha e nitro ──────────
        //
        // A geometria vem de `hud::rodape`, no núcleo, e **não** é recalculada
        // aqui. A versão anterior montava a pilha com deslocamentos fixos
        // (`base_y - 2·escala`) para um glifo de `7·escala` de altura, e em
        // 640×480 os três elementos caíram na mesma faixa de 40 px — o defeito
        // que o usuário viu como "o velocímetro em cima do nitro". Com as
        // caixas no núcleo, `os_elementos_do_rodape_nao_se_sobrepoem` afirma a
        // separação em quatro telas, coisa que olhar a tela não faz.
        let branco = Color3D::new(1.0, 1.0, 1.0, 1.0);
        let cinza = Color3D::new(0.70, 0.76, 0.90, 1.0);

        let velocidade = hud::velocidade(corrida.carro.kmh());
        let marcha = format!("{}", corrida.carro.marcha());
        let r = hud::rodape(largura, altura, &velocidade, &marcha);

        self.texto(
            render,
            inst,
            camera,
            &velocidade,
            r.velocidade.x,
            r.velocidade.y,
            r.escala_velocidade,
            branco,
        )?;
        self.texto(
            render,
            inst,
            camera,
            "KM/H",
            r.unidade.x,
            r.unidade.y,
            r.escala_unidade,
            cinza,
        )?;
        self.texto(
            render,
            inst,
            camera,
            &marcha,
            r.marcha.x,
            r.marcha.y,
            r.escala_marcha,
            NEON_CIANO,
        )?;

        // Barra de nitro: fundo escuro + preenchimento ciano.
        self.retangulo(
            render,
            inst,
            camera,
            r.barra.x,
            r.barra.y,
            r.barra.largura,
            r.barra.altura,
            Color3D::new(0.10, 0.12, 0.20, 0.92),
        )?;
        let cheio = r.barra.largura * corrida.carro.nitro_fracao();
        if cheio > 1.0 {
            self.retangulo(
                render,
                inst,
                camera,
                r.barra.x,
                r.barra.y,
                cheio,
                r.barra.altura,
                NEON_CIANO,
            )?;
        }
        self.texto(
            render,
            inst,
            camera,
            "NITRO",
            r.rotulo.x,
            r.rotulo.y,
            r.escala_rotulo,
            cinza,
        )?;

        // ── contagem regressiva ───────────────────────────────────
        if corrida.contagem > 0.0 {
            let numero = format!("{}", corrida.contagem.ceil().clamp(1.0, 3.0) as u32);
            let escala_conta = escala * 4.0;
            let w = fonte::largura_texto(&numero) as f32 * escala_conta;
            self.texto(
                render,
                inst,
                camera,
                &numero,
                (largura - w) * 0.5,
                altura * 0.36,
                escala_conta,
                Color3D::new(1.0, 0.9, 0.3, 1.0),
            )?;
        }

        Ok(())
    }

    fn hud_resultado(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        corrida: &Corrida,
        largura: f32,
        altura: f32,
    ) -> Result<(), Box<dyn Error>> {
        let escala = (largura / 200.0).clamp(2.0, 5.0);

        let (titulo, cor) = match corrida.desfecho {
            Some(giochi_neon_drift_core::Desfecho::Vitoria) => ("VITORIA", NEON_CIANO),
            Some(giochi_neon_drift_core::Desfecho::ChegouDepois) => ("DERROTA", NEON_MAGENTA),
            Some(giochi_neon_drift_core::Desfecho::TempoEsgotado) => ("TEMPO ESGOTADO", NEON_MAGENTA),
            None => ("FIM", NEON_CIANO),
        };
        let w = fonte::largura_texto(titulo) as f32 * escala;
        self.texto(
            render,
            inst,
            camera,
            titulo,
            (largura - w) * 0.5,
            altura * 0.16,
            escala,
            cor,
        )?;

        let linhas = [
            format!("POSICAO {}", corrida.posicao),
            format!("TEMPO {}", hud::tempo(corrida.tempo)),
            format!("DRIFT {}", hud::pontos(corrida.drift.pontos)),
        ];
        let escala_corpo = escala * 0.7;
        let mut y = altura * 0.42;
        for linha in &linhas {
            let wl = fonte::largura_texto(linha) as f32 * escala_corpo;
            self.texto(
                render,
                inst,
                camera,
                linha,
                (largura - wl) * 0.5,
                y,
                escala_corpo,
                Color3D::new(0.88, 0.92, 1.0, 1.0),
            )?;
            y += escala_corpo * 12.0;
        }

        let pisca = 0.55 + 0.45 * (self.tempo * 4.0).sin();
        let dica = "A OU START  REINICIAR";
        let wd = fonte::largura_texto(dica) as f32 * escala_corpo;
        self.texto(
            render,
            inst,
            camera,
            dica,
            (largura - wd) * 0.5,
            altura * 0.82,
            escala_corpo,
            Color3D::new(1.0, 0.95, 0.45, pisca),
        )?;
        // A segunda saída, menor: reiniciar é o que o dedo procura, voltar ao
        // menu é o que ele precisa quando o resultado foi ruim. Sem esta linha,
        // o único caminho de volta seria matar o jogo.
        let sair = "Y  VOLTAR AO MENU";
        let ws = fonte::largura_texto(sair) as f32 * (escala_corpo * 0.8);
        self.texto(
            render,
            inst,
            camera,
            sair,
            (largura - ws) * 0.5,
            altura * 0.90,
            escala_corpo * 0.8,
            Color3D::new(0.70, 0.76, 0.88, 1.0),
        )?;
        Ok(())
    }

    /// Linhas de velocidade nas laterais, mais longas com nitro.
    fn hud_linhas_de_velocidade(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        corrida: &Corrida,
        largura: f32,
        altura: f32,
    ) -> Result<(), Box<dyn Error>> {
        if corrida.fase != Fase::Correndo {
            return Ok(());
        }
        let carro = &corrida.carro;
        let rapidez = (carro.velocidade / 62.0).clamp(0.0, 1.4);
        let nitro = carro.nitro_usando;
        let intensidade = (rapidez * 0.55 + if nitro { 0.75 } else { 0.0 }).clamp(0.0, 1.0);
        if intensidade < 0.12 {
            return Ok(());
        }

        self.instancias.clear();
        let quantas = (10.0 + intensidade * 22.0) as u32;
        for i in 0..quantas {
            let semente = (i as f32 * 12.9898 + self.tempo * 3.1).sin() * 43758.545;
            let aleatorio = semente - semente.floor();
            let lado = if i % 2 == 0 { 0.0 } else { 1.0 };
            let y = (aleatorio * altura * 0.9) + altura * 0.05;
            let comprimento = (largura * 0.05) * (0.5 + intensidade) * (0.4 + aleatorio);
            let x = if lado < 0.5 {
                largura * 0.02 + aleatorio * largura * 0.06
            } else {
                largura * 0.92 + aleatorio * largura * 0.06
            };
            self.instancias
                .push(hud_matriz(self.tela_hud(), x, y, comprimento, 1.5 + intensidade * 1.5));
        }
        let cor = Color3D::new(0.75, 0.92, 1.0, 0.10 + intensidade * 0.22);
        render.draw_instanced(self.quad, inst, &self.instancias, camera, cor)?;
        Ok(())
    }


    /// Raios de velocidade saindo do ponto de fuga.
    ///
    /// É o "motion blur" barato: em vez de borrar a imagem (que exige
    /// render-to-texture e um passe de leitura do quadro anterior), o mesmo efeito
    /// de leitura vem de riscos longos que apontam para fora do centro, no ponto
    /// de fuga da pista. O olho lê direção e comprimento como movimento, e custa
    /// **uma** chamada de desenho.
    fn hud_raios_de_velocidade(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        corrida: &Corrida,
        largura: f32,
        altura: f32,
    ) -> Result<(), Box<dyn Error>> {
        if corrida.fase != Fase::Correndo {
            return Ok(());
        }
        let carro = &corrida.carro;
        let rapidez = (carro.velocidade / 62.0).clamp(0.0, 1.4);
        let nitro = if carro.nitro_usando { 0.55 } else { 0.0 };
        let intensidade = (rapidez * 0.45 + nitro).clamp(0.0, 1.0);
        if intensidade < 0.18 {
            return Ok(());
        }

        let (cx, cy) = (largura * 0.5, altura * 0.46);
        // Fim do risco proporcional ao tamanho da tela, para o efeito ser o mesmo
        // em 640×480 e em 1280×720.
        let alcance = (largura * largura + altura * altura).sqrt() * 0.5;
        self.instancias.clear();
        let quantos = (14.0 + intensidade * 40.0) as u32;
        for i in 0..quantos {
            let semente = (i as f32 * 78.233 + self.tempo * 2.7).sin() * 43758.545;
            let a = semente - semente.floor();
            let angulo = a * std::f32::consts::TAU + i as f32 * 0.37;
            let inicio = alcance * (0.34 + 0.30 * a);
            let comprimento = alcance * (0.10 + 0.34 * intensidade) * (0.4 + a);
            let (s, c) = angulo.sin_cos();
            // O centro do quad fica no meio do segmento; o eixo longo aponta
            // radialmente para fora.
            let x = cx + c * (inicio + comprimento * 0.5);
            let y = cy + s * (inicio + comprimento * 0.5);
            self.instancias.push(hud_matriz_girada(
                self.tela_hud(),
                x,
                y,
                angulo,
                comprimento,
                1.6 + intensidade * 2.2,
            ));
        }
        let cor = Color3D::new(0.80, 0.94, 1.0, 0.06 + intensidade * 0.16);
        render.draw_instanced(self.quad, inst, &self.instancias, camera, cor)?;
        Ok(())
    }

    /// Clarão frio de tela cheia enquanto o nitro está aberto.
    ///
    /// Um quad do tamanho da tela com alfa baixo: o suficiente para o quadro
    /// "estourar" no acionamento. Barato (uma chamada) e é o que separa "usou o
    /// nitro" de "usou o nitro e o jogo respondeu".
    fn hud_flash_de_nitro(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        corrida: &Corrida,
    ) -> Result<(), Box<dyn Error>> {
        if corrida.fase != Fase::Correndo || !corrida.carro.nitro_usando {
            return Ok(());
        }
        let pulso = 0.55 + 0.45 * (self.tempo * 18.0).sin();
        self.tela_cheia(
            render,
            inst,
            camera,
            Color3D::new(0.35, 0.75, 1.0, 0.05 + 0.05 * pulso),
        )?;
        Ok(())
    }

    /// Desenha a **prova de rotação**: três barras de cor sobre fundo preto.
    ///
    /// ## Por que existe
    ///
    /// "O jogo está de ponta a cabeça" e "as informações estão na vertical" são
    /// relatos que não dizem **qual** peça girou errado: o mundo, o HUD, ou os
    /// dois em sentidos opostos. Olhar a tela não separa os casos, e comparar o
    /// aparelho com o PC não ajuda quando os dois erram igual (AGENTS.md §24).
    ///
    /// A sonda elimina a dúvida por construção: três retângulos em posições
    /// conhecidas do **projeto** (vermelho no topo, verde à esquerda, azul no
    /// canto superior esquerdo) e uma predição numérica de onde cada um tem de
    /// cair no **painel**. Quem medir o BMP confere a predição ou não.
    ///
    /// A predição para `Ccw90` num painel 720×1280 com projeto 1280×720:
    ///
    /// ```text
    /// vermelho (0,0,1280,24)  -> (0, 0, 24, 1280)     borda ESQUERDA, vertical
    /// verde    (0,0,24,720)   -> (0, 1256, 720, 24)   borda INFERIOR, horizontal
    /// azul     (40,40,64,64)  -> (40, 1176, 64, 64)   perto do canto inferior esquerdo
    /// ```
    pub fn desenhar_prova_rotacao(
        &mut self,
        render: &mut RenderContext3D,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(inst) = self.instanciado else {
            return Err("sem shader instanciado: a prova de rotação não roda neste driver".into());
        };
        let tela = render.drawable_size();
        let projeto = match self.rotacao {
            Rotacao::Nenhuma => tela,
            _ => (tela.1, tela.0),
        };
        self.painel = tela;
        self.largura_projeto = projeto.0 as f32;
        self.altura_projeto = projeto.1 as f32;
        let (l, a) = (projeto.0 as f32, projeto.1 as f32);
        let hud = self.tela_hud();
        let camera = camara_hud(tela);

        render.set_light(DirectionalLight {
            direction: Vec3::new(0.0, 0.0, -1.0),
            color: Color3D::BLACK,
            ambient: 1.0,
            specular: 0.0,
            shininess: 1.0,
            _reserved: [0; 4],
        });

        for (rect, cor) in [
            ((0.0, 0.0, l, 24.0), Color3D::new(1.0, 0.0, 0.0, 1.0)),
            ((0.0, 0.0, 24.0, a), Color3D::new(0.0, 1.0, 0.0, 1.0)),
            ((40.0, 40.0, 64.0, 64.0), Color3D::new(0.0, 0.0, 1.0, 1.0)),
        ] {
            self.instancias.clear();
            self.instancias
                .push(hud_matriz(hud, rect.0, rect.1, rect.2, rect.3));
            render.draw_instanced(self.quad, inst, &self.instancias, &camera, cor)?;
        }
        eprintln!(
            "neon-drift: prova de rotação — {} ({:?}), projeto {}x{}, painel {}x{}",
            self.rotacao.nome(),
            self.rotacao,
            projeto.0,
            projeto.1,
            tela.0,
            tela.1
        );
        Ok(())
    }

    /// Desenha um retângulo em pixels de projeto.
    fn retangulo(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        cor: Color3D,
    ) -> Result<(), Box<dyn Error>> {
        self.instancias.clear();
        // A tela do HUD é o projeto: `hud_matriz` recebe as medidas em pixels.
        self.instancias.push(hud_matriz(
            self.tela_hud(),
            x,
            y,
            w,
            h,
        ));
        render.draw_instanced(self.quad, inst, &self.instancias, camera, cor)?;
        Ok(())
    }

    /// Preenche a **tela inteira** — o único caminho para véu e flash.
    ///
    /// Não use `retangulo(0, 0, largura, altura)` para isto: pela `hud_matriz`
    /// esse retângulo cobre só o quarto central (ver `hud_matriz_tela`).
    fn tela_cheia(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        cor: Color3D,
    ) -> Result<(), Box<dyn Error>> {
        self.instancias.clear();
        self.instancias.push(hud_matriz_tela(self.tela_hud()));
        render.draw_instanced(self.quad, inst, &self.instancias, camera, cor)?;
        Ok(())
    }

    /// Desenha um texto de 5×7 pontos em pixels de projeto.
    #[allow(clippy::too_many_arguments)]
    fn texto(
        &mut self,
        render: &mut RenderContext3D,
        inst: ShaderHandle,
        camera: &Camera3D,
        texto: &str,
        x: f32,
        y: f32,
        escala: f32,
        cor: Color3D,
    ) -> Result<(), Box<dyn Error>> {
        // Normaliza **aqui**, e não em cada chamador: a fonte não tem acento nem
        // minúscula, e um texto que escape da normalização vira uma palavra com
        // uma letra faltando — foi o que a dica de dificuldade fez ao chegar com
        // "fábrica". O `para_ascii` troca caractere por caractere, então a
        // medida que o chamador fez com `largura_texto` continua valendo.
        let texto = fonte::para_ascii(texto);
        let pontos = fonte::pontos(&texto);
        if pontos.is_empty() {
            return Ok(());
        }
        self.instancias.clear();
        let tela = self.tela_hud();
        // 1.06 de sobreposição: sem ela, cada ponto fica com uma fresta de fundo e
        // o texto parece serrilhado a partir de longe.
        let ponto = escala * 1.06;
        for [px, py] in pontos {
            self.instancias.push(hud_matriz(
                tela,
                x + px as f32 * escala,
                y + py as f32 * escala,
                ponto,
                ponto,
            ));
        }
        render.draw_instanced(self.quad, inst, &self.instancias, camera, cor)?;
        Ok(())
    }
}

// ════════════════════════════════════════════════════════════
// CONSTRUÇÃO DO MUNDO
// ════════════════════════════════════════════════════════════

/// Geometria estática separada por cor (uma malha por cor).
#[derive(Default)]
struct Mundo {
    /// Asfalto texturizado (sem cor por malha).
    asfalto_t: Construtor,
    /// Concreto texturizado: mureta da avenida, parede do túnel, prédios baixos.
    concreto_t: Construtor,
    /// Pedra texturizada: o paredão da serra.
    pedra_t: Construtor,
    /// Mata: as copas dos dois lados da estrada de serra.
    mata_t: Construtor,
    /// Guarda-corpo metálico da descida.
    guarda_t: Construtor,
    /// Xadrez da linha de largada.
    xadrez_t: Construtor,
    /// Outdoors e painéis de neon.
    painel_t: Construtor,
    /// Faixa central da pista (tracejado na textura, não na geometria).
    faixas_t: Construtor,
    /// Fitas de advertência das barreiras (grade de neon).
    grade_t: Construtor,
    asfalto: Construtor,
    guia_a: Construtor,
    guia_b: Construtor,
    muros: Construtor,
    neon: Construtor,
    neon_b: Construtor,
    reflexos: Construtor,
    poco_luz: Construtor,
    telhados: Construtor,
    longe: Construtor,
    morros: Construtor,
    predios_a: Construtor,
    predios_b: Construtor,
    predios_c: Construtor,
    janelas: Construtor,
    tunel: Construtor,
    tunel_luzes: Construtor,
    tunel_luzes_b: Construtor,
    largada_b: Construtor,
    /// Instâncias estáticas: postes de rua, refletores, cones e portais.
    ///
    /// Ficam fora das malhas assadas porque `draw_instanced` desenha **uma
    /// geometria** N vezes com uma chamada: é exatamente o caso de 20 postes
    /// iguais. Assar cada um na malha do mundo trocaria uma chamada por 20.
    postes_inst: Vec<Mat4>,
    luzes_inst: Vec<Mat4>,
    cones_inst: Vec<Mat4>,
    portais_inst: Vec<Mat4>,
}

impl Mundo {
    /// Quantas malhas estáticas não vazias o mundo produziu.
    fn contagem_estaticas(&self) -> usize {
        let texturizadas = [
            &self.asfalto_t,
            &self.concreto_t,
            &self.pedra_t,
            &self.mata_t,
            &self.guarda_t,
            &self.xadrez_t,
            &self.painel_t,
            &self.faixas_t,
            &self.grade_t,
        ];
        let chapadas = [
            &self.asfalto,
            &self.guia_a,
            &self.guia_b,
            &self.muros,
            &self.neon,
            &self.neon_b,
            &self.reflexos,
            &self.poco_luz,
            &self.telhados,
            &self.longe,
            &self.morros,
            &self.predios_a,
            &self.predios_b,
            &self.predios_c,
            &self.janelas,
            &self.tunel,
            &self.tunel_luzes,
            &self.tunel_luzes_b,
            &self.largada_b,
        ];
        texturizadas
            .iter()
            .chain(chapadas.iter())
            .filter(|c| !c.vazio())
            .count()
    }
}

fn criar(render: &mut RenderContext3D, c: &Construtor) -> Result<MeshHandle, Box<dyn Error>> {
    // Malha vazia não vai para a GPU: `create_mesh` recusaria, e o erro
    // apareceria como falha de abertura do jogo.
    if c.vazio() {
        let (v, i) = malha::quad();
        return Ok(render.create_mesh(&v, &i)?);
    }
    // Todas as peças de um material já vêm concatenadas pelo `Construtor` do
    // jogo; o `StaticBatchBuilder` da engine é quem compõe o modelo final. O
    // ganho visível é o mesmo de antes (uma malha por material), mas agora a
    // transformação de normal e a checagem de índice são as da engine — e o
    // `StaticBatch` registra o resultado no mesmo `mesh_count`/`release_all`
    // que todas as outras malhas.
    let builder = StaticBatchBuilder::with_capacity(c.vertices().len(), c.indices().len())
        .add_mesh(c.vertices(), c.indices(), &Mat4::IDENTITY)?;
    let batch = StaticBatch::build(render, builder)?;
    Ok(batch.mesh())
}

/// Gira um ponto do mundo para o espaço de projeto, **espelhando X**.
///
/// ## O espelhamento não é gosto: é a convenção da câmera
///
/// `Mat4::look_at` é *right-handed* e monta a base com `s = cross(f, up)`. Com a
/// câmera atrás do carro olhando para **+Z** (a direção em que ele anda, ver
/// `direcao()` no núcleo), isso dá:
///
/// ```text
/// f = (0,0,1), up = (0,1,0)  ->  s = cross(f, up) = (-1, 0, 0)
/// ```
///
/// Ou seja, o eixo X do mundo aponta para a **esquerda** na tela. Medido, não
/// deduzido — um ponto em `+X` a 10 m à frente projeta em NDC x = **-1,687**
/// (esquerda), e `-X` em **+1,687** (direita).
///
/// A consequência prática é o relato do dono: *"a esquerda e a direita estão
/// invertidos"*. O volante estava certo (aperta direita → `virar = +1` → `θ`
/// cresce → o carro vai para `+X`), mas `+X` é a esquerda da tela.
///
/// ## Por que consertar aqui, e não no volante
///
/// Três candidatos foram descartados com medida:
///
/// | Onde | Por que não |
/// | --- | --- |
/// | inverter `virar` em `entrada_jogo::combinar` | o piloto automático **não** passa por ali (`virar: erro * 3.2` direto no núcleo), então o jogador e o piloto passariam a discordar |
/// | inverter o sinal do `angulo` no núcleo | muda a física e obriga a inverter o piloto junto: o mesmo defeito espalhado por dois arquivos |
/// | **espelhar X na conversão mundo→tela** | **um lugar só**, e a física, a pista e o piloto ficam intactos |
///
/// O espelhamento é um reflexo do mundo inteiro (pista, cenário, carros,
/// partículas), então nada fica inconsistente: só a mão muda. E como o HUD não
/// passa por aqui, ele não é afetado.
fn girar_ponto(rot: Rotacao, p: Ponto) -> (f32, f32) {
    let _ = rot;
    // O giro **do painel** é feito pelo `roll` da câmera; aqui só entra o
    // espelhamento, que vale em qualquer orientação.
    (-p.x, p.z)
}

/// Matriz de modelo do carro, com a **frente apontando para onde ele anda**.
///
/// ## O `+ PI` não é decoração
///
/// Duas convenções coexistem neste jogo e discordavam em 180°:
///
/// | Onde | Diz |
/// | --- | --- |
/// | `core/carro.rs` — `direcao()` | `(sin θ, cos θ)` — com `θ = 0` o carro **anda para +Z** |
/// | `ui-sdl/malha.rs` — `carro()` | faróis em `z = -2,14` — a **frente desenhada é -Z** |
///
/// Girar o modelo por `θ` leva o vetor `(0,0,-1)` para `(-sin θ, -cos θ)`, que é
/// **menos** a `direcao()`. O carro desenhado olhava para o lado oposto ao que
/// andava, e como a câmera fica atrás do movimento o jogador via os faróis na
/// cara — o "os carros estão correndo de ré" relatado em 09/11/2026.
///
/// Girar por `θ + π` leva `(0,0,-1)` para `(sin θ, cos θ)`, exatamente a
/// `direcao()`. Foi a correção cirúrgica: uma constante, sem tocar na física nem
/// em nenhum dos `z` da malha.
///
/// `o_carro_desenhado_aponta_para_onde_ele_anda` afirma a regra comparando o
/// centro dos faróis com `direcao()`. Quem inverter a malha no futuro quebra
/// esse teste, e é assim que se sabe — antes desta rodada nenhuma das duas
/// convenções estava afirmada, e o defeito passou por três sessões.
/// Matriz de uma instância estática: escala → giro em Y → translação.
///
/// A ordem é a mesma de `Transform3D` da engine (ADR: escala, rotação,
/// translação). Usar a mesma composição evita a peça sair no lugar certo no
/// desenho individual e errado no instanciado.
fn instancia(centro: [f32; 3], tamanho: [f32; 3], giro: f32) -> Mat4 {
    Transform3D::new(
        Vec3::new(centro[0], centro[1], centro[2]),
        Quat::from_axis_angle(Vec3::UP, giro),
        Vec3::new(tamanho[0], tamanho[1], tamanho[2]),
    )
    .matrix()
}

#[allow(dead_code)]
fn matriz_de_carro(rot: Rotacao, pos: Ponto, angulo: f32, altura: f32) -> Mat4 {
    matriz_de_carro_inclinado(rot, pos, angulo, altura, 0.0)
}

/// A mesma matriz, com o carro **empinado** no ar.
///
/// `inclinacao` é o ângulo em torno do eixo lateral, positivo com o nariz para
/// cima. Em voo o carro tem de acompanhar a própria velocidade vertical: deitado
/// ele parece flutuar, e num salto de 6 m isso é a diferença entre "pulou" e
/// "está deslizando no ar".
#[allow(dead_code)]
fn matriz_de_carro_inclinado(
    rot: Rotacao,
    pos: Ponto,
    angulo: f32,
    altura: f32,
    inclinacao: f32,
) -> Mat4 {
    transform_de_carro(rot, pos, angulo, altura, inclinacao).matrix()
}

/// A mesma composição da matriz, como `Transform3D`.
///
/// Existe porque o Phong é desenhado por `draw_mesh_colored`, que recebe
/// `Transform3D`, e o resto do mundo (instâncias, HUD) trabalha com `Mat4`. As
/// duas saem da mesma função para não divergirem: um carro desenhado pelo
/// caminho instanciado e outro pelo Phong tinham de cair no mesmo lugar.
fn transform_de_carro(
    rot: Rotacao,
    pos: Ponto,
    angulo: f32,
    altura: f32,
    inclinacao: f32,
) -> Transform3D {
    let (x, z) = girar_ponto(rot, pos);
    let angulo = girar_angulo(rot, angulo) + std::f32::consts::PI;
    // Yaw ∘ pitch: o `+ PI` alinha a frente desenhada com `direcao()` (ver
    // `matriz_de_carro`), e o pitch vem depois, no referencial do carro.
    let giro = Quat::from_axis_angle(Vec3::UP, angulo)
        * Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), inclinacao.clamp(-0.5, 0.5));
    Transform3D::new(Vec3::new(x, altura, z), giro, Vec3::new(1.0, 1.0, 1.0))
}

fn girar_direcao(rot: Rotacao, p: Ponto) -> (f32, f32) {
    girar_ponto(rot, p)
}

/// Ângulo do carro no espaço **espelhado**.
///
/// Espelhar X inverte o sentido dos ângulos: um giro anti-horário visto de cima
/// em `(x, z)` vira horário quando `x` troca de sinal. Sem este `-`, o carro
/// seria espelhado mas continuaria apontando para o lado oposto ao que anda —
/// era o "carro andando de ré" de outra forma.
fn girar_angulo(rot: Rotacao, angulo: f32) -> f32 {
    let _ = rot;
    -angulo
}

/// Constrói a geometria estática do mapa, escolhendo o cenário pelo **tipo**
/// declarado nele.
///
/// O tipo não é o nome: dois mapas podem ser `Avenida` com traçados diferentes.
/// O que muda entre os dois cenários é o que existe em volta do asfalto — prédio
/// e outdoor de um lado, pedra e mata do outro — e é a única coisa do desenho
/// que precisa saber qual mapa está em jogo.
fn construir_mundo(pista: &Pista, rotacao: Rotacao, cenario: Cenario) -> Mundo {
    let mut m = Mundo::default();

    // A pista é comum aos dois: asfalto, faixas, guias, mureta e a tira de neon.
    construir_pista(&mut m, pista, rotacao);
    construir_largada(&mut m, pista, rotacao);
    construir_portais(&mut m, pista, rotacao);
    construir_cones(&mut m, pista, rotacao);
    construir_tunel(&mut m, pista, rotacao);

    match cenario {
        Cenario::Avenida => {
            construir_arcos(&mut m, pista, rotacao);
            construir_passarelas(&mut m, pista, rotacao);
            construir_outdoors(&mut m, pista, rotacao);
            construir_cidade(&mut m, pista, rotacao);
            construir_longe(&mut m, pista, rotacao);
        }
        Cenario::Descida => {
            construir_serra(&mut m, pista, rotacao);
            construir_morros(&mut m, pista, rotacao);
            construir_cidade_ao_longe(&mut m, pista, rotacao);
        }
        Cenario::Circuito => {
            construir_barreiras(&mut m, pista, rotacao);
            construir_torres_de_luz(&mut m, pista, rotacao);
            construir_arquibancada(&mut m, pista, rotacao);
        }
        Cenario::Porto => {
            construir_conteineres(&mut m, pista, rotacao);
            construir_guindastes(&mut m, pista, rotacao);
            construir_mar(&mut m, pista, rotacao);
        }
        Cenario::Salar => {
            construir_estacas(&mut m, pista, rotacao);
            construir_montanhas_salar(&mut m, pista, rotacao);
        }
    }

    // As rampas são comuns a todos os cenários: se um mapa tem salto, o salto
    // precisa de sinalização na tela, senão o jogador descobre a rampa no ar.
    construir_rampas(&mut m, pista, rotacao);

    m
}

fn construir_pista(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let tile = malha::Construtor::METROS_POR_TILE;
    for i in 0..amostras.len().saturating_sub(1) {
        let a = amostras[i];
        let b = amostras[i + 1];
        let (ax, az) = girar_ponto(rotacao, a.centro);
        let (bx, bz) = girar_ponto(rotacao, b.centro);
        let (anx, anz) = girar_ponto(rotacao, a.normal);
        let (bnx, bnz) = girar_ponto(rotacao, b.normal);
        // A altura vem da amostra: numa descida o vão entre duas amostras
        // atravessa o terreno, e usar um `y` só faria o asfalto degrauar.
        let (ya, yb) = (a.altura, b.altura);

        let al = (ax - anx * a.meia_largura, az - anz * a.meia_largura);
        let ar = (ax + anx * a.meia_largura, az + anz * a.meia_largura);
        let bl = (bx - bnx * b.meia_largura, bz - bnz * b.meia_largura);
        let br = (bx + bnx * b.meia_largura, bz + bnz * b.meia_largura);

        // Asfalto **texturizado**: uma malha, uma chamada, e o UV vem do mundo.
        m.asfalto_t
            .chao3([al.0, al.1], [bl.0, bl.1], [br.0, br.1], [ar.0, ar.1], ya, yb, yb, ya, tile);

        // Faixa central tracejada: 6 m acesa, 6 m apagada.
        let ciclo = (a.s / 12.0) as i32;
        if ciclo % 2 == 0 {
            let meia = 0.18;
            let al2 = (ax - anx * meia, az - anz * meia);
            let ar2 = (ax + anx * meia, az + anz * meia);
            let bl2 = (bx - bnx * meia, bz - bnz * meia);
            let br2 = (bx + bnx * meia, bz + bnz * meia);
            m.faixas_t.chao3(
                [al2.0, al2.1],
                [bl2.0, bl2.1],
                [br2.0, br2.1],
                [ar2.0, ar2.1],
                ya + 0.022,
                yb + 0.022,
                yb + 0.022,
                ya + 0.022,
                tile,
            );
        }

        // Guias alternadas vermelho/branco na borda, como zebra de curva.
        let largura_guia = 1.0;
        let deslocamento = a.meia_largura + largura_guia * 0.5;
        for lado in [-1.0f32, 1.0] {
            let a0 = (
                ax + anx * deslocamento * lado,
                az + anz * deslocamento * lado,
            );
            let b0 = (
                bx + bnx * deslocamento * lado,
                bz + bnz * deslocamento * lado,
            );
            let a1 = (
                ax + anx * (deslocamento + largura_guia) * lado,
                az + anz * (deslocamento + largura_guia) * lado,
            );
            let b1 = (
                bx + bnx * (deslocamento + largura_guia) * lado,
                bz + bnz * (deslocamento + largura_guia) * lado,
            );
            let malha = if lado > 0.0 {
                &mut m.guia_a
            } else {
                &mut m.guia_b
            };
            malha.chao3(
                [a0.0, a0.1],
                [b0.0, b0.1],
                [b1.0, b1.1],
                [a1.0, a1.1],
                ya + 0.012,
                yb + 0.012,
                yb + 0.012,
                ya + 0.012,
                tile,
            );
        }

        // Mureta: uma caixa a cada duas amostras, do lado de fora. Concreto
        // texturizado — é a superfície que o jogador vê de perto a corrida
        // inteira, e a que mais ganha com tijolo em vez de cor chapada.
        if i % 2 == 0 {
            let dist = a.meia_largura + 2.4;
            for lado in [-1.0f32, 1.0] {
                let (px, pz) = (ax + anx * dist * lado, az + anz * dist * lado);
                let (dx, dz) = girar_direcao(rotacao, a.direcao);
                let giro = dx.atan2(dz);
                let comprimento = (b.s - a.s).max(1.0) * 2.2;
                m.concreto_t.caixa_texturizada(
                    [px, ya + 0.45, pz],
                    [0.45, 0.9, comprimento],
                    giro,
                    tile,
                );
            }
        }

        // Refletor no topo da mureta, a cada quatro amostras. É o que dá a
        // sequência de pontos correndo na borda da tela — a leitura de
        // velocidade mais barata que existe numa avenida à noite.
        if i % 4 == 0 {
            let dist = a.meia_largura + 2.4;
            for lado in [-1.0f32, 1.0] {
                let (px, pz) = (ax + anx * dist * lado, az + anz * dist * lado);
                m.luzes_inst
                    .push(instancia([px, ya + 1.05, pz], [0.34, 0.22, 0.34], 0.0));
            }
        }

        // Tira de neon na borda: **contínua** e de duas cores, ciano de um lado
        // e magenta do outro. Era uma caixa a cada 3 amostras, 0,22 m de largura,
        // e cobria 0,8% do quadro — a cidade não lia como neon nenhum.
        {
            let (dx, dz) = girar_direcao(rotacao, a.direcao);
            let giro = dx.atan2(dz);
            let comprimento = ((b.s - a.s) * 1.15).max(0.6);
            for lado in [-1.0f32, 1.0] {
                let dist = a.meia_largura + 0.5;
                let (px, pz) = (ax + anx * dist * lado, az + anz * dist * lado);
                let malha = if lado > 0.0 {
                    &mut m.neon
                } else {
                    &mut m.neon_b
                };
                malha.caixa_girada([px, ya + 0.42, pz], [0.26, 0.72, comprimento], giro);

                // Reflexo: um quad deitado, mais largo, do lado de dentro. Sem
                // render-to-texture, o "asfalto molhado" é este brilho.
                m.reflexos.caixa_girada(
                    [px - anx * 1.9 * lado, ya + 0.012, pz - anz * 1.9 * lado],
                    [2.6, 0.012, comprimento * 0.95],
                    giro,
                );
            }
        }
    }
}

// ────────────────────────────────────────────────────────────
// CENÁRIO: serra (a descida)
// ────────────────────────────────────────────────────────────
//
// O que muda em relação à avenida não é a pista, é o que existe em volta:
//
// * **paredão de pedra** do lado de dentro da curva, em vez de prédio;
// * **mata** dos dois lados, em vez de janela iluminada;
// * **guarda-corpo** do lado de fora, porque numa descida o lado de fora é um
//   precipício e a mureta de concreto leria como avenida;
// * **a cidade lá embaixo**, longe e baixa, em vez de um skyline alto ao redor.
//
// Tudo isso entra nas malhas texturizadas que já existem — pedra, mata,
// guarda-corpo — então o cenário novo custa **zero** chamada de desenho.

fn construir_serra(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let tile = malha::Construtor::METROS_POR_TILE;

    for i in 0..amostras.len().saturating_sub(1) {
        let a = amostras[i];
        let b = amostras[i + 1];
        let (ax, az) = girar_ponto(rotacao, a.centro);
        let (bx, bz) = girar_ponto(rotacao, b.centro);
        let (anx, anz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        let (ya, yb) = (a.altura, b.altura);

        // Dentro do túnel não há serra: a parede de pedra a 5 m do asfalto
        // atravessava o concreto e o quadro virava uma parede laranja sem
        // pista. O túnel tem geometria própria.
        if a.tunel || b.tunel {
            continue;
        }

        // ── guarda-corpo do lado de fora ─────────────────────
        // Chapa de 0,9 m com montante a cada duas amostras. Fica logo além da
        // guia, onde na avenida há mureta.
        if i % 2 == 0 {
            let dist = a.meia_largura + 0.9;
            for lado in [-1.0f32, 1.0] {
                let (px, pz) = (ax + anx * dist * lado, az + anz * dist * lado);
                m.guarda_t.caixa_texturizada(
                    [px, ya + 0.55, pz],
                    [0.12, 0.55, (b.s - a.s).max(1.0) * 2.1],
                    giro,
                    tile,
                );
            }
            // Montante: só do lado de fora, para o guarda-corpo não virar uma
            // cerca dos dois lados (a serra é de um lado só).
            let (px, pz) = (ax + anx * dist * -1.0, az + anz * dist * -1.0);
            m.guarda_t.caixa_texturizada(
                [px, ya + 0.30, pz],
                [0.16, 0.60, 0.16],
                0.0,
                tile,
            );
        }

        // ── paredão de pedra do lado de dentro ───────────────
        // Tira que acompanha o traçado, não caixa: a caixa de 19 m cortava a
        // curva por dentro (ver `parede3`). O lado vem da curvatura porque
        // numa serra a encosta fica sempre do lado de dentro da curva.
        let curvatura = curvatura_local(pista, a.s);
        let lado = if curvatura >= 0.0 { -1.0 } else { 1.0 };
        let dist = a.meia_largura + 6.0;
        let (pax, paz) = (ax + anx * dist * lado, az + anz * dist * lado);
        let (pbx, pbz) = (bx + anx * dist * lado, bz + anz * dist * lado);
        let altura = 5.0 + ((a.s * 0.03).sin() * 1.6).abs();
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        m.pedra_t.parede3(
            [pax, paz],
            [pbx, pbz],
            ya - 0.4,
            yb - 0.4,
            altura,
            [nx * lado, nz * lado],
            tile * 2.0,
        );

        // ── mata dos dois lados ──────────────────────────────
        // Copas em duas fileiras, mais altas do lado de fora (a encosta desce).
        if i % 3 == 0 {
            let semente = (a.s * 0.31).sin() * 43758.545;
            let r = semente - semente.floor();
            for (fileira, lado_mata) in [(0usize, -1.0f32), (1, 1.0)] {
                let d = a.meia_largura + 11.0 + fileira as f32 * 8.0 + r * 5.0;
                let (px, pz) = (ax + anx * d * lado_mata, az + anz * d * lado_mata);
                let altura_copa = 5.0 + r * 5.0 + fileira as f32 * 2.5;
                m.mata_t.caixa_texturizada(
                    [px, ya - 2.5 + altura_copa * 0.5, pz],
                    [6.5, altura_copa, 6.5],
                    0.0,
                    tile * 1.6,
                );
            }
        }
    }
}

/// Curvatura local em `s`, positiva para a direita. Usada para decidir de que
/// lado fica o paredão: numa curva para a direita a serra está à esquerda.
fn curvatura_local(pista: &Pista, s: f32) -> f32 {
    let passo = 12.0;
    let a = pista.direcao_em(s);
    let b = pista.direcao_em(s + passo);
    let cruz = a.x * b.z - a.z * b.x;
    let escalar = a.ponto(b).clamp(-1.0, 1.0);
    cruz.signum() * escalar.acos()
}

/// Morros do outro lado do vale.
///
/// Sem eles o horizonte da serra é uma linha reta entre o asfalto e o céu, e a
/// descida perde a profundidade — o quadro fica com um terço de céu vazio em
/// cima e nada que diga "há um vale aqui". São caixas largas e baixas, muito
/// longe, com topo irregular; a 300 m, o que se vê é a silhueta.
fn construir_morros(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = 31;
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let semente = (indice as f32 * 3.117).sin() * 51247.19;
        let r = semente - semente.floor();
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        for lado in [-1.0f32, 1.0] {
            // **Longe**, e o número é medido: a 210 m com 125 m de altura o topo
            // do morro ficava 4° **acima** do horizonte e ele tapava metade do
            // quadro com uma chapa azul chapada (48% dos pixels, medido). A
            // 420–700 m ele vira a faixa de silhueta que se espera de um vale.
            let distancia = 420.0 + r * 280.0;
            let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
            let na_pista = a.centro.mais(a.normal.vezes(distancia * lado));
            if pista.folga_em(na_pista) < 60.0 {
                continue;
            }
            // Base bem abaixo e topo variável: o que importa é a silhueta
            // contra o céu, e ela tem de ser irregular para não virar um muro.
            let altura = 30.0 + r * 45.0;
            let largura = 150.0 + r * 190.0;
            m.morros.caixa_girada(
                [bx, -60.0 + altura * 0.5, bz],
                [largura, altura, largura * 0.8],
                giro,
            );
        }
    }
}

/// A cidade **lá embaixo**: luzes baixas e distantes, sem prédio alto.
///
/// É o que dá a altura da serra. Um skyline alto ao redor diria "avenida"; pontos
/// de luz abaixo do nível da pista dizem "estou no morro".
fn construir_cidade_ao_longe(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = 17;
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let semente = (indice as f32 * 5.733).sin() * 31847.219;
        let r = semente - semente.floor();
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        for lado in [-1.0f32, 1.0] {
            if r > 0.55 && lado > 0.0 {
                // Só um lado em metade das amostras: duas fileiras de torre
                // fechariam o horizonte e a serra perderia a vista.
                continue;
            }
            let distancia = a.meia_largura + 120.0 + r * 190.0;
            let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
            let na_pista = a.centro.mais(a.normal.vezes(distancia * lado));
            if pista.folga_em(na_pista) < 30.0 {
                continue;
            }
            // **Abaixo** da pista: a cidade no fundo do vale. `-40` de base é o
            // que faz a diferença de altura existir na tela.
            let base = -42.0 + a.altura * 0.12;
            let altura = 12.0 + r * 26.0;
            let largura = 9.0 + r * 14.0;
            m.longe.caixa_girada(
                [bx, base + altura * 0.5, bz],
                [largura, altura, largura * 1.1],
                giro,
            );
            // Janelas: uma faixa fina e acesa em cada torre. A 150 m isso é o
            // suficiente para ler como cidade, e custa uma caixa.
            m.janelas.caixa_girada(
                [bx, base + altura * 0.72, bz],
                [largura * 1.02, 0.7, largura * 1.12],
                giro,
            );
        }
    }
}

// ────────────────────────────────────────────────────────────
// CENÁRIO: avenida (o circuito original)
// ────────────────────────────────────────────────────────────

fn construir_arcos(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let mut proximo = 24.0f32;
    let mut indice_arco = 0usize;
    for i in 0..amostras.len().saturating_sub(1) {
        let a = amostras[i];
        if a.s < proximo {
            continue;
        }
        proximo += 26.0;
        if pista.em_tunel(a.s) {
            continue;
        }
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        let vao = (a.meia_largura + 1.6) * 2.0;
        let y = a.altura;

        // Duas colunas e a viga. As colunas ficam **fora** da guia, para não
        // virarem obstáculo visual no meio da pista.
        for lado in [-1.0f32, 1.0] {
            let d = a.meia_largura + 1.6;
            let (cx, cz) = (px + nx * d * lado, pz + nz * d * lado);
            m.muros.caixa_girada([cx, y + 3.6, cz], [0.34, 7.2, 0.34], giro);
        }
        m.muros.caixa_girada([px, y + 7.15, pz], [vao, 0.42, 0.46], giro);

        // Tira de neon embaixo da viga, alternando ciano e magenta. Duas cores
        // alternadas custam zero: as duas malhas já existem.
        let malha = if indice_arco % 2 == 0 {
            &mut m.neon
        } else {
            &mut m.neon_b
        };
        malha.caixa_girada([px, y + 6.86, pz], [vao * 0.94, 0.13, 0.16], giro);
        // E o lampião da viga, que ilumina a pista de verdade — instanciado
        // com as outras luzes.
        m.luzes_inst
            .push(instancia([px, y + 6.62, pz], [vao * 0.55, 0.10, 0.10], giro));
        indice_arco += 1;
    }
}

/// Três passarelas cruzando a pista. Referência de "trecho novo" e teto visual
/// que faz a cidade ter dois níveis em vez de um.
fn construir_passarelas(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    for s_alvo in [140.0f32, 470.0, 810.0] {
        let s = pista.normalizar_s(s_alvo);
        if pista.em_tunel(s) {
            continue;
        }
        let a = pista.ponto_lateral(s, 0.0);
        let normal = pista.normal_em(s);
        let direcao = pista.direcao_em(s);
        let (px, pz) = girar_ponto(rotacao, a);
        let (nx, nz) = girar_ponto(rotacao, normal);
        let (dx, dz) = girar_direcao(rotacao, direcao);
        let giro = dx.atan2(dz);
        let meia = pista.meia_largura_em(s);
        let vao = (meia + 5.0) * 2.0;
        let y = pista.altura_em(s);

        // Laje e as duas torres de apoio.
        m.concreto_t
            .caixa_texturizada([px, y + 8.6, pz], [vao, 0.9, 4.2], giro, 3.0);
        for lado in [-1.0f32, 1.0] {
            let d = meia + 5.0;
            let (cx, cz) = (px + nx * d * lado, pz + nz * d * lado);
            m.predios_c
                .caixa_girada([cx, y + 4.3, cz], [1.1, 8.6, 1.1], giro);
        }
        // Faixa de neon na face de baixo — é o que dá o "teto" luminoso.
        m.telhados
            .caixa_girada([px, y + 8.1, pz], [vao * 0.96, 0.16, 3.9], giro);
    }
}

/// Outdoors luminosos na beira da pista.
///
/// Placa alta com moldura de neon. A face é a **textura de painel** (faixas
/// diagonais com alfa), então o outdoor tem conteúdo em vez de ser um retângulo
/// chapado — e continua sem texto, que não há como desenhar numa caixa.
fn construir_outdoors(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let mut proximo = 62.0f32;
    let mut n = 0usize;
    for a in amostras.iter() {
        if a.s < proximo {
            continue;
        }
        proximo += 62.0;
        if pista.em_tunel(a.s) || a.s < 40.0 {
            continue;
        }
        // Um lado por vez, alternando: dois outdoors na mesma altura dos dois
        // lados fechariam o quadro como um portal.
        let lado = if n % 2 == 0 { 1.0f32 } else { -1.0 };
        n += 1;
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        let d = a.meia_largura + 8.0;
        let (bx, bz) = (px + nx * d * lado, pz + nz * d * lado);
        if pista.folga_em(a.centro.mais(a.normal.vezes(d * lado))) < 8.0 {
            continue;
        }
        let y = a.altura;

        // Estrutura, face texturizada e moldura.
        m.predios_c.caixa_girada([bx, y + 8.0, bz], [8.0, 5.2, 0.5], giro);
        m.painel_t.caixa_texturizada(
            [bx, y + 8.1, bz - 0.3 * lado],
            [7.0, 4.2, 0.12],
            giro,
            2.2,
        );
        m.neon_b.caixa_girada([bx, y + 10.7, bz], [8.2, 0.20, 0.14], giro);
        m.neon_b.caixa_girada([bx, y + 5.3, bz], [8.2, 0.20, 0.14], giro);
        // Mastro, para a placa não flutuar: instanciado com os outros postes.
        m.postes_inst
            .push(instancia([bx, y + 4.0, bz], [0.26, 8.0, 0.26], giro));
    }
}

fn construir_cidade(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = 9;

    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        let y = a.altura;

        // Pseudoaleatório determinístico por índice: a cidade tem de ser a mesma
        // em todo quadro e em todo boot, senão a referência de comparação muda.
        let semente = (indice as f32 * 12.9898).sin() * 43758.5453;
        let r = semente - semente.floor();

        for lado in [-1.0f32, 1.0] {
            let distancia = a.meia_largura + 16.0 + r * 26.0;
            let altura = 10.0 + r * 34.0;
            let largura = 7.0 + r * 9.0;
            let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);

            // Prédio em cima de outro trecho da pista é o defeito que a cidade
            // aleatória produz sozinha num laço fechado: os do fim da volta
            // nasciam sobre a reta de largada e a câmera amanhecia dentro de um
            // deles. A folga é medida **no espaço da pista**.
            let na_pista = a.centro.mais(a.normal.vezes(distancia * lado));
            if pista.folga_em(na_pista) < largura * 0.75 + 4.0 {
                continue;
            }

            let grupo = ((indice / passo) + if lado > 0.0 { 1 } else { 0 }) % 3;
            let malha = match grupo {
                0 => &mut m.predios_a,
                1 => &mut m.predios_b,
                _ => &mut m.predios_c,
            };
            malha.caixa_girada(
                [bx, y + altura * 0.5, bz],
                [largura, altura, largura * (0.8 + r * 0.5)],
                giro,
            );

            // Faixa de neon no topo do prédio: de longe é o que desenha a
            // silhueta da cidade, e custa uma caixa por prédio.
            m.telhados.caixa_girada(
                [bx, y + altura + 0.35, bz],
                [largura * 0.92, 0.30, largura * 0.75],
                giro,
            );

            // Faixas de janelas: três caixas finas e brilhantes por prédio. É o
            // que faz a cidade parecer habitada à noite.
            let altura_janela = altura * 0.5;
            for k in 0..3 {
                let yj = y + altura * (0.22 + k as f32 * 0.26);
                let frente = (
                    bx + nx * (largura * 0.52) * -lado,
                    bz + nz * (largura * 0.52) * -lado,
                );
                m.janelas.caixa_girada(
                    [frente.0, yj, frente.1],
                    [largura * 0.62, 0.5 + altura_janela * 0.02, 0.35],
                    giro,
                );
            }

            // Poste de luz a cada dois prédios, mais perto da pista.
            if indice % (passo * 2) == 0 {
                let dp = a.meia_largura + 3.2;
                let (lx, lz) = (px + nx * dp * lado, pz + nz * dp * lado);
                m.postes_inst
                    .push(instancia([lx, y + 3.4, lz], [0.22, 6.8, 0.22], 0.0));
                m.luzes_inst
                    .push(instancia([lx, y + 6.9, lz], [0.9, 0.28, 0.9], 0.0));
                // Poça de luz no asfalto: é o que faz o poste existir à noite.
                // Sem ela, o lampião flutua e a rua continua preta embaixo dele.
                let (qx, qz) = (
                    px + nx * (a.meia_largura - 2.6) * lado,
                    pz + nz * (a.meia_largura - 2.6) * lado,
                );
                m.poco_luz
                    .caixa_girada([qx, y + 0.015, qz], [6.5, 0.015, 9.0], 0.0);
            }
        }
    }
}

/// Skyline distante: um anel de torres altas e escuras, longe da pista.
///
/// Sem ele o horizonte termina no último prédio e o céu fica um retângulo preto
/// do tamanho da tela. As torres são mais escuras e mais azuis que as da beira
/// — é o que dá a impressão de profundidade sem nenhum efeito de névoa.
// ── circuito de teste ────────────────────────────────────────
//
// Três construções, nenhuma malha nova: barreira é `muros` + `neon`, torre é
// `muros` + `neon_b`, arquibancada é `predios_a/b` + `telhados`. É a regra do
// §29 ("cenário novo não pode criar malha nova") — cada malha a mais custa uma
// chamada de desenho por quadro, e o orçamento no RK3326 é de ~50.

fn construir_barreiras(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    // A cada duas amostras: a barreira é contínua, não um bloco por amostra.
    for i in (0..amostras.len().saturating_sub(2)).step_by(2) {
        let a = amostras[i];
        let b = amostras[i + 2];
        let (ax, az) = girar_ponto(rotacao, a.centro);
        let (bx, bz) = girar_ponto(rotacao, b.centro);
        let (anx, anz) = girar_ponto(rotacao, a.normal);
        let (bnx, bnz) = girar_ponto(rotacao, b.normal);
        for lado in [-1.0f32, 1.0] {
            let da = a.meia_largura + 3.0;
            let db = b.meia_largura + 3.0;
            let al = (ax + anx * da * lado, az + anz * da * lado);
            let bl = (bx + bnx * db * lado, bz + bnz * db * lado);
            let y = a.altura;
            // Parede de 1,1 m: baixa o bastante para a câmera ver por cima e
            // alta o bastante para ler como limite.
            m.muros.chao3(
                [al.0, al.1],
                [bl.0, bl.1],
                [bl.0, bl.1],
                [al.0, al.1],
                y,
                y + 1.1,
                y + 1.1,
                y,
                1.0, // UV degenerado: a barreira é cor chapada, não textura de chão
            );
            // Fita de advertência no topo: a grade de neon vai na textura, e o
            // pulso do `u_tint` faz a fita correr — sem um vertex a mais.
            m.grade_t.chao3(
                [al.0, al.1],
                [bl.0, bl.1],
                [bl.0, bl.1],
                [al.0, al.1],
                y + 1.12,
                y + 1.28,
                y + 1.28,
                y + 1.12,
                1.0,
            );
        }
    }
}

fn construir_torres_de_luz(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    // Uma torre a cada ~140 m, alternando o lado: o suficiente para justificar
    // a luz ambiente mais alta do mapa.
    let passo = (amostras.len() / 14).max(8);
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let lado = if (indice / passo) % 2 == 0 { -1.0 } else { 1.0 };
        let distancia = a.meia_largura + 14.0;
        let na_pista = a.centro.mais(a.normal.vezes(distancia * lado));
        if pista.folga_em(na_pista) < 8.0 {
            continue;
        }
        let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
        let y = a.altura;
        // Mastro de 18 m, instanciado com os postes.
        m.postes_inst
            .push(instancia([bx, y + 9.0, bz], [0.7, 18.0, 0.7], 0.0));
        // Painel de refletores no topo, instanciado com as luzes.
        m.luzes_inst
            .push(instancia([bx, y + 18.4, bz], [3.4, 1.1, 0.5], 0.0));
    }
}

fn construir_arquibancada(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = (amostras.len() / 6).max(10);
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        let lado = if (indice / passo) % 2 == 0 { 1.0 } else { -1.0 };
        let distancia = a.meia_largura + 26.0;
        let na_pista = a.centro.mais(a.normal.vezes(distancia * lado));
        if pista.folga_em(na_pista) < 20.0 {
            continue;
        }
        let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
        let y = a.altura;
        // Três degraus: cada um mais alto e mais recuado que o anterior.
        for k in 0..3 {
            let recuo = k as f32 * 4.0;
            let altura = 2.0 + k as f32 * 1.8;
            m.predios_a.caixa_girada(
                [bx + nx * recuo * lado, y + altura * 0.5, bz + nz * recuo * lado],
                [30.0, altura, 3.4],
                giro,
            );
            // Faixa de público: uma tira clara em cada degrau lê como gente.
            m.telhados.caixa_girada(
                [bx + nx * recuo * lado, y + altura + 0.2, bz + nz * recuo * lado],
                [28.0, 0.32, 3.0],
                giro,
            );
        }
    }
}

// ── porto ────────────────────────────────────────────────────

fn construir_conteineres(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = 6;
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let semente = (indice as f32 * 4.771).sin() * 34567.1234;
        let r = semente - semente.floor();
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        let y = a.altura;
        for lado in [-1.0f32, 1.0] {
            let distancia = a.meia_largura + 11.0 + r * 14.0;
            let na_pista = a.centro.mais(a.normal.vezes(distancia * lado));
            if pista.folga_em(na_pista) < 10.0 {
                continue;
            }
            let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
            // Pilha de um a três contêineres: 12 × 2,6 × 2,4 m cada.
            let pilha = 1 + (r * 3.0) as u32;
            for k in 0..pilha {
                let malha = match (indice + k as usize) % 3 {
                    0 => &mut m.predios_a,
                    1 => &mut m.predios_b,
                    _ => &mut m.predios_c,
                };
                malha.caixa_girada(
                    [bx, y + 1.3 + k as f32 * 2.6, bz],
                    [12.0, 2.5, 2.4],
                    giro + 0.03 * k as f32,
                );
            }
            // Tampa iluminada no topo da pilha: é o que desenha a silhueta.
            m.telhados.caixa_girada(
                [bx, y + 1.3 + pilha as f32 * 2.6, bz],
                [11.4, 0.22, 2.1],
                giro,
            );
        }
    }
}

fn construir_guindastes(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = (amostras.len() / 5).max(10);
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        let lado = if (indice / passo) % 2 == 0 { 1.0 } else { -1.0 };
        let distancia = a.meia_largura + 42.0;
        let na_pista = a.centro.mais(a.normal.vezes(distancia * lado));
        if pista.folga_em(na_pista) < 26.0 {
            continue;
        }
        let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
        let y = a.altura;
        // Mastro, lança e contrapeso: três caixas, nenhuma malha nova.
        m.muros.caixa([bx, y + 20.0, bz], [2.2, 40.0, 2.2]);
        m.muros.caixa_girada([bx + nx * 16.0 * lado, y + 38.0, bz + nz * 16.0 * lado],
            [58.0, 1.8, 1.8], giro);
        m.neon_b.caixa_girada([bx + nx * 16.0 * lado, y + 39.2, bz + nz * 16.0 * lado],
            [6.0, 0.5, 0.6], giro);
    }
}

fn construir_mar(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = 20;
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        for lado in [-1.0f32, 1.0] {
            let distancia = a.meia_largura + 120.0;
            let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
            // Chapa de água de 200 × 200 m, no nível do mar (y = -1,2): o cais
            // é aterrado e o mar fica abaixo dele.
            m.longe.caixa_girada([bx, -1.2, bz], [200.0, 0.6, 200.0], giro);
            // Linha de luzes da outra margem.
            if (indice / passo) % 2 == 0 {
                m.neon.chao3(
                    [bx - nx * 60.0, bz - nz * 60.0],
                    [bx + nx * 60.0, bz + nz * 60.0],
                    [bx + nx * 60.0, bz + nz * 60.0],
                    [bx - nx * 60.0, bz - nz * 60.0],
                    1.4,
                    1.6,
                    1.6,
                    1.4,
                    1.0,
                );
            }
        }
    }
}

// ── salar ────────────────────────────────────────────────────

fn construir_estacas(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    // De 25 em 25 m: no salar não há referência nenhuma, e sem estaca o jogador
    // perde a noção de velocidade — o chão branco e liso não dá paralaxe.
    let passo = (amostras.len() / 60).max(4);
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let y = a.altura;
        for lado in [-1.0f32, 1.0] {
            let distancia = a.meia_largura + 6.0;
            let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
            // Estaca de 1,6 m, alternando ciano e magenta.
            let cor = if (indice / passo) % 2 == 0 { &mut m.neon } else { &mut m.neon_b };
            cor.caixa([bx, y + 0.8, bz], [0.22, 1.6, 0.22]);
        }
    }
}

fn construir_montanhas_salar(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = 26;
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let semente = (indice as f32 * 9.117).sin() * 51234.789;
        let r = semente - semente.floor();
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        for lado in [-1.0f32, 1.0] {
            let distancia = a.meia_largura + 220.0 + r * 120.0;
            let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
            let altura = 40.0 + r * 90.0;
            let largura = 120.0 + r * 90.0;
            m.longe.caixa_girada([bx, altura * 0.5, bz], [largura, altura, largura * 0.8], giro);
        }
    }
}

// ── rampas ───────────────────────────────────────────────────

/// Sinaliza cada rampa: chevrons na subida e uma fita no topo.
///
/// Sem isto o jogador descobre a rampa **no ar** — e nos mapas escuros a subida
/// de 3 m some no asfalto. A fita do topo é o que dá o ponto de mira, e custa
/// duas caixas por rampa.
fn construir_rampas(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    for rampa in pista.rampas() {
        let comprimento = (rampa.topo - rampa.inicio).max(1.0);
        // Dez faixas ao longo da subida.
        let faixas = 10;
        for k in 0..faixas {
            let s = rampa.inicio + comprimento * (k as f32 + 0.5) / faixas as f32;
            let centro = pista.ponto_em(s);
            let normal = pista.normal_em(s);
            let (px, pz) = girar_ponto(rotacao, centro);
            let (nx, nz) = girar_ponto(rotacao, normal);
            let y = pista.altura_em(s);
            // Uma faixa clara atravessada a cada 10 % da rampa.
            let meia = pista.meia_largura_em(s) * 0.72;
            let (esq_x, esq_z) = (px - nx * meia, pz - nz * meia);
            let (dir_x, dir_z) = (px + nx * meia, pz + nz * meia);
            m.faixas_t.chao3(
                [esq_x, esq_z],
                [dir_x, dir_z],
                [dir_x, dir_z],
                [esq_x, esq_z],
                y + 0.03,
                y + 0.03,
                y + 0.03,
                y + 0.03,
                6.0,
            );
        }
        // Fita no topo, na cor de advertência: é o ponto de mira do salto.
        let s_topo = (rampa.topo - 0.2).max(rampa.inicio);
        let centro = pista.ponto_em(s_topo);
        let normal = pista.normal_em(s_topo);
        let (px, pz) = girar_ponto(rotacao, centro);
        let (nx, nz) = girar_ponto(rotacao, normal);
        let meia = pista.meia_largura_em(s_topo);
        let y = pista.altura_em(s_topo) + 0.05;
        m.neon.chao3(
            [px - nx * meia, pz - nz * meia],
            [px + nx * meia, pz + nz * meia],
            [px + nx * meia, pz + nz * meia],
            [px - nx * meia, pz - nz * meia],
            y,
            y,
            y,
            y,
            1.0,
        );
    }
}

fn construir_longe(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    let passo = 23;
    for (indice, a) in amostras.iter().enumerate().step_by(passo) {
        let semente = (indice as f32 * 7.919).sin() * 24634.6345;
        let r = semente - semente.floor();
        let (px, pz) = girar_ponto(rotacao, a.centro);
        let (nx, nz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        for lado in [-1.0f32, 1.0] {
            let distancia = a.meia_largura + 90.0 + r * 70.0;
            let (bx, bz) = (px + nx * distancia * lado, pz + nz * distancia * lado);
            let altura = 45.0 + r * 55.0;
            let largura = 18.0 + r * 16.0;
            let na_pista = a.centro.mais(a.normal.vezes(distancia * lado));
            if pista.folga_em(na_pista) < largura * 0.75 + 8.0 {
                continue;
            }
            m.longe
                .caixa_girada([bx, altura * 0.5, bz], [largura, altura, largura * 1.2], giro);
            // Faixa de neon na torre distante: dá andares ao bloco escuro, e a
            // 100 m da pista isso é toda a diferença entre parede e cidade.
            m.telhados.caixa_girada(
                [bx, altura * 0.62, bz],
                [largura * 0.72, 0.34, largura * 0.9],
                giro,
            );
        }
    }
}

fn construir_tunel(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let amostras = pista.amostras();
    for i in 0..amostras.len().saturating_sub(1) {
        let a = amostras[i];
        if !a.tunel {
            continue;
        }
        let b = amostras[i + 1];
        let (ax, az) = girar_ponto(rotacao, a.centro);
        let (anx, anz) = girar_ponto(rotacao, a.normal);
        let (dx, dz) = girar_direcao(rotacao, a.direcao);
        let giro = dx.atan2(dz);
        let comprimento = (b.s - a.s).max(1.2) * 1.15;
        let y = a.altura;

        // Parede dos dois lados e teto, em concreto texturizado.
        for lado in [-1.0f32, 1.0] {
            let d = a.meia_largura + 1.4;
            let (px, pz) = (ax + anx * d * lado, az + anz * d * lado);
            m.concreto_t.caixa_texturizada(
                [px, y + 3.0, pz],
                [0.8, 6.0, comprimento],
                giro,
                3.0,
            );
        }
        let teto = 6.2;
        m.concreto_t.caixa_texturizada(
            [ax, y + teto, az],
            [a.meia_largura * 2.0 + 2.8, 0.5, comprimento],
            giro,
            3.0,
        );

        // Lâmpadas no teto, uma a cada duas amostras, alternando entre **duas**
        // malhas. Duas malhas de cor independente e pulsos em oposição de fase dão
        // a perseguição de luz sem nenhum estado por vértice — a engine só tem
        // cor uniforme por desenho, então a única forma de animar em sequência é
        // separar em grupos. Custa uma chamada.
        if i % 2 == 0 {
            let alvo = if i % 4 == 0 {
                &mut m.tunel_luzes
            } else {
                &mut m.tunel_luzes_b
            };
            alvo.caixa_girada(
                [ax, y + teto - 0.35, az],
                [a.meia_largura * 0.9, 0.16, 1.1],
                giro,
            );
        }
        // Faixa de luz nas paredes: o túnel precisa ser visível de dentro.
        if i % 3 == 0 {
            for lado in [-1.0f32, 1.0] {
                let d = a.meia_largura + 1.0;
                let (px, pz) = (ax + anx * d * lado, az + anz * d * lado);
                m.tunel_luzes.caixa_girada(
                    [px, y + 2.2, pz],
                    [0.12, 0.22, comprimento],
                    giro,
                );
            }
        }
    }
}

fn construir_cones(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let tile = malha::Construtor::METROS_POR_TILE;
    for obstaculo in pista.obstaculos() {
        let p = pista.ponto_lateral(obstaculo.s, obstaculo.lateral);
        let (x, z) = girar_ponto(rotacao, p);
        let y = pista.altura_em(obstaculo.s);
        let _ = tile;
        m.cones_inst.push(instancia(
            [x, y + obstaculo.raio * 0.6, z],
            [
                obstaculo.raio * 1.2,
                obstaculo.raio * 1.2,
                obstaculo.raio * 1.2,
            ],
            0.0,
        ));
    }
}

/// Portais de checkpoint a cada quarto de volta.
///
/// A geometria é **uma** (`portal.obj`, instanciada): um anel e duas colunas por
/// checkpoint, quatro por volta. Assar quatro portais numa malha por mapa
/// gastaria a memória de um modelo para desenhar a mesma coisa quatro vezes.
fn construir_portais(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    let comprimento = pista.comprimento();
    for i in 0..4 {
        let s = pista.normalizar_s(comprimento * i as f32 / 4.0);
        if pista.em_tunel(s) {
            continue;
        }
        let p = pista.ponto_lateral(s, 0.0);
        let (x, z) = girar_ponto(rotacao, p);
        let (dx, dz) = girar_direcao(rotacao, pista.direcao_em(s));
        let giro = dx.atan2(dz);
        let y = pista.altura_em(s);
        // O anel do OBJ tem o centro a 4,6 m; a base fica no asfalto.
        m.portais_inst
            .push(instancia([x, y, z], [1.0, 1.0, 1.0], giro));
    }
}

fn construir_largada(m: &mut Mundo, pista: &Pista, rotacao: Rotacao) {
    // Quadriculado de 2 fileiras na linha de largada, em **textura de xadrez**:
    // é o único lugar do jogo onde a textura substitui 20 caixas.
    let meia = pista.meia_largura_em(0.0);
    let y = pista.altura_em(0.0);
    let (dx, dz) = girar_direcao(rotacao, pista.direcao_em(0.0));
    let giro = dx.atan2(dz);
    // A posição vem do **ponto da pista**, não da origem do mundo: a largada da
    // serra não fica em (0,0) e a primeira versão desenhava o xadrez no meio do
    // nada.
    let centro = pista.ponto_lateral(1.6, 0.0);
    let (cx, cz) = girar_ponto(rotacao, centro);
    m.xadrez_t
        .caixa_texturizada([cx, y + 0.02, cz], [meia * 2.0, 0.02, 3.2], giro, 1.6);
}

// ════════════════════════════════════════════════════════════
// MATRIZES DE HUD
// ════════════════════════════════════════════════════════════

/// Nome da parte que o diagnóstico `GIOCHI_SEM` usa para um material texturizado.
///
/// `--so` aceita o nome da malha ("asfalto_t"); `GIOCHI_SEM` usa o nome do
/// **material** ("asfalto"), porque é assim que se pensa ao medir: "sem asfalto".
fn parte_de_textura(nome: &str) -> &str {
    match nome {
        "asfalto_t" => "asfalto",
        "concreto_t" => "concreto",
        "pedra_t" => "pedra",
        "mata_t" => "mata",
        "guarda_t" => "guarda",
        "painel_t" => "painel",
        "faixas_t" => "faixas",
        "grade_t" => "grade",
        outro => outro,
    }
}

/// Multiplica o RGB por um fator, preservando o alfa.
fn multiplicar(cor: Color3D, k: f32) -> Color3D {
    Color3D::new(
        (cor.r * k).min(1.0),
        (cor.g * k).min(1.0),
        (cor.b * k).min(1.0),
        cor.a,
    )
}

/// Achata a matriz de um carro num quad deitado no chão.
///
/// O quad da engine é um plano em XY virado para +Z. A matriz de um carro gira
/// em torno de Y, então reusar as colunas dela mantém o quad **em pé** — foi o
/// que aconteceu com a primeira aura: um painel vertical de 5 m à frente do
/// carro, que cobria 40% do quadro e não lia como luz no chão. Aqui o eixo Y do
/// quad vira o eixo **frente** do carro (`-Z` local) e o eixo X vira a direita,
/// o que deita o quad no asfalto.
///
/// `altura` é a altura **sobre o chão do carro**, não sobre o nível do mar. A
/// diferença importa: a serra desce de 74 m, e um decalque cravado em `y = 0`
/// fica enterrado sob o asfalto — some a poça e some o farol, sem erro nenhum.
/// Quem já foi ao boavista viu um carro sem luz nenhuma embaixo.
///
/// A terceira coluna é o **normal**: ela leva o `+Z` do quad (a normal da
/// malha) para `+Y`, e não para `+Z` do mundo. Errar isso não muda a posição
/// (os quatro cantos têm `z = 0`), muda a **luz**: o `flat` e o `textured`
/// calculam `0,35 + 0,65·lambert`, e com a normal apontando para o horizonte o
/// fator travava em 0,54 — a poça saía 1,7× mais escura que o tint pedia, sem
/// erro nenhum de GL. Ver `os_decalques_de_luz_apontam_para_cima`.
fn achatar(m: Mat4, largura: f32, comprimento: f32, altura: f32) -> Mat4 {
    let d = m.data;
    // **Zero, não identidade.** A identidade deixa `m[1]` e `m[5]` em 1,0, e
    // esses dois campos são a componente Y dos eixos X e Y do quad: o resultado
    // é um cisalhamento de 1 m que inclina o quad para cima. Foi assim que a
    // aura do carro virou um painel vertical de 5 m em vez de uma poça no chão.
    let mut q = [0.0f32; 16];
    q[9] = 1.0;
    q[15] = 1.0;
    q[0] = d[0] * largura;
    q[2] = d[2] * largura;
    // `-d[8..10]` é a frente do carro: o modelo aponta para -Z no espaço local.
    q[4] = -d[8] * comprimento;
    q[6] = -d[10] * comprimento;
    q[12] = d[12];
    q[13] = d[13] + altura;
    q[14] = d[14];
    Mat4::from_cols_array(&q)
}

/// Retângulo do HUD **girado** no plano da tela, centrado em `(cx, cy)` pixels.
///
/// O que o HUD precisa saber para desenhar: onde se posiciona e onde cai.
///
/// ## Por que isto existe
///
/// O HUD se posiciona em **pixels de projeto** (paisagem, 1280×720 no X55) e é
/// desenhado no **painel** (720×1280). Com giro, essas duas coisas não são a
/// mesma, e durante muito tempo este arquivo tratou como se fossem: a câmera do
/// HUD era montada com as medidas do projeto, então o texto saía esticado 3,2×
/// na horizontal e **deitado** em relação ao mundo — o relato do dono no X55 foi
/// exatamente "as informações estão na vertical".
///
/// A ponte é a mesma que o launcher usa (`video::girar_rect` / `girar_ponto`),
/// porque a orientação do painel é fato do sistema e os dois apps têm de
/// concordar com ela.
#[derive(Clone, Copy, Debug)]
struct TelaHud {
    rotacao: Rotacao,
    /// Medidas do espaço de projeto, em pixels. Paisagem nos dois sentidos.
    projeto: (f32, f32),
    /// Medidas reais do alvo de desenho, em pixels.
    painel: (u32, u32),
}

impl TelaHud {
    /// Quanto o ângulo de um elemento gira junto com o painel.
    fn giro_do_angulo(self) -> f32 {
        // `girar_rect(Cw90)` leva (dx, dy) em (-dy, dx), que é `angulo + 90°`;
        // `Ccw90` leva em (dy, -dx), que é `angulo - 90°`. Em radianos.
        match self.rotacao {
            Rotacao::Nenhuma => 0.0,
            Rotacao::Cw90 => std::f32::consts::FRAC_PI_2,
            Rotacao::Ccw90 => -std::f32::consts::FRAC_PI_2,
        }
    }
}

/// Retângulo de projeto → retângulo de painel, arredondado ao pixel.
///
/// Delega a `video::girar_rect` em vez de repetir a fórmula: duas fórmulas para
/// a mesma rotação divergem em silêncio, e a UI já saiu de cabeça para baixo uma
/// vez por isso.
fn girar_rect_hud(tela: TelaHud, x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    if tela.rotacao == Rotacao::Nenhuma {
        return (x, y, w, h);
    }
    let design = (tela.projeto.0 as i32, tela.projeto.1 as i32);
    let (dx, dy, dw, dh) = girar_rect(
        tela.rotacao,
        design,
        x.round() as i32,
        y.round() as i32,
        w.round().max(1.0) as i32,
        h.round().max(1.0) as i32,
    );
    (dx as f32, dy as f32, dw as f32, dh as f32)
}

/// Ponto de projeto → ponto de painel.
fn girar_ponto_hud(tela: TelaHud, x: f32, y: f32) -> (f32, f32) {
    if tela.rotacao == Rotacao::Nenhuma {
        return (x, y);
    }
    let design = (tela.projeto.0 as i32, tela.projeto.1 as i32);
    let (dx, dy) = girar_ponto_do_painel(tela.rotacao, design, x.round() as i32, y.round() as i32);
    (dx as f32, dy as f32)
}

/// `hud_matriz` só faz retângulo alinhado aos eixos; os raios de velocidade
/// precisam apontar para fora do centro. A rotação é feita em espaço de pixel e
/// depois convertida, porque girar em espaço de NDC distorceria o ângulo quando
/// a tela não é quadrada.
fn hud_matriz_girada(
    tela: TelaHud,
    cx: f32,
    cy: f32,
    angulo: f32,
    comprimento: f32,
    espessura: f32,
) -> Mat4 {
    // Mesma ponte do `hud_matriz`: centro em projeto → centro no painel, e o
    // ângulo ganha o quarto de volta que a rotação do painel impõe.
    let (dx, dy) = girar_ponto_hud(tela, cx, cy);
    let angulo = angulo + tela.giro_do_angulo();
    let aspecto = tela.painel.0 as f32 / tela.painel.1.max(1) as f32;
    let ndc_x = dx / tela.painel.0.max(1) as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - dy / tela.painel.1.max(1) as f32 * 2.0;
    let (s, c) = angulo.sin_cos();
    // Eixos em pixel: o curto é perpendicular ao raio, o longo aponta para fora.
    let (curto_x, curto_y) = (-s * espessura, c * espessura);
    let (longo_x, longo_y) = (c * comprimento, s * comprimento);
    // Pixel → unidades da câmera do HUD (ver `hud_matriz`).
    let kx = PROFUNDIDADE_HUD * aspecto / tela.painel.0.max(1) as f32;
    let ky = PROFUNDIDADE_HUD / tela.painel.1.max(1) as f32;

    let mut m = Mat4::IDENTITY.to_cols_array();
    m[0] = curto_x * kx;
    m[1] = -curto_y * ky;
    m[4] = longo_x * kx;
    m[5] = -longo_y * ky;
    m[12] = ndc_x * PROFUNDIDADE_HUD * aspecto;
    m[13] = ndc_y * PROFUNDIDADE_HUD;
    m[14] = -PROFUNDIDADE_HUD;
    Mat4::from_cols_array(&m)
}

/// Quad deitado à frente do carro, no chão: o feixe do farol.
///
/// É o [`achatar`] deslocado para a frente — mesma normal, mesmo achatamento.
/// Antes esta função repetia a matriz inteira e por isso escapou da correção do
/// normal: a poça apontava para cima e o feixe continuava apontando para o
/// horizonte. Uma função, um lugar para errar.
fn feixe(m: Mat4) -> Mat4 {
    let mut q = achatar(m, FEIXE_LARGURA, FEIXE_COMPRIMENTO, ALTURA_FEIXE).to_cols_array();
    // 5,5 m à frente do para-choque, para o feixe não nascer debaixo do carro.
    // Curto de propósito: 15 m de feixe translúcido no chão custam ~8 fps neste
    // painel, e o que se ganha em alcance se perde em quadro.
    let d = m.data;
    q[12] -= d[8] * INICIO_FEIXE;
    q[14] -= d[10] * INICIO_FEIXE;
    Mat4::from_cols_array(&q)
}

/// O que cada carro joga de luz no chão.
///
/// Nasce no mesmo laço que monta a matriz do carro porque as três coisas de que
/// a poça e o feixe precisam — matriz, cor e estado de drift — são lidas ali.
struct LuzDoCarro {
    /// Matriz do carro, de onde saem o decalque achatado e o feixe.
    base: Mat4,
    /// Cor do neon do carro.
    cor: [f32; 3],
    /// Rampa do drift: `0.0` é piso, `1.0` é o auge.
    deriva: f32,
    /// No ar o decalque sai: uma poça no chão sem emissor lê como sombra errada.
    no_ar: bool,
}

/// Rampa do drift, `0.0..=1.0`.
///
/// O `tempo_drift` do núcleo é quanto o drift atual já dura. A rampa existe para
/// um toque de volante não acender a poça como um drift de três segundos: em
/// `RAMPA_DRIFT` segundos ela chega ao auge e fica lá enquanto o drift durar.
/// Quando o drift acaba, o núcleo zera o tempo e a poça volta ao piso — de
/// propósito, porque o fim do drift é informação, não um decaimento.
fn rampa_de_drift(derrapando: bool, tempo_drift: f32) -> f32 {
    if derrapando {
        (tempo_drift / RAMPA_DRIFT).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Tinta da poça de neon de um carro: cor **e** alfa no mesmo `Color3D`.
///
/// O `u_tint` do desenho instanciado é um uniforme só; devolver as duas coisas
/// juntas evita o estado intermediário em que a cor foi passada e o alfa
/// esquecido.
///
/// O drift faz duas coisas, e as duas custam zero pixel:
///
/// 1. **acende** — o alfa sobe de [`POCA_ALFA_BASE`] para [`POCA_ALFA_DRIFT`];
/// 2. **esquenta** — a cor puxa 40% para o branco, que é como neon saturado lê
///    no talo. Sem isso o carro verde-limão e o adversário magenta ficariam
///    ambos "mais fortes", indistinguíveis de um ganho de brilho da cena.
///
/// O pulso só existe durante o drift: parado, um neon que pisca vira pisca-pisca
/// e o jogador procura um defeito no jogo.
fn tinta_da_poca(cor: [f32; 3], deriva: f32, tempo: f32) -> Color3D {
    // `NaN` não passa por `clamp` (devolve `NaN`), e `NaN` no tint é um quad
    // que o driver pode descartar inteiro — some a poça de todos os carros, não
    // só a de quem tinha o valor ruim. Piso é a resposta segura.
    let deriva = if deriva.is_finite() {
        deriva.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let pulso = if deriva > 0.0 {
        0.92 + 0.08 * (tempo * 37.0).sin()
    } else {
        1.0
    };
    let quente = 0.40 * deriva;
    Color3D::new(
        cor[0] + (1.0 - cor[0]) * quente,
        cor[1] + (1.0 - cor[1]) * quente,
        cor[2] + (1.0 - cor[2]) * quente,
        (POCA_ALFA_BASE + (POCA_ALFA_DRIFT - POCA_ALFA_BASE) * deriva).min(1.0) * pulso,
    )
}

/// Agrupa as poças por tinta: **uma** chamada de desenho por cor distinta.
///
/// O desenho instanciado aplica um `u_tint` ao lote inteiro, e cada poça tem a
/// cor do carro. Agrupar é o que faz dois carros da mesma cor custarem uma
/// chamada em vez de duas — e, no caso comum (cores todas diferentes), mantém
/// uma por carro, que é o mínimo possível com um uniforme por lote.
///
/// A ordem é a de chegada: o primeiro carro de cada cor define a posição do
/// grupo. Ordem estável importa para o desenho translúcido, onde a mistura não é
/// comutativa.
fn agrupar_por_tinta(pocas: Vec<(Color3D, Mat4)>) -> Vec<(Color3D, Vec<Mat4>)> {
    let mut grupos: Vec<(Color3D, Vec<Mat4>)> = Vec::new();
    for (tinta, matriz) in pocas {
        match grupos.iter_mut().find(|(t, _)| *t == tinta) {
            Some((_, lista)) => lista.push(matriz),
            None => grupos.push((tinta, vec![matriz])),
        }
    }
    grupos
}

/// Matriz de um retângulo de HUD, posicionado em **pixels** da tela de projeto.
fn hud_matriz(tela: TelaHud, x: f32, y: f32, w: f32, h: f32) -> Mat4 {
    // Retângulo do **projeto** → retângulo do **painel**. É a mesma função que o
    // launcher usa (`video::girar_rect`), de propósito: um comando que gira um
    // app tem de girar o outro, senão o teste mente sobre um deles
    // (AGENTS.md §12).
    let (dx, dy, dw, dh) = girar_rect_hud(tela, x, y, w, h);
    let aspecto = tela.painel.0 as f32 / tela.painel.1.max(1) as f32;
    let ndc_x = (dx + dw * 0.5) / tela.painel.0.max(1) as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - (dy + dh * 0.5) / tela.painel.1.max(1) as f32 * 2.0;
    Transform3D::new(
        Vec3::new(
            ndc_x * PROFUNDIDADE_HUD * aspecto,
            ndc_y * PROFUNDIDADE_HUD,
            -PROFUNDIDADE_HUD,
        ),
        Quat::IDENTITY,
        Vec3::new(
            (dw / tela.painel.0.max(1) as f32) * PROFUNDIDADE_HUD * aspecto,
            (dh / tela.painel.1.max(1) as f32) * PROFUNDIDADE_HUD,
            1.0,
        ),
    )
    .matrix()
}

/// Matriz de um quad que cobre a **tela inteira**, para véus e flashes.
///
/// ## Por que não dá para usar `hud_matriz` aqui
///
/// `hud_matriz` mapeia pixel → um espaço de HUD onde **1 px de projeto vale
/// `PROF/painel` unidades**, ou seja, o *centro* de cada elemento cai no pixel
/// certo, mas o *tamanho* sai pela metade: o NDC vai de -1 a +1 (duas unidades)
/// e a escala dela entrega uma. Medido numa captura de 1024×768, um retângulo
/// de tela cheia pela `hud_matriz` cobria 512×384 — o quarto central — em vez
/// dos 1024×768. Passou despercebido porque o único uso era o flash de nitro,
/// com alfa 0,05; o véu das telas de texto, opaco, expôs o erro.
///
/// Corrigir `hud_matriz` está fora de questão: todo o texto do HUD é
/// dimensionado nesse espaço e dobrar a escala faria as linhas se sobreporem.
/// Esta função existe só para quem precisa dos **cantos exatos** da tela.
fn hud_matriz_tela(tela: TelaHud) -> Mat4 {
    let aspecto = tela.painel.0 as f32 / tela.painel.1.max(1) as f32;
    // A câmera do HUD abre 90° na vertical a `PROFUNDIDADE_HUD` de distância,
    // então meia-altura visível = `PROFUNDIDADE_HUD` e meia-largura =
    // `PROFUNDIDADE_HUD * aspecto`; o quad unitário precisa do dobro disso.
    Transform3D::new(
        Vec3::new(0.0, 0.0, -PROFUNDIDADE_HUD),
        Quat::IDENTITY,
        Vec3::new(
            2.0 * PROFUNDIDADE_HUD * aspecto,
            2.0 * PROFUNDIDADE_HUD,
            1.0,
        ),
    )
    .matrix()
}

/// Inclinação da câmera do mundo, já com a rotação do sistema.
///
/// `roll` gira a câmera em torno do próprio eixo de visão — é o que faz um
/// painel retrato mostrar a cena paisagem sem tocar em nenhum vértice.
///
/// ## O sinal, e por que ele é o **oposto** do HUD
///
/// Girar a **câmera** por θ gira a **imagem** por −θ. O HUD é posicionado por
/// `video::girar_rect`, a mesma função do launcher (99,9 % no aparelho), e
/// `giro_do_angulo` diz qual é o giro da imagem que ela produz: `Ccw90` leva
/// `(dx, dy)` em `(dy, −dx)`, que é **−90°**. Logo o `roll` da câmera tem de ser
/// **+90°** — exatamente o oposto.
///
/// ## O defeito que isto corrige
///
/// Durante uma rodada inteira o sinal esteve trocado, e o argumento parecia
/// certo ("a consequência aritmética da convenção do launcher"). O que faltava
/// era **medir os dois lados**: com o sinal antigo o mundo exigia a rotação
/// contrária à do HUD para voltar ao espaço de projeto — 180° de diferença — e
/// o X55 mostrou "o jogo de ponta a cabeça" enquanto as barras do
/// `--prova-rotacao` (que passam pelo `girar_rect`) caíam no lugar certo.
///
/// Medido agora, no PC: o quadro `Ccw90` desfeito com a inversa correta dá
/// **99,99 %** no mundo e **99,74 %** no quadro completo. O teste
/// `o_roll_do_mundo_e_o_oposto_do_giro_do_hud` amarra a relação, para o sinal
/// não voltar a ser escolhido por raciocínio.
fn roll_do_mundo(rotacao: Rotacao, roll_da_derrapagem: f32) -> f32 {
    match rotacao {
        Rotacao::Nenhuma => roll_da_derrapagem,
        Rotacao::Cw90 => -std::f32::consts::FRAC_PI_2 + roll_da_derrapagem,
        Rotacao::Ccw90 => std::f32::consts::FRAC_PI_2 + roll_da_derrapagem,
    }
}

/// Câmera ortográfica-de-fato do HUD: `fov` de 90° e plano a `PROFUNDIDADE_HUD`.
fn camara_hud(tela: (u32, u32)) -> Camera3D {
    Camera3D::from_parts(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), Vec3::UP)
        .with_fov_y(std::f32::consts::FRAC_PI_2)
        // Plano próximo em 9,0 com o HUD a 10: a profundidade normalizada do HUD
        // fica ~0,10, à frente de qualquer geometria do mundo (≥0,87 a partir de
        // 2 m). Era 0,5..60 e dava ~0,96 — o HUD ficava **atrás** de um prédio a
        // 30 m, e o sintoma era "o texto some quando a cidade chega perto".
        .with_clip_planes(9.0, 4000.0)
        .with_drawable_size(tela.0, tela.1)
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::vetor::produto_vetorial;
    use giochi_neon_drift_core::Ponto as PontoCore;

    #[test]
    fn a_matriz_do_hud_mapeia_os_cantos_da_tela() {
        // Canto superior esquerdo de uma tela 640×480, um retângulo de 1 px: o
        // centro tem de cair no NDC (-1, 1) e o tamanho tem de ser 1 px.
        let tela = TelaHud {
            rotacao: Rotacao::Nenhuma,
            projeto: (640.0, 480.0),
            painel: (640, 480),
        };
        let m = hud_matriz(tela, 0.0, 0.0, 1.0, 1.0);
        let x = m.data[12];
        let y = m.data[13];
        let aspecto = 640.0 / 480.0;
        // ndc_x = -1 + 1/640, ndc_y = 1 - 1/480
        let ndc_x = x / (PROFUNDIDADE_HUD * aspecto);
        let ndc_y = y / PROFUNDIDADE_HUD;
        assert!((ndc_x - (-1.0 + 1.0 / 640.0)).abs() < 1e-4, "ndc_x {ndc_x}");
        assert!((ndc_y - (1.0 - 1.0 / 480.0)).abs() < 1e-4, "ndc_y {ndc_y}");
        assert!((m.data[14] + PROFUNDIDADE_HUD).abs() < 1e-5);
    }

    /// Tela do X55: projeto paisagem 1280×720, painel retrato 720×1280.
    fn tela_x55(rotacao: Rotacao) -> TelaHud {
        TelaHud {
            rotacao,
            projeto: (1280.0, 720.0),
            painel: (720, 1280),
        }
    }

    /// Posição em **unidades de câmera do HUD** de um ponto do painel.
    ///
    /// É a conversão inversa de `hud_matriz`, e é a unidade em que `m.data[12]`
    /// e `m.data[13]` vêm — comparar com NDC cru daria erro de fator.
    fn camera_do_painel(tela: TelaHud, x: f32, y: f32) -> (f32, f32) {
        let aspecto = tela.painel.0 as f32 / tela.painel.1 as f32;
        (
            (x / tela.painel.0 as f32 * 2.0 - 1.0) * PROFUNDIDADE_HUD * aspecto,
            (1.0 - y / tela.painel.1 as f32 * 2.0) * PROFUNDIDADE_HUD,
        )
    }

    /// Tamanho em pixels do painel que a escala local `m.data[0]`/`m.data[5]`
    /// cobre. O quad do HUD é 1×1, então a escala **é** o tamanho.
    fn pixels_do_painel(tela: TelaHud, escala_x: f32, escala_y: f32) -> (f32, f32) {
        let aspecto = tela.painel.0 as f32 / tela.painel.1 as f32;
        (
            escala_x.abs() / (PROFUNDIDADE_HUD * aspecto) * tela.painel.0 as f32,
            escala_y.abs() / PROFUNDIDADE_HUD * tela.painel.1 as f32,
        )
    }

    #[test]
    fn com_giro_o_hud_cai_onde_o_launcher_poria() {
        // A regra é a do launcher: `girar_rect(Ccw90)` leva o canto superior
        // esquerdo do projeto para o canto inferior esquerdo do painel. Se o HUD
        // não fizer isso, ele fica **deitado** em relação ao mundo — foi o
        // relato do dono no X55 ("as informações estão na vertical").
        let tela = tela_x55(Rotacao::Ccw90);
        // Um elemento no canto superior esquerdo do projeto (10, 10) medindo 100×20.
        let m = hud_matriz(tela, 10.0, 10.0, 100.0, 20.0);
        // `girar_rect(Ccw90, (1280,720), 10, 10, 100, 20)` = (10, 1280-10-100, 20, 100)
        // = (10, 1170, 20, 100): um retângulo **alto e estreito** no painel, que é
        // como um elemento horizontal de projeto aparece num buffer retrato girado.
        let (largura_px, altura_px) = pixels_do_painel(tela, m.data[0], m.data[5]);
        assert!(
            (largura_px - 20.0).abs() < 1.0 && (altura_px - 100.0).abs() < 1.0,
            "com Ccw90 um elemento 100x20 de projeto tem de virar 20x100 no painel; veio {largura_px}x{altura_px}"
        );
        // E o centro tem de cair em (20, 1220), que é embaixo à esquerda.
        let cento = camera_do_painel(tela, 20.0, 1220.0);
        assert!(
            (m.data[12] - cento.0).abs() < 1.0 && (m.data[13] - cento.1).abs() < 1.0,
            "centro em câmera ({}, {}) contra ({}, {})",
            m.data[12],
            m.data[13],
            cento.0,
            cento.1
        );
    }

    #[test]
    fn com_giro_o_hud_nao_fica_esticado_pelo_aspecto_errado() {
        // O outro sintoma era o texto 3,2× mais largo que alto: a câmera do HUD
        // era montada com as medidas do **projeto** (1280×720) enquanto o alvo
        // real era o painel (720×1280). Este teste mede a **proporção** de um
        // quadrado de projeto depois de girado: ele continua quadrado.
        for rotacao in [Rotacao::Cw90, Rotacao::Ccw90] {
            let tela = tela_x55(rotacao);
            let m = hud_matriz(tela, 100.0, 100.0, 50.0, 50.0);
            let (largura_px, altura_px) = pixels_do_painel(tela, m.data[0], m.data[5]);
            assert!(
                (largura_px - altura_px).abs() < 1.0,
                "{rotacao:?}: quadrado 50x50 saiu {largura_px}x{altura_px} no painel"
            );
        }
    }

    #[test]
    fn sem_giro_o_hud_continua_em_pixels_de_projeto() {
        // O caminho sem giro é o de todos os aparelhos de 640×480, e não pode
        // mudar com a correção do giro.
        let tela = TelaHud {
            rotacao: Rotacao::Nenhuma,
            projeto: (640.0, 480.0),
            painel: (640, 480),
        };
        let m = hud_matriz(tela, 10.0, 20.0, 30.0, 40.0);
        let esperado = camera_do_painel(tela, 25.0, 40.0);
        assert!((m.data[12] - esperado.0).abs() < 1e-3);
        assert!((m.data[13] - esperado.1).abs() < 1e-3);
    }

    #[test]
    fn o_angulo_dos_raios_gira_com_o_painel() {
        // Um raio apontando para a direita no projeto (angulo 0) tem de apontar
        // para **cima** no painel girado 90° anti-horário. `girar_rect(Ccw90)`
        // leva (dx,dy) em (dy,-dx), então o ângulo cai 90° no espaço de tela (y
        // para baixo).
        let sem = TelaHud {
            rotacao: Rotacao::Nenhuma,
            projeto: (1280.0, 720.0),
            painel: (1280, 720),
        };
        assert_eq!(sem.giro_do_angulo(), 0.0);
        assert!((tela_x55(Rotacao::Ccw90).giro_do_angulo() + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!((tela_x55(Rotacao::Cw90).giro_do_angulo() - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn a_camera_do_hud_e_montada_no_painel_e_nao_no_projeto() {
        // A câmera descreve o alvo real de desenho: o que entra é o **painel**.
        // Montá-la com as medidas do projeto num painel retrato é o que esticava
        // o texto 3,2× na horizontal. A prova é a projeção: o canto superior
        // direito do NDC tem de cair no canto do painel, não no do projeto.
        let painel = camara_hud((720, 1280));
        let m = painel.projection_matrix().expect("projeção válida");
        // `m[0]` = 1/(aspecto·tan(fov/2)); com fov de 90° e aspecto 720/1280,
        // `m[0]` = 1280/720 = 1,78. Se a câmera tivesse sido montada com o
        // projeto, daria 720/1280 = 0,5625 — exatamente o inverso.
        assert!(
            (m.data[0] - 1280.0 / 720.0).abs() < 1e-3,
            "m[0] = {} (esperado {})",
            m.data[0],
            1280.0 / 720.0
        );
    }

    /// O mundo e o HUD têm de girar no mesmo sentido — e não giravam.
    ///
    /// Foi o defeito do X55: "o jogo de ponta a cabeça e as informações na
    /// vertical". Cada lado estava coerente sozinho (o HUD pelo `girar_rect`, o
    /// mundo pelo `roll`) e mesmo assim discordavam 180°.
    ///
    /// A regra que os amarra: girar a **câmera** por θ gira a **imagem** por −θ.
    /// O HUD diz quanto a imagem gira (`giro_do_angulo`); o `roll` tem de ser o
    /// oposto. Trocar o sinal de `roll_do_mundo` reprova aqui.
    #[test]
    fn o_roll_do_mundo_e_o_oposto_do_giro_do_hud() {
        for rotacao in [Rotacao::Nenhuma, Rotacao::Cw90, Rotacao::Ccw90] {
            let tela = tela_x55(rotacao);
            let roll = roll_do_mundo(rotacao, 0.0);
            let giro = tela.giro_do_angulo();
            assert!(
                (roll + giro).abs() < 1e-6,
                "{rotacao:?}: roll {roll} e giro do HUD {giro} — tinham de ser opostos"
            );
        }
    }

    /// A derrapagem soma ao `roll` do sistema, não o substitui.
    #[test]
    fn a_derrapagem_soma_ao_roll_do_sistema() {
        assert!((roll_do_mundo(Rotacao::Nenhuma, 0.07) - 0.07).abs() < 1e-6);
        assert!(
            (roll_do_mundo(Rotacao::Ccw90, 0.07) - (std::f32::consts::FRAC_PI_2 + 0.07)).abs()
                < 1e-6
        );
    }

    #[test]
    fn a_hud_matriz_dimensiona_pela_metade_do_pixel() {
        // Armadilha que só apareceu na captura de tela: `hud_matriz` acerta o
        // **centro** de qualquer retângulo no pixel pedido, mas o **tamanho**
        // sai pela metade — o NDC vai de -1 a +1 (duas unidades) e a escala
        // dela entrega uma. Um retângulo de "tela cheia" por ela cobre o quarto
        // central: medido, 512×384 numa captura de 1024×768.
        //
        // O teste anterior "provava" a cobertura multiplicando por 2 no lugar
        // de dividir, e passou por anos. Este afirma o valor real: 1,0 de NDC,
        // meia tela. Não se corrige `hud_matriz` — todo o texto do HUD é
        // calibrado nesse espaço e dobrá-lo faria as linhas se sobreporem.
        // Tela cheia é `hud_matriz_tela`, testada logo abaixo.
        let tela = TelaHud {
            rotacao: Rotacao::Nenhuma,
            projeto: (640.0, 480.0),
            painel: (640, 480),
        };
        let m = hud_matriz(tela, 0.0, 0.0, 640.0, 480.0);
        let aspecto = 640.0 / 480.0;
        // NDC = escala / (PROF · aspecto) na largura e escala / PROF na altura.
        let largura_ndc = m.data[0] / (PROFUNDIDADE_HUD * aspecto);
        let altura_ndc = m.data[5] / PROFUNDIDADE_HUD;
        assert!((largura_ndc - 1.0).abs() < 1e-4, "largura {largura_ndc}");
        assert!((altura_ndc - 1.0).abs() < 1e-4, "altura {altura_ndc}");
    }

    #[test]
    fn a_matriz_de_tela_cheia_encosta_nos_quatro_cantos_do_ndc() {
        // A prova que faltava: projetar os cantos do quad pela câmera do HUD de
        // verdade (`view_projection`) e exigir ±1 nos dois eixos. Inclui o
        // painel girado, onde a proporção vem do **painel** e não do projeto.
        let casos = [
            (Rotacao::Nenhuma, (640.0, 480.0), (640u32, 480u32)),
            (Rotacao::Nenhuma, (1024.0, 768.0), (1024, 768)),
            (Rotacao::Cw90, (1024.0, 768.0), (1024, 768)),
            (Rotacao::Ccw90, (1024.0, 768.0), (1024, 768)),
        ];
        for (rotacao, projeto, painel) in casos {
            let tela = TelaHud {
                rotacao,
                projeto,
                painel,
            };
            let vp = camara_hud(painel).view_projection().expect("projeção");
            let m = hud_matriz_tela(tela);
            let mut vistos = Vec::new();
            for (sx, sy) in [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)] {
                let mundo = m.transform_point(Vec3::new(sx, sy, 0.0));
                let ndc = vp.project_point(mundo).expect("canto atrás da câmera");
                assert!(
                    (ndc.x.abs() - 1.0).abs() < 1e-3,
                    "{rotacao:?} {painel:?}: canto x={sx} sy={sy} em NDC {}",
                    ndc.x
                );
                assert!(
                    (ndc.y.abs() - 1.0).abs() < 1e-3,
                    "{rotacao:?} {painel:?}: canto x={sx} sy={sy} em NDC {}",
                    ndc.y
                );
                vistos.push((ndc.x.signum(), ndc.y.signum()));
            }
            // Um canto para cada quadrante: o quad está centrado e não torcido.
            vistos.sort_by(|a, b| a.partial_cmp(b).expect("sem NaN"));
            vistos.dedup();
            assert_eq!(vistos.len(), 4, "{rotacao:?} {painel:?}: {vistos:?}");
        }
    }

    #[test]
    fn o_billboard_fica_centrado_na_particula() {
        let m =
            efeitos::matriz_billboard(Vec3::new(3.0, 2.0, -4.0), Vec3::new(1.0, 0.0, 0.0), Vec3::UP, 0.5);
        assert_eq!(m.data[12], 3.0);
        assert_eq!(m.data[13], 2.0);
        assert_eq!(m.data[14], -4.0);
        assert_eq!(m.data[0], 0.5);
        assert_eq!(m.data[5], 0.5);
    }

    #[test]
    fn o_quad_deitado_do_carro_fica_com_a_normal_para_cima() {
        // A aura e o feixe do farol são quads no chão. Normal para baixo + face
        // culling = efeito invisível, e o defeito não dá erro nenhum.
        //
        // Esta é a normal **geométrica** (produto vetorial dos dois eixos do
        // decalque), a que decide o descarte de faces. A normal que o shader
        // lê é outra coisa — é a terceira coluna da matriz —, e ela escapou
        // desta conta por anos: ver `os_decalques_de_luz_apontam_para_cima`.
        for angulo in [0.0f32, 1.1, -2.3, 3.0] {
            let m = Transform3D::new(
                Vec3::ZERO,
                Quat::from_axis_angle(Vec3::UP, angulo),
                Vec3::new(1.0, 1.0, 1.0),
            )
            .matrix();
            for q in [achatar(m, 2.0, 4.0, 0.02), feixe(m)] {
                let a = Vec3::new(q.data[0], q.data[1], q.data[2]);
                let b = Vec3::new(q.data[4], q.data[5], q.data[6]);
                let n = produto_vetorial(a, b).normalized();
                assert!(n.y > 0.99, "ângulo {angulo}: normal {n:?}");
            }
        }
    }

    #[test]
    fn o_carro_desenhado_aponta_para_onde_ele_anda() {
        // O defeito relatado em 09/11/2026: "os carros parecem correr de ré".
        //
        // Duas convenções existem no jogo e discordavam em 180°: a física usa
        // `direcao() = (sin, cos)` (θ=0 anda para +Z) e a malha põe os faróis em
        // `z = -2,14` (frente desenhada em -Z). Girar o modelo por θ levava a
        // frente para o lado oposto do movimento.
        //
        // A verificação é a que faltava: pegar o **centro dos faróis** — os
        // vértices da malha, não uma constante — passar pela matriz de desenho e
        // exigir que ele caia à frente do carro, no sentido de `direcao()`.
        //
        // Comparar com a malha em vez de com um comentário é o que faz este
        // teste valer: inverter os `z` de `malha::carro()` reprova aqui.
        use giochi_neon_drift_core::Carro;

        let formas = malha::carro();
        // Centro dos faróis em espaço local (média dos vértices).
        let farois = &formas.farois.0;
        let n = farois.len() as f32;
        let local = Vec3::new(
            farois.iter().map(|v| v.position[0]).sum::<f32>() / n,
            farois.iter().map(|v| v.position[1]).sum::<f32>() / n,
            farois.iter().map(|v| v.position[2]).sum::<f32>() / n,
        );
        assert!(local.z < 0.0, "a malha põe os faróis em z = {:.2}", local.z);

        for angulo in [0.0f32, 0.6, 1.2, -0.9, 2.5, 3.0] {
            let carro = Carro::novo(PontoCore::ZERO, angulo);
            let d = carro.direcao();
            let m = matriz_de_carro(Rotacao::Nenhuma, PontoCore::ZERO, angulo, 0.0);

            // Aplica a matriz ao centro dos faróis (só rotação e translação).
            let world = Vec3::new(
                m.data[0] * local.x + m.data[4] * local.y + m.data[8] * local.z + m.data[12],
                m.data[1] * local.x + m.data[5] * local.y + m.data[9] * local.z + m.data[13],
                m.data[2] * local.x + m.data[6] * local.y + m.data[10] * local.z + m.data[14],
            );
            let frente = Vec3::new(world.x, 0.0, world.z).normalized();
            // A `direcao()` do núcleo passa por `girar_ponto` antes de virar
            // pixel — e essa função espelha X (ver o comentário dela). Comparar
            // com a direção crua exigiria que o carro apontasse para o lado
            // oposto ao que anda na tela, que é o bug que o espelhamento
            // conserta.
            let (ex, ez) = girar_direcao(Rotacao::Nenhuma, d);
            let esperado = Vec3::new(ex, 0.0, ez).normalized();
            let alinhamento = frente.x * esperado.x + frente.z * esperado.z;
            assert!(
                alinhamento > 0.99,
                "com ângulo {angulo:.2} os faróis apontam para {frente:?} e o carro \
                 anda para {esperado:?} (produto {alinhamento:.3}) — 180° de diferença \
                 é o 'correndo de ré'"
            );
        }
    }

    #[test]
    fn virar_para_a_direita_poe_o_carro_a_direita_na_tela() {
        // O defeito relatado em 10/11/2026: *"a esquerda e a direita estao
        // invertidos"*. O volante estava certo (aperta direita -> `virar = +1`
        // -> `theta` cresce -> o carro vai para `+X`); o que estava errado era
        // `+X` cair à **esquerda** da tela, porque `Mat4::look_at` é
        // right-handed e monta `s = cross(f, up)` — com `f = +Z` isso dá `-X`
        // como "direita da tela".
        //
        // Este teste é a afirmação que faltava, e ele mede a única coisa que o
        // jogador mede: para que lado do **quadro** o carro vai. Ele passa pelo
        // caminho de produção (`girar_ponto` e depois a `Camera3D`), não por uma
        // reimplementação — foi a lição do `--teclas` que imprimia estado sem
        // ter mudado nada (§24) e do contador que media a intenção (§30).
        //
        // Devolver `(p.x, p.z)` cru em `girar_ponto` reprova com NDC x = -1,687.
        // Medido, não estimado.
        use giochi_engine_3d::math3d::Camera3D;

        // A câmera de perseguição do jogo: atrás do carro, olhando para +Z.
        let cam = Camera3D::from_parts(
            Vec3::new(0.0, 3.0, -7.0),
            Vec3::new(0.0, 1.0, 7.0),
            Vec3::UP,
        )
        .with_fov_y(std::f32::consts::FRAC_PI_2)
        .with_clip_planes(0.25, 900.0)
        .with_drawable_size(1280, 720);
        let vp = cam.view_projection().expect("matriz view-projection");

        // Depois de virar à direita, o carro andou alguns metros para `+X`.
        let theta = 0.35f32;
        let mundo = Ponto {
            x: theta.sin() * 12.0,
            z: theta.cos() * 12.0,
        };
        assert!(
            mundo.x > 1.0,
            "o cenário do teste precisa de deslocamento lateral: x = {:.2}",
            mundo.x
        );

        let (px, pz) = girar_ponto(Rotacao::Nenhuma, mundo);
        let ndc = vp.transform_point(Vec3::new(px, 1.0, pz));
        assert!(
            ndc.x > 0.05,
            "o carro virou à direita e caiu em NDC x = {:+.3}, que é a ESQUERDA \
             da tela — é a inversão de esquerda/direita relatada",
            ndc.x
        );

        // A contraprova: virar à esquerda tem de cair do outro lado. Sem ela,
        // uma projeção que devolvesse sempre o mesmo sinal passaria.
        let esquerda = Ponto {
            x: -theta.sin() * 12.0,
            z: theta.cos() * 12.0,
        };
        let (ex, ez) = girar_ponto(Rotacao::Nenhuma, esquerda);
        let ndc_esq = vp.transform_point(Vec3::new(ex, 1.0, ez));
        assert!(
            ndc_esq.x < -0.05,
            "o carro virou à esquerda e caiu em NDC x = {:+.3}",
            ndc_esq.x
        );
    }

    #[test]
    fn as_lanternas_ficam_atras_dos_farois() {
        // A contraprova do teste acima: se alguém "consertar" invertendo a
        // malha, o farol e a lanterna trocam de lugar. Aqui as duas caixas são
        // comparadas entre si, então o teste sabe a diferença mesmo se as duas
        // forem invertidas juntas.
        let formas = malha::carro();
        let z_farol = formas.farois.0.iter().map(|v| v.position[2]).sum::<f32>()
            / formas.farois.0.len() as f32;
        let z_lanterna = formas.lanternas.0.iter().map(|v| v.position[2]).sum::<f32>()
            / formas.lanternas.0.len() as f32;
        assert!(
            z_farol < z_lanterna,
            "faróis em z={z_farol:.2} deviam estar à frente (mais -Z) das lanternas em z={z_lanterna:.2}"
        );
    }

    /// Aplica uma `Mat4` (coluna-maior) a um ponto 3D.
    fn aplicar(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
        let d = m.data;
        [
            d[0] * p[0] + d[4] * p[1] + d[8] * p[2] + d[12],
            d[1] * p[0] + d[5] * p[1] + d[9] * p[2] + d[13],
            d[2] * p[0] + d[6] * p[1] + d[10] * p[2] + d[14],
        ]
    }

    #[test]
    fn os_decalques_de_luz_apontam_para_cima() {
        // O `flat` e o `textured` da engine calculam `0,35 + 0,65·lambert`. A
        // normal do quad da engine é `+Z`; se o decalque não a levar para `+Y`,
        // ela fica no plano do chão e o fator trava em 0,54 — a poça sai 1,7×
        // mais escura que o tint pediu, e **sem erro de GL**: o defeito é só uma
        // luz fraca. Foi assim que o feixe ficou para trás quando a poça foi
        // corrigida.
        for angulo in [0.0f32, 0.6, 1.2, -0.9, 2.5, 3.0] {
            let carro = matriz_de_carro(Rotacao::Nenhuma, PontoCore::ZERO, angulo, 0.0);
            for (nome, m) in [
                ("poça", achatar(carro, POCA_LARGURA, POCA_COMPRIMENTO, ALTURA_POCA)),
                ("feixe", feixe(carro)),
            ] {
                let origem = aplicar(&m, [0.0, 0.0, 0.0]);
                let normal = aplicar(&m, [0.0, 0.0, 1.0]);
                let normal = [
                    normal[0] - origem[0],
                    normal[1] - origem[1],
                    normal[2] - origem[2],
                ];
                assert!(
                    normal[1] > 0.99,
                    "{nome} com ângulo {angulo:.2}: normal {normal:?} — o decalque não está \
                     deitado no chão"
                );
            }
        }
    }

    #[test]
    fn a_poca_cobre_o_carro_e_fica_no_chao() {
        for angulo in [0.0f32, 0.7, 2.1, -1.3] {
            let carro = matriz_de_carro(
                Rotacao::Nenhuma,
                PontoCore::new(20.0, -7.0),
                angulo,
                0.0,
            );
            let m = achatar(carro, POCA_LARGURA, POCA_COMPRIMENTO, ALTURA_POCA);

            // Cantos do quad unitário (a malha é de -0,5 a +0,5 em X e Y).
            let cantos = [
                aplicar(&m, [-0.5, -0.5, 0.0]),
                aplicar(&m, [0.5, -0.5, 0.0]),
                aplicar(&m, [0.5, 0.5, 0.0]),
                aplicar(&m, [-0.5, 0.5, 0.0]),
            ];
            for canto in cantos {
                assert!(
                    (canto[1] - ALTURA_POCA).abs() < 1e-5,
                    "ângulo {angulo:.2}: canto a y = {} — o decalque saiu do chão",
                    canto[1]
                );
            }

            // A largura e o comprimento do decalque são os pedidos, medidos no
            // mundo: é o que impede uma mudança de escala silenciosa (a poça de
            // 9,6 m custou 9 fps numa rodada anterior).
            let largura = ((cantos[1][0] - cantos[0][0]).powi(2)
                + (cantos[1][2] - cantos[0][2]).powi(2))
            .sqrt();
            let comprimento = ((cantos[3][0] - cantos[0][0]).powi(2)
                + (cantos[3][2] - cantos[0][2]).powi(2))
            .sqrt();
            assert!(
                (largura - POCA_LARGURA).abs() < 1e-3,
                "ângulo {angulo:.2}: largura {largura:.3} em vez de {POCA_LARGURA}"
            );
            assert!(
                (comprimento - POCA_COMPRIMENTO).abs() < 1e-3,
                "ângulo {angulo:.2}: comprimento {comprimento:.3} em vez de {POCA_COMPRIMENTO}"
            );

            // O centro do decalque é o do carro: a poça tem de estar **sob** ele,
            // não à frente.
            let centro = aplicar(&m, [0.0, 0.0, 0.0]);
            let (cx, cz) = girar_ponto(
                Rotacao::Nenhuma,
                PontoCore::new(20.0, -7.0),
            );
            assert!((centro[0] - cx).abs() < 1e-4 && (centro[2] - cz).abs() < 1e-4);
        }
    }

    #[test]
    fn a_poca_acompanha_o_relevo_da_serra() {
        // O boavista desce de 74 m. Um decalque cravado em `y = 0` fica enterrado
        // sob o asfalto: some a poça e some o feixe, sem erro nenhum de GL — o
        // sintoma é "esse mapa não tem luz embaixo do carro".
        for altura in [0.0f32, 12.5, 74.0] {
            let carro = matriz_de_carro(Rotacao::Nenhuma, PontoCore::ZERO, 0.3, altura);
            let poca = achatar(carro, POCA_LARGURA, POCA_COMPRIMENTO, ALTURA_POCA);
            let feixe = feixe(carro);
            for (nome, m, esperado) in [
                ("poça", poca, altura + ALTURA_POCA),
                ("feixe", feixe, altura + ALTURA_FEIXE),
            ] {
                let centro = aplicar(&m, [0.0, 0.0, 0.0]);
                assert!(
                    (centro[1] - esperado).abs() < 1e-4,
                    "{nome} a {altura} m: y = {:.3}, esperado {esperado:.3}",
                    centro[1]
                );
            }
        }
    }

    #[test]
    fn o_feixe_comeca_a_frente_do_para_choque() {
        // O feixe nasce 5,5 m à frente do carro. Nascer embaixo dele faria a luz
        // atravessar a lataria, e o farol deixaria de "abrir" a pista.
        let carro = matriz_de_carro(Rotacao::Nenhuma, PontoCore::ZERO, 0.0, 0.0);
        let m = feixe(carro);
        let centro = aplicar(&m, [0.0, 0.0, 0.0]);
        let frente = {
            let d = carro.data;
            [-d[8], -d[10]]
        };
        let deslocamento = centro[0] * frente[0] + centro[2] * frente[1];
        assert!(
            (deslocamento - INICIO_FEIXE).abs() < 1e-4,
            "o feixe começou a {deslocamento:.2} m do carro, não a {INICIO_FEIXE}"
        );
        assert!((centro[1] - ALTURA_FEIXE).abs() < 1e-5);
    }

    #[test]
    fn a_rampa_do_drift_sobe_e_satura() {
        assert_eq!(rampa_de_drift(false, 0.0), 0.0);
        assert_eq!(rampa_de_drift(false, 5.0), 0.0, "parado, o tempo não importa");
        assert_eq!(rampa_de_drift(true, 0.0), 0.0);
        assert!(
            (rampa_de_drift(true, RAMPA_DRIFT / 2.0) - 0.5).abs() < 1e-5,
            "no meio da rampa o brilho devia estar na metade"
        );
        assert_eq!(rampa_de_drift(true, RAMPA_DRIFT), 1.0);
        assert_eq!(rampa_de_drift(true, 60.0), 1.0, "a rampa tem teto");

        let mut anterior = 0.0;
        for passo in 0..20 {
            let v = rampa_de_drift(true, RAMPA_DRIFT * passo as f32 / 10.0);
            assert!(v >= anterior, "a rampa desceu de {anterior} para {v}");
            anterior = v;
        }
    }

    #[test]
    fn o_drift_acende_e_esquenta_o_neon() {
        let cor = COR_JOGADOR;
        let parado = tinta_da_poca(cor, 0.0, 1.23);
        let no_talo = tinta_da_poca(cor, 1.0, 1.23);

        assert!(
            no_talo.a > parado.a * 1.5,
            "o drift devia acender a poça: alfa {:.2} contra {:.2}",
            parado.a,
            no_talo.a
        );
        assert!(no_talo.a <= 1.0 && parado.a > 0.0, "alfa fora de 0..1");

        // "Esquenta": a distância até o branco diminui em **todos** os canais.
        let distancia_ao_branco = |c: Color3D| {
            (1.0 - c.r) + (1.0 - c.g) + (1.0 - c.b)
        };
        assert!(
            distancia_ao_branco(no_talo) < distancia_ao_branco(parado),
            "no auge do drift a cor devia puxar para o branco"
        );

        // Parado não pulsa: dois instantes, o mesmo alfa. Um neon que pisca com
        // o carro parado lê como defeito.
        assert_eq!(parado.a, tinta_da_poca(cor, 0.0, 9.87).a);
        // Em drift pulsa.
        assert_ne!(
            no_talo.a,
            tinta_da_poca(cor, 1.0, 1.30).a,
            "em drift a poça devia pulsar"
        );

        // Entradas fora da faixa não produzem NaN nem alfa negativo.
        for deriva in [-3.0f32, 0.5, 7.0, f32::NAN] {
            let t = tinta_da_poca(cor, deriva, 3.0);
            assert!(t.a.is_finite() && (0.0..=1.0).contains(&t.a), "alfa {}", t.a);
            assert!((0.0..=1.0).contains(&t.r), "r {}", t.r);
        }
    }

    #[test]
    fn as_pocas_do_mesmo_tom_cabem_numa_chamada() {
        let azul = Color3D::new(0.2, 0.86, 1.0, 0.5);
        let verde = Color3D::new(0.55, 1.0, 0.35, 0.5);
        let m = matriz_de_carro(Rotacao::Nenhuma, PontoCore::ZERO, 0.0, 0.0);
        let grupos = agrupar_por_tinta(vec![
            (azul, m),
            (verde, m),
            (azul, m),
            (verde, m),
            (azul, m),
        ]);

        assert_eq!(grupos.len(), 2, "duas cores deviam dar dois lotes");
        assert_eq!(grupos[0].0, azul, "a ordem é a de chegada");
        assert_eq!(grupos[1].0, verde);
        assert_eq!(grupos[0].1.len(), 3);
        assert_eq!(grupos[1].1.len(), 2);
        assert_eq!(
            grupos.iter().map(|(_, m)| m.len()).sum::<usize>(),
            5,
            "nenhuma poça pode sumir no agrupamento"
        );
    }

    #[test]
    fn o_veu_escurece_o_menu_e_some_na_corrida() {
        // O alvo é o que o jogador vê: a tela de texto escurecida e a gameplay
        // limpa. Se `alvo_veu` devolvesse o mesmo número para as duas, o jogo
        // sairia escuro — ou o texto ficaria ilegível, que é o defeito original.
        assert_eq!(alvo_veu(Fase::Correndo), 0.0, "a gameplay é a tela cheia");
        assert!(alvo_veu(Fase::Titulo) > 0.5, "o título precisa de véu forte");
        assert!(
            alvo_veu(Fase::Titulo) > alvo_veu(Fase::Resultado),
            "o título tem mais texto sobre o cenário que o resultado"
        );
        for fase in [Fase::Titulo, Fase::Resultado, Fase::Correndo] {
            let v = alvo_veu(fase);
            assert!((0.0..=1.0).contains(&v), "{fase:?}: alfa {v}");
        }
    }

    #[test]
    fn o_veu_chega_ao_alvo_em_meio_segundo() {
        // A transição do título para a corrida tem de terminar antes da
        // contagem: um véu que ainda escurece quando o carro larga é o defeito
        // que esta mudança veio corrigir, só que ao contrário.
        let dt = 1.0 / 75.0;
        let mut veu = VEU_TITULO;
        let mut quadros = 0;
        while veu > 0.002 && quadros < 75 {
            veu = avancar_veu(veu, 0.0, dt);
            quadros += 1;
        }
        assert!(
            quadros <= 38,
            "o véu levou {quadros} quadros (mais de 0,5 s) para sumir"
        );
        assert_eq!(veu, 0.0, "o encaixe tem de zerar, não deixar 1e-7");
    }

    #[test]
    fn o_veu_sobe_e_desce_sem_pular() {
        // A troca tem de ser vista, não sentida: um salto de 0,62 para 0,0 num
        // quadro pisca a tela no "JOGAR", no instante em que o jogador começa a
        // olhar a pista.
        let dt = 1.0 / 75.0;
        let mut veu = VEU_TITULO;
        let mut anterior = veu;
        for _ in 0..40 {
            veu = avancar_veu(veu, 0.0, dt);
            let passo = anterior - veu;
            assert!(passo >= 0.0, "o véu voltou a subir na saída");
            assert!(passo <= 0.2, "salto de {passo:.3} num quadro — isso pisca");
            anterior = veu;
        }

        // E a volta (menu depois da corrida) também é gradual.
        let mut anterior = veu;
        for _ in 0..40 {
            veu = avancar_veu(veu, VEU_TITULO, dt);
            assert!(veu >= anterior - 1e-6, "o véu desceu ao reaparecer");
            assert!(veu - anterior <= 0.2);
            anterior = veu;
        }
        assert!((veu - VEU_TITULO).abs() < 1e-3);
    }

    #[test]
    fn o_veu_nunca_passa_do_alvo_nem_produz_nan() {
        let dt = 1.0 / 75.0;
        // `dt` zero (relógio parado) e negativo não movem nem estragam.
        for dt_ruim in [0.0f32, -0.5, f32::NAN] {
            assert_eq!(
                avancar_veu(VEU_TITULO, 0.0, dt_ruim),
                VEU_TITULO,
                "dt {dt_ruim} não pode mexer no véu"
            );
        }
        // Entradas corrompidas: um alfa `NaN` faria o driver descartar o quad.
        for (atual, alvo) in [
            (f32::NAN, 0.5f32),
            (0.5, f32::NAN),
            (-3.0, 2.0),
            (0.3, -1.0),
        ] {
            let v = avancar_veu(atual, alvo, dt);
            assert!(v.is_finite() && (0.0..=1.0).contains(&v), "{atual}/{alvo} → {v}");
        }
        // Convergência monotônica em passos grandes.
        let mut veu = 0.0;
        for _ in 0..200 {
            let novo = avancar_veu(veu, VEU_TITULO, 0.25);
            assert!(novo >= veu.min(VEU_TITULO) && novo <= VEU_TITULO);
            veu = novo;
        }
        assert_eq!(veu, VEU_TITULO);
    }

    #[test]
    fn a_camera_do_hud_olha_para_menos_z() {
        let c = camara_hud((640, 480));
        assert_eq!(c.position, Vec3::ZERO);
        assert_eq!(c.target, Vec3::new(0.0, 0.0, -1.0));
        assert!(c.aspect > 1.3 && c.aspect < 1.4, "aspecto {}", c.aspect);
    }
}
