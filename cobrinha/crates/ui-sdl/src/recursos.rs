//! Onde estão os assets e como a textura da fruta é desenhada.
//!
//! O jogo pode ser iniciado de qualquer diretório (o launcher chama o binário
//! com o cwd dele), por isso um caminho relativo fixo não serve. A procura passa
//! pelo `GIOCHI_ASSETS`, pelo cwd e por dois níveis acima do executável — que é
//! onde `bin/../assets` cai no pacote do aparelho.

use std::path::PathBuf;

/// Procura um asset relativo em todos os lugares em que ele pode estar.
pub fn procurar(relativo: &str) -> Option<PathBuf> {
    let mut tentativas = Vec::with_capacity(4);
    if let Some(raiz) = std::env::var_os("GIOCHI_ASSETS") {
        tentativas.push(PathBuf::from(raiz).join(relativo));
    }
    tentativas.push(PathBuf::from(relativo));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            tentativas.push(dir.join(relativo));
            if let Some(pai) = dir.parent() {
                tentativas.push(pai.join(relativo));
                if let Some(avo) = pai.parent() {
                    tentativas.push(avo.join(relativo));
                }
            }
        }
    }
    tentativas
        .into_iter()
        .find(|caminho| caminho.is_file())
        .map(absoluto)
}

/// Caminho absoluto quando o sistema deixa, para o vigia de hot reload comparar
/// eventos (que o `notify` entrega absolutos) com os handles registados.
pub fn absoluto(caminho: PathBuf) -> PathBuf {
    caminho.canonicalize().unwrap_or(caminho)
}

/// A textura da maçã: uma atlas 128x128 com a casca, o cabo e a folha.
///
/// É procedural de propósito: o jogo continua com **um** binário e nenhum PNG
/// para o empacotador esquecer. A textura é POT, então a engine pede mipmaps e
/// `GL_REPEAT` — e as UVs do OBJ ficam dentro das faixas certas.
pub fn textura_maca() -> (u32, u32, Vec<u8>) {
    const LADO: u32 = 128;
    let mut pixels = vec![0u8; (LADO * LADO * 4) as usize];
    let mut ruido = 0x9E37_79B9u32;

    for y in 0..LADO {
        let v = (y as f32 + 0.5) / LADO as f32;
        for x in 0..LADO {
            let u = (x as f32 + 0.5) / LADO as f32;
            ruido ^= ruido << 13;
            ruido ^= ruido >> 17;
            ruido ^= ruido << 5;
            let grao = ((ruido & 0xFFFF) as f32 / 65535.0) - 0.5;

            let (r, g, b, a) = if u < 0.62 {
                // Casca: vinho no centro, mais escura nas bordas, com pintinhas.
                let au = u / 0.62;
                let centro = 1.0 - ((au - 0.5).powi(2) + (v - 0.45).powi(2)).sqrt();
                let brilho = (0.72 + 0.30 * centro.max(0.0) + 0.05 * grao).clamp(0.0, 1.0);
                let cova = if v < 0.075 && (0.35..=0.60).contains(&au) {
                    0.45
                } else {
                    1.0
                };
                let pinta = if ruido % 37 == 0 { 0.35 } else { 1.0 };
                (
                    (196.0 * brilho * cova * pinta) as u8,
                    (42.0 * brilho * cova * pinta) as u8,
                    (58.0 * brilho * cova * pinta) as u8,
                    255,
                )
            } else if (0.66..0.98).contains(&u) && (0.02..0.21).contains(&v) {
                // Cabo: castanho, mais claro numa das faces.
                let luz = 0.75 + 0.35 * (u - 0.66) / 0.32;
                (
                    (92.0 * luz) as u8,
                    (58.0 * luz) as u8,
                    (34.0 * luz) as u8,
                    255,
                )
            } else if (0.68..0.98).contains(&u) && (0.26..0.52).contains(&v) {
                // Folha: verde com uma nervura clara ao centro.
                let au = (u - 0.68) / 0.30;
                let nervura = 1.0 - (au - 0.5).abs() * 1.8;
                let luz = 0.65 + 0.45 * nervura.max(0.0) + 0.06 * grao;
                (
                    (42.0 * luz) as u8,
                    (150.0 * luz) as u8,
                    (58.0 * luz) as u8,
                    255,
                )
            } else {
                (14, 18, 30, 255)
            };
            let i = ((y * LADO + x) * 4) as usize;
            pixels[i] = r;
            pixels[i + 1] = g;
            pixels[i + 2] = b;
            pixels[i + 3] = a;
        }
    }
    (LADO, LADO, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_textura_tem_o_tamanho_pedido_e_nao_e_toda_preta() {
        let (w, h, pixels) = textura_maca();
        assert_eq!((w, h), (128, 128));
        assert_eq!(pixels.len(), (w * h * 4) as usize);
        assert!(pixels.chunks_exact(4).any(|p| p[0] > 80 && p[1] < 90));
        assert!(pixels.chunks_exact(4).any(|p| p[1] > 80));
    }

    #[test]
    fn a_procura_acha_o_modelo_do_repositorio() {
        // O teste roda na raiz do workspace (crates/ui-sdl), então o asset está
        // em `../../assets`.
        let caminho = procurar("../../assets/modelos/maca.obj")
            .or_else(|| procurar("assets/modelos/maca.obj"));
        assert!(caminho.is_some(), "maca.obj não encontrado a partir do cwd");
    }
}
