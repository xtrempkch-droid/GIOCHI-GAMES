//! Texturas procedurais: asfalto, concreto, rocha, mata, guarda-corpo, neon e as
//! duas **máscaras de luz** (halo da poça de neon e cone do farol).
//!
//! ## Por que gerar em vez de carregar PNG
//!
//! Duas razões, e a segunda é a que decide:
//!
//! 1. **O jogo não linka `SDL2_image`.** O `NEEDED` do binário é SDL2 +
//!    `libgcc_s` + `libm` + `libc`, e nada mais. Ler PNG exigiria essa
//!    biblioteca ou `libpng` — dependência nova para um recurso usado uma vez.
//!    (Já houve um motivo mais forte: a imagem de 07/11 do R35S foi compilada
//!    sem `--enable-png` e `IMG_Load` respondia "PNG images are not supported".
//!    A imagem de 09/11 corrigiu isso; a decisão não dependia dela e não muda.)
//! 2. **Uma textura é uma fórmula.** Asfalto é ruído escuro com pedrisco;
//!    concreto são blocos com junta; mata é ruído verde em duas escalas. Isso são
//!    vinte linhas cada, e gerar custa microssegundos no boot — sem arquivo para
//!    empacotar, sem caminho para faltar no aparelho e sem binário opaco no
//!    repositório.
//!
//! É a mesma decisão dos ícones do launcher (listas de linhas em vez de PNG) e
//! das malhas do carro (aritmética em vez de `.obj`).
//!
//! ## Determinismo
//!
//! O sorteio é um xorshift com semente fixa por textura. Duas execuções geram
//! bytes idênticos, então a comparação de quadro PC × aparelho continua valendo
//! — uma textura aleatória de verdade faria cada execução desenhar diferente e
//! nenhum teste de imagem poderia existir.
//!
//! ## Tudo é cinza-claro por baixo
//!
//! A cor final no shader é `amostra × tint`. As texturas nascem **neutras**
//! (cinza com variação) e a cor vem do `tint`, que é o que permite usar a mesma
//! textura de concreto no muro azulado da avenida e na pedra da serra sem gerar
//! duas texturas.

/// Lado das texturas, em pixels. 64 é potência de dois (exigência do GLES 2.0
/// para `REPEAT`) e dá densidade suficiente para 4 m de asfalto.
pub const LADO: u32 = 64;

/// Uma textura pronta para subir para a GPU.
pub struct Textura {
    /// Nome, para o diagnóstico.
    pub nome: &'static str,
    /// Largura em pixels.
    pub largura: u32,
    /// Altura em pixels.
    pub altura: u32,
    /// RGBA8, linha a linha.
    pub pixels: Vec<u8>,
}

impl Textura {
    /// Quantos bytes tem a textura.
    pub fn bytes(&self) -> usize {
        self.pixels.len()
    }
}

/// xorshift32 com semente, para o ruído ser reprodutível.
struct Sorteio(u32);

impl Sorteio {
    fn new(semente: u32) -> Self {
        // Semente zero trava o xorshift em zero para sempre.
        Self(if semente == 0 { 0x9e37_79b9 } else { semente })
    }

    fn proximo(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// `0.0..1.0`.
    fn unidades(&mut self) -> f32 {
        (self.proximo() >> 8) as f32 / 16_777_216.0
    }

}

/// Empacota um pixel RGBA.
fn px(r: f32, g: f32, b: f32, a: f32) -> [u8; 4] {
    [
        (r.clamp(0.0, 1.0) * 255.0) as u8,
        (g.clamp(0.0, 1.0) * 255.0) as u8,
        (b.clamp(0.0, 1.0) * 255.0) as u8,
        (a.clamp(0.0, 1.0) * 255.0) as u8,
    ]
}

/// Asfalto: ruído escuro com pedrisco claro e uma faixa de desgaste.
///
/// A textura é **acinzentada e clara**: o escuro vem do `tint` do material. Uma
/// textura já escura, multiplicada por um tint escuro, vira preto e o asfalto
/// some — foi o que aconteceu com a primeira tentativa de paleta do jogo.
pub fn asfalto() -> Textura {
    let lado = LADO;
    let mut s = Sorteio::new(0x7f4a_1c3b);
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            // Base: cinza médio com variação fina.
            let base = 0.84 + s.unidades() * 0.09;
            // Pedrisco: pontos claros esparsos, o que dá a leitura de asfalto
            // granulado de longe.
            let grao = if s.unidades() > 0.86 {
                0.10 + s.unidades() * 0.10
            } else {
                0.0
            };
            // Trinca: uma linha escura serpenteando no eixo X. Sem ela a textura
            // repete de forma visível a cada 4 m.
            let trinca = if (y as f32 + (x as f32 * 0.6).sin() * 3.0).rem_euclid(13.0) < 1.0 {
                -0.16
            } else {
                0.0
            };
            let v = base + grao + trinca;
            pixels.extend_from_slice(&px(v, v * 1.01, v * 1.05, 1.0));
        }
    }
    Textura {
        nome: "asfalto",
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Concreto: blocos com junta escura e manchas de umidade.
pub fn concreto() -> Textura {
    let lado = LADO;
    const BLOCO: u32 = 16;
    let mut s = Sorteio::new(0x2b91_55d3);
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            // Fiada alternada: a cada duas carreiras os blocos deslocam meio
            // bloco. Sem o deslocamento o muro lê como grade.
            let carreira = y / BLOCO;
            let desloc = if carreira % 2 == 0 { 0 } else { BLOCO / 2 };
            let na_junta = y % BLOCO == 0 || (x + desloc) % BLOCO == 0;
            let base = if na_junta {
                0.62
            } else {
                0.86 + s.unidades() * 0.07
            };
            // Mancha de umidade: ruído de baixa frequência (seno), porque um
            // segundo sorteio por pixel só faria chuvisco.
            let mancha = ((x as f32 * 0.14).sin() * (y as f32 * 0.11).cos()) * 0.06;
            let v = base + mancha;
            pixels.extend_from_slice(&px(v, v, v * 1.04, 1.0));
        }
    }
    Textura {
        nome: "concreto",
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Rocha: escura, com veios diagonais e aresta clara.
///
/// É o material da parede de pedra da serra — e o contraste com o concreto é o
/// que faz o mapa novo ler como outro lugar, e não como a avenida com outro
/// traçado.
pub fn rocha() -> Textura {
    let lado = LADO;
    let mut s = Sorteio::new(0x51c2_9a07);
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            let base = 0.62 + s.unidades() * 0.16;
            // Veio diagonal: duas frequências somadas dão um risco que não
            // repete no olho.
            let veio = ((x as f32 * 0.5 + y as f32 * 0.9).sin() * 0.5 + 0.5).powi(6);
            let v = base + veio * 0.22;
            // Leve tom quente na rocha: pedra pura cinza lê como concreto sujo.
            pixels.extend_from_slice(&px(v * 1.05, v, v * 0.93, 1.0));
        }
    }
    Textura {
        nome: "rocha",
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Mata atlântica vista de longe: manchas de verde em duas escalas.
///
/// Não é folha — é **copa**. A 10 m de distância o que se vê são massas verdes
/// com pontos de luz, e é isso que a textura entrega.
pub fn mata() -> Textura {
    let lado = LADO;
    let mut s = Sorteio::new(0x18f3_b627);
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            // Massa: seno cruzado em escala de 8 px.
            let massa = ((x as f32 * 0.8).sin() * (y as f32 * 0.72).cos()) * 0.5 + 0.5;
            // Folhagem: ruído fino por cima.
            let folha = s.unidades();
            let verde = 0.62 + massa * 0.22 + folha * 0.14;
            // Um em cada sete pixels é um ponto claro — reflexo de folha.
            let brilho = if s.unidades() > 0.86 { 0.16 } else { 0.0 };
            pixels.extend_from_slice(&px(verde * 0.42 + brilho, verde + brilho, verde * 0.40, 1.0));
        }
    }
    Textura {
        nome: "mata",
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Guarda-corpo: metal com montantes verticais.
///
/// A textura é aplicada no plano da chapa, então as faixas verticais viram as
/// nervuras — e elas passando depressa são metade da sensação de velocidade na
/// serra.
pub fn guarda_corpo() -> Textura {
    let lado = LADO;
    let mut s = Sorteio::new(0x6a05_2e11);
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            // Nervura a cada 8 px, com sombra de um lado e luz do outro.
            let fase = x % 8;
            let v = match fase {
                0 => 0.52,
                1 => 0.98,
                2 => 0.90,
                7 => 0.68,
                _ => 0.82 + s.unidades() * 0.05,
            };
            // Barra horizontal no meio: o perfil do guarda-corpo.
            let v = if (y as i32 - 32).abs() < 2 { v + 0.10 } else { v };
            let _ = y;
            pixels.extend_from_slice(&px(v, v, v * 1.02, 1.0));
        }
    }
    Textura {
        nome: "guarda-corpo",
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Painel de neon: faixas diagonais claras sobre fundo escuro.
///
/// A única textura com **alfa**: ela é aplicada nos outdoors e nas faixas
/// luminosas, e o alfa deixa o fundo da cena aparecer entre as faixas.
pub fn painel_neon() -> Textura {
    let lado = LADO;
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            // Faixa diagonal de 6 px a cada 16: `(x+y) mod 16 < 6`.
            let faixa = (x + y) % 16 < 6;
            if faixa {
                pixels.extend_from_slice(&px(1.0, 0.96, 0.92, 1.0));
            } else {
                // Fundo com alfa baixo: o painel "vaza" o neon em vez de ser uma
                // placa opaca no meio da noite.
                pixels.extend_from_slice(&px(0.10, 0.10, 0.14, 0.35));
            }
        }
    }
    Textura {
        nome: "painel-neon",
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Xadrez de largada/chegada: 8×8 casas em preto e branco.
pub fn xadrez() -> Textura {
    let lado = LADO;
    const CASA: u32 = 8;
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            let claro = ((x / CASA) + (y / CASA)) % 2 == 0;
            let v = if claro { 0.96 } else { 0.10 };
            pixels.extend_from_slice(&px(v, v, v, 1.0));
        }
    }
    Textura {
        nome: "xadrez",
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Faixa de pista: duas linhas longitudinais com tracejado.
///
/// Substitui a faixa central que era cor chapada (`m.faixas`). A textura dá o
/// tracejado **sem geometria**: um quad de 12 m mostra oito traços por repetição
/// em vez de oito quads, e o `u_tint` do desenho continua mandando na cor.
///
/// Fundo escuro e traço claro: o material é multiplicado pelo tint, então o que
/// é preto fica preto e o que é branco recebe a cor do pulso.
pub fn faixa_pista() -> Textura {
    let lado = LADO;
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            // Duas linhas em X (a faixa do meio, vista de cima) e tracejado em
            // Y de 8 em 8. A textura é aplicada com UV de mundo, então o
            // "8 de 8" vira 8 m de traço e 8 m de vazio com `tile` de 12 m.
            let na_linha = (2..6).contains(&(x % 16)) || (10..14).contains(&(x % 16));
            let no_traco = (y % 8) < 5;
            if na_linha && no_traco {
                pixels.extend_from_slice(&px(1.0, 1.0, 1.0, 1.0));
            } else {
                pixels.extend_from_slice(&px(0.06, 0.06, 0.08, 1.0));
            }
        }
    }
    Textura {
        nome: "faixa-pista",
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Grade de neon: linhas finas cruzadas sobre fundo escuro.
///
/// É a textura das fitas de advertência das barreiras. Grade e não traço
/// diagonal (que é o `painel-neon`) porque a leitura na pista é outra: o painel
/// é um **outdoor** visto de frente, a grade é uma fita **rasante** ao chão, e a
/// malha ortogonal sobrevive à compressão de perspectiva que a diagonal perde.
pub fn grade_neon() -> Textura {
    let lado = LADO;
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            let linha = x % 16 < 2 || y % 16 < 2;
            if linha {
                pixels.extend_from_slice(&px(1.0, 1.0, 1.0, 1.0));
            } else {
                pixels.extend_from_slice(&px(0.05, 0.05, 0.07, 1.0));
            }
        }
    }
    Textura {
        nome: "grade-neon",
        largura: lado,
        altura: lado,
        pixels,
    }
}

// ───────────────────────────── máscaras de luz ──────────────────────────────
//
// As duas texturas abaixo **não são materiais**: o RGB é branco em toda a
// imagem e o que desenha é o **alfa**, que cai do ponto quente até zero. Elas
// ficam fora de [`todas`] de propósito, pela mesma razão que o atlas de sprites
// fica: `todas()` é a lista que o `GIOCHI_TEXTURAS` troca por um arquivo
// pintado, e um halo pintado à mão perderia a única propriedade que importa
// (o alfa cair sem degrau). O sintoma de trocá-las por material é conhecido e
// está documentado no `cena3d.rs`: a poça de luz vira um retângulo de borda
// dura — foi o que havia antes destas máscaras.

/// Centro do texel em coordenadas `0.0..1.0`.
///
/// **Centro**, não canto: amostrar no canto faz a última coluna valer 1,0 e o
/// `LINEAR` da borda interpolar com um texel que não existe.
fn centro_do_texel(x: u32, y: u32, lado: u32) -> (f32, f32) {
    (
        (x as f32 + 0.5) / lado as f32,
        (y as f32 + 0.5) / lado as f32,
    )
}

/// Máscara branca com o alfa dado por uma função da posição do texel.
fn mascara(nome: &'static str, alfa: impl Fn(f32, f32) -> f32) -> Textura {
    let lado = LADO;
    let mut pixels = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            let (u, v) = centro_do_texel(x, y, lado);
            pixels.extend_from_slice(&px(1.0, 1.0, 1.0, alfa(u, v)));
        }
    }
    Textura {
        nome,
        largura: lado,
        altura: lado,
        pixels,
    }
}

/// Halo elíptico da luz indireta que o neon do carro joga no asfalto.
///
/// O alfa é `(1 - r)^1.2`, com `r` a distância normalizada ao centro: cheio
/// embaixo do carro e zero na borda do quad. Como o quad da poça tem a
/// proporção do carro (2,5 × 4,2 m), o círculo em UV vira a elipse em metros,
/// que é a forma certa — alongada no sentido do carro.
///
/// O expoente é **baixo de propósito** (1,2 e não 3): a luz de um neon não tem
/// bico, ela some devagar. Um expoente alto desenharia um ponto brilhante no
/// meio do carro e nada em volta, que lê como adesivo, não como luz.
pub fn halo_neon() -> Textura {
    mascara("halo-neon", |u, v| {
        let dx = (u - 0.5) * 2.0;
        let dy = (v - 0.5) * 2.0;
        let r = (dx * dx + dy * dy).sqrt();
        (1.0 - r).max(0.0).powf(1.2)
    })
}

/// Cone do farol projetado no asfalto.
///
/// `v` corre do para-choque (`0`) para a ponta do feixe (`1`); `u`, de lado a
/// lado. O alfa é o produto de duas quedas:
///
/// - **lateral**: o feixe é estreito junto do carro e abre com a distância, o
///   que dá o triângulo do cone dentro de um quad retangular;
/// - **de alcance**: a luz morre com a distância, então a ponta apaga.
///
/// Sem a primeira, o feixe seria uma faixa de largura constante — que é
/// exatamente o que havia antes e o que lia como "um retângulo azul no chão".
pub fn cone_farol() -> Textura {
    mascara("cone-farol", |u, v| {
        // Meia-largura do cone, em fração da meia-largura do quad.
        let meia_largura = 0.45 + 0.55 * v;
        let lateral = ((1.0 - ((u - 0.5).abs() * 2.0) / meia_largura).max(0.0)).powf(1.3);
        let alcance = (1.0 - v).powf(1.15);
        lateral * alcance
    })
}

/// Todas as texturas do jogo, na ordem em que são criadas.
pub fn todas() -> Vec<Textura> {
    vec![
        asfalto(),
        concreto(),
        rocha(),
        mata(),
        guarda_corpo(),
        painel_neon(),
        xadrez(),
        faixa_pista(),
        grade_neon(),
    ]
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn toda_textura_tem_o_tamanho_declarado() {
        for t in todas() {
            assert_eq!(t.largura, LADO, "{}", t.nome);
            assert_eq!(t.altura, LADO, "{}", t.nome);
            assert_eq!(
                t.bytes(),
                (LADO * LADO * 4) as usize,
                "{}: {} bytes não batem com {LADO}x{LADO} RGBA",
                t.nome,
                t.bytes()
            );
        }
    }

    #[test]
    fn a_geracao_e_deterministica() {
        // Sem isto cada execução desenharia um asfalto diferente e nenhuma
        // comparação PC × aparelho poderia existir.
        let a = asfalto();
        let b = asfalto();
        assert_eq!(a.pixels, b.pixels, "duas chamadas deram bitmaps diferentes");
    }

    #[test]
    fn nenhuma_textura_e_degenerada() {
        // Uma textura de cor única repete sem dar relevo e o material "some" no
        // meio da cena — o sintoma é o objeto parecer não ter textura.
        for t in todas() {
            let mut menor = 255u8;
            let mut maior = 0u8;
            for p in t.pixels.chunks(4) {
                menor = menor.min(p[0]);
                maior = maior.max(p[0]);
            }
            assert!(
                maior as i32 - menor as i32 > 20,
                "{}: variação de {} níveis — textura chapada",
                t.nome,
                maior as i32 - menor as i32
            );
        }
    }

    /// Texturas cujo papel é **padrão**, não superfície.
    ///
    /// `faixa-pista` e `grade-neon` são decalques: o fundo escuro é o asfalto e o
    /// que desenha são as linhas. Medir a média delas reprovaria uma textura
    /// correta — o que importa é que a **linha** chegue clara e cubra área
    /// suficiente para ser vista de longe.
    fn eh_decalque(nome: &str) -> bool {
        matches!(nome, "faixa-pista" | "grade-neon")
    }

    #[test]
    fn as_texturas_sao_claras_o_suficiente_para_o_tint_mandar() {
        // O material do jogo já é escuro (noite), e a cor final é
        // `amostra × tint`. Uma textura escura multiplicada por tint escuro vira
        // preto: foi o que apagou a primeira versão do asfalto.
        for t in todas() {
            // O **maior canal** de cada pixel, e não o vermelho: a mata é verde
            // de propósito, e medir o R dela reprovaria uma textura correta. O
            // que precisa ser claro é a luminância que o `tint` vai multiplicar.
            let luminancia: Vec<u8> = t
                .pixels
                .chunks(4)
                .map(|p| p[0].max(p[1]).max(p[2]))
                .collect();
            if eh_decalque(t.nome) {
                let claros = luminancia.iter().filter(|v| **v > 200).count();
                let fracao = claros as f32 / luminancia.len() as f32;
                assert!(
                    fracao > 0.05,
                    "{}: só {:.1}% de pixels claros — o padrão não vai aparecer",
                    t.nome,
                    fracao * 100.0
                );
                continue;
            }
            let media = luminancia.iter().map(|v| u64::from(*v)).sum::<u64>() / (LADO * LADO) as u64;
            assert!(
                media > 90,
                "{}: média de {media} de 255 — escura demais para receber tint",
                t.nome
            );
        }
    }

    #[test]
    fn o_painel_de_neon_tem_alfa_e_as_outras_nao() {
        let painel = painel_neon();
        let com_alfa = painel.pixels.chunks(4).filter(|p| p[3] < 250).count();
        assert!(com_alfa > 500, "o painel devia ter fundo translúcido: {com_alfa} px");

        for nome in ["asfalto", "concreto", "rocha", "mata", "guarda-corpo", "xadrez"] {
            let t = todas().into_iter().find(|t| t.nome == nome).unwrap();
            assert!(
                t.pixels.chunks(4).all(|p| p[3] == 255),
                "{nome} não devia ter alfa variável"
            );
        }
    }

    #[test]
    fn o_xadrez_alterna_casas() {
        let t = xadrez();
        let em = |x: u32, y: u32| t.pixels[((y * LADO + x) * 4) as usize];
        assert!(em(0, 0) > 200, "a primeira casa devia ser clara");
        assert!(em(8, 0) < 60, "a casa seguinte devia ser escura");
        assert!(em(0, 8) < 60, "a casa de baixo devia ser escura");
        assert!(em(8, 8) > 200, "a diagonal devia voltar a ser clara");
    }

    /// Alfa de um texel da máscara.
    fn alfa(t: &Textura, x: u32, y: u32) -> u8 {
        t.pixels[((y * LADO + x) * 4 + 3) as usize]
    }

    #[test]
    fn as_mascaras_de_luz_sao_brancas_e_so_o_alfa_varia() {
        // A cor final é `amostra × tint`. Se o RGB da máscara não fosse branco,
        // o neon de cada carro sairia com a cor do carro **vezes** a cor da
        // máscara, e as quatro cores da paleta deixariam de ser as quatro que o
        // jogador aprendeu a distinguir.
        for t in [halo_neon(), cone_farol()] {
            assert!(t.largura.is_power_of_two() && t.altura.is_power_of_two(), "{}", t.nome);
            assert!(
                t.pixels.chunks(4).all(|p| p[0] == 255 && p[1] == 255 && p[2] == 255),
                "{}: o RGB tem de ser branco em toda a máscara",
                t.nome
            );
            let com_alfa = t.pixels.chunks(4).filter(|p| p[3] < 250).count();
            assert!(
                com_alfa > LADO as usize * LADO as usize / 2,
                "{}: só {com_alfa} texels com alfa — não é um gradiente",
                t.nome
            );
        }
    }

    #[test]
    fn o_halo_apaga_do_centro_para_a_borda() {
        let t = halo_neon();
        let centro = alfa(&t, LADO / 2, LADO / 2);
        assert!(centro > 200, "o centro da poça devia ser quase opaco: {centro}");
        for (x, y) in [(0, 0), (LADO - 1, 0), (0, LADO - 1), (LADO - 1, LADO - 1)] {
            assert_eq!(alfa(&t, x, y), 0, "o canto ({x},{y}) devia estar apagado");
        }
        // Monotônico ao longo do raio horizontal: um halo que voltasse a
        // acender na borda desenharia um anel, não uma poça.
        let mut anterior = 255;
        for x in (LADO / 2)..LADO {
            let atual = alfa(&t, x, LADO / 2);
            assert!(atual <= anterior, "alfa subiu de {anterior} para {atual} em x={x}");
            anterior = atual;
        }
    }

    #[test]
    fn o_cone_do_farol_estreita_no_carro_e_apaga_na_ponta() {
        let t = cone_farol();

        // A largura iluminada cresce com a distância: é o que faz um quad
        // retangular ler como cone.
        let largura_em = |v: u32| {
            (0..LADO).filter(|x| alfa(&t, *x, v) > 40).count()
        };
        let junto = largura_em(2);
        let longe = largura_em(LADO / 2);
        assert!(
            longe > junto,
            "o feixe devia abrir: {junto} texels junto ao carro contra {longe} no meio"
        );

        // A ponta apaga e o para-choque acende.
        assert_eq!(largura_em(LADO - 1), 0, "a ponta do feixe devia apagar");
        assert!(
            alfa(&t, LADO / 2, 1) > 200,
            "junto ao para-choque o feixe devia estar cheio"
        );

        // A borda lateral do quad fica apagada em qualquer distância: sem isso
        // o cone teria a largura do quad e voltaria a ser uma faixa.
        for v in [0, LADO / 4, LADO / 2] {
            assert_eq!(alfa(&t, 0, v), 0, "a borda esquerda em v={v} devia apagar");
            assert_eq!(alfa(&t, LADO - 1, v), 0, "a borda direita em v={v} devia apagar");
        }
    }
}

// ───────────────────────────── textura vinda de arquivo ─────────────────────

/// Extensões procuradas, **na ordem em que são tentadas**.
///
/// `png` primeiro porque é o formato em que se pinta; `bmp` porque é o que o
/// aparelho desenha sem depender de `libpng` (AGENTS.md §24 — a imagem do R35S
/// já foi compilada sem `--enable-png`); `jpg`/`jpeg` porque quem faz textura
/// fotorreferenciada exporta assim.
pub const EXTENSOES: [&str; 4] = ["png", "bmp", "jpg", "jpeg"];

/// Variável de ambiente com o diretório das texturas de superfície.
///
/// Vazia ou ausente = as nove procedurais, que é o comportamento de sempre.
pub const VARIAVEL: &str = "GIOCHI_TEXTURAS";

/// O diretório pedido por `GIOCHI_TEXTURAS`, se houver um.
///
/// Não valida que existe: quem chama precisa distinguir "não pediram" de "pediram
/// e o caminho está errado", e as duas coisas levam a mensagens diferentes.
pub fn diretorio() -> Option<std::path::PathBuf> {
    let valor = std::env::var(VARIAVEL).ok()?;
    let valor = valor.trim();
    if valor.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(valor))
}

/// O arquivo de uma textura num diretório, tentando `EXTENSOES` em ordem.
///
/// Devolve `None` quando nenhuma das quatro existe — e isso **não** é erro: usar
/// a procedural para o material que não veio é o comportamento documentado, e a
/// queda é avisada por quem chama (a regra do §32: queda silenciosa faz o usuário
/// concluir que a arte dele não serve, quando o que falhou foi o caminho).
///
/// Existe como função pura, e não embutida no upload, porque o upload precisa de
/// contexto GL e portanto não roda em teste. Resolver caminho é a metade que
/// pode ser afirmada sem GPU.
pub fn arquivo_para(diretorio: &std::path::Path, nome: &str) -> Option<std::path::PathBuf> {
    EXTENSOES.iter().find_map(|ext| {
        let caminho = diretorio.join(format!("{nome}.{ext}"));
        caminho.is_file().then_some(caminho)
    })
}


#[cfg(test)]
mod testes_de_arquivo {
    use super::*;

    /// Diretório temporário próprio do teste, sem dependência nova.
    ///
    /// `std::env::temp_dir()` + pid: dois testes do mesmo binário não colidem, e
    /// um teste que falha no meio não deixa lixo no diretório do usuário.
    fn temp(nome: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("giochi-texturas-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("criar temp");
        dir
    }

    #[test]
    fn arquivo_para_acha_a_extensao_certa() {
        // Cada extensão aceita precisa ser encontrada sozinha: procurar só "png"
        // faria a textura em BMP sumir sem mensagem — e BMP é justamente o que o
        // aparelho desenha sem depender de `libpng`.
        for ext in EXTENSOES {
            let dir = temp(ext);
            std::fs::write(dir.join(format!("asfalto.{ext}")), b"x").unwrap();
            let achado = arquivo_para(&dir, "asfalto").expect("extensão aceita");
            assert_eq!(achado.extension().and_then(|e| e.to_str()), Some(ext));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn arquivo_para_prefere_png_quando_ha_dois() {
        // A ordem de `EXTENSOES` é contrato: quem exporta PNG e BMP com o mesmo
        // nome espera que o PNG vença, senão edita o PNG e o jogo mostra o BMP.
        let dir = temp("ordem");
        std::fs::write(dir.join("rocha.bmp"), b"bmp").unwrap();
        std::fs::write(dir.join("rocha.png"), b"png").unwrap();
        let achado = arquivo_para(&dir, "rocha").expect("achou");
        assert_eq!(achado.extension().and_then(|e| e.to_str()), Some("png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn arquivo_para_devolve_none_sem_arquivo() {
        // `None` não é erro: é o material que continua procedural. O que não pode
        // acontecer é devolver um caminho que não existe — o `create_texture`
        // devolveria erro e a textura sairia preta.
        let dir = temp("vazio");
        assert!(arquivo_para(&dir, "asfalto").is_none());
        assert!(arquivo_para(&dir.join("nao-existe"), "asfalto").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn o_nome_do_arquivo_e_o_do_material_e_nao_o_do_arquivo_de_origem() {
        // O nome vem de `todas()`, que é o que o log imprime. Um material com o
        // nome trocado faria o usuário pintar `asphalt.png` e nada acontecer.
        let dir = temp("nomes");
        let nomes: Vec<&str> = todas().iter().map(|t| t.nome).collect();
        for nome in &nomes {
            std::fs::write(dir.join(format!("{nome}.bmp")), b"x").unwrap();
            assert!(arquivo_para(&dir, nome).is_some(), "{nome}");
        }
        assert!(arquivo_para(&dir, "asphalt").is_none(), "nome em inglês não é contrato");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
