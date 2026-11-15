//! Caminho dos modelos `.obj` e o fallback embutido.
//!
//! ## Por que há um modelo embutido
//!
//! O `bin/giochi-neon-drift` é instalado sozinho em `/user/apps/neon-drift`: não
//! existe um diretório `assets/` ao lado dele. Se o carro dependesse de um `.obj`
//! só no disco, o jogo abriria sem lataria — e o log diria "modelo ausente" longe
//! da tela preta.
//!
//! A ordem tenta o arquivo do artista primeiro (`GIOCHI_MODELOS`, depois
//! `assets/modelos` ao lado do executável ou do projeto) e, se não achar, grava
//! o modelo embutido num diretório temporário e o carrega pelo mesmo
//! `render.load_obj_mesh`. Assim o caminho de carga é **um só** nos dois casos:
//! não há um "modo embutido" que esconde defeito de triangulação.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Nome do arquivo da lataria do carro.
pub const CARRO: &str = "carro.obj";
/// Nome do arquivo do portal de checkpoint.
pub const PORTAL: &str = "portal.obj";

/// Variável que aponta para um diretório de modelos do artista.
pub const VARIAVEL: &str = "GIOCHI_MODELOS";

const CARRO_EMBUTIDO: &str = include_str!("../../../assets/modelos/carro.obj");
const PORTAL_EMBUTIDO: &str = include_str!("../../../assets/modelos/portal.obj");

/// Diretório temporário usado quando nenhum arquivo externo existe.
fn cache_dir() -> PathBuf {
    std::env::temp_dir().join("giochi-neon-drift-modelos")
}

/// Procura `nome` nos lugares que fazem sentido nesta ordem.
fn procurar(nome: &str) -> Option<PathBuf> {
    let mut candidatos: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var(VARIAVEL) {
        if !dir.trim().is_empty() {
            candidatos.push(PathBuf::from(dir).join(nome));
        }
    }
    candidatos.push(PathBuf::from("assets/modelos").join(nome));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidatos.push(dir.join("modelos").join(nome));
            candidatos.push(dir.join("assets/modelos").join(nome));
            candidatos.push(dir.join("../assets/modelos").join(nome));
        }
    }
    candidatos.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/modelos")
            .join(nome),
    );
    candidatos.into_iter().find(|p| p.is_file())
}

/// Devolve um caminho existente para `nome`, gravando a cópia embutida se preciso.
///
/// `nome` é [`CARRO`] ou [`PORTAL`]; outro nome é erro de programação e volta
/// como `None` — sem pânico no laço de abertura do jogo.
pub fn caminho(nome: &str) -> Option<PathBuf> {
    static RESOLVIDOS: OnceLock<[Option<PathBuf>; 2]> = OnceLock::new();
    let resolvidos = RESOLVIDOS.get_or_init(|| {
        let carro = resolver(CARRO, CARRO_EMBUTIDO);
        let portal = resolver(PORTAL, PORTAL_EMBUTIDO);
        if carro.is_none() {
            eprintln!("neon-drift: modelo {CARRO} indisponível");
        }
        if portal.is_none() {
            eprintln!("neon-drift: modelo {PORTAL} indisponível");
        }
        [carro, portal]
    });
    match nome {
        CARRO => resolvidos[0].clone(),
        PORTAL => resolvidos[1].clone(),
        _ => None,
    }
}

fn resolver(nome: &str, embutido: &str) -> Option<PathBuf> {
    if let Some(caminho) = procurar(nome) {
        eprintln!(
            "neon-drift: modelo '{nome}' de arquivo: {}",
            caminho.display()
        );
        return Some(caminho);
    }

    let dir = cache_dir();
    if let Err(erro) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "neon-drift: modelo '{nome}' sem arquivo e sem cache ({}): {erro}",
            dir.display()
        );
        return None;
    }
    let destino = dir.join(nome);
    // Compare antes de escrever: o arquivo embutido só muda quando o binário
    // muda, e reescrever a cada boot gasta I/O num aparelho com flash lenta.
    let precisa = std::fs::read_to_string(&destino)
        .map(|atual| atual != embutido)
        .unwrap_or(true);
    if precisa {
        if let Err(erro) = std::fs::write(&destino, embutido) {
            eprintln!(
                "neon-drift: não gravei o modelo embutido '{}': {erro}",
                destino.display()
            );
            return None;
        }
    }
    eprintln!(
        "neon-drift: modelo '{nome}' embutido em {}",
        destino.display()
    );
    Some(destino)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn os_modelos_embutidos_sao_obj_triangulados() {
        for (nome, texto) in [(CARRO, CARRO_EMBUTIDO), (PORTAL, PORTAL_EMBUTIDO)] {
            assert!(texto.contains("\nv "), "{nome} sem vértices");
            assert!(texto.contains("\nf "), "{nome} sem faces");
            // Triangulação: nenhuma face pode ter mais de três índices. O
            // `load_obj` triangula, mas um arquivo já triangulado não depende
            // do parser para estar certo.
            for linha in texto.lines().filter(|l| l.starts_with("f ")) {
                let indices = linha.split_whitespace().skip(1).count();
                assert_eq!(indices, 3, "{nome}: face com {indices} índices: {linha}");
            }
        }
    }

    /// Vértices e triângulos de um OBJ embutido, sem normais.
    fn triangulos(texto: &str) -> (Vec<[f32; 3]>, Vec<[usize; 3]>) {
        let mut vertices = Vec::new();
        let mut faces = Vec::new();
        for linha in texto.lines() {
            let mut campos = linha.split_whitespace();
            match campos.next() {
                Some("v") => {
                    let c: Vec<f32> = campos.map(|v| v.parse().expect("número")).collect();
                    vertices.push([c[0], c[1], c[2]]);
                }
                Some("f") => {
                    let c: Vec<usize> = campos
                        .map(|v| v.split('/').next().unwrap().parse::<usize>().expect("índice") - 1)
                        .collect();
                    assert_eq!(c.len(), 3, "face não triangulada: {linha}");
                    faces.push([c[0], c[1], c[2]]);
                }
                _ => {}
            }
        }
        (vertices, faces)
    }

    /// O winding é anti-horário visto de **fora**, conferido por volume.
    ///
    /// Para uma malha fechada e consistentemente orientada, o volume com sinal
    /// (teorema da divergência, `V = Σ a·(b×c) / 6`) é **positivo** com winding
    /// anti-horário e **negativo** com winding invertido. É a única medida que
    /// não se engana com peça longe do centroide: a versão anterior deste teste
    /// comparava cada normal com a direção radial do centro do modelo, e as
    /// colunas do portal — longe do centro — reprovavam mesmo corretas.
    ///
    /// Cada **peça** (componente conexa do grafo de vértices) é medida
    /// separadamente, porque um modelo com duas peças pode ter uma certa e a
    /// outra invertida; o volume somado esconderia justamente metade do erro.
    ///
    /// Este é o defeito invisível de um `.obj`: com a ordem trocada a peça
    /// **continua aparecendo** — você vê o interior do lado oposto — e o único
    /// sintoma é a iluminação errada. As quatro faces laterais da lataria
    /// nasceram assim.
    fn volumes_positivos_por_peca(texto: &str, nome: &str) {
        let (vertices, faces) = triangulos(texto);

        // Union-find: cada peça é um conjunto de vértices ligados por faces.
        let mut pai: Vec<usize> = (0..vertices.len()).collect();
        fn raiz(pai: &mut [usize], mut i: usize) -> usize {
            while pai[i] != i {
                pai[i] = pai[pai[i]];
                i = pai[i];
            }
            i
        }
        for [a, b, c] in &faces {
            let (ra, rb, rc) = (raiz(&mut pai, *a), raiz(&mut pai, *b), raiz(&mut pai, *c));
            pai[rb] = ra;
            let rc = raiz(&mut pai, rc);
            pai[rc] = ra;
        }

        let mut volumes: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
        for [a, b, c] in &faces {
            let (pa, pb, pc) = (vertices[*a], vertices[*b], vertices[*c]);
            let cruz = [
                pb[1] * pc[2] - pb[2] * pc[1],
                pb[2] * pc[0] - pb[0] * pc[2],
                pb[0] * pc[1] - pb[1] * pc[0],
            ];
            let seis_vezes = pa[0] * cruz[0] + pa[1] * cruz[1] + pa[2] * cruz[2];
            let peca = raiz(&mut pai, *a);
            *volumes.entry(peca).or_insert(0.0) += seis_vezes / 6.0;
        }

        assert!(!volumes.is_empty(), "{nome} sem faces");
        for (peca, volume) in &volumes {
            assert!(
                *volume > 0.0,
                "{nome}: peça {peca} tem volume {volume:.4} (≤ 0) — winding invertido"
            );
        }
    }

    #[test]
    fn a_lataria_tem_winding_para_fora() {
        volumes_positivos_por_peca(CARRO_EMBUTIDO, CARRO);
    }

    #[test]
    fn o_portal_tem_winding_para_fora() {
        volumes_positivos_por_peca(PORTAL_EMBUTIDO, PORTAL);
    }

    #[test]
    fn a_lataria_cabe_na_caixa_de_uma_vaga() {
        // O carro desenhado tem de ocupar o mesmo volume que a malha procedural
        // antiga: 5 m de comprimento, 2 m de largura e menos de 1 m de altura.
        let (vertices, _) = triangulos(CARRO_EMBUTIDO);
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &vertices {
            for eixo in 0..3 {
                min[eixo] = min[eixo].min(v[eixo]);
                max[eixo] = max[eixo].max(v[eixo]);
            }
        }
        assert!(max[0] - min[0] < 2.1, "largura {}", max[0] - min[0]);
        assert!(max[1] - min[1] < 1.1, "altura {}", max[1] - min[1]);
        assert!(max[2] - min[2] < 5.1, "comprimento {}", max[2] - min[2]);
        // Nariz em -Z e teto em +Y: o contrato de `malha::carro()`.
        assert!(min[2] < -2.0 && max[2] > 2.0);
        assert!(min[1] >= 0.0);
    }

    #[test]
    fn um_nome_desconhecido_nao_quebra() {
        assert!(caminho("nao-existe.obj").is_none());
    }

    #[test]
    fn o_caminho_resolvido_existe_e_tem_conteudo() {
        for nome in [CARRO, PORTAL] {
            let caminho = caminho(nome).unwrap_or_else(|| panic!("{nome} não resolveu"));
            let tamanho = std::fs::metadata(&caminho)
                .unwrap_or_else(|e| panic!("{}: {e}", caminho.display()))
                .len();
            assert!(tamanho > 100, "{nome} com {tamanho} bytes");
        }
    }
}
