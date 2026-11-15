//! Corrida LAN host-autoritativa sobre `giochi-net`.
//!
//! ## Divisão de responsabilidade
//!
//! | Dado | Quem manda | Por onde |
//! | --- | --- | --- |
//! | física do carro | host | `Corrida`/`Carro::passo` no passo fixo |
//! | input do cliente | cliente | `Mensagem::Input` (UDP seqüenciado) |
//! | posição/rotação | host | `Mensagem::Inimigos`, 15 Hz, filtrada por AOI |
//! | largada, voltas, checkpoints, resultado | host | `Mensagem::Global` no `laminar` (confiável-ordenado) |
//! | descoberta de lobby | mDNS | `Anunciante`/`Descoberta` |
//!
//! ## Por que dois canais, e não um
//!
//! Snapshot de posição é **descartável**: o próximo corrige, e reenviar um
//! pacote velho só gasta banda. Largada e resultado final não são: perder o
//! "largou" deixa o cliente parado na grade para sempre. O `giochi-net` já
//! separa os dois (`Host::tick_snapshot` usa o transporte próprio; o
//! `laminar_backend` dá `reliable_ordered` para `Mensagem::Global`), e este
//! módulo usa cada um para o que ele serve.
//!
//! Isso custa um segundo socket: o snapshot escuta na porta anunciada pelo
//! mDNS e o controle na **porta seguinte**. A convenção é fixa e documentada
//! aqui porque um cliente precisa derivá-la do único endereço que o mDNS dá.
//!
//! ## Cliente não prediz
//!
//! O cliente não roda a física do próprio carro: ele manda input e interpola o
//! que o host devolve. Em LAN o RTT é de 1–4 ms; predição resolveria um
//! problema que não existe e criaria dessincronização sem forma de reproduzir.
//! A posição do carro local chega com um intervalo de snapshot de atraso, e o
//! HUD do cliente mostra a mesma verdade que o host.
//!
//! ## Determinismo
//!
//! Toda a simulação (host e carros dos clientes) roda dentro do `passo` fixo,
//! com `dt = TIQUE`. A interpolação é **só de desenho**: ela nunca escreve no
//! estado que a física usa antes do próximo passo.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use giochi_net::client::Cliente;
use giochi_net::host::Host;
use giochi_net::laminar_backend::Transporte;
use giochi_net::peer::Modo;
use giochi_net::wire::{Mensagem, SnapshotInimigo};
use giochi_net::{SNAPSHOT_HZ, PORTA_PADRAO};
use giochi_neon_drift_core::carro::{Carro, Entrada};
use giochi_neon_drift_core::corrida::{Corrida, Desfecho, Fase};
use giochi_neon_drift_core::drift::Drift;
use giochi_neon_drift_core::ia::Adversario;
use giochi_neon_drift_core::pista::Ponto;
use giochi_neon_drift_core::{COR_JOGADOR, CORES_ADVERSARIOS};

/// Intervalo entre snapshots, em segundos.
pub const INTERVALO_SNAPSHOT: f32 = 1.0 / SNAPSHOT_HZ as f32;

/// Raio de interesse pedido ao host, em metros.
///
/// A pista maior tem ~1,3 km. 260 m cobrem com folga o que cabe no FOV a 60 m/s
/// e ainda deixam de fora quem ficou duas curvas atrás — que é o ponto do AOI.
pub const RAIO_AOI: f32 = 260.0;

/// Quanto tempo um cliente pode ficar calado antes de o host zerar o input.
pub const TIMEOUT_INPUT: Duration = Duration::from_millis(600);

/// Intervalo entre reenvios do "olá" enquanto o host não responde no controle.
const INTERVALO_OLA: Duration = Duration::from_secs(1);

/// Intervalo entre estados de corrida (tempo/volta/posição).
const INTERVALO_ESTADO: Duration = Duration::from_millis(250);

/// Primeiro id de adversário controlado pela IA nos snapshots.
const ID_IA: u16 = 100;

/// Papel deste processo na rede.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Papel {
    /// Sem rede: corrida local.
    Off,
    /// Simula o jogo e aceita clientes.
    Host,
    /// Manda input e desenha o que o host devolve.
    Cliente,
}

/// Um carro remoto pronto para desenhar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarroRede {
    /// Id de rede (0 = host, 1..=8 clientes, 100+ IA).
    pub id: u16,
    /// Posição no mundo.
    pub pos: Ponto,
    /// Rumo em radianos.
    pub angulo: f32,
    /// Altura sobre o nível do mar.
    pub altura: f32,
    /// Velocidade no rumo, m/s.
    pub velocidade: f32,
    /// Cor RGB.
    pub cor: [f32; 3],
    /// Derrapando no snapshot.
    pub derrapando: bool,
    /// No ar no snapshot.
    pub no_ar: bool,
    /// Nitro ativo no snapshot.
    pub nitro: bool,
}

impl CarroRede {
    /// Cor de um carro da rede, do ponto de vista de **quem desenha**.
    ///
    /// `meu_id` é `None` no host (o carro local é o `id 0`) e `Some(id)` no
    /// cliente. Sem ele a cor saía só do id e cada lado pintava uma corrida
    /// diferente: no cliente o carro do host recebia `COR_JOGADOR` — verde, a
    /// cor do jogador — e o carro do próprio cliente recebia a cor de um
    /// adversário, o que fazia a tela do cliente parecer a do host. As três IA
    /// ficavam cinza, embora o host as desenhe nas cores de adversário.
    pub fn cor_do_id(id: u16, meu_id: Option<u8>) -> [f32; 3] {
        if Some(id) == meu_id.map(u16::from) {
            return COR_JOGADOR;
        }
        match id {
            // No host o carro local é o id 0; num cliente ele é só mais um.
            0 if meu_id.is_none() => COR_JOGADOR,
            0 => CORES_ADVERSARIOS[0],
            ID_IA.. => CORES_ADVERSARIOS[(id - ID_IA) as usize % CORES_ADVERSARIOS.len()],
            n => CORES_ADVERSARIOS[(n as usize) % CORES_ADVERSARIOS.len()],
        }
    }
}

/// Amostra de rede de um carro.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Amostra {
    x: f32,
    z: f32,
    angulo: f32,
    velocidade: f32,
    flags: u8,
    recebido: Instant,
}

impl Amostra {
    fn no_ar(&self) -> bool {
        self.flags & 0x01 != 0
    }
    fn derrapando(&self) -> bool {
        self.flags & 0x02 != 0
    }
    fn nitro(&self) -> bool {
        self.flags & 0x04 != 0
    }
}

/// Duas amostras e o instante da última, para interpolar no desenho.
#[derive(Debug, Clone)]
struct Trilha {
    anterior: Amostra,
    atual: Amostra,
}

impl Trilha {
    fn nova(amostra: Amostra) -> Self {
        Self {
            anterior: amostra,
            atual: amostra,
        }
    }

    fn inserir(&mut self, amostra: Amostra) {
        self.anterior = self.atual;
        self.atual = amostra;
    }

    /// Pose interpolada. `t` é o tempo desde a última amostra, em segundos.
    ///
    /// Fora de `[0, intervalo]` a pose é **segurada** na ponta: extrapolar
    /// posição em rede com perda faz o carro atravessar a mureta e voltar, e
    /// um congelamento de 66 ms é menos feio que um teletransporte.
    fn pose(&self, agora: Instant, intervalo: f32) -> (f32, f32, f32, f32, Amostra) {
        let t = (agora.duration_since(self.atual.recebido).as_secs_f32() / intervalo.max(0.001))
            .clamp(0.0, 1.0);
        let x = self.anterior.x + (self.atual.x - self.anterior.x) * t;
        let z = self.anterior.z + (self.atual.z - self.anterior.z) * t;
        let angulo = lerp_angulo(self.anterior.angulo, self.atual.angulo, t);
        let velocidade = self.anterior.velocidade
            + (self.atual.velocidade - self.anterior.velocidade) * t;
        // Flags e a amostra "de referência" vêm da amostra mais nova: estado
        // discreto (no ar, derrapando) não interpola.
        (x, z, angulo, velocidade, self.atual)
    }
}

/// Interpolação angular pelo caminho mais curto.
fn lerp_angulo(a: f32, b: f32, t: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut delta = (b - a) % tau;
    if delta > std::f32::consts::PI {
        delta -= tau;
    } else if delta < -std::f32::consts::PI {
        delta += tau;
    }
    // Normaliza para `(-PI, PI]`: o meio de 350° → 10° é 0°, não 360°. Um
    // resultado logo abaixo de TAU é o mesmo ângulo que 0, mas comparar os
    // dois como números crus daria uma diferença de uma volta inteira.
    let mut resultado = (a + delta * t) % tau;
    if resultado > std::f32::consts::PI {
        resultado -= tau;
    } else if resultado < -std::f32::consts::PI {
        resultado += tau;
    }
    resultado
}

/// Um cliente humano simulado pelo host.
struct ClienteCorrida {
    id: u8,
    carro: Carro,
    entrada: Entrada,
    ultimo_input: Instant,
    progresso: f32,
    pos_anterior: Ponto,
    volta: u32,
    ultimo_checkpoint: i32,
    finalizado: Option<u32>,
    resultado_enviado: bool,
    iniciado: bool,
    drift: Drift,
    ultima_volta_enviada: u32,
    ultimo_checkpoint_enviado: i32,
}

/// Eventos do canal de controle (`Mensagem::Global`).
#[derive(Debug, Clone, PartialEq)]
pub enum EventoCtl {
    /// C→H. Primeira mensagem no canal do `laminar`; associa endereço ao id.
    Ola {
        /// Id atribuído pelo handshake do `giochi-net`.
        id: u8,
    },
    /// H→C. Configuração da partida.
    Lobby {
        /// Id do cliente.
        id: u8,
        /// Total de carros na pista (humanos + IA).
        total: u8,
        /// Voltas da corrida.
        voltas: u8,
        /// Comprimento da pista em metros (checagem de mapa).
        comprimento: u16,
    },
    /// H→C. Estado periódico de corrida.
    Estado {
        /// Tempo decorrido, centésimos.
        tempo_cs: u32,
        /// Contagem regressiva restante, centésimos.
        contagem_cs: u16,
        /// Pontos de drift do cliente.
        drift: u32,
        /// Volta atual.
        volta: u8,
        /// Posição do cliente.
        posicao: u8,
        /// Total de carros.
        total: u8,
        /// Fase (`0` título, `1` correndo, `2` resultado).
        fase: u8,
        /// Progresso do cliente desde a linha de largada, centímetros.
        ///
        /// Negativo antes de cruzar a linha. O cliente não simula o carro, então
        /// este é o único jeito de o progresso local dele ser verdadeiro.
        progresso_cm: i32,
    },
    /// H→C. A contagem terminou.
    Largada,
    /// H→C ou H→todos. Um carro completou volta.
    Volta {
        /// Carro.
        id: u8,
        /// Nova volta.
        volta: u8,
    },
    /// H→C. Um carro cruzou um checkpoint.
    Checkpoint {
        /// Carro.
        id: u8,
        /// Índice cumulativo (0..).
        indice: u8,
        /// Volta.
        volta: u8,
    },
    /// H→C. Resultado final de um carro.
    Resultado {
        /// Carro.
        id: u8,
        /// Posição final.
        posicao: u8,
        /// `0` vitória, `1` chegou depois, `2` tempo esgotado.
        desfecho: u8,
        /// Tempo final, centésimos.
        tempo_cs: u32,
    },
}

const CTL_OLA: u8 = 1;
const CTL_LOBBY: u8 = 2;
const CTL_ESTADO: u8 = 3;
const CTL_LARGADA: u8 = 4;
const CTL_VOLTA: u8 = 5;
const CTL_CHECKPOINT: u8 = 6;
const CTL_RESULTADO: u8 = 7;

impl EventoCtl {
    /// Serializa no payload opaco de `Mensagem::Global`.
    pub fn codificar(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(24);
        match *self {
            EventoCtl::Ola { id } => {
                b.push(CTL_OLA);
                b.push(id);
            }
            EventoCtl::Lobby {
                id,
                total,
                voltas,
                comprimento,
            } => {
                b.push(CTL_LOBBY);
                b.push(id);
                b.push(total);
                b.push(voltas);
                b.extend_from_slice(&comprimento.to_le_bytes());
            }
            EventoCtl::Estado {
                tempo_cs,
                contagem_cs,
                drift,
                volta,
                posicao,
                total,
                fase,
                progresso_cm,
            } => {
                b.push(CTL_ESTADO);
                b.extend_from_slice(&tempo_cs.to_le_bytes());
                b.extend_from_slice(&contagem_cs.to_le_bytes());
                b.extend_from_slice(&drift.to_le_bytes());
                b.push(volta);
                b.push(posicao);
                b.push(total);
                b.push(fase);
                b.extend_from_slice(&progresso_cm.to_le_bytes());
            }
            EventoCtl::Largada => b.push(CTL_LARGADA),
            EventoCtl::Volta { id, volta } => {
                b.push(CTL_VOLTA);
                b.push(id);
                b.push(volta);
            }
            EventoCtl::Checkpoint {
                id,
                indice,
                volta,
            } => {
                b.push(CTL_CHECKPOINT);
                b.push(id);
                b.push(indice);
                b.push(volta);
            }
            EventoCtl::Resultado {
                id,
                posicao,
                desfecho,
                tempo_cs,
            } => {
                b.push(CTL_RESULTADO);
                b.push(id);
                b.push(posicao);
                b.push(desfecho);
                b.extend_from_slice(&tempo_cs.to_le_bytes());
            }
        }
        b
    }

    /// Lê um payload. `None` para tag desconhecida ou truncada — pacote de
    /// versão futura não pode derrubar o laço.
    pub fn decodificar(b: &[u8]) -> Option<Self> {
        match *b.first()? {
            CTL_OLA if b.len() >= 2 => Some(EventoCtl::Ola { id: b[1] }),
            CTL_LOBBY if b.len() >= 6 => Some(EventoCtl::Lobby {
                id: b[1],
                total: b[2],
                voltas: b[3],
                comprimento: u16::from_le_bytes([b[4], b[5]]),
            }),
            CTL_ESTADO if b.len() >= 19 => Some(EventoCtl::Estado {
                tempo_cs: u32::from_le_bytes([b[1], b[2], b[3], b[4]]),
                contagem_cs: u16::from_le_bytes([b[5], b[6]]),
                drift: u32::from_le_bytes([b[7], b[8], b[9], b[10]]),
                volta: b[11],
                posicao: b[12],
                total: b[13],
                fase: b[14],
                progresso_cm: i32::from_le_bytes([b[15], b[16], b[17], b[18]]),
            }),
            CTL_LARGADA => Some(EventoCtl::Largada),
            CTL_VOLTA if b.len() >= 3 => Some(EventoCtl::Volta {
                id: b[1],
                volta: b[2],
            }),
            CTL_CHECKPOINT if b.len() >= 4 => Some(EventoCtl::Checkpoint {
                id: b[1],
                indice: b[2],
                volta: b[3],
            }),
            CTL_RESULTADO if b.len() >= 8 => Some(EventoCtl::Resultado {
                id: b[1],
                posicao: b[2],
                desfecho: b[3],
                tempo_cs: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            }),
            _ => None,
        }
    }
}

/// Codifica uma pose no `SnapshotInimigo` do `giochi-net`.
///
/// O protocolo tem cinco campos além de posição; nenhum deles é "ângulo". O
/// `hp` (u16) carrega o rumo quantizado — 65536 passos dão 0,0001 rad, muito
/// abaixo do que a interpolação consegue mostrar. `tipo` carrega a velocidade
/// e `fase_chefe` os três bits de estado.
fn codificar_snapshot(id: u16, carro: &Carro) -> SnapshotInimigo {
    let tau = std::f32::consts::TAU;
    let angulo = carro.angulo.rem_euclid(tau) / tau;
    let mut flags = 0u8;
    if carro.no_ar {
        flags |= 0x01;
    }
    if carro.derrapando {
        flags |= 0x02;
    }
    if carro.nitro_usando {
        flags |= 0x04;
    }
    SnapshotInimigo {
        id,
        x: carro.pos.x,
        z: carro.pos.z,
        hp: (angulo * f32::from(u16::MAX)) as u16,
        tipo: ((carro.velocidade.max(0.0) / 90.0) * 255.0).clamp(0.0, 255.0) as u8,
        fase_chefe: flags,
    }
}

fn decodificar_snapshot(s: &SnapshotInimigo, agora: Instant) -> Option<Amostra> {
    if !s.x.is_finite() || !s.z.is_finite() {
        return None;
    }
    Some(Amostra {
        x: s.x,
        z: s.z,
        angulo: f32::from(s.hp) / f32::from(u16::MAX) * std::f32::consts::TAU,
        velocidade: f32::from(s.tipo) / 255.0 * 90.0,
        flags: s.fase_chefe,
        recebido: agora,
    })
}

/// Sessão de rede do jogo.
pub struct Rede {
    papel: Papel,
    host: Option<Host>,
    cliente: Option<Cliente>,
    /// Mantido vivo enquanto a sessão viver; o `Drop` do `Anuncio` manda o
    /// goodbye do mDNS.
    #[allow(dead_code)]
    anunciante: Anuncio,
    controle_host: Option<Transporte>,
    controle_cliente: Option<Transporte>,
    endereco_host: Option<SocketAddr>,
    meu_id: u8,
    // ── host ────────────────────────────────────────────────
    clientes: HashMap<u8, ClienteCorrida>,
    controle_addr: HashMap<u8, SocketAddr>,
    ultimo_estado: Instant,
    ultima_contagem: f32,
    desfecho_host_enviado: bool,
    /// Clientes que já receberam o `Lobby`.
    lobby_enviado: HashMap<u8, bool>,
    // ── cliente ─────────────────────────────────────────────
    trilhas: HashMap<u16, Trilha>,
    ultimo_ola: Instant,
    /// `true` quando o primeiro `Lobby` chegou: é o sinal de que o canal
    /// confiável está de pé, e não só o UDP de snapshots.
    recebeu_lobby: bool,
    estado_servidor: Option<EventoCtl>,
    erro: Option<String>,
    /// Contador de diagnóstico do snapshot mais recente.
    ultimo_snapshot: Instant,
    snapshots_recebidos: u64,
    // ── diagnóstico ─────────────────────────────────────────
    aviso: String,
}

impl Rede {
    /// Sessão inativa (corrida local).
    pub fn off() -> Self {
        Self {
            papel: Papel::Off,
            host: None,
            cliente: None,
            anunciante: Anuncio::nenhum(),
            controle_host: None,
            controle_cliente: None,
            endereco_host: None,
            meu_id: 0,
            clientes: HashMap::new(),
            controle_addr: HashMap::new(),
            ultimo_estado: Instant::now() - INTERVALO_ESTADO,
            ultima_contagem: 0.0,
            desfecho_host_enviado: false,
            lobby_enviado: HashMap::new(),
            trilhas: HashMap::new(),
            ultimo_ola: Instant::now() - INTERVALO_OLA,
            recebeu_lobby: false,
            estado_servidor: None,
            erro: None,
            ultimo_snapshot: Instant::now(),
            snapshots_recebidos: 0,
            aviso: "H: HOSPEDAR   J: ENTRAR NA LAN   F3: ESCALA".into(),
        }
    }

    /// Sobe um host e anuncia por mDNS.
    pub fn hospedar(porta: Option<u16>) -> Result<Self, String> {
        let porta = porta.unwrap_or(PORTA_PADRAO);
        if porta == u16::MAX {
            return Err("porta de controle estouraria em 65535".into());
        }
        let host = Host::novo(Some(porta)).map_err(|e| format!("host UDP na porta {porta}: {e}"))?;
        let endereco = host
            .endereco_anunciado()
            .or_else(|_| host.endereco())
            .map_err(|e| format!("endereço do host: {e}"))?;
        let controle = Transporte::abrir(("0.0.0.0", porta + 1), Modo::Host)
            .map_err(|e| format!("controle laminar na porta {}: {e}", porta + 1))?;
        let anunciante = match giochi_net::Anunciante::anunciar(porta) {
            Ok(a) => {
                eprintln!(
                    "neon-drift: LAN host em {endereco}, anúncio mDNS '{}' (controle UDP {})",
                    a.nome(),
                    porta + 1
                );
                Anuncio::novo(a)
            }
            Err(erro) => {
                // Sem interface de rede (ou mDNS indisponível) o host direto
                // continua válido; o cliente entra por IP.
                eprintln!("neon-drift: aviso — mDNS não anunciou ({erro}); use o IP {endereco}");
                Anuncio::nenhum()
            }
        };
        let aviso = format!("LAN HOST {endereco} - JOGADORES 0");
        Ok(Self {
            papel: Papel::Host,
            host: Some(host),
            cliente: None,
            anunciante,
            controle_host: Some(controle),
            controle_cliente: None,
            endereco_host: Some(endereco),
            clientes: HashMap::new(),
            ultimo_estado: Instant::now() - INTERVALO_ESTADO,
            aviso,
            ..Self::off()
        })
    }

    /// Entra num host já conhecido.
    pub fn entrar(endereco: SocketAddr) -> Result<Self, String> {
        if endereco.port() == u16::MAX {
            return Err("porta de controle inválida".into());
        }
        let cliente = Cliente::entrar(endereco).map_err(|e| format!("cliente UDP: {e}"))?;
        let controle = Transporte::abrir("0.0.0.0:0", Modo::Cliente)
            .map_err(|e| format!("controle laminar: {e}"))?;
        let controle_host = SocketAddr::new(endereco.ip(), endereco.port() + 1);
        eprintln!(
            "neon-drift: LAN cliente para {endereco} (controle {}), procurando handshake",
            controle_host
        );
        Ok(Self {
            papel: Papel::Cliente,
            host: None,
            cliente: Some(cliente),
            anunciante: Anuncio::nenhum(),
            controle_host: None,
            controle_cliente: Some(controle),
            endereco_host: Some(endereco),
            aviso: "LAN: CONECTANDO...".into(),
            ..Self::off()
        })
    }

    /// Procura um host por mDNS, bloqueando no máximo `espera`.
    ///
    /// Bloqueia de propósito: isto roda na tela de título ou na linha de
    /// comando, onde parar 3 s é aceitável. O laço de corrida nunca chama.
    pub fn procurar(espera: Duration) -> Result<SocketAddr, String> {
        let descoberta =
            giochi_net::Descoberta::iniciar().map_err(|e| format!("mDNS: {e}"))?;
        let prazo = Instant::now() + espera;
        let mut achado = None;
        while Instant::now() < prazo {
            if let Some(encontrado) = descoberta.drenar(16).into_iter().next() {
                eprintln!(
                    "neon-drift: lobby mDNS '{}' em {}",
                    encontrado.nome, encontrado.endereco
                );
                achado = Some(encontrado.endereco);
                break;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        descoberta.encerrar();
        achado.ok_or_else(|| {
            format!(
                "nenhum lobby na LAN em {:.0} s (o host precisa estar anunciando)",
                espera.as_secs_f32()
            )
        })
    }

    /// Papel atual.
    pub fn papel(&self) -> Papel {
        self.papel
    }

    /// `true` para host ou cliente.
    pub fn ativo(&self) -> bool {
        self.papel != Papel::Off
    }

    /// `true` quando a simulação local do jogador deve rodar.
    pub fn simula_localmente(&self) -> bool {
        self.papel != Papel::Cliente
    }

    /// Texto de estado para o overlay/HUD.
    pub fn aviso(&self) -> &str {
        &self.aviso
    }

    /// Erro de rede que interrompeu a sessão, se houve.
    pub fn erro(&self) -> Option<&str> {
        self.erro.as_deref()
    }

    /// Resumo de uma linha para o overlay F1.
    pub fn resumo(&self) -> String {
        match self.papel {
            Papel::Off => String::new(),
            Papel::Host => {
                let clientes = self.host.as_ref().map(|h| h.clientes()).unwrap_or(0);
                format!(
                    "LAN HOST {}  CLIENTES {}  SNAP {}HZ",
                    self.endereco_host
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "?".into()),
                    clientes,
                    SNAPSHOT_HZ
                )
            }
            Papel::Cliente => {
                let (id, rtt) = self
                    .cliente
                    .as_ref()
                    .map(|c| (c.jogador().unwrap_or(0), c.rtt_ms().unwrap_or(0)))
                    .unwrap_or((0, 0));
                format!(
                    "LAN CLIENTE {}  {}  RTT {}MS  SNAP {}HZ",
                    id,
                    if self.recebeu_lobby { "OK" } else { "SEM LOBBY" },
                    rtt,
                    SNAPSHOT_HZ
                )
            }
        }
    }

    /// Versão de **uma linha curta** do estado da LAN, para o menu do título.
    ///
    /// O [`Rede::resumo`] tem o que o diagnóstico precisa (Hz, clientes, lobby)
    /// e por isso é longo: 58 caracteres que, no painel de 640×480, saem pelas
    /// duas bordas. No menu o que o jogador precisa saber é **em que papel ele
    /// está** e com quem; o resto vive no overlay F1.
    pub fn resumo_curto(&self) -> String {
        match self.papel {
            Papel::Off => String::new(),
            Papel::Host => {
                let clientes = self.host.as_ref().map(|h| h.clientes()).unwrap_or(0);
                format!(
                    "HOST {} ({})",
                    self.endereco_host
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "?".into()),
                    clientes
                )
            }
            Papel::Cliente => {
                let (id, rtt) = self
                    .cliente
                    .as_ref()
                    .map(|c| (c.jogador().unwrap_or(0), c.rtt_ms().unwrap_or(0)))
                    .unwrap_or((0, 0));
                if self.recebeu_lobby {
                    format!("CLIENTE {id} {rtt}MS")
                } else {
                    format!("CLIENTE {id} SEM LOBBY")
                }
            }
        }
    }

    /// Processa o que chegou. Chamar uma vez por quadro, antes dos passos.
    pub fn processar(&mut self, corrida: &mut Corrida) {
        match self.papel {
            Papel::Off => {}
            Papel::Host => self.processar_host(corrida),
            Papel::Cliente => self.processar_cliente(corrida),
        }
    }

    fn processar_host(&mut self, _corrida: &mut Corrida) {
        let Some(host) = self.host.as_mut() else {
            return;
        };
        let eventos = host.processar(64);
        for (id, mensagem) in eventos {
            if let Mensagem::Input {
                mover_x,
                mover_z,
                acoes,
                camera_x,
                camera_z,
                ..
            } = mensagem
            {
                if let Some(cliente) = self.clientes.get_mut(&id) {
                    cliente.entrada = Entrada {
                        acelerar: mover_z.max(0.0),
                        frear: (-mover_z).max(0.0),
                        virar: mover_x.clamp(-1.0, 1.0),
                        freio_mao: acoes & 0x01 != 0,
                        nitro: acoes & 0x02 != 0,
                    };
                    cliente.ultimo_input = Instant::now();
                }
                // O AOI do host é centrado na câmera que o cliente reportou.
                let _ = (camera_x, camera_z);
            }
        }

        // Controle confiável: "olá" associa o endereço do laminar ao id.
        // Coleta primeiro e altera o estado depois: `criar_cliente` empresta
        // `self` inteiro, e fazer isso dentro do laço que segura o `Transporte`
        // não compila — e, pior, esconderia uma reentrância.
        let mut olas: Vec<(u8, SocketAddr)> = Vec::new();
        if let Some(controle) = self.controle_host.as_mut() {
            for evento in controle.drenar(64) {
                let giochi_net::Evento::Mensagem { de, mensagem } = evento else {
                    continue;
                };
                let Mensagem::Global { dados, .. } = mensagem else {
                    continue;
                };
                let Some(EventoCtl::Ola { id }) = EventoCtl::decodificar(&dados) else {
                    continue;
                };
                if id != 0 {
                    olas.push((id, de));
                }
            }
        }
        for (id, de) in olas {
            self.controle_addr.insert(id, de);
            // `entry().or_insert(false)`: se o lobby ainda não saiu, sai no
            // próximo `publicar_host`; se já saiu, o "olá" periódico é só o
            // batimento que mantém a conexão do `laminar` viva.
            self.lobby_enviado.entry(id).or_insert(false);
            self.criar_cliente(id);
        }
    }

    fn criar_cliente(&mut self, id: u8) {
        if self.clientes.contains_key(&id) {
            return;
        }
        eprintln!("neon-drift: LAN cliente {id} entrou");
        self.clientes.insert(
            id,
            ClienteCorrida {
                id,
                // O carro de verdade é posicionado pelo host em `passo_host`,
                // quando a corrida existir. Até lá fica na origem.
                carro: Carro::novo(Ponto::ZERO, 0.0),
                entrada: Entrada::PARADA,
                ultimo_input: Instant::now(),
                progresso: 0.0,
                pos_anterior: Ponto::ZERO,
                volta: 1,
                ultimo_checkpoint: -1,
                finalizado: None,
                resultado_enviado: false,
                iniciado: false,
                drift: Drift::novo(),
                ultima_volta_enviada: 1,
                ultimo_checkpoint_enviado: -1,
            },
        );
    }

    /// Simula os carros dos clientes dentro do passo fixo.
    ///
    /// `dt` é sempre o do timestep (`TIQUE`), nunca o tempo do quadro: é o que
    /// torna a corrida reproduzível e igual à do host.
    pub fn passo_host(&mut self, corrida: &mut Corrida, dt: f32) {
        if self.papel != Papel::Host || corrida.fase != Fase::Correndo {
            return;
        }
        let comprimento = corrida.pista.comprimento().max(1.0);
        let total_voltas = corrida.total_voltas.max(1);
        let agora = Instant::now();
        for cliente in self.clientes.values_mut() {
            // A grade é montada no primeiro passo da corrida, e não no `olá`:
            // o mapa e a dificuldade só estão decididos quando a corrida começa.
            if !cliente.iniciado {
                let s = (comprimento - 4.0 - f32::from(cliente.id) * 4.0).max(0.0);
                cliente
                    .carro
                    .posicionar(&corrida.pista, s, -3.0 + f32::from(cliente.id) * 1.6);
                cliente.carro.limitador = corrida.dificuldade.fator_jogador();
                cliente.pos_anterior = cliente.carro.pos;
                cliente.progresso = -4.0 - f32::from(cliente.id) * 4.0;
                cliente.ultimo_checkpoint = -1;
                cliente.iniciado = true;
            }
            if corrida.contagem > 0.0 || cliente.finalizado.is_some() {
                continue;
            }
            let entrada = if agora.duration_since(cliente.ultimo_input) > TIMEOUT_INPUT {
                Entrada::PARADA
            } else {
                cliente.entrada
            };
            let evento = cliente.carro.passo(&entrada, &corrida.pista, dt);
            cliente.drift.passo(
                cliente.carro.derrapando,
                cliente.carro.velocidade,
                cliente.carro.derrapagem,
                dt,
            );
            if evento == giochi_neon_drift_core::carro::Evento::Bateu {
                cliente.drift.colidir();
            }
            let direcao = corrida.pista.direcao_em(cliente.carro.s);
            let deslocamento = cliente.carro.pos.menos(cliente.pos_anterior);
            cliente.pos_anterior = cliente.carro.pos;
            let ds = deslocamento.x * direcao.x + deslocamento.z * direcao.z;
            // `NaN` propaga: um `s` não finito (de uma posição ruim vinda do
            // cliente) envenenaria o progresso para sempre, e o ranking
            // passaria a devolver 1 para o carro em toda comparação — foi o
            // sintoma intermitente de "o cliente aparece em primeiro com o
            // carro parado". O avanço inválido é descartado, não somado.
            if ds.is_finite() {
                cliente.progresso += ds;
            }

            let volta = ((cliente.progresso / comprimento).floor() as i64 + 1).max(1) as u32;
            if volta > cliente.volta {
                cliente.volta = volta;
                if cliente.volta > total_voltas {
                    cliente.finalizado = Some((corrida.tempo * 100.0) as u32);
                }
            }
            let checkpoint = (cliente.progresso / (comprimento / 4.0)).floor() as i32;
            if checkpoint > cliente.ultimo_checkpoint && cliente.progresso >= 0.0 {
                cliente.ultimo_checkpoint = checkpoint;
            }
        }
    }

    /// Publica snapshots e estado, e devolve as poses dos clientes para desenho.
    ///
    /// Chamar **uma vez por quadro**. O snapshot em si é limitado a 15 Hz
    /// dentro de [`Host::tick_snapshot`]; o estado de corrida sai a 4 Hz e nas
    /// bordas (largada, volta, checkpoint, resultado).
    pub fn publicar_host(&mut self, corrida: &mut Corrida) -> Vec<CarroRede> {
        if self.papel != Papel::Host {
            return Vec::new();
        }
        let agora = Instant::now();

        // O host também precisa de um desfecho honesto quando um cliente
        // cruzou a linha antes dele. O núcleo só conhece jogador + IA.
        if corrida.desfecho.is_some() {
            let humanos_terminaram = self
                .clientes
                .values()
                .filter(|c| c.finalizado.is_some())
                .count();
            if humanos_terminaram > 0 {
                corrida_posicao_override(corrida, humanos_terminaram + 1);
            }
        }

        let mut eventos: Vec<(Option<u8>, EventoCtl)> = Vec::new();

        // Largada: borda de contagem > 0 → 0.
        if corrida.fase == Fase::Correndo && self.ultima_contagem > 0.0 && corrida.contagem <= 0.0
        {
            eventos.push((None, EventoCtl::Largada));
        }
        self.ultima_contagem = corrida.contagem;

        // Resultado do host: uma vez.
        if corrida.desfecho.is_some() && !self.desfecho_host_enviado {
            self.desfecho_host_enviado = true;
            let desfecho = corrida.desfecho;
            eventos.push((
                None,
                EventoCtl::Resultado {
                    id: 0,
                    posicao: corrida.posicao.min(255) as u8,
                    desfecho: codigo_desfecho(desfecho),
                    tempo_cs: (corrida.tempo * 100.0) as u32,
                },
            ));
            // Quem ainda não cruzou recebe o resultado do host (a corrida
            // acabou para todos) em vez de ficar preso em "CORRENDO".
            for cliente in self.clientes.values() {
                if cliente.finalizado.is_none() && !cliente.resultado_enviado {
                    eventos.push((
                        Some(cliente.id),
                        EventoCtl::Resultado {
                            id: cliente.id,
                            posicao: self.posicao_de(u16::from(cliente.id), corrida).min(255)
                                as u8,
                            desfecho: if desfecho == Some(Desfecho::TempoEsgotado) {
                                2
                            } else {
                                1
                            },
                            tempo_cs: (corrida.tempo * 100.0) as u32,
                        },
                    ));
                }
            }
        }

        let periodic = agora.duration_since(self.ultimo_estado) >= INTERVALO_ESTADO;
        if periodic {
            self.ultimo_estado = agora;
        }
        let total = (self.clientes.len() + corrida.adversarios.len() + 1).min(255) as u8;
        if std::env::var_os("GIOCHI_REDE_DEBUG").is_some() && periodic {
            // Diagnóstico do ranking: sem estes números, "posição 1" no
            // cliente é afirmação sem prova — e foi assim que um progresso
            // absurdo do carro remoto passou despercebido.
            eprintln!(
                "rede: host {:.1} m · {}",
                corrida.progresso_jogador(),
                self.clientes
                    .values()
                    .map(|c| format!(
                        "cliente {} prog {:.1} pos {} volta {}",
                        c.id,
                        c.progresso,
                        self.posicao_de(u16::from(c.id), corrida),
                        c.volta
                    ))
                    .collect::<Vec<_>>()
                    .join(" · "),
            );
        }
        let mut destinos: Vec<u8> = self.controle_addr.keys().copied().collect();
        destinos.sort_unstable();
        for id in destinos {
            let posicao = self.posicao_de(u16::from(id), corrida).min(255) as u8;
            if !self.lobby_enviado.get(&id).copied().unwrap_or(false) {
                self.lobby_enviado.insert(id, true);
                eventos.push((
                    Some(id),
                    EventoCtl::Lobby {
                        id,
                        total,
                        voltas: corrida.total_voltas.min(255) as u8,
                        comprimento: corrida.pista.comprimento().min(f32::from(u16::MAX)) as u16,
                    },
                ));
            }
            if periodic {
                let (volta, drift, progresso_cm) = self
                    .clientes
                    .get(&id)
                    .map(|c| (c.volta, c.drift.total(), (c.progresso * 100.0) as i32))
                    .unwrap_or((1, 0, 0));

                eventos.push((
                    Some(id),
                    EventoCtl::Estado {
                        tempo_cs: (corrida.tempo * 100.0) as u32,
                        contagem_cs: (corrida.contagem * 100.0).max(0.0) as u16,
                        drift,
                        volta: volta.min(255) as u8,
                        posicao,
                        total,
                        fase: codigo_fase(corrida.fase),
                        progresso_cm,
                    },
                ));
            }
            // Volta e checkpoint só na borda. Enviá-los todo quadro seria
            // tráfego confiável inútil a 60 Hz e o cliente reiniciaria o HUD
            // de volta a cada pacote.
            if let Some(cliente) = self.clientes.get(&id) {
                if cliente.volta != cliente.ultima_volta_enviada {
                    eventos.push((
                        Some(id),
                        EventoCtl::Volta {
                            id,
                            volta: cliente.volta.min(255) as u8,
                        },
                    ));
                }
                if cliente.ultimo_checkpoint >= 0
                    && cliente.ultimo_checkpoint != cliente.ultimo_checkpoint_enviado
                {
                    eventos.push((
                        Some(id),
                        EventoCtl::Checkpoint {
                            id,
                            indice: cliente.ultimo_checkpoint.min(255) as u8,
                            volta: cliente.volta.min(255) as u8,
                        },
                    ));
                }
                if let Some(tempo_cs) = cliente.finalizado {
                    if !cliente.resultado_enviado {
                        eventos.push((
                            Some(id),
                            EventoCtl::Resultado {
                                id,
                                posicao: self.posicao_de(u16::from(id), corrida).min(255) as u8,
                                desfecho: if self.posicao_de(u16::from(id), corrida) == 1 {
                                    0
                                } else {
                                    1
                                },
                                tempo_cs,
                            },
                        ));
                    }
                }
            }
        }

        // Marca o que já foi enfileirado, depois de os empréstimos imutáveis
        // acabarem.
        for (destino, evento) in &eventos {
            if let (Some(id), EventoCtl::Resultado { .. }) = (destino, evento) {
                if let Some(cliente) = self.clientes.get_mut(id) {
                    cliente.resultado_enviado = true;
                }
            }
            if let (Some(id), EventoCtl::Volta { .. }) = (destino, evento) {
                if let Some(cliente) = self.clientes.get_mut(id) {
                    cliente.ultima_volta_enviada = cliente.volta;
                }
            }
            if let (Some(id), EventoCtl::Checkpoint { .. }) = (destino, evento) {
                if let Some(cliente) = self.clientes.get_mut(id) {
                    cliente.ultimo_checkpoint_enviado = cliente.ultimo_checkpoint;
                }
            }
        }

        if let Some(controle) = self.controle_host.as_mut() {
            for (destino, evento) in eventos {
                let dados = evento.codificar();
                let msg = Mensagem::Global { tick: 0, dados };
                match destino {
                    Some(id) => {
                        if let Some(addr) = self.controle_addr.get(&id) {
                            let _ = controle.enviar(*addr, &msg);
                        }
                    }
                    None => {
                        for addr in self.controle_addr.values() {
                            let _ = controle.enviar(*addr, &msg);
                        }
                    }
                }
            }
        }

        // Nada de snapshot antes da corrida: no título os carros dos clientes
        // ainda estão na origem (a grade só é montada no primeiro passo), e
        // publicá-los desenharia um carro parado no meio do nada.
        if corrida.fase == Fase::Correndo {
            let snapshots = self.snapshots_host(corrida);
            if let Some(host) = self.host.as_mut() {
                for id in self.controle_addr.keys() {
                    host.definir_area(*id, RAIO_AOI);
                }
                let _ = host.tick_snapshot(&snapshots);
            }
        }

        self.clientes
            .values()
            .map(|c| CarroRede {
                id: u16::from(c.id),
                pos: c.carro.pos,
                angulo: c.carro.angulo,
                altura: c.carro.altura,
                velocidade: c.carro.velocidade,
                // No host o jogador local é o id 0; os clientes são adversários.
                cor: CarroRede::cor_do_id(u16::from(c.id), None),
                derrapando: c.carro.derrapando,
                no_ar: c.carro.no_ar,
                nitro: c.carro.nitro_usando,
            })
            .collect()
    }

    fn snapshots_host(&self, corrida: &Corrida) -> Vec<SnapshotInimigo> {
        let mut lista = Vec::with_capacity(self.clientes.len() + corrida.adversarios.len() + 1);
        lista.push(codificar_snapshot(0, &corrida.carro));
        for (i, adversario) in corrida.adversarios.iter().enumerate() {
            let mut carro = Carro::novo(
                adversario.posicao(&corrida.pista),
                adversario.angulo(&corrida.pista),
            );
            carro.velocidade = adversario.velocidade;
            lista.push(codificar_snapshot(ID_IA + i as u16, &carro));
        }
        for cliente in self.clientes.values() {
            lista.push(codificar_snapshot(u16::from(cliente.id), &cliente.carro));
        }
        lista
    }

    /// Posição de um carro no pelotão: quem terminou vem antes; entre os que
    /// correm, vale o progresso acumulado.
    fn posicao_de(&self, id: u16, corrida: &Corrida) -> usize {
        let total = corrida.pista.comprimento().max(1.0) * corrida.total_voltas.max(1) as f32;
        let progresso = |id: u16| -> f32 {
            if id == 0 {
                corrida.progresso_jogador()
            } else if id >= ID_IA {
                corrida
                    .adversarios
                    .get((id - ID_IA) as usize)
                    .map(|a| a.progresso(&corrida.pista))
                    .unwrap_or(f32::NEG_INFINITY)
            } else {
                self.clientes
                    .get(&(id as u8))
                    .map(|c| {
                        if c.finalizado.is_some() {
                            total + 1.0
                        } else {
                            c.progresso
                        }
                    })
                    .unwrap_or(f32::NEG_INFINITY)
            }
        };
        let meu = progresso(id);
        let mut outros: Vec<f32> = Vec::with_capacity(corrida.adversarios.len() + self.clientes.len() + 1);
        outros.push(progresso(0));
        for i in 0..corrida.adversarios.len() {
            outros.push(progresso(ID_IA + i as u16));
        }
        for c in self.clientes.keys() {
            outros.push(progresso(u16::from(*c)));
        }
        posicao_entre(meu, &outros)
    }

    fn processar_cliente(&mut self, corrida: &mut Corrida) {
        let Some(cliente) = self.cliente.as_mut() else {
            return;
        };
        let mensagens = cliente.processar(64);
        if let giochi_net::client::Estado::Conectado(id) = cliente.estado() {
            if self.meu_id != id {
                self.meu_id = id;
                self.aviso = format!("LAN CLIENTE {id} - CONECTADO");
                eprintln!("neon-drift: LAN cliente id {id}");
            }
        }
        if matches!(cliente.estado(), giochi_net::client::Estado::Recusado)
            && self.erro.is_none()
        {
            self.erro = Some("host recusou ou parou de responder".into());
            self.aviso = "LAN: DESCONECTADO".into();
        }
        let agora = Instant::now();
        for mensagem in mensagens {
            if let Mensagem::Inimigos { inimigos, .. } = mensagem {
                for s in &inimigos {
                    if let Some(amostra) = decodificar_snapshot(s, agora) {
                        match self.trilhas.get_mut(&s.id) {
                            Some(trilha) => trilha.inserir(amostra),
                            None => {
                                self.trilhas.insert(s.id, Trilha::nova(amostra));
                            }
                        }
                    }
                }
                self.ultimo_snapshot = agora;
                self.snapshots_recebidos += 1;
            }
        }

        // Canal confiável do controle.
        let Some(controle) = self.controle_cliente.as_mut() else {
            return;
        };
        for evento in controle.drenar(64) {
            let giochi_net::Evento::Mensagem { mensagem, .. } = evento else {
                continue;
            };
            let Mensagem::Global { dados, .. } = mensagem else {
                continue;
            };
            let Some(evento) = EventoCtl::decodificar(&dados) else {
                continue;
            };
            if !matches!(evento, EventoCtl::Ola { .. }) {
                self.recebeu_lobby = true;
            }
            self.aplicar_evento(evento, corrida);
        }
    }

    fn aplicar_evento(&mut self, evento: EventoCtl, corrida: &mut Corrida) {
        self.estado_servidor = Some(evento.clone());
        match evento {
            EventoCtl::Lobby {
                id,
                total,
                voltas,
                comprimento,
            } => {
                self.meu_id = id;
                let nosso = corrida.pista.comprimento();
                if (f32::from(comprimento) - nosso).abs() > 2.0 {
                    self.erro = Some(format!(
                        "mapa diferente: host {comprimento} m, local {nosso:.0} m — use --mapa igual"
                    ));
                    eprintln!("neon-drift: {}", self.erro.as_deref().unwrap_or(""));
                }
                while corrida.adversarios.len() + 1 < usize::from(total) {
                    let indice = corrida.adversarios.len();
                    corrida.adversarios.push(Adversario::novo(
                        "REDE",
                        0.0,
                        0.8,
                        indice as f32,
                        indice % CORES_ADVERSARIOS.len(),
                    ));
                }
                let _ = voltas;
                self.aviso = format!("LAN CLIENTE {id} - {total} CARROS");
            }
            EventoCtl::Estado {
                tempo_cs,
                contagem_cs,
                drift,
                volta,
                posicao,
                total,
                fase,
                progresso_cm,
            } => {
                if fase == 1 && corrida.fase != Fase::Correndo {
                    corrida.comecar();
                }
                // O cliente não roda `passo`, então o progresso local ficaria na
                // grade: `largou()` diria "não" a corrida inteira.
                corrida.sincronizar_progresso(progresso_cm as f32 / 100.0);
                corrida.tempo = tempo_cs as f32 / 100.0;
                corrida.contagem = contagem_cs as f32 / 100.0;
                corrida.drift.pontos = drift;
                corrida.volta = u32::from(volta.max(1));
                corrida.posicao = usize::from(posicao.max(1));
                while corrida.adversarios.len() + 1 < usize::from(total) {
                    let indice = corrida.adversarios.len();
                    corrida.adversarios.push(Adversario::novo(
                        "REDE",
                        0.0,
                        0.8,
                        indice as f32,
                        indice % CORES_ADVERSARIOS.len(),
                    ));
                }
            }
            EventoCtl::Largada => {
                if corrida.fase != Fase::Correndo {
                    corrida.comecar();
                    // As amostras da corrida anterior não podem interpolar
                    // dentro da largada nova.
                    self.trilhas.clear();
                }
                corrida.contagem = 0.0;
            }
            EventoCtl::Volta { id, volta } => {
                if id == self.meu_id {
                    corrida.volta = u32::from(volta.max(1));
                }
            }
            EventoCtl::Checkpoint { id, indice, volta } => {
                if id == self.meu_id {
                    let _ = (indice, volta);
                }
            }
            EventoCtl::Resultado {
                id,
                posicao,
                desfecho,
                tempo_cs,
            } => {
                if id == self.meu_id || id == 0 {
                    corrida.fase = Fase::Resultado;
                    corrida.posicao = usize::from(posicao.max(1));
                    corrida.tempo = tempo_cs as f32 / 100.0;
                    corrida.desfecho = Some(match desfecho {
                        2 => Desfecho::TempoEsgotado,
                        1 => Desfecho::ChegouDepois,
                        _ => Desfecho::Vitoria,
                    });
                }
            }
            EventoCtl::Ola { .. } => {}
        }
    }

    /// Manda o input e devolve as poses interpoladas para desenhar.
    ///
    /// Chamar **uma vez por quadro**, depois dos passos fixos. O `dt` é o do
    /// quadro e só serve para o contador local de contagem; a posição vem do
    /// host.
    pub fn aplicar_cliente(
        &mut self,
        corrida: &mut Corrida,
        entrada: &Entrada,
        dt: f32,
    ) -> Vec<CarroRede> {
        if self.papel != Papel::Cliente {
            return Vec::new();
        }

        // Input a 60 Hz: `Input` é sequenciado no `laminar`, então um atrasado
        // não chega depois do seguinte.
        if let Some(cliente) = self.cliente.as_mut() {
            let camera = corrida.carro.pos;
            let msg = Mensagem::Input {
                tick: 0,
                mover_x: entrada.virar.clamp(-1.0, 1.0),
                mover_z: entrada.acelerar - entrada.frear,
                mirar_x: 0.0,
                mirar_z: 0.0,
                acoes: u32::from(entrada.freio_mao) | (u32::from(entrada.nitro) << 1),
                camera_x: camera.x,
                camera_z: camera.z,
            };
            let _ = cliente.enviar_input(msg);
        }

        // "Olá" periódico, **sempre** — não só até o lobby chegar.
        //
        // O `laminar` fecha uma conexão ociosa (`idle_connection_timeout`) e,
        // depois disso, `send` enfileira num stream que ninguém mais entrega:
        // medido, o cliente parava de receber `Estado` ~5 s depois da largada e
        // ficava com a posição congelada na última mensagem. Um pacote por
        // segundo custa 6 bytes e mantém a conexão viva.
        let agora = Instant::now();
        if agora.duration_since(self.ultimo_ola) >= INTERVALO_OLA {
            self.ultimo_ola = agora;
            if let (Some(controle), Some(end)) =
                (self.controle_cliente.as_mut(), self.endereco_host)
            {
                if self.meu_id != 0 {
                    let msg = Mensagem::Global {
                        tick: 0,
                        dados: EventoCtl::Ola { id: self.meu_id }.codificar(),
                    };
                    let _ = controle.enviar(SocketAddr::new(end.ip(), end.port() + 1), &msg);
                }
            }
        }

        // A contagem regressiva roda local para o número andar entre dois
        // `Estado` (4 Hz), e o host a corrige no próximo. O **tempo** não:
        // incrementá-lo aqui faria o HUD avançar mesmo com a rede parada, e o
        // sintoma seria um cronômetro que mente em vez de um cliente que
        // parece travado.
        if corrida.fase == Fase::Correndo && corrida.contagem > 0.0 {
            corrida.contagem = (corrida.contagem - dt).max(0.0);
        }

        let pista = &corrida.pista;
        let meu_id = Some(self.meu_id);
        let mut carros = Vec::with_capacity(self.trilhas.len());
        for (id, trilha) in &self.trilhas {
            let (x, z, angulo, velocidade, amostra) =
                trilha.pose(agora, INTERVALO_SNAPSHOT);
            let pos = Ponto::new(x, z);
            let local = pista.localizar(pos, None);
            let altura = pista.altura_em(local.s);
            let carro = CarroRede {
                id: *id,
                pos,
                angulo,
                altura,
                velocidade,
                cor: CarroRede::cor_do_id(*id, meu_id),
                derrapando: amostra.derrapando(),
                no_ar: amostra.no_ar(),
                nitro: amostra.nitro(),
            };
            if u16::from(self.meu_id) == *id {
                // O carro local é o mesmo objeto que a câmera e as partículas
                // leem; escrever aqui mantém uma verdade só.
                corrida.carro.pos = pos;
                corrida.carro.angulo = angulo;
                corrida.carro.velocidade = velocidade;
                corrida.carro.altura = altura;
                corrida.carro.s = local.s;
                corrida.carro.indice_pista = Some(local.indice);
                corrida.carro.fora = local.fora_por();
                corrida.carro.derrapando = amostra.derrapando();
                corrida.carro.no_ar = amostra.no_ar();
                corrida.carro.nitro_usando = amostra.nitro();
            }
            carros.push(carro);
        }
        carros
    }
}

/// Dono do anúncio mDNS, com *goodbye* no `Drop`.
///
/// O `Rede` não implementa `Drop` de propósito: os construtores usam
/// `..Self::off()` para preencher os campos que não mudam, e um tipo com `Drop`
/// não pode ser movido por atualização de struct. O anúncio ganha um dono
/// próprio — que é quem precisa do *goodbye* de verdade.
struct Anuncio(Option<giochi_net::Anunciante>);

impl Anuncio {
    fn novo(anunciante: giochi_net::Anunciante) -> Self {
        Self(Some(anunciante))
    }

    fn nenhum() -> Self {
        Self(None)
    }
}

impl Drop for Anuncio {
    fn drop(&mut self) {
        // `Anunciante::encerrar` manda o goodbye (TTL 0). Sem isso o lobby
        // fica na lista dos outros aparelhos até o TTL expirar, e quem entra
        // tenta um host que já morreu.
        if let Some(anunciante) = self.0.take() {
            anunciante.encerrar();
        }
    }
}

/// Posição de corrida de um carro dado o progresso dele e o dos outros.
///
/// Comparação com `is_finite()` explícito: `NaN > x` é sempre falso, então um
/// progresso envenenado faria o carro "vencer" todas as comparações e aparecer
/// em primeiro. Tratar não-finito como ausente mantém o ranking honesto.
fn posicao_entre(meu: f32, outros: &[f32]) -> usize {
    if !meu.is_finite() {
        // Progresso desconhecido vai para o fim da fila, nunca para o topo:
        // `NaN > x` é falso, e deixar a comparação decidir dava "1º" para o
        // carro que o host não sabia onde estava.
        return outros.len() + 1;
    }
    1 + outros.iter().filter(|p| p.is_finite() && **p > meu).count()
}

/// Corrige o desfecho do host quando um humano cruzou antes dele.
fn corrida_posicao_override(corrida: &mut Corrida, posicao: usize) {
    if posicao > 1 {
        corrida.posicao = posicao;
        corrida.desfecho = Some(Desfecho::ChegouDepois);
    }
}

fn codigo_fase(fase: Fase) -> u8 {
    match fase {
        Fase::Titulo => 0,
        Fase::Correndo => 1,
        Fase::Resultado => 2,
    }
}

fn codigo_desfecho(desfecho: Option<Desfecho>) -> u8 {
    match desfecho {
        Some(Desfecho::TempoEsgotado) => 2,
        Some(Desfecho::ChegouDepois) => 1,
        Some(Desfecho::Vitoria) | None => 0,
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn agora_menos(segundos: f32) -> Instant {
        Instant::now() - Duration::from_secs_f32(segundos)
    }

    #[test]
    fn o_evento_ida_e_volta_preserva_todos_os_campos() {
        let eventos = [
            EventoCtl::Ola { id: 3 },
            EventoCtl::Lobby {
                id: 2,
                total: 5,
                voltas: 2,
                comprimento: 1200,
            },
            EventoCtl::Estado {
                tempo_cs: 1234,
                contagem_cs: 250,
                drift: 9000,
                volta: 2,
                posicao: 3,
                total: 5,
                fase: 1,
                // Negativo de propósito: a grade põe o cliente atrás da linha, e
                // um `u32`/`u16` aqui devolveria "quase uma volta".
                progresso_cm: -1234,
            },
            EventoCtl::Largada,
            EventoCtl::Volta { id: 4, volta: 2 },
            EventoCtl::Checkpoint {
                id: 4,
                indice: 7,
                volta: 2,
            },
            EventoCtl::Resultado {
                id: 0,
                posicao: 1,
                desfecho: 0,
                tempo_cs: 4567,
            },
        ];
        for evento in eventos {
            let bytes = evento.codificar();
            assert_eq!(EventoCtl::decodificar(&bytes), Some(evento));
        }
    }

    #[test]
    fn um_payload_truncado_nao_decodifica() {
        assert_eq!(EventoCtl::decodificar(&[]), None);
        assert_eq!(EventoCtl::decodificar(&[CTL_ESTADO, 0, 1]), None);
        assert_eq!(EventoCtl::decodificar(&[99, 1, 2, 3]), None);
    }

    #[test]
    fn o_snapshot_codifica_rumo_velocidade_e_flags() {
        let mut carro = Carro::novo(Ponto::new(10.0, -4.0), 1.25);
        carro.velocidade = 45.0;
        carro.derrapando = true;
        carro.no_ar = true;
        let s = codificar_snapshot(7, &carro);
        assert_eq!(s.id, 7);
        assert!((s.x - 10.0).abs() < 1e-6);
        assert!((s.z + 4.0).abs() < 1e-6);
        let volta = decodificar_snapshot(&s, Instant::now()).expect("amostra");
        assert!((volta.angulo - 1.25).abs() < 0.001);
        assert!((volta.velocidade - 45.0).abs() < 1.0);
        assert!(volta.no_ar());
        assert!(volta.derrapando());
        assert!(!volta.nitro());
    }

    #[test]
    fn a_pose_sobrevive_ao_fio_com_coordenada_negativa_e_rumo_cheio() {
        // Regressão do cliente com o carro colado na origem: o `giochi-net`
        // quantizava `x`/`z` em centímetros `u16` **sem sinal** (negativo virava
        // 0) e mandava só o byte baixo do `hp`, onde este jogo empacota o rumo.
        // O teste de ida e volta do crate usava só posições positivas e `hp`
        // abaixo de 256, então passava com o bug dentro.
        let mut carro = Carro::novo(Ponto::new(-37.25, -412.75), 5.9);
        carro.velocidade = 33.0;
        let s = codificar_snapshot(1, &carro);
        // O snapshot atravessa o fio como o host o manda, não como o struct diz.
        let bytes = Mensagem::Inimigos {
            tick: 1,
            inimigos: vec![s],
        }
        .codificar();
        let Mensagem::Inimigos { inimigos, .. } =
            Mensagem::decodificar(&bytes).expect("snapshot no fio")
        else {
            panic!("variante errada");
        };
        let amostra = decodificar_snapshot(&inimigos[0], Instant::now()).expect("amostra");
        assert!((amostra.x + 37.25).abs() < 1e-3, "x = {}", amostra.x);
        assert!((amostra.z + 412.75).abs() < 1e-3, "z = {}", amostra.z);
        // O rumo volta em `0..TAU` (é o que `codificar_snapshot` normaliza) e o
        // erro do arredondamento de 16 bits é 1e-4 rad; comparar o ângulo como
        // ângulo evita depender da volta escolhida.
        let diferenca = (amostra.angulo - 5.9).rem_euclid(std::f32::consts::TAU);
        let diferenca = diferenca.min(std::f32::consts::TAU - diferenca);
        assert!(diferenca < 1e-3, "rumo = {} (diferença {diferenca})", amostra.angulo);
    }

    #[test]
    fn a_interpolacao_anda_do_anterior_para_o_atual() {
        let t0 = agora_menos(0.033);
        let mut trilha = Trilha::nova(Amostra {
            x: 0.0,
            z: 0.0,
            angulo: 0.0,
            velocidade: 10.0,
            flags: 0,
            recebido: t0 - Duration::from_millis(66),
        });
        trilha.inserir(Amostra {
            x: 10.0,
            z: 0.0,
            angulo: std::f32::consts::PI,
            velocidade: 20.0,
            flags: 0x02,
            recebido: t0,
        });
        let (x, _, angulo, velocidade, amostra) =
            trilha.pose(t0 + Duration::from_millis(33), INTERVALO_SNAPSHOT);
        assert!((x - 5.0).abs() < 0.6, "x = {x}");
        assert!((angulo - std::f32::consts::FRAC_PI_2).abs() < 0.1);
        assert!((velocidade - 15.0).abs() < 1.0);
        assert!(amostra.derrapando(), "flag discreta vem da amostra nova");
    }

    #[test]
    fn a_interpolacao_segura_na_ponta_quando_a_rede_atrasa() {
        let t0 = agora_menos(0.5);
        let mut trilha = Trilha::nova(Amostra {
            x: 1.0,
            z: 2.0,
            angulo: 0.0,
            velocidade: 5.0,
            flags: 0,
            recebido: t0,
        });
        trilha.inserir(Amostra {
            x: 2.0,
            z: 2.0,
            angulo: 0.0,
            velocidade: 5.0,
            flags: 0,
            recebido: t0,
        });
        let (x, _, _, _, _) = trilha.pose(Instant::now(), INTERVALO_SNAPSHOT);
        assert!((x - 2.0).abs() < 1e-6, "não extrapola: {x}");
    }

    #[test]
    fn o_angulo_interpola_pelo_caminho_mais_curto() {
        // 350° → 10° tem de passar por 0°, não dar a volta por 180°.
        let a = 350.0f32.to_radians();
        let b = 10.0f32.to_radians();
        let meio = lerp_angulo(a, b, 0.5);
        let esperado = 0.0;
        let diff = (meio - esperado).abs();
        assert!(diff < 1e-5, "meio = {meio}");
    }

    #[test]
    fn a_cor_do_carro_e_estavel_e_distingue_host_de_ia() {
        assert_eq!(CarroRede::cor_do_id(0, None), COR_JOGADOR);
        assert_ne!(
            CarroRede::cor_do_id(1, None),
            CarroRede::cor_do_id(2, None)
        );
        assert_eq!(
            CarroRede::cor_do_id(100, None),
            CarroRede::cor_do_id(103, None)
        );
        assert_ne!(
            CarroRede::cor_do_id(0, None),
            CarroRede::cor_do_id(100, None)
        );
    }

    #[test]
    fn cada_lado_pinta_como_jogador_o_carro_que_esta_sendo_dirigido() {
        // Regressão do "a tela do cliente parece a do host": a cor vinha só do
        // id, então no cliente o carro do host (id 0) saía verde de jogador e o
        // carro do próprio cliente saía com cor de adversário.
        assert_eq!(CarroRede::cor_do_id(0, None), COR_JOGADOR);
        assert_eq!(CarroRede::cor_do_id(1, Some(1)), COR_JOGADOR);
        assert_ne!(CarroRede::cor_do_id(0, Some(1)), COR_JOGADOR);
        assert_ne!(CarroRede::cor_do_id(1, None), COR_JOGADOR);
        // Um segundo cliente vê o primeiro como adversário e a si mesmo como
        // jogador — as duas telas concordam sobre quem é quem.
        assert_eq!(CarroRede::cor_do_id(2, Some(2)), COR_JOGADOR);
        assert_ne!(CarroRede::cor_do_id(1, Some(2)), COR_JOGADOR);
    }

    #[test]
    fn a_ia_tem_as_mesmas_cores_nos_dois_lados() {
        // O host desenha a IA com `CORES_ADVERSARIOS[i]`; o cliente via cinza
        // fixo, e a mesma corrida tinha duas paletas.
        for i in 0..CORES_ADVERSARIOS.len() as u16 {
            assert_eq!(
                CarroRede::cor_do_id(ID_IA + i, Some(1)),
                CORES_ADVERSARIOS[i as usize]
            );
        }
    }

    #[test]
    fn o_ranking_conta_quem_esta_a_frente() {
        assert_eq!(posicao_entre(10.0, &[5.0, 20.0, 30.0]), 3);
        assert_eq!(posicao_entre(100.0, &[5.0, 20.0, 30.0]), 1);
    }

    #[test]
    fn progresso_nao_finito_nao_vence_a_corrida() {
        // Um carro com progresso NaN não pode aparecer em primeiro — é o
        // sintoma que apareceu quando um `s` ruim envenenou o acumulador.
        // `meu` desconhecido → fim da fila, nunca primeiro.
        assert_eq!(posicao_entre(f32::NAN, &[1.0, 2.0]), 3);
        // Um **outro** desconhecido não conta como quem está à frente.
        assert_eq!(posicao_entre(5.0, &[f32::NAN, 2.0]), 1);
        assert_eq!(posicao_entre(5.0, &[f32::INFINITY, 2.0]), 1);
        // Mas um outro finito à frente conta normalmente.
        assert_eq!(posicao_entre(5.0, &[f32::NAN, 9.0]), 2);
    }

    #[test]
    fn o_batimento_do_controle_e_mais_rapido_que_o_timeout_do_laminar() {
        // O `laminar` fecha conexão ociosa em poucos segundos; medido, o
        // cliente parava de receber `Estado` ~5 s depois da largada e congelava
        // a posição. O batimento tem de ser bem mais frequente que isso.
        assert!(
            INTERVALO_OLA <= Duration::from_secs(2),
            "batimento de {INTERVALO_OLA:?} é lento demais para manter a conexão"
        );
    }

    #[test]
    fn a_sessao_inativa_nao_desvia_do_laco_local() {
        let rede = Rede::off();
        assert_eq!(rede.papel(), Papel::Off);
        assert!(rede.simula_localmente());
        assert!(!rede.ativo());
        assert!(rede.resumo().is_empty());
    }

    #[test]
    fn os_codigos_de_fase_e_desfecho_sao_os_do_protocolo() {
        assert_eq!(codigo_fase(Fase::Titulo), 0);
        assert_eq!(codigo_fase(Fase::Correndo), 1);
        assert_eq!(codigo_fase(Fase::Resultado), 2);
        assert_eq!(codigo_desfecho(Some(Desfecho::Vitoria)), 0);
        assert_eq!(codigo_desfecho(Some(Desfecho::ChegouDepois)), 1);
        assert_eq!(codigo_desfecho(Some(Desfecho::TempoEsgotado)), 2);
    }
}
