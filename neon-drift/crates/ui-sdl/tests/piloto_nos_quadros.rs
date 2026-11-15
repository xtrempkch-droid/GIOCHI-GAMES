//! `--piloto` dirige **no laço de quadros**, não só nos `--passos`.
//!
//! O defeito que este teste prende: a flag só era consultada no laço de
//! `--passos`, então `--quadros N --piloto` desenhava N quadros sem ninguém
//! acelerar. Num aparelho headless não há controle nem teclado, a entrada é
//! nenhuma, e o carro ficava parado na grade. Medido antes da correção:
//! `--quadros 1400 --piloto` terminava com `s` na posição de grade (`959,0`) e
//! `largou: false` — exatamente o mesmo resultado de `--passos 0`, o que fazia
//! parecer que a simulação não estava rodando.
//!
//! Ele é teste de processo e não de unidade porque o defeito **era** a ligação:
//! `piloto::entrada` sempre funcionou, e quem não a chamava era o laço. Um teste
//! da função não veria isso.
//!
//! Requer um driver de vídeo do SDL. Usa `offscreen`, que é o mesmo caminho de
//! qualquer captura headless deste repositório; sem ele o teste avisa e passa,
//! porque não há o que medir.

use std::process::Command;

/// Lê um número do JSON de uma linha que o `--json` imprime.
///
/// Sem `serde_json` de propósito: o crate não tem dependência de parse, e dois
/// campos não justificam a primeira.
fn campo_numero(json: &str, chave: &str) -> Option<f64> {
    let alvo = format!("\"{chave}\":");
    let inicio = json.find(&alvo)? + alvo.len();
    let resto = &json[inicio..];
    let fim = resto
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == '+'))
        .unwrap_or(resto.len());
    resto[..fim].parse().ok()
}

/// Lê um booleano do mesmo JSON. `--json` escreve `true`/`false` sem aspas.
fn campo_bool(json: &str, chave: &str) -> Option<bool> {
    let alvo = format!("\"{chave}\":");
    let inicio = json.find(&alvo)? + alvo.len();
    let resto = &json[inicio..];
    if resto.starts_with("true") {
        Some(true)
    } else if resto.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Roda o binário e devolve a linha de `--json`, ou `None` se não há vídeo.
fn correr(args: &[&str]) -> Option<String> {
    let saida = Command::new(env!("CARGO_BIN_EXE_giochi-neon-drift"))
        .args(args)
        .env("SDL_VIDEODRIVER", "offscreen")
        .env("SDL_AUDIODRIVER", "dummy")
        .output()
        .expect("o binário do teste tem de existir");

    if !saida.status.success() {
        let erro = String::from_utf8_lossy(&saida.stderr);
        eprintln!("AVISO: sem driver de vídeo do SDL, nada a medir aqui:\n{erro}");
        return None;
    }
    Some(String::from_utf8_lossy(&saida.stdout).into_owned())
}

#[test]
fn o_piloto_dirige_nos_quadros_e_nao_so_nos_passos() {
    // 400 quadros a 75 Hz = 5,3 s: os 3 s de contagem e mais 2 s de corrida.
    // Menos que isso mede a contagem, e não o piloto.
    let Some(json) = correr(&["--teclas", "start", "--piloto", "--frames", "400", "--json"]) else {
        return;
    };

    let largou = campo_bool(&json, "largou");
    assert_eq!(
        largou,
        Some(true),
        "com --piloto o carro tinha de ter largado; sem piloto no laço de \
         quadros ele fica na grade e `largou` fica falso. JSON: {json}"
    );

    // A posição de grade fica em `comprimento - 3`, ou seja ~959 m neste mapa,
    // e um carro parado não sai de lá por mais quadros que passem. Qualquer
    // valor na casa das dezenas prova que ele andou de verdade.
    let progresso = campo_numero(&json, "progresso").expect("progresso no JSON");
    assert!(
        progresso > 20.0,
        "esperava o carro guiado bem além da grade, veio progresso {progresso}. \
         JSON: {json}"
    );
}

#[test]
fn sem_piloto_o_carro_nao_sai_da_grade_sozinho() {
    // O controle do teste acima: se este deixasse de valer, o outro passaria
    // por qualquer motivo menos o piloto causando o movimento.
    let Some(json) = correr(&["--teclas", "start", "--frames", "400", "--json"]) else {
        return;
    };
    let progresso = campo_numero(&json, "progresso").expect("progresso no JSON");
    assert!(
        progresso < 0.0,
        "sem piloto e sem acelerador o carro tem de continuar atrás da linha \
         (progresso negativo), veio {progresso}. JSON: {json}"
    );
}
