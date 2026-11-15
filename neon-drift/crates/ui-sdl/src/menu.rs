//! Menu do título: o caminho para **tudo** sem teclado.
//!
//! ## Por que ele existe
//!
//! O jogo nasceu com teclas para as opções que não são de corrida: `H`/`J`
//! para LAN, `F3` para a escala de render, `F1` para o overlay, `-`/`=` para
//! andar de degrau. Num handheld **não há teclado**, então essas quatro coisas
//! eram inalcançáveis justamente nos aparelhos em que a escala de render mais
//! importa (fill-rate de Mali-G31) e em que a LAN é o modo de jogar com alguém.
//!
//! O menu resolve isso com o que o aparelho tem: direcional, A e START.
//!
//! ## Por que ele é um módulo puro
//!
//! Sem GPU, sem SDL e sem estado global: recebe passos, devolve [`Pedido`]. O
//! laço aplica o pedido (reconstruir a pista, mexer no renderer, abrir socket) e
//! o menu não sabe de nada disso. Assim o cursor que dá a volta, a escada de
//! resolução que satura nas pontas e o ciclo de ação da LAN podem ser testados
//! sem aparelho — e é onde erros de "uma linha a mais" aparecem.

/// A ação de LAN escolhida na linha do menu.
///
/// Mora aqui e não em `rede` porque é uma escolha de **tela**: `rede` não
/// precisa saber que existe um menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AcaoLan {
    /// Nenhuma ação escolhida.
    #[default]
    Nada,
    /// Abrir um lobby nesta máquina.
    Hospedar,
    /// Procurar por mDNS e entrar no primeiro lobby que aparecer.
    Entrar,
}

impl AcaoLan {
    /// Próxima ação do ciclo: `Nada → Hospedar → Entrar → Nada`.
    pub fn proxima(self) -> Self {
        match self {
            AcaoLan::Nada => AcaoLan::Hospedar,
            AcaoLan::Hospedar => AcaoLan::Entrar,
            AcaoLan::Entrar => AcaoLan::Nada,
        }
    }

    /// Rótulo na tela.
    pub fn rotulo(self) -> &'static str {
        match self {
            AcaoLan::Nada => "desligada",
            AcaoLan::Hospedar => "hospedar",
            AcaoLan::Entrar => "entrar por mdns",
        }
    }
}

/// As linhas do menu, na ordem em que aparecem na tela.
///
/// A ordem é deliberada: primeiro o que muda a corrida, depois o que muda o
/// aparelho, depois a rede, e `JOGAR` por último — o olho desce e o dedo
/// confirma embaixo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Linha {
    /// Pista.
    Mapa,
    /// Modo de dificuldade.
    Dificuldade,
    /// Escala de renderização (o "botão do fill-rate").
    Qualidade,
    /// Overlay de desempenho.
    Perfil,
    /// Hospedar / entrar na LAN.
    Lan,
    /// Começar a corrida.
    Jogar,
}

impl Linha {
    /// Nome curto da linha, para o `--json`.
    pub fn nome(self) -> &'static str {
        match self {
            Linha::Mapa => "mapa",
            Linha::Dificuldade => "dificuldade",
            Linha::Qualidade => "qualidade",
            Linha::Perfil => "perfil",
            Linha::Lan => "lan",
            Linha::Jogar => "jogar",
        }
    }

    /// A linha do índice, dando a volta nas duas pontas.
    fn do_indice(indice: usize) -> Self {
        match indice % LINHAS {
            0 => Linha::Mapa,
            1 => Linha::Dificuldade,
            2 => Linha::Qualidade,
            3 => Linha::Perfil,
            4 => Linha::Lan,
            _ => Linha::Jogar,
        }
    }

    /// Rótulo fixo da linha (o valor vem depois, entre colchetes).
    fn rotulo(self) -> &'static str {
        match self {
            Linha::Mapa => "MAPA",
            Linha::Dificuldade => "DIFICULDADE",
            Linha::Qualidade => "QUALIDADE",
            Linha::Perfil => "PERFIL",
            Linha::Lan => "LAN",
            Linha::Jogar => "JOGAR",
        }
    }
}

/// Quantas linhas o menu tem.
pub const LINHAS: usize = 6;

/// O que o laço tem de fazer depois de uma tecla no menu.
///
/// Cada variante é uma decisão que o menu **não** pode tomar sozinho: ele não
/// tem a pista, o renderer nem o socket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pedido {
    /// Nada a fazer (a linha não tem valor, ou o valor não mudou).
    Nada,
    /// Andar `-1`/`+1` no mapa.
    Mapa(i32),
    /// Andar `-1`/`+1` na dificuldade.
    Dificuldade(i32),
    /// Aplicar esta escala de renderização.
    Escala(Option<f32>),
    /// Ligar/desligar o overlay.
    Perfil(bool),
    /// Abrir um lobby nesta máquina.
    Hospedar,
    /// Procurar um lobby por mDNS e entrar nele.
    Entrar,
    /// Sair da LAN e voltar ao modo de um jogador só.
    DesligarLan,
    /// Começar a corrida.
    Jogar,
}

/// O ciclo do `F3`, sem GPU: `1,00 → 0,75 → 0,50 → 1,00`.
///
/// O primeiro toque cai em **0,75** de propósito: é o valor que o R35S mediu
/// como suficiente para tirar o quadro do limite de fill-rate sem perder
/// legibilidade do HUD. Um ciclo que começasse em 0,5 obrigaria a procurar o
/// meio.
pub fn proxima_escala(atual: Option<f32>) -> Option<f32> {
    match atual {
        None => Some(0.75),
        Some(escala) if escala > 0.7 => Some(0.5),
        Some(_) => None,
    }
}

/// `-`/`=` no menu: anda um degrau na escada `[0,50 · 0,75 · 1,00]`.
///
/// Satura nas pontas: passar de 0,50 para baixo deixaria o HUD ilegível num
/// painel de 3,5", e passar de 1,00 não existe (o upscaler só reduz).
pub fn escala_adjacente(atual: Option<f32>, passo: i32) -> Option<f32> {
    const DEGRAUS: [f32; 3] = [0.5, 0.75, 1.0];
    let atual_f = atual.unwrap_or(1.0);
    let indice = DEGRAUS
        .iter()
        .position(|v| (v - atual_f).abs() < 0.01)
        .unwrap_or(2) as i32;
    let novo = (indice + passo).clamp(0, DEGRAUS.len() as i32 - 1) as usize;
    if DEGRAUS[novo] >= 0.999 {
        None
    } else {
        Some(DEGRAUS[novo])
    }
}

/// Texto da escala para a tela: `1.00`, `0.75`, `0.50`.
///
/// Duas casas e ponto, nunca vírgula: a fonte 5×7 tem `,` mas a mesma palavra
/// aparece no `--json` e no log, e duas grafias para o mesmo número foi como o
/// projeto já perdeu tempo antes (`--quadros`/`--frames`).
pub fn rotulo_escala(escala: Option<f32>) -> String {
    format!("{:.2}", escala.unwrap_or(1.0))
}

/// Estado do menu do título.
pub struct Menu {
    indice: usize,
    escala: Option<f32>,
    perfil: bool,
    lan: AcaoLan,
}

impl Menu {
    /// Menu com tudo no padrão (sem escala reduzida, overlay desligado, LAN
    /// desligada) e o cursor na primeira linha.
    pub fn nova() -> Self {
        Self {
            indice: 0,
            escala: None,
            perfil: false,
            lan: AcaoLan::Nada,
        }
    }

    /// O mesmo menu, já com a escala que veio da linha de comando.
    pub fn com_escala(escala: Option<f32>) -> Self {
        Self {
            escala,
            ..Self::nova()
        }
    }

    /// A linha em que o cursor está.
    pub fn linha(&self) -> Linha {
        Linha::do_indice(self.indice)
    }

    /// O cursor, para o `--json` e para os testes.
    pub fn indice(&self) -> usize {
        self.indice
    }

    /// Escala de renderização escolhida.
    pub fn escala(&self) -> Option<f32> {
        self.escala
    }

    /// `true` quando o overlay de desempenho está ligado.
    pub fn perfil(&self) -> bool {
        self.perfil
    }

    /// A ação de LAN escolhida na linha (antes de apertar A).
    pub fn lan(&self) -> AcaoLan {
        self.lan
    }

    /// Move o cursor, dando a volta.
    ///
    /// Dar a volta é o que faz a linha `JOGAR` ter um vizinho para cima: sem
    /// isso, estar em `JOGAR` e apertar para baixo não faria nada e o jogador
    /// concluiria que o direcional travou.
    pub fn navegar(&mut self, passo: i32) {
        if passo == 0 {
            return;
        }
        let total = LINHAS as i32;
        self.indice = ((self.indice as i32 + passo).rem_euclid(total)) as usize;
    }

    /// Aplica um passo horizontal na linha do cursor.
    pub fn ajustar(&mut self, passo: i32) -> Pedido {
        if passo == 0 {
            return Pedido::Nada;
        }
        match self.linha() {
            Linha::Mapa => Pedido::Mapa(passo.signum()),
            Linha::Dificuldade => Pedido::Dificuldade(passo.signum()),
            Linha::Qualidade => {
                let nova = escala_adjacente(self.escala, passo.signum());
                let mudou = nova != self.escala;
                self.escala = nova;
                if mudou {
                    Pedido::Escala(nova)
                } else {
                    Pedido::Nada
                }
            }
            Linha::Perfil => {
                self.perfil = !self.perfil;
                Pedido::Perfil(self.perfil)
            }
            Linha::Lan => {
                // Com a LAN já ativa, a linha mostra o estado e não é ajustável:
                // mudar de host no meio de uma corrida não é uma ação, é um
                // reinício disfarçado.
                self.lan = self.lan.proxima();
                Pedido::Nada
            }
            Linha::Jogar => Pedido::Nada,
        }
    }

    /// Apertou A.
    ///
    /// `lan_ativa` é o que separa "escolher uma ação" de "desligar": com a LAN
    /// no ar, a única coisa que o A pode fazer na linha é sair dela.
    pub fn aceitar(&mut self, lan_ativa: bool) -> Pedido {
        match self.linha() {
            Linha::Jogar => Pedido::Jogar,
            Linha::Lan => {
                if lan_ativa {
                    Pedido::DesligarLan
                } else {
                    match self.lan {
                        AcaoLan::Nada => Pedido::Nada,
                        AcaoLan::Hospedar => Pedido::Hospedar,
                        AcaoLan::Entrar => Pedido::Entrar,
                    }
                }
            }
            Linha::Perfil => {
                self.perfil = !self.perfil;
                Pedido::Perfil(self.perfil)
            }
            // Nas linhas de valor o A não faz nada: quem muda é o direcional, e
            // um A que também mudasse transformaria um toque em duas intenções.
            Linha::Mapa | Linha::Dificuldade | Linha::Qualidade => Pedido::Nada,
        }
    }

    /// Liga/desliga o overlay sem passar pelo cursor (tecla `F1` / botão `X`).
    pub fn alternar_perfil(&mut self) -> bool {
        self.perfil = !self.perfil;
        self.perfil
    }

    /// Sincroniza o overlay quando o liga/desliga veio de fora do menu.
    pub fn definir_perfil(&mut self, ligado: bool) {
        self.perfil = ligado;
    }

    /// Aplica a escala que veio da linha de comando, para o rótulo não mentir.
    pub fn definir_escala(&mut self, escala: Option<f32>) {
        self.escala = escala;
    }
}

/// O que o menu precisa saber do resto do jogo para escrever os rótulos.
pub struct Contexto<'a> {
    /// Nome da pista em vigor.
    pub mapa: &'a str,
    /// Nome da dificuldade em vigor.
    pub dificuldade: &'a str,
    /// A frase que explica o que a dificuldade faz.
    pub dificuldade_dica: &'a str,
    /// Resumo da LAN (`rede.resumo()`), vazio quando ela está desligada.
    pub rede: &'a str,
}

impl Menu {
    /// As linhas prontas para desenhar: texto e "é a linha do cursor".
    ///
    /// A formatação mora aqui, e não no desenho, porque é ela que diz se o
    /// jogador consegue **ler** o que vai escolher — e um teste consegue
    /// afirmar que o nome do mapa aparece.
    pub fn linhas(&self, ctx: &Contexto<'_>) -> Vec<(String, bool)> {
        let valores: [String; LINHAS] = [
            ctx.mapa.to_uppercase(),
            ctx.dificuldade.to_uppercase(),
            rotulo_escala(self.escala),
            if self.perfil {
                "LIGADO".into()
            } else {
                "DESLIGADO".into()
            },
            if ctx.rede.is_empty() {
                self.lan.rotulo().to_uppercase()
            } else {
                // Com a LAN no ar o valor não se escolhe: a linha vira o estado.
                ctx.rede.to_uppercase()
            },
            String::new(),
        ];
        (0..LINHAS)
            .map(|i| {
                let selecionada = i == self.indice;
                let marcador = if selecionada { "> " } else { "  " };
                let rotulo = Linha::do_indice(i).rotulo();
                // Rótulo alinhado à esquerda na largura do maior deles
                // ("DIFICULDADE"): sem isso os colchetes saem em escada e o
                // menu lê como uma lista de frases, não como uma tabela de
                // valores.
                let texto = match valores[i].as_str() {
                    "" => format!("{marcador}{rotulo}"),
                    valor => format!("{marcador}{rotulo:<11} [ {valor} ]"),
                };
                (texto, selecionada)
            })
            .collect()
    }

    /// A frase de ajuda da linha do cursor.
    ///
    /// Existe porque um menu de seis linhas com dois botões precisa dizer o que
    /// cada botão faz **naquela linha**: foi a falta disso que deixou a LAN e a
    /// escala invisíveis por três versões.
    pub fn dica(&self, ctx: &Contexto<'_>, lan_ativa: bool) -> String {
        match self.linha() {
            Linha::Mapa => "ESQ/DIR TROCA O MAPA".into(),
            Linha::Dificuldade => ctx.dificuldade_dica.to_uppercase(),
            Linha::Qualidade => "ESQ/DIR MUDA A RESOLUCAO INTERNA".into(),
            Linha::Perfil => "ESQ/DIR OU A LIGA O OVERLAY".into(),
            Linha::Lan => {
                if lan_ativa {
                    "A DESLIGA A LAN".into()
                } else {
                    "ESQ/DIR ESCOLHE   A EXECUTA".into()
                }
            }
            Linha::Jogar => "A OU START COMECAM A CORRIDA".into(),
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn contexto() -> Contexto<'static> {
        Contexto {
            mapa: "Avenida Central",
            dificuldade: "Normal",
            dificuldade_dica: "Ritmo de fábrica",
            rede: "",
        }
    }

    #[test]
    fn o_menu_comeca_na_primeira_linha() {
        let menu = Menu::nova();
        assert_eq!(menu.linha(), Linha::Mapa);
        assert_eq!(menu.indice(), 0);
        assert_eq!(menu.linhas(&contexto()).len(), LINHAS);
    }

    #[test]
    fn o_cursor_da_a_volta_nas_duas_pontas() {
        let mut menu = Menu::nova();
        menu.navegar(-1);
        assert_eq!(menu.linha(), Linha::Jogar, "para cima na primeira volta");
        menu.navegar(1);
        assert_eq!(menu.linha(), Linha::Mapa);
        for _ in 0..LINHAS {
            menu.navegar(1);
        }
        assert_eq!(menu.linha(), Linha::Mapa, "uma volta inteira volta ao começo");
    }

    #[test]
    fn a_escada_de_qualidade_satura_nas_pontas() {
        let mut menu = Menu::nova();
        menu.navegar(2);
        assert_eq!(menu.linha(), Linha::Qualidade);
        assert_eq!(menu.ajustar(-1), Pedido::Escala(Some(0.75)));
        assert_eq!(menu.ajustar(-1), Pedido::Escala(Some(0.5)));
        assert_eq!(menu.ajustar(-1), Pedido::Nada, "0,50 é o piso");
        assert_eq!(menu.escala(), Some(0.5));
        assert_eq!(menu.ajustar(1), Pedido::Escala(Some(0.75)));
        assert_eq!(menu.ajustar(1), Pedido::Escala(None));
        assert_eq!(menu.ajustar(1), Pedido::Nada, "1,00 é o teto");
        assert_eq!(menu.escala(), None);
    }

    #[test]
    fn a_linha_de_perfil_liga_e_desliga() {
        let mut menu = Menu::nova();
        menu.navegar(3);
        assert_eq!(menu.linha(), Linha::Perfil);
        assert_eq!(menu.ajustar(1), Pedido::Perfil(true));
        assert!(menu.perfil());
        assert_eq!(menu.aceitar(false), Pedido::Perfil(false));
        assert!(!menu.perfil());
    }

    #[test]
    fn a_linha_de_lan_cicla_a_acao_sem_executar() {
        let mut menu = Menu::nova();
        menu.navegar(4);
        assert_eq!(menu.linha(), Linha::Lan);
        assert_eq!(menu.lan(), AcaoLan::Nada);
        assert_eq!(menu.ajustar(1), Pedido::Nada, "mudar de ação não abre socket");
        assert_eq!(menu.lan(), AcaoLan::Hospedar);
        assert_eq!(menu.ajustar(1), Pedido::Nada);
        assert_eq!(menu.lan(), AcaoLan::Entrar);
        assert_eq!(menu.ajustar(1), Pedido::Nada);
        assert_eq!(menu.lan(), AcaoLan::Nada, "o ciclo fecha");
    }

    #[test]
    fn o_a_executa_a_acao_de_lan_escolhida() {
        let mut menu = Menu::nova();
        menu.navegar(4);
        assert_eq!(menu.aceitar(false), Pedido::Nada, "com NADA escolhido não faz isso");
        menu.ajustar(1);
        assert_eq!(menu.aceitar(false), Pedido::Hospedar);
        menu.ajustar(1);
        assert_eq!(menu.aceitar(false), Pedido::Entrar);
    }

    #[test]
    fn com_a_lan_no_ar_o_a_desliga() {
        // A única ação da linha passa a ser sair: escolher outra ação no meio de
        // uma corrida seria um reinício disfarçado.
        let mut menu = Menu::nova();
        menu.navegar(4);
        menu.ajustar(1);
        assert_eq!(menu.aceitar(true), Pedido::DesligarLan);
    }

    #[test]
    fn o_a_na_linha_jogar_pede_a_corrida() {
        let mut menu = Menu::nova();
        menu.navegar(-1);
        assert_eq!(menu.linha(), Linha::Jogar);
        assert_eq!(menu.aceitar(false), Pedido::Jogar);
    }

    #[test]
    fn as_linhas_de_valor_mostram_o_valor_e_o_cursor() {
        let menu = Menu::com_escala(Some(0.75));
        let linhas = menu.linhas(&contexto());
        assert!(
            linhas[0].0.contains("AVENIDA CENTRAL"),
            "o nome do mapa tem de estar na tela: {:?}",
            linhas[0].0
        );
        assert!(linhas[0].1, "o cursor começa no mapa");
        assert!(linhas[2].0.contains("0.75"), "escala: {:?}", linhas[2].0);
        assert!(linhas[3].0.contains("DESLIGADO"), "{:?}", linhas[3].0);
        assert_eq!(linhas[5].0.trim(), "JOGAR", "JOGAR não tem valor");
        assert!(linhas[5].0.starts_with(' '), "JOGAR começa sem cursor");
        for (i, (texto, selecionada)) in linhas.iter().enumerate() {
            assert_eq!(*selecionada, i == 0, "só uma linha tem o cursor: {texto}");
        }
    }

    #[test]
    fn o_cursor_aparece_em_uma_linha_so_em_qualquer_posicao() {
        let mut menu = Menu::nova();
        for passo in 0..LINHAS * 2 {
            let linhas = menu.linhas(&contexto());
            let marcadas: Vec<usize> = linhas
                .iter()
                .enumerate()
                .filter(|(_, (_, sel))| *sel)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(marcadas, vec![menu.indice()], "passo {passo}");
            menu.navegar(1);
        }
    }

    #[test]
    fn a_linha_de_lan_mostra_o_estado_quando_ativa() {
        let ativa = Contexto {
            rede: "LAN HOST 10.0.0.2:47800  CLIENTES 1  SNAP 15HZ",
            ..contexto()
        };
        let menu = Menu::nova();
        let linhas = menu.linhas(&ativa);
        assert!(
            linhas[4].0.contains("10.0.0.2"),
            "com a LAN no ar a linha tem de dizer o endereço: {:?}",
            linhas[4].0
        );
    }

    #[test]
    fn a_dica_muda_com_a_linha() {
        let ctx = contexto();
        let mut menu = Menu::nova();
        let mut vistas = std::collections::HashSet::new();
        for _ in 0..LINHAS {
            let dica = menu.dica(&ctx, false);
            assert!(!dica.is_empty(), "linha {:?} sem dica", menu.linha());
            // A cena normaliza antes de desenhar (`fonte::para_ascii`); conferir
            // a forma normalizada é conferir o que o jogador lê.
            let desenhada = giochi_neon_drift_core::fonte::para_ascii(&dica);
            assert!(
                desenhada.chars().all(|c| giochi_neon_drift_core::fonte::glifo(c).is_some()),
                "a dica {desenhada:?} usa caractere que a fonte não tem"
            );
            vistas.insert(dica);
            menu.navegar(1);
        }
        assert_eq!(vistas.len(), LINHAS, "duas linhas com a mesma dica");
        // A dica da LAN muda quando ela está ativa.
        menu.navegar(4);
        assert_ne!(menu.dica(&ctx, false), menu.dica(&ctx, true));
    }

    #[test]
    fn o_f1_e_o_x_sincronizam_o_mesmo_estado() {
        // O `X` do controle e o `F1` do teclado ligam a mesma coisa, e o menu
        // precisa refletir isso — senão a linha diz DESLIGADO com o overlay na
        // tela.
        let mut menu = Menu::nova();
        assert!(menu.alternar_perfil());
        assert!(menu.perfil());
        menu.definir_perfil(false);
        assert!(!menu.perfil());
    }

    #[test]
    fn o_rotulo_da_escala_usa_ponto_e_duas_casas() {
        assert_eq!(rotulo_escala(None), "1.00");
        assert_eq!(rotulo_escala(Some(0.75)), "0.75");
        assert_eq!(rotulo_escala(Some(0.5)), "0.50");
    }

    #[test]
    fn o_f3_leva_primeiro_a_setenta_e_cinco_por_cento() {
        // O requisito é `set_render_scale(Some(0.75))` no primeiro toque.
        assert_eq!(proxima_escala(None), Some(0.75));
        assert_eq!(proxima_escala(Some(0.75)), Some(0.5));
        assert_eq!(proxima_escala(Some(0.5)), None);
    }

    #[test]
    fn a_escada_do_menu_satura_nas_pontas() {
        assert_eq!(escala_adjacente(None, -1), Some(0.75));
        assert_eq!(escala_adjacente(Some(0.75), -1), Some(0.5));
        assert_eq!(escala_adjacente(Some(0.5), -1), Some(0.5), "não desce de 0,50");
        assert_eq!(escala_adjacente(Some(0.75), 1), None);
        assert_eq!(escala_adjacente(None, 1), None);
    }

    #[test]
    fn o_passo_zero_nao_muda_nada() {
        let mut menu = Menu::nova();
        menu.navegar(0);
        assert_eq!(menu.indice(), 0);
        assert_eq!(menu.ajustar(0), Pedido::Nada);
    }
}
