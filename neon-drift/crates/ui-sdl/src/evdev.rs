//! Leitura dos botões do controle por evdev, com **estado** e não eventos.
//!
//! ## Por que bitmask, e não fila de ações
//!
//! A cobrinha consome ações discretas ("virou para cima agora"), porque lá cada
//! toque vale uma virada. Corrida é o oposto: acelerar e virar são **contínuos**,
//! e o que o jogo precisa saber a cada quadro é "o A está apertado agora?".
//!
//! Guardar os bits de pressionado num `AtomicU32` compartilhado resolve isso com
//! uma thread por dispositivo e nenhum bloqueio no laço. O custo é que o estado
//! é global ao processo — e é por isso que `--teclas` no PC entra por outro
//! caminho (`entrada_jogo::combinar`), em vez de fingir um evdev.
//!
//! ## Um bit por botão **físico**, e o significado na fusão
//!
//! A máscara diz "o botão A está apertado", não "o carro está acelerando". Quem
//! traduz botão em intenção é `entrada_jogo::combinar_com_analogico`, onde as
//! regras dão para testar sem aparelho. Duas consequências que parecem detalhe e
//! não são:
//!
//! 1. **O direcional para cima não acende `ACELERAR`.** Se acendesse, soltar o
//!    direcional apagaria o bit do A que o jogador continua segurando — e o
//!    carro pararia de acelerar sozinho. Quem anda com o direcional usa `CIMA`,
//!    que a fusão também lê como acelerar.
//! 2. **O caminho do SDL (`gamepad.rs`) fala a mesma língua, mas o mapa dele
//!    depende do `gamecontrollerdb`.** Sem o contrato, o SDL chama de
//!    `Button::A` o primeiro botão do device, que aqui é `BTN_SOUTH` — o B do
//!    plástico. Por isso as duas máscaras **não** são somadas: quem manda é
//!    `entrada_jogo::dono` (issues #1 e #2), uma fonte por quadro.
//! 3. **Nem todo nó de `/dev/input` é controle.** O bitmask de
//!    `capabilities/key` diz quais códigos o nó emite, e só os que declaram
//!    código que o jogo usa ganham thread de leitura (`Botoes::controles`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

/// Botão A: acelerar com o dedo, e aceitar a linha do menu.
pub const ACELERAR: u32 = 1 << 0;
/// Botão B: frear / ré.
pub const FREAR: u32 = 1 << 1;
/// Direcional para a esquerda.
pub const ESQUERDA: u32 = 1 << 2;
/// Direcional para a direita.
pub const DIREITA: u32 = 1 << 3;
/// Freio de mão (L1 ou R1).
pub const FREIO_MAO: u32 = 1 << 4;
/// Nitro (L2 ou R2).
pub const NITRO: u32 = 1 << 5;
/// Confirmar a linha do menu (o **mesmo** A que acelera).
///
/// É bit separado porque no menu o carro está parado e acelerar não significa
/// nada: assim o A pode aceitar sem que a fusão precise saber em que tela está.
pub const ACEITAR: u32 = 1 << 6;
/// Voltar: do menu para a corrida? Não — da corrida para o **menu**, e do menu
/// para fora do jogo. (Y ou SELECT.)
pub const VOLTAR: u32 = 1 << 7;
/// Jogar/reiniciar sem passar pelo cursor (START).
pub const INICIAR: u32 = 1 << 8;
/// Overlay de desempenho (X).
pub const PERFIL: u32 = 1 << 9;
/// Direcional para cima.
pub const CIMA: u32 = 1 << 10;
/// Direcional para baixo.
pub const BAIXO: u32 = 1 << 11;

/// Códigos evdev do jogo.
///
/// Medidos nos `buttons.map` dos aparelhos. O direcional está na faixa
/// `BTN_DPAD_*` (544-547), não em `KEY_*`.
///
/// **Estes são os valores da convenção X55/X35H/R35S** (`a = BTN_EAST`), e
/// servem de tabela embutida. A **X35S** troca A e B de propósito
/// (`a = BTN_SOUTH`), e é por isso que a tabela em vigor é [`Mapa`], lida de
/// `/etc/giochi/buttons.map`, e não um `match` fixo.
pub mod codigos {
    /// Direcional para cima.
    pub const DPAD_UP: u16 = 544;
    /// Direcional para baixo.
    pub const DPAD_DOWN: u16 = 545;
    /// Direcional para a esquerda.
    pub const DPAD_LEFT: u16 = 546;
    /// Direcional para a direita.
    pub const DPAD_RIGHT: u16 = 547;
    /// Botão B do plástico (`BTN_SOUTH`).
    pub const SUL: u16 = 304;
    /// Botão A do plástico (`BTN_EAST`).
    pub const LESTE: u16 = 305;
    /// Botão X (`BTN_NORTH`).
    pub const NORTE: u16 = 307;
    /// Botão Y (`BTN_WEST`).
    pub const OESTE: u16 = 308;
    /// Ombro L1.
    pub const TL: u16 = 310;
    /// Ombro R1.
    pub const TR: u16 = 311;
    /// Gatilho L2.
    pub const TL2: u16 = 312;
    /// Gatilho R2.
    pub const TR2: u16 = 313;
    /// SELECT.
    pub const SELECT: u16 = 314;
    /// START.
    pub const START: u16 = 315;
}

/// Bit do jogo para o **nome lógico** do `buttons.map`.
///
/// É o nome do plástico ("a", "b", "l2"), não o código do kernel: quem sabe
/// qual código é o A nesta placa é o `buttons.map`, e é isso que permite jogar
/// na X55 (`a = BTN_EAST`) e na X35S (`a = BTN_SOUTH`) com o **mesmo** binário.
///
/// **Um botão, um conjunto próprio de bits.** Nenhum bit é acendido por dois
/// botões diferentes, senão soltar um apagaria o outro (ver o cabeçalho).
pub fn bit_do_nome(nome: &str) -> Option<u32> {
    Some(match nome {
        // O A acelera **e** aceita: os dois bits nascem e morrem juntos porque
        // são o mesmo botão.
        "a" => ACELERAR | ACEITAR,
        "b" => FREAR,
        "up" => CIMA,
        "down" => BAIXO,
        "left" => ESQUERDA,
        "right" => DIREITA,
        // L1 e R1 fazem o mesmo (freio de mão), L2 e R2 o mesmo (nitro): num
        // aparelho de duas mãos, tanto faz o dedo.
        "l" | "r" => FREIO_MAO,
        "l2" | "r2" => NITRO,
        "x" => PERFIL,
        // Y e SELECT fazem o mesmo: o Y é onde o polegar já está e o SELECT é o
        // que o desenho do plástico chama de "menu".
        "y" | "select" => VOLTAR,
        "start" => INICIAR,
        _ => return None,
    })
}

/// Código evdev de um nome de símbolo do `buttons.map` (`BTN_EAST` → 305).
fn codigo_do_simbolo(simbolo: &str) -> Option<u16> {
    use codigos::*;
    Some(match simbolo {
        // Os dois nomes modernos e os antigos (`BTN_A`…) convivem nos mapas.
        "BTN_SOUTH" | "BTN_A" | "BTN_GAMEPAD" => SUL,
        "BTN_EAST" | "BTN_B" => LESTE,
        "BTN_NORTH" | "BTN_X" => NORTE,
        "BTN_WEST" | "BTN_Y" => OESTE,
        "BTN_TL" => TL,
        "BTN_TR" => TR,
        "BTN_TL2" => TL2,
        "BTN_TR2" => TR2,
        "BTN_SELECT" => SELECT,
        "BTN_START" => START,
        "BTN_DPAD_UP" => DPAD_UP,
        "BTN_DPAD_DOWN" => DPAD_DOWN,
        "BTN_DPAD_LEFT" => DPAD_LEFT,
        "BTN_DPAD_RIGHT" => DPAD_RIGHT,
        _ => return None,
    })
}

/// Tabela **código evdev → bits do jogo** em vigor.
///
/// ## Por que ela é um dado e não um `match`
///
/// O código do A muda de placa para placa: no X55, X35H e R35S o `buttons.map`
/// diz `a = BTN_EAST` (305); na **X35S** diz `a = BTN_SOUTH` (304) — o contrário.
/// Uma tabela compilada acerta três aparelhos e erra o quarto, e o sintoma é
/// "o A freia", que já foi a issue #1 do R35S.
///
/// O sistema instala `/etc/giochi/buttons.map` em todas as imagens
/// (`giochi-buttons` e `giochi-selftest` leem o mesmo arquivo). Este jogo lê o
/// arquivo — é artefato declarado, não caminho presumido — e cai na tabela
/// embutida quando ele não existe, que é o caso do PC.
#[derive(Debug, Clone)]
pub struct Mapa {
    tabela: std::collections::HashMap<u16, u32>,
}

/// Onde a imagem instala o mapa do plástico.
pub const CAMINHO_DO_MAPA: &str = "/etc/giochi/buttons.map";

/// Nome do controle virtual que o GIOCHI-SYSTEM publica (issue #2).
///
/// Um nó só, 17 botões de códigos fixos e 4 eixos em -32767..32767. É a
/// **tradução**: o `buttons.map` descreve o hardware físico de cada placa, e o
/// virtual normaliza tudo para o layout da X55 (`a = BTN_EAST`). Por isso o
/// mapa que se aplica a ele é [`Mapa::padrao`] e **não** o da placa — na X35S,
/// aplicar o mapa do aparelho ao virtual inverteria o A/B.
pub const NOME_DO_CONTRATO: &str = "Giochi Gamepad";

impl Mapa {
    /// A tabela do **contrato**, e a reserva de quem não tem mapa.
    ///
    /// É o layout fixo do `Giochi Gamepad` (`a = BTN_EAST`, `b = BTN_SOUTH`,
    /// `l`/`r` nos ombros, `l2`/`r2` nos gatilhos) — que por acaso é o mesmo do
    /// `buttons.map` da X55, X35H e R35S. Não é o mapa "do hardware": é o mapa
    /// **da tradução**, e é ele que vale para o nó virtual em qualquer placa.
    pub fn padrao() -> Self {
        let mut tabela = std::collections::HashMap::new();
        for codigo in 0..CODIGOS_POSSIVEIS {
            if let Some(bit) = bit_do_codigo(codigo as u16) {
                tabela.insert(codigo as u16, bit);
            }
        }
        Self { tabela }
    }

    /// Lê um `buttons.map` em texto. `None` = não serve (sem `a = BTN_*`).
    pub fn do_texto(texto: &str) -> Option<Self> {
        let mut tabela = std::collections::HashMap::new();
        for linha in texto.lines() {
            // `# ` é comentário até o fim da linha, e o mapa comenta muito.
            let linha = linha.split('#').next().unwrap_or_default();
            let Some((nome, simbolo)) = linha.split_once('=') else {
                continue;
            };
            let Some(bit) = bit_do_nome(nome.trim()) else {
                continue;
            };
            if let Some(codigo) = codigo_do_simbolo(simbolo.trim()) {
                tabela.insert(codigo, bit);
            }
        }
        // Sem o `a` o arquivo não descreve um controle jogável: é recorte,
        // exemplo ou lixo. Aí vale a tabela embutida.
        if tabela.values().any(|b| b & ACELERAR != 0) {
            Some(Self { tabela })
        } else {
            None
        }
    }

    /// O mapa instalado pela imagem, se houver.
    ///
    /// `GIOCHI_MAPA_BOTOES=<arquivo>` aponta para outro mapa — é o que permite
    /// exercitar a convenção de outra placa no PC, e o que dá para usar como
    /// A/B (`GIOCHI_MAPA_BOTOES` + `GIOCHI_CONTROLE=evdev`).
    pub fn do_sistema() -> Option<Self> {
        let caminho = std::env::var_os("GIOCHI_MAPA_BOTOES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(CAMINHO_DO_MAPA));
        Self::do_texto(&std::fs::read_to_string(caminho).ok()?)
    }

    /// Bits de um código. `None` = botão que o jogo ignora.
    pub fn bit(&self, codigo: u16) -> Option<u32> {
        self.tabela.get(&codigo).copied()
    }

    /// A tabela em texto, uma linha por código, em ordem crescente.
    ///
    /// Existe para responder no aparelho a pergunta que gerou a issue #1 —
    /// "o que esta imagem declara como A?" — sem adivinhar por qual botão
    /// acelera.
    pub fn resumo(&self) -> String {
        let mut codigos: Vec<(&u16, &u32)> = self.tabela.iter().collect();
        codigos.sort_by_key(|(codigo, _)| **codigo);
        codigos
            .iter()
            .map(|(codigo, bits)| format!("{codigo:>3} -> {}\n", bits_em_texto(**bits)))
            .collect()
    }
}

/// Nome dos bits, para log e diagnóstico (`ACELERAR|ACEITAR`).
pub fn bits_em_texto(bits: u32) -> String {
    const NOMES: [(u32, &str); 12] = [
        (ACELERAR, "ACELERAR"),
        (FREAR, "FREAR"),
        (ESQUERDA, "ESQUERDA"),
        (DIREITA, "DIREITA"),
        (FREIO_MAO, "FREIO_MAO"),
        (NITRO, "NITRO"),
        (ACEITAR, "ACEITAR"),
        (VOLTAR, "VOLTAR"),
        (INICIAR, "INICIAR"),
        (PERFIL, "PERFIL"),
        (CIMA, "CIMA"),
        (BAIXO, "BAIXO"),
    ];
    let nomes: Vec<&str> = NOMES
        .iter()
        .filter(|(bit, _)| bits & bit != 0)
        .map(|(_, nome)| *nome)
        .collect();
    if nomes.is_empty() {
        "NENHUM".to_string()
    } else {
        nomes.join("|")
    }
}

impl Default for Mapa {
    fn default() -> Self {
        Self::padrao()
    }
}

/// Bit correspondente a um código evdev **na tabela embutida**. `None` = botão
/// que o jogo ignora, ou que outra placa mapeia de outro jeito (ver [`Mapa`]).
pub fn bit_do_codigo(codigo: u16) -> Option<u32> {
    use codigos::*;
    Some(match codigo {
        LESTE => ACELERAR | ACEITAR,
        SUL => FREAR,
        DPAD_UP => CIMA,
        DPAD_DOWN => BAIXO,
        DPAD_LEFT => ESQUERDA,
        DPAD_RIGHT => DIREITA,
        TL | TR => FREIO_MAO,
        TL2 | TR2 => NITRO,
        NORTE => PERFIL,
        OESTE | SELECT => VOLTAR,
        START => INICIAR,
        _ => return None,
    })
}

/// Struct do kernel: `timeval` (16) + tipo (2) + código (2) + valor (4).
const EVENTO_TAMANHO: usize = 24;
const EV_KEY: u16 = 0x01;

/// Onde o kernel publica o que cada nó de `/dev/input` sabe fazer.
const SYSFS_ENTRADA: &str = "/sys/class/input";

/// Quantos códigos `EV_KEY` o kernel pode emitir (`KEY_MAX` arredondado).
/// O maior código que este jogo usa é 547 (direcional).
const CODIGOS_POSSIVEIS: usize = 768;

/// O bitmap de `capabilities/key` declara algum botão que o jogo **usa**?
///
/// O kernel imprime o bitmap do código **maior para o menor**, uma palavra de
/// 64 bits em hexadecimal por coluna — é o mesmo `EVIOCGBIT` que o `evtest`
/// mostra, só que legível de um arquivo, sem `ioctl` e sem `unsafe` (que este
/// jogo não tem).
///
/// A pergunta é "declara código do jogo", e não "declara `BTN_GAMEPAD`", por
/// dois motivos medidos: o `adc-joystick` da X55 tem eixos e **nenhum** botão
/// (abrir uma thread ali não acrescenta um bit), e uma placa que separe o
/// direcional num nó próprio — só `BTN_DPAD_*` — perderia a direção se a regra
/// exigisse a faixa `BTN_GAMEPAD`. Já o `BTN_TOUCH` de um touchpad não passa,
/// porque o jogo não o mapeia.
///
/// `None` = o texto não é um bitmap (arquivo vazio, lixo). Quem chama decide
/// abrir por conta e risco: um controle invisível é pior que uma thread a mais.
pub fn declara_botoes_de_controle(texto: &str) -> Option<bool> {
    let palavras: Vec<u64> = texto
        .split_whitespace()
        .map(|p| u64::from_str_radix(p, 16))
        .collect::<Result<_, _>>()
        .ok()?;
    let total = palavras.len();
    let tem = |codigo: usize| {
        let (palavra, bit) = (codigo / 64, codigo % 64);
        palavra < total && (palavras[total - 1 - palavra] >> bit) & 1 == 1
    };
    Some((0..CODIGOS_POSSIVEIS).any(|c| bit_do_codigo(c as u16).is_some() && tem(c)))
}

/// Nome do dispositivo, como o `EVIOCGNAME` devolveria.
///
/// Vem do sysfs (`.../<nome>/device/name`), e não de `ioctl`: o jogo tem zero
/// `unsafe`, e o `EVIOCGNAME` é o único caminho pelo descarregador. O kernel
/// publica o mesmo texto dos dois jeitos.
fn nome_do_no(sysfs: &std::path::Path, nome: &str) -> Option<String> {
    std::fs::read_to_string(sysfs.join(nome).join("device").join("name")).ok()
}

/// Lê `.../<nome>/device/capabilities/key`. `None` quando não há sysfs.
fn capacidades(sysfs: &std::path::Path, nome: &str) -> Option<String> {
    std::fs::read_to_string(
        sysfs
            .join(nome)
            .join("device")
            .join("capabilities")
            .join("key"),
    )
    .ok()
}

/// Aplica um `input_event` cru na máscara. Devolve `true` se algo mudou.
///
/// Aperto repetido (`value == 2`) é ignorado: repetição do kernel não muda o
/// estado, e tratá-la como aperto faria o nitro ser rearmado a cada 30 ms.
///
/// O mapa entra por parâmetro porque o A **não** é o mesmo código em toda placa
/// (ver [`Mapa`]).
pub fn aplicar_evento(mascara: &AtomicU32, bytes: &[u8], mapa: &Mapa) -> bool {
    if bytes.len() < EVENTO_TAMANHO {
        return false;
    }
    let tipo = u16::from_le_bytes([bytes[16], bytes[17]]);
    if tipo != EV_KEY {
        return false;
    }
    let codigo = u16::from_le_bytes([bytes[18], bytes[19]]);
    let valor = i32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let Some(bit) = mapa.bit(codigo) else {
        return false;
    };
    match valor {
        1 => {
            mascara.fetch_or(bit, Ordering::Relaxed);
            true
        }
        0 => {
            mascara.fetch_and(!bit, Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

/// Botões do aparelho, compartilhados com as threads de leitura.
pub struct Botoes {
    /// Máscara do nó do **contrato** (`Giochi Gamepad`).
    ///
    /// Separada da física de propósito: se o virtual aparece **depois** da
    /// abertura (o `S52gamepad` pode subir depois do jogo), as threads dos nós
    /// físicos continuam vivas, mas o que vale muda de lado sem matar thread
    /// nenhuma. Duas máscaras somadas seriam evento duplicado — e, num mapa
    /// divergente, o defeito da issue #1 de volta.
    mascara_contrato: Arc<AtomicU32>,
    /// Máscara dos nós **físicos**, usada só quando não há contrato.
    mascara_fisica: Arc<AtomicU32>,
    abertos: usize,
    /// Quantos dos abertos **declaram** botão de controle. É este número, e não
    /// `abertos`, que decide quem manda nos botões (ver
    /// `entrada_jogo::dono`): um teclado aberto não é um controle.
    controles: usize,
    /// O nó `Giochi Gamepad` foi aberto?
    contrato: bool,
    /// Um leitor por nó aberto, com o sinalizador de vida.
    ///
    /// Existe porque um `read` de evdev devolve `ENODEV` quando o nó é
    /// destruído — o `S52gamepad` reiniciar basta. Sem o sinalizador, a máscara
    /// congelava no último estado (o A apertado no instante do restart deixava o
    /// carro acelerando para sempre) e `bits()` continuava a devolver um nó que
    /// não existe mais, ignorando o que viesse depois.
    leitores: Vec<Leitor>,
    /// Tabela código → bits dos nós **físicos**, compartilhada com as threads.
    mapa: Arc<Mapa>,
    /// O mapa veio de `/etc/giochi/buttons.map` (e não da tabela embutida)?
    mapa_do_sistema: bool,
    /// Guardado para os leitores que nascem **depois** da abertura: um controle
    /// Bluetooth cria o nó só quando conecta, e o launcher pode estar rodando.
    dir: std::path::PathBuf,
    sysfs: std::path::PathBuf,
    conhecidos: std::collections::HashSet<std::path::PathBuf>,
    proxima_revisao: std::time::Instant,
}

/// Um nó aberto: onde, se é o contrato, e se a thread ainda está viva.
struct Leitor {
    caminho: std::path::PathBuf,
    contrato: bool,
    vivo: Arc<AtomicBool>,
}

/// De quanto em quanto tempo `/dev/input` é reexaminado.
///
/// Mesmo valor do launcher, de propósito: um controle Bluetooth aparece como um
/// `event*` novo quando conecta, e abrir a lista uma vez deixava o controle
/// recém-pareado invisível para sempre. Aqui o sintoma seria "o controle
/// pareou e o jogo não responde" — a mesma classe do `buttons.map` sem o
/// símbolo, que já custou duas rodadas.
const REVISAO_MS: u64 = 2_000;

impl Botoes {
    /// Abre todos os `/dev/input/event*` e sobe uma thread por dispositivo.
    ///
    /// Sem `/dev/input` (o PC) devolve zero dispositivos e máscara zerada — o
    /// teclado do SDL cobre esse caso, e falhar aqui impediria de rodar no CI.
    pub fn iniciar() -> Self {
        // `GIOCHI_INPUT_DIR` / `GIOCHI_SYSFS_INPUT` existem para provar no PC o
        // caminho do aparelho sem `uinput` (que exige root): apontando para uma
        // árvore de mentira, o `--json` mostra `contrato_evdev` e `botoes_de`
        // como mostraria no aparelho. É diagnóstico, como `GIOCHI_SEM`.
        let dir = std::env::var_os("GIOCHI_INPUT_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/dev/input"));
        let sysfs = std::env::var_os("GIOCHI_SYSFS_INPUT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(SYSFS_ENTRADA));
        Self::com_dirs(dir, sysfs)
    }

    fn com_dirs(dir: std::path::PathBuf, sysfs: std::path::PathBuf) -> Self {
        // O mapa da imagem manda; a tabela embutida é a reserva (o PC não tem
        // `/etc/giochi/buttons.map`).
        let do_sistema = Mapa::do_sistema();
        let mapa_do_sistema = do_sistema.is_some();
        let mapa = Arc::new(do_sistema.unwrap_or_default());
        Self::com_mapa(dir, sysfs, mapa, mapa_do_sistema)
    }

    fn com_mapa(
        dir: std::path::PathBuf,
        sysfs: std::path::PathBuf,
        mapa: Arc<Mapa>,
        mapa_do_sistema: bool,
    ) -> Self {
        let mut this = Self {
            mascara_contrato: Arc::new(AtomicU32::new(0)),
            mascara_fisica: Arc::new(AtomicU32::new(0)),
            abertos: 0,
            controles: 0,
            contrato: false,
            leitores: Vec::new(),
            mapa,
            mapa_do_sistema,
            dir,
            sysfs,
            conhecidos: std::collections::HashSet::new(),
            proxima_revisao: std::time::Instant::now()
                + std::time::Duration::from_millis(REVISAO_MS),
        };
        this.abrir_novos();
        this
    }

    /// Construtor com diretório explícito e **sem** sysfs. Só teste.
    #[cfg(test)]
    fn em_dir(dir: std::path::PathBuf) -> Self {
        Self::em_dirs(dir, std::path::PathBuf::from("/sys-que-nao-existe"))
    }

    /// Construtor com os dois diretórios explícitos. Só teste.
    #[cfg(test)]
    fn em_dirs(dir: std::path::PathBuf, sysfs: std::path::PathBuf) -> Self {
        Self::com_mapa(dir, sysfs, Arc::new(Mapa::padrao()), false)
    }

    /// Abre os `event*` que ainda não têm leitor. Idempotente.
    fn abrir_novos(&mut self) -> usize {
        let Ok(dir) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut caminhos: Vec<_> = dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("event"))
                    .unwrap_or(false)
            })
            .collect();
        caminhos.sort();

        // Duas passagens, e não uma: os nós vêm em ordem alfabética, então
        // abrir no caminho abriria `event0`..`event5` (físicos) antes de
        // `event17` (o virtual) e ficaria com os dois lidos. A primeira
        // passagem só **olha**: existe contrato em `/dev/input`?
        let mut candidatos: Vec<(std::path::PathBuf, String, Option<bool>, bool)> = Vec::new();
        let mut tem_contrato = self.contrato;
        for caminho in caminhos {
            if self.conhecidos.contains(&caminho) {
                continue;
            }
            let nome = caminho
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            // Um nó que declara **não** ter botão de controle (teclado, mouse,
            // botão de energia, microfone) não ganha thread de leitura nem conta
            // como controle. Bitmap ilegível (sem sysfs) abre: um controle
            // invisível é pior que uma thread a mais.
            let declarado = capacidades(&self.sysfs, &nome)
                .as_deref()
                .and_then(declara_botoes_de_controle);
            let do_contrato = nome_do_no(&self.sysfs, &nome)
                .as_deref()
                .is_some_and(|n| n.trim() == NOME_DO_CONTRATO);
            tem_contrato |= do_contrato;
            candidatos.push((caminho, nome, declarado, do_contrato));
        }

        let mut novos = 0;
        for (caminho, nome, declarado, do_contrato) in candidatos {
            if declarado == Some(false) {
                continue;
            }
            // Com o contrato presente, os nós **físicos** não são lidos: eles
            // descrevem o mesmo estado, e somar dois nós daria evento
            // duplicado. O virtual já é a tradução de todos eles.
            if tem_contrato && !do_contrato {
                continue;
            }
            let Ok(arquivo) = std::fs::File::open(&caminho) else {
                continue;
            };
            // O mapa segue o **tipo** do nó, não a placa: o virtual já é a
            // tradução e usa o layout fixo; o físico usa o `buttons.map` (na
            // X35S o `a` é `BTN_SOUTH`, e aplicar o mapa da placa ao virtual
            // inverteria o A/B).
            let (mascara, mapa) = if do_contrato {
                (
                    Arc::clone(&self.mascara_contrato),
                    Arc::new(Mapa::padrao()),
                )
            } else {
                (Arc::clone(&self.mascara_fisica), Arc::clone(&self.mapa))
            };
            let vivo = Arc::new(AtomicBool::new(true));
            let sinal = Arc::clone(&vivo);
            let _ = std::thread::Builder::new()
                .name(format!("evdev-{nome}"))
                .spawn(move || {
                    ler_dispositivo(arquivo, mascara, &mapa);
                    // EOF ou `ENODEV`: o nó morreu. Quem varre é o `revisar`.
                    sinal.store(false, Ordering::Relaxed);
                });
            self.leitores.push(Leitor {
                caminho: caminho.clone(),
                contrato: do_contrato,
                vivo,
            });
            self.conhecidos.insert(caminho);
            self.abertos = self.conhecidos.len();
            self.controles += 1;
            self.contrato |= do_contrato;
            novos += 1;
        }
        novos
    }

    /// Reexamina `/dev/input`. Chamado por `bits`, com janela de 2 s.
    ///
    /// A janela fica no **chamador** e não numa thread: `bits` já roda uma vez
    /// por quadro, então não há relógio extra nem canal a mais.
    fn revisar(&mut self) {
        if std::time::Instant::now() < self.proxima_revisao {
            return;
        }
        self.proxima_revisao =
            std::time::Instant::now() + std::time::Duration::from_millis(REVISAO_MS);
        let enterrados = self.enterrar_leitores();
        let novos = self.abrir_novos() + enterrados;
        if novos > 0 {
            eprintln!(
                "neon-drift: {novos} dispositivo(s) de entrada novo(s) — {} no total",
                self.abertos
            );
        }
    }

    /// Tira de `conhecidos` os nós cujo leitor morreu, e devolve quantos.
    ///
    /// Um `read` de evdev falha quando o nó é destruído (`ENODEV`), e é o que
    /// acontece quando o serviço do gamepad reinicia. Sem esta varredura, a
    /// máscara congelava no último estado e o nó novo — que pode até ter o mesmo
    /// `eventN` — nunca era relido.
    fn enterrar_leitores(&mut self) -> usize {
        let mortos: Vec<usize> = self
            .leitores
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.vivo.load(Ordering::Relaxed))
            .map(|(i, _)| i)
            .collect();
        for i in mortos.iter().rev() {
            let l = self.leitores.remove(*i);
            self.conhecidos.remove(&l.caminho);
            self.abertos = self.conhecidos.len();
            self.controles = self.controles.saturating_sub(1);
            if l.contrato {
                // Os bits do nó morto não valem mais: deixá-los congelaria o
                // último botão apertado para sempre.
                self.contrato = false;
                self.mascara_contrato.store(0, Ordering::Relaxed);
            }
        }
        mortos.len()
    }

    /// Máscara de botões pressionados neste instante.
    ///
    /// Uma das duas, nunca a soma: o nó do contrato quando existe, senão os
    /// físicos (ver [`Botoes::contrato`]).
    pub fn bits(&mut self) -> u32 {
        self.revisar();
        if self.contrato {
            self.mascara_contrato.load(Ordering::Relaxed)
        } else {
            self.mascara_fisica.load(Ordering::Relaxed)
        }
    }

    /// O nó `Giochi Gamepad` está aberto?
    ///
    /// É o primeiro degrau de [`crate::entrada_jogo::dono`]: com ele, os
    /// códigos são fixos e nenhuma tabela por aparelho é consultada.
    pub fn contrato(&self) -> bool {
        self.contrato
    }

    /// Quantos dispositivos foram abertos.
    pub fn abertos(&self) -> usize {
        self.abertos
    }

    /// O mapa em vigor veio de `/etc/giochi/buttons.map`?
    ///
    /// É a resposta para "esta placa descreve o próprio plástico?" — o PC, que
    /// não tem o arquivo, responde `false` e deixa os botões para o SDL.
    pub fn mapa_do_sistema(&self) -> bool {
        self.mapa_do_sistema
    }

    /// Quantos dos abertos são **controles** (declaram código do jogo).
    ///
    /// É este número, e não `abertos`, que decide quem manda nos botões: o
    /// teclado e o botão de energia também são nós de `/dev/input`.
    pub fn controles(&self) -> usize {
        self.controles
    }
}

fn ler_dispositivo(mut arquivo: std::fs::File, mascara: Arc<AtomicU32>, mapa: &Mapa) {
    use std::io::Read;
    let mut buffer = [0u8; EVENTO_TAMANHO * 16];
    loop {
        match arquivo.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                for pedaco in buffer[..n].chunks_exact(EVENTO_TAMANHO) {
                    aplicar_evento(&mascara, pedaco, mapa);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn evento(tipo: u16, codigo: u16, valor: i32) -> [u8; EVENTO_TAMANHO] {
        let mut b = [0u8; EVENTO_TAMANHO];
        b[16..18].copy_from_slice(&tipo.to_le_bytes());
        b[18..20].copy_from_slice(&codigo.to_le_bytes());
        b[20..24].copy_from_slice(&valor.to_le_bytes());
        b
    }

    #[test]
    fn apertar_e_soltar_ligam_e_desligam_o_bit() {
        let m = AtomicU32::new(0);
        assert!(aplicar_evento(&m, &evento(EV_KEY, codigos::LESTE, 1), &Mapa::padrao()));
        assert_eq!(m.load(Ordering::Relaxed) & ACELERAR, ACELERAR);
        assert!(aplicar_evento(&m, &evento(EV_KEY, codigos::LESTE, 0), &Mapa::padrao()));
        assert_eq!(m.load(Ordering::Relaxed) & ACELERAR, 0);
    }

    #[test]
    fn dois_botoes_ao_mesmo_tempo_nao_se_apagam() {
        // Acelerar e virar juntos é o caso normal de corrida. Com `store` em vez
        // de `fetch_or`, um apagaria o outro.
        let m = AtomicU32::new(0);
        aplicar_evento(&m, &evento(EV_KEY, codigos::LESTE, 1), &Mapa::padrao());
        aplicar_evento(&m, &evento(EV_KEY, codigos::DPAD_RIGHT, 1), &Mapa::padrao());
        let bits = m.load(Ordering::Relaxed);
        assert_eq!(bits & ACELERAR, ACELERAR);
        assert_eq!(bits & DIREITA, DIREITA);
    }

    #[test]
    fn repeticao_do_kernel_nao_mexe_no_estado() {
        let m = AtomicU32::new(0);
        assert!(!aplicar_evento(&m, &evento(EV_KEY, codigos::LESTE, 2), &Mapa::padrao()));
        assert_eq!(m.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn botao_fora_do_mapa_e_ignorado() {
        assert_eq!(bit_do_codigo(9999), None);
        let m = AtomicU32::new(0);
        assert!(!aplicar_evento(&m, &evento(EV_KEY, 9999, 1), &Mapa::padrao()));
    }

    #[test]
    fn evento_que_nao_e_tecla_e_ignorado() {
        let m = AtomicU32::new(0);
        assert!(!aplicar_evento(&m, &evento(0x03, 0, 5), &Mapa::padrao())); // EV_ABS
        assert_eq!(m.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn buffer_curto_nao_estoura() {
        let m = AtomicU32::new(0);
        assert!(!aplicar_evento(&m, &[0u8; 10], &Mapa::padrao()));
        assert!(!aplicar_evento(&m, &[], &Mapa::padrao()));
    }

    #[test]
    fn os_botoes_do_jogo_tem_codigo_medido_no_aparelho() {
        // Estes são os do `buttons.map` real. Trocar um deles por chute deixa o
        // botão morto, e o sintoma é "não freia" sem erro em lugar nenhum.
        assert_eq!(
            bit_do_codigo(305),
            Some(ACELERAR | ACEITAR),
            "A = BTN_EAST: acelera e aceita a linha do menu"
        );
        assert_eq!(bit_do_codigo(304), Some(FREAR), "B = BTN_SOUTH");
        assert_eq!(bit_do_codigo(546), Some(ESQUERDA));
        assert_eq!(bit_do_codigo(547), Some(DIREITA));
        assert_eq!(bit_do_codigo(310), Some(FREIO_MAO), "L1");
        assert_eq!(bit_do_codigo(311), Some(FREIO_MAO), "R1");
        assert_eq!(bit_do_codigo(313), Some(NITRO), "R2");
        assert_eq!(bit_do_codigo(307), Some(PERFIL), "X = BTN_NORTH");
        assert_eq!(bit_do_codigo(308), Some(VOLTAR), "Y = BTN_WEST");
        assert_eq!(bit_do_codigo(314), Some(VOLTAR), "SELECT");
        assert_eq!(bit_do_codigo(315), Some(INICIAR), "START");
    }

    #[test]
    fn o_direcional_nao_acende_o_bit_do_botao_de_acelerar() {
        // Se acendesse, soltar o direcional apagaria o bit do A que o jogador
        // continua segurando, e o carro pararia de acelerar sozinho no meio da
        // reta. Quem anda com o direcional usa `CIMA`, que a fusão lê como
        // acelerar.
        let cima = bit_do_codigo(codigos::DPAD_UP).unwrap();
        let baixo = bit_do_codigo(codigos::DPAD_DOWN).unwrap();
        assert_eq!(cima, CIMA);
        assert_eq!(baixo, BAIXO);
        assert_eq!(cima & ACELERAR, 0);
        assert_eq!(baixo & FREAR, 0);
    }

    #[test]
    fn cada_botao_acende_conjunto_proprio() {
        // Dois botões que compartilham um bit se apagam mutuamente ao soltar.
        let codigos_do_jogo = [
            codigos::LESTE,
            codigos::SUL,
            codigos::NORTE,
            codigos::OESTE,
            codigos::DPAD_UP,
            codigos::DPAD_DOWN,
            codigos::DPAD_LEFT,
            codigos::DPAD_RIGHT,
            codigos::TL,
            codigos::TR,
            codigos::TL2,
            codigos::TR2,
            codigos::START,
            codigos::SELECT,
        ];
        for (i, a) in codigos_do_jogo.iter().enumerate() {
            let ba = bit_do_codigo(*a).unwrap();
            for b in &codigos_do_jogo[i + 1..] {
                let bb = bit_do_codigo(*b).unwrap();
                // As duas exceções são pares físicos que o jogo trata como um
                // botão só (L1/R1 e L2/R2 fazem o mesmo).
                let par = matches!(
                    (*a, *b),
                    (codigos::TL, codigos::TR) | (codigos::TL2, codigos::TR2)
                        | (codigos::OESTE, codigos::SELECT)
                );
                if par {
                    assert_eq!(ba, bb, "o par {:?}/{:?} devia fazer o mesmo", a, b);
                } else {
                    assert_eq!(ba & bb, 0, "{a} e {b} compartilham um bit");
                }
            }
        }
    }

    #[test]
    fn o_deslocamento_dos_campos_bate_com_o_do_kernel() {
        let b = evento(EV_KEY, 0x0123, 1);
        assert_eq!(u16::from_le_bytes([b[16], b[17]]), EV_KEY);
        assert_eq!(u16::from_le_bytes([b[18], b[19]]), 0x0123);
        assert_eq!(i32::from_le_bytes([b[20], b[21], b[22], b[23]]), 1);
    }

    #[test]
    fn diretorio_inexistente_abre_zero_e_nao_quebra() {
        // No CI não há `/dev/input`. A condição tem de ser **zero**, e não
        // "zero ou não" — a versão anterior deste teste era
        // `abertos() == 0 || abertos() > 0`, que passa dos dois lados e por
        // isso nunca provou nada.
        let mut b = Botoes::em_dir(std::path::PathBuf::from("/dev/input-que-nao-existe"));
        assert_eq!(b.abertos(), 0);
        assert_eq!(b.bits(), 0);
    }

    /// Um `event*` que aparece **depois** da abertura tem de ser lido.
    ///
    /// É o caso do controle Bluetooth: o nó só nasce quando ele conecta, e
    /// abrir a lista uma vez deixava o controle recém-pareado invisível para
    /// sempre. O sintoma seria "pareou e o jogo não responde" — indistinguível
    /// de "não pareou".
    #[test]
    fn dispositivo_novo_e_aberto_na_revisao_seguinte() {
        let dir = std::env::temp_dir().join(format!("nd-evdev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("event0"), []).expect("event0");

        let mut b = Botoes::em_dir(dir.clone());
        assert_eq!(b.abertos(), 1);

        std::fs::write(dir.join("event1"), []).expect("event1");
        // Vence a janela de 2 s à mão em vez de dormir.
        b.proxima_revisao = std::time::Instant::now() - std::time::Duration::from_millis(1);
        let _ = b.bits();
        assert_eq!(b.abertos(), 2, "o event1 apareceu depois da abertura");

        // Idempotente: revisar de novo não duplica o leitor.
        b.proxima_revisao = std::time::Instant::now() - std::time::Duration::from_millis(1);
        let _ = b.bits();
        assert_eq!(b.abertos(), 2, "a segunda revisão duplicou");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── quem é controle: o bitmap de `capabilities/key` ─────────────

    /// Bitmap como o kernel imprime: hexadecimal, palavra de 64 bits, da mais
    /// alta para a mais baixa.
    fn bitmap(codigos: &[usize]) -> String {
        let maior = codigos.iter().copied().max().unwrap_or(0);
        let mut palavras = vec![0u64; maior / 64 + 1];
        for c in codigos {
            palavras[c / 64] |= 1 << (c % 64);
        }
        palavras
            .iter()
            .rev()
            .map(|v| format!("{v:x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    // ── o mapa do plástico vem do `buttons.map` ─────────────────────

    /// Trecho **real** do `buttons.map` da X55 (GIOCHI-SYSTEM), com os
    /// comentários de fim de linha que o arquivo tem.
    const MAPA_X55: &str = "\
up      = BTN_DPAD_UP        # gpio4 PA0
down    = BTN_DPAD_DOWN      # gpio4 PA1
left    = BTN_DPAD_LEFT      # gpio3 PD6
right   = BTN_DPAD_RIGHT     # gpio3 PD7
a       = BTN_EAST           # gpio3 PD3
b       = BTN_SOUTH          # gpio3 PD2
x       = BTN_NORTH          # gpio3 PD5
y       = BTN_WEST           # gpio3 PD4
l       = BTN_TL             # gpio3 PD0
r       = BTN_TR             # gpio3 PC6
l2      = BTN_TL2            # gpio3 PD1
r2      = BTN_TR2            # gpio3 PC7
start   = BTN_START          # gpio4 PA2
select  = BTN_SELECT         # gpio4 PA4
home    = BTN_MODE           # gpio3 PB7 -- OPCIONAL
";

    #[test]
    fn o_mapa_da_x55_poe_o_a_no_leste() {
        let mapa = Mapa::do_texto(MAPA_X55).expect("mapa da X55");
        assert_eq!(mapa.bit(codigos::LESTE), Some(ACELERAR | ACEITAR), "A");
        assert_eq!(mapa.bit(codigos::SUL), Some(FREAR), "B");
        assert_eq!(mapa.bit(codigos::NORTE), Some(PERFIL), "X");
        assert_eq!(mapa.bit(codigos::OESTE), Some(VOLTAR), "Y");
        assert_eq!(mapa.bit(codigos::TL), Some(FREIO_MAO), "L1");
        assert_eq!(mapa.bit(codigos::TR), Some(FREIO_MAO), "R1");
        assert_eq!(mapa.bit(codigos::TL2), Some(NITRO), "L2");
        assert_eq!(mapa.bit(codigos::TR2), Some(NITRO), "R2");
        assert_eq!(mapa.bit(codigos::DPAD_UP), Some(CIMA));
        assert_eq!(mapa.bit(codigos::START), Some(INICIAR));
        // O mapa da X55 não tem `BTN_MODE` no jogo: `home` é ignorado.
        assert_eq!(mapa.bit(316), None, "home não é botão do jogo");
    }

    #[test]
    fn o_mapa_da_x35s_poe_o_a_no_sul() {
        // A **X35S** troca A e B no device tree ("A e B invertidos em relação à
        // X55 de propósito"), e é a placa que a tabela compilada errava: no
        // binário antigo o A dela freava. Com o mapa como dado, o mesmo binário
        // acerta as quatro placas.
        let texto = MAPA_X55
            .replace("a       = BTN_EAST", "a       = BTN_SOUTH")
            .replace("b       = BTN_SOUTH", "b       = BTN_EAST");
        let mapa = Mapa::do_texto(&texto).expect("mapa da X35S");
        assert_eq!(mapa.bit(codigos::SUL), Some(ACELERAR | ACEITAR), "A");
        assert_eq!(mapa.bit(codigos::LESTE), Some(FREAR), "B");

        // E o evento do kernel segue a tabela nova, não a embutida.
        let m = AtomicU32::new(0);
        assert!(aplicar_evento(&m, &evento(EV_KEY, codigos::SUL, 1), &mapa));
        let bits = m.load(Ordering::Relaxed);
        assert_eq!(bits & ACELERAR, ACELERAR, "o A da X35S acelera");
        assert_eq!(bits & FREAR, 0, "e não freia");
    }

    #[test]
    fn mapa_sem_o_a_nao_vale() {
        // Recorte, exemplo ou lixo: sem `a = BTN_*` não há controle jogável, e a
        // reserva é a tabela embutida.
        assert!(Mapa::do_texto("b = BTN_SOUTH\nl = BTN_TL\n").is_none());
        assert!(Mapa::do_texto("").is_none());
        assert!(Mapa::do_texto("axis_x_min = -1800\naxis_y_deadzone = 32").is_none());
        assert!(Mapa::do_texto("a = KEY_ENTER").is_none(), "símbolo que não é BTN_*");
    }

    #[test]
    fn o_resumo_lista_o_a_para_conferir_no_aparelho() {
        // É o texto do `--mapa-botoes`: no aparelho, "o A é o 305" tem de ser
        // legível sem apertar botão nenhum.
        let resumo = Mapa::padrao().resumo();
        assert!(resumo.contains("305 -> ACELERAR|ACEITAR"), "{resumo}");
        assert!(resumo.contains("313 -> NITRO"), "{resumo}");
        assert_eq!(bits_em_texto(0), "NENHUM");
        assert_eq!(bits_em_texto(FREIO_MAO), "FREIO_MAO");
    }

    #[test]
    fn o_mapa_embutido_e_a_convencao_dos_aparelhos() {
        let mapa = Mapa::padrao();
        assert_eq!(mapa.bit(codigos::LESTE), Some(ACELERAR | ACEITAR));
        assert_eq!(mapa.bit(codigos::SUL), Some(FREAR));
        assert_eq!(mapa.bit(9999), None);
    }

    #[test]
    fn o_bitmap_de_um_controle_e_reconhecido() {
        // Lista real dos códigos do `r36s_Gamepad` do R35S, medida com o
        // `sdl-input-probe` do GIOCHI-SYSTEM: A/B/X/Y, L1/R1/L2/R2, SELECT,
        // START, HOME, os dois sticks e o direcional.
        let do_controle = bitmap(&[
            304, 305, 307, 308, 310, 311, 312, 313, 314, 315, 316, 317, 318, 544, 545, 546, 547,
        ]);
        assert_eq!(declara_botoes_de_controle(&do_controle), Some(true));
    }

    #[test]
    fn teclado_e_mouse_nao_passam_por_controle() {
        // Texto **real** de `/sys/class/input/event4/device/capabilities/key`
        // neste PC (`SONiX USB DEVICE Keyboard`), copiado e não inventado: é o
        // formato que o kernel imprime, palavras de 64 bits da maior para a
        // menor, e o último token pode ser mais curto.
        let teclado = "3f00733fff 0 0 483ffff17aff32d bfd4444600000000 1 130ff38b17d000 \
                      ffe77bfad941dfed 81beffcd01cfffff febffbffdffffffe";
        assert_eq!(declara_botoes_de_controle(teclado), Some(false));
        // `SONiX USB DEVICE` (o nó de sistema do mesmo teclado).
        assert_eq!(
            declara_botoes_de_controle(
                "1000000000007 ff9f207ac14057ff febeffdfffefffff fffffffffffffffe"
            ),
            Some(false)
        );
        // `USB OPTICAL MOUSE`: BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_TOOL_FINGER,
        // BTN_TOUCH — todos fora do mapa do jogo.
        let mouse = bitmap(&[272, 273, 274, 325, 330]);
        assert_eq!(declara_botoes_de_controle(&mouse), Some(false));
    }

    #[test]
    fn a_ordem_das_palavras_e_a_do_kernel() {
        // Um só bit no mapa. Se a leitura invertesse a ordem das palavras, 304
        // seria lido como um código da casa dos milhares e nada casaria.
        assert_eq!(declara_botoes_de_controle(&bitmap(&[304])), Some(true));
        assert_eq!(declara_botoes_de_controle(&bitmap(&[546])), Some(true),
            "só o direcional já é um nó que o jogo lê");
        assert_eq!(declara_botoes_de_controle(&bitmap(&[116])), Some(false),
            "KEY_POWER não é botão de jogo");
    }

    #[test]
    fn texto_que_nao_e_bitmap_nao_e_afirmado() {
        // `None` = "não sei": quem chama abre por precaução, porque um controle
        // invisível é pior que uma thread a mais.
        assert_eq!(declara_botoes_de_controle(""), Some(false));
        assert_eq!(declara_botoes_de_controle("zz 12"), None);
        assert_eq!(declara_botoes_de_controle("0"), Some(false));
    }

    #[test]
    fn o_teclado_nao_ganha_thread_nem_vira_controle() {
        // A decisão de quem manda nos botões usa `controles`, e um teclado
        // aberto não pode contá-lo: no PC seria o teclado tirando o controle
        // do SDL.
        let base = std::env::temp_dir().join(format!("nd-evdev-caps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("input");
        let sysfs = base.join("sys");
        std::fs::create_dir_all(&dir).expect("dir");
        for (no, codigos) in [
            ("event0", vec![1, 2, 3, 28, 57, 103]),
            ("event1", vec![304, 305, 307, 308, 314, 315, 544, 545, 546, 547]),
        ] {
            std::fs::write(dir.join(no), []).expect(no);
            let caps = sysfs.join(no).join("device").join("capabilities");
            std::fs::create_dir_all(&caps).expect(no);
            std::fs::write(caps.join("key"), bitmap(&codigos)).expect(no);
        }
        // `event2` não tem entrada no sysfs (aparelho sem sysfs): abre.
        std::fs::write(dir.join("event2"), []).expect("event2");

        let mut b = Botoes::em_dirs(dir, sysfs);
        assert_eq!(b.abertos(), 2, "o teclado (event0) não devia abrir");
        assert_eq!(b.controles(), 2, "event1 e o nó sem sysfs contam");
        assert!(!b.contrato());
        // A máscara vem dos físicos, e `bits()` é quem revisa.
        let _ = b.bits();
        let _ = std::fs::remove_dir_all(&base);
    }

    // ── o contrato: `Giochi Gamepad` ────────────────────────────────
    //
    // O GIOCHI-SYSTEM publica um controle virtual com layout fixo (issue #2).
    // Num aparelho real, `/dev/input` passa a ter **o virtual e os físicos**.

    /// Monta um sysfs de mentira: `(nó, nome, códigos)`.
    fn arvore(
        base: &std::path::Path,
        nos: &[(&str, &str, &[usize])],
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = base.join("input");
        let sysfs = base.join("sys");
        std::fs::create_dir_all(&dir).expect("dir");
        for (no, nome, codigos) in nos {
            std::fs::write(dir.join(no), []).expect(no);
            let dev = sysfs.join(no).join("device");
            std::fs::create_dir_all(dev.join("capabilities")).expect(no);
            std::fs::write(dev.join("name"), format!("{nome}\n")).expect(no);
            std::fs::write(dev.join("capabilities").join("key"), bitmap(codigos)).expect(no);
        }
        (dir, sysfs)
    }

    fn base_teste(sufixo: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "nd-evdev-{sufixo}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        base
    }

    /// Os 17 códigos que o contrato declara (inclusive `home`, que o X35H não
    /// tem fisicamente).
    const CODIGOS_DO_CONTRATO: [usize; 17] = [
        304, 305, 307, 308, 310, 311, 312, 313, 314, 315, 316, 317, 318, 544, 545, 546, 547,
    ];

    #[test]
    fn com_o_contrato_os_nos_fisicos_nao_sao_lidos() {
        // É o pedido 3 da issue: "não leia todos e some". Com o virtual
        // presente, ler os físicos daria evento duplicado — e, num mapa
        // divergente, o defeito da issue #1 de volta.
        let base = base_teste("contrato");
        let (dir, sysfs) = arvore(
            &base,
            &[
                ("event0", "gpio-keys-control", &[304, 305, 307, 308, 544, 545]),
                ("event1", "adc-joystick", &[]),
                ("event2", NOME_DO_CONTRATO, &CODIGOS_DO_CONTRATO),
            ],
        );
        let mut b = Botoes::em_dirs(dir, sysfs);
        assert!(b.contrato(), "o virtual devia ser reconhecido pelo nome");
        assert_eq!(b.abertos(), 1, "só o virtual é lido");
        assert_eq!(b.controles(), 1);
        let _ = b.bits();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn sem_o_contrato_os_fisicos_continuam_sendo_lidos() {
        // Imagem antiga: nada de virtual, e o jogo tem de continuar jogável.
        let base = base_teste("sem-contrato");
        let (dir, sysfs) = arvore(
            &base,
            &[("event0", "gpio-keys-control", &[304, 305, 307, 308])],
        );
        let mut b = Botoes::em_dirs(dir, sysfs);
        assert!(!b.contrato());
        assert_eq!(b.abertos(), 1);
        let _ = b.bits();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn o_virtual_usa_o_layout_fixo_e_nao_o_mapa_da_placa() {
        // O ponto mais fácil de errar: o virtual **é** a tradução. Na X35S o
        // `buttons.map` diz `a = BTN_SOUTH` (o físico); aplicar esse mapa ao
        // virtual inverteria o A/B, porque o virtual já normalizou tudo para o
        // layout da X55.
        let base = base_teste("mapa-virtual");
        let (dir, sysfs) = arvore(
            &base,
            &[("event0", NOME_DO_CONTRATO, &CODIGOS_DO_CONTRATO)],
        );
        // Mapa "da X35S", onde o `a` é 304.
        let mapa_da_placa = Mapa::do_texto("a = BTN_SOUTH\nb = BTN_EAST\n").expect("mapa");
        assert_eq!(
            mapa_da_placa.bit(304),
            Some(ACELERAR | ACEITAR),
            "a premissa do teste: no mapa da X35S o 304 é o a"
        );
        let mut b = Botoes::com_mapa(
            dir,
            sysfs,
            std::sync::Arc::new(mapa_da_placa),
            true,
        );
        assert!(b.contrato());
        // A máscara do virtual é escrita com a tabela fixa: o 305 é ACELERAR.
        let m = Arc::clone(&b.mascara_contrato);
        aplicar_evento(&m, &evento(EV_KEY, 305, 1), &Mapa::padrao());
        let bits = b.bits();
        assert_eq!(bits & ACELERAR, ACELERAR, "o a do virtual acelera");
        assert_eq!(bits & FREAR, 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn o_virtual_que_aparece_depois_toma_o_comando() {
        // O `S52gamepad` pode subir depois do jogo. As threads dos físicos
        // continuam vivas, mas quem responde passa a ser o virtual — e sem
        // somar as duas máscaras.
        let base = base_teste("virtual-tarde");
        let (dir, sysfs) = arvore(
            &base,
            &[("event0", "gpio-keys-control", &[304, 305, 307, 308])],
        );
        let mut b = Botoes::em_dirs(dir.clone(), sysfs.clone());
        assert!(!b.contrato());
        assert_eq!(b.abertos(), 1, "só o físico por enquanto");

        // O virtual nasce: um nó novo, com nome e bitmap no sysfs.
        let no = "event9";
        std::fs::write(dir.join(no), []).expect(no);
        let dev = sysfs.join(no).join("device");
        std::fs::create_dir_all(dev.join("capabilities")).expect(no);
        std::fs::write(dev.join("name"), NOME_DO_CONTRATO).expect(no);
        std::fs::write(
            dev.join("capabilities").join("key"),
            bitmap(&CODIGOS_DO_CONTRATO),
        )
        .expect(no);

        // Vence a janela de 2 s à mão em vez de dormir.
        b.proxima_revisao = std::time::Instant::now() - std::time::Duration::from_millis(1);
        let _ = b.bits();
        assert!(b.contrato(), "o virtual tomou o comando");

        // O que importa não é quantos leitores sobraram — o físico do fixture é
        // um arquivo vazio e morre no EOF, e *quando* ele morre é corrida —, e
        // sim que a máscara dele **não** chega ao jogo. Um bit aceso à força no
        // lado físico tem de continuar invisível.
        b.mascara_fisica.store(FREAR, Ordering::Relaxed);
        assert_eq!(
            b.bits() & FREAR,
            0,
            "com o contrato, a máscara do físico é ignorada"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn o_contrato_que_morre_nao_deixa_botao_preso() {
        // `S52gamepad` reiniciar destrói o nó: o `read` devolve `ENODEV` e a
        // thread morre. Sem a varredura, `contrato` continuava `true` e a
        // máscara ficava congelada — o A apertado no instante do restart deixava
        // o carro acelerando para sempre, e o nó novo (que pode reusar o mesmo
        // `eventN`) nunca era relido.
        let base = base_teste("contrato-morre");
        let (dir, sysfs) = arvore(
            &base,
            &[("event0", NOME_DO_CONTRATO, &CODIGOS_DO_CONTRATO)],
        );
        // Um `EV_KEY BTN_EAST` pressionado, escrito antes da abertura: o leitor
        // aplica o bit e morre no EOF.
        std::fs::write(dir.join("event0"), evento(EV_KEY, 305, 1)).expect("evento");
        let mut b = Botoes::em_dirs(dir.clone(), sysfs);
        assert!(b.contrato());
        // Espera o leitor aplicar, sem disparar revisão.
        b.proxima_revisao = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut bits = 0;
        for _ in 0..400 {
            bits = b.bits();
            if bits & ACELERAR != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(bits & ACELERAR, ACELERAR, "o A do virtual foi lido");

        // Espera a thread do leitor morrer de fato (EOF do arquivo): sem isso a
        // varredura não teria o que enterrar, e o teste seria uma corrida.
        for _ in 0..400 {
            if b.leitores.iter().all(|l| !l.vivo.load(Ordering::Relaxed)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        // O nó morre (serviço reiniciou) e a varredura roda.
        std::fs::remove_file(dir.join("event0")).expect("remover");
        b.proxima_revisao = std::time::Instant::now() - std::time::Duration::from_millis(1);
        assert_eq!(b.bits(), 0, "a máscara do nó morto tem de zerar");
        assert!(!b.contrato(), "sem nó não há contrato");
        assert_eq!(b.abertos(), 0);
        let _ = std::fs::remove_dir_all(&base);
    }
}
