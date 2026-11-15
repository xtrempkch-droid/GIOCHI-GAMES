# Neon Drift

Corrida arcade 3D noturna, em Rust, sobre a **GIOCHI-ENGINE-3D** e o KMSDRM do
aparelho. Um jogador, três adversários, **cinco pistas** e um teto rígido de
**75 fps**.

O teto é de **simulação**, não de desenho: a física roda em passo fixo de 75 Hz e
o laço só desenha o que couber. Quanto cada aparelho entrega está na tabela de
[desempenho](#desempenho-medido-nos-três-aparelhos) — e o que sobra até 75 é do
painel, não do jogo: no R35S, cujo display é de 60 Hz, **uma malha só** já marca
59,7 fps, então nenhum jogo passa disso ali. No X55 a medição deu o mesmo número
(59,7 fps com `--so asfalto`), o que fecha a conta: o aparelho entrega no máximo
um quadro por vblank e o flip **não** se sobrepõe ao desenho.

```bash
cargo test --workspace                        # 211 (ui-sdl) + 146 (core, 2 ignorados)
cargo build --release
cargo clippy --workspace --all-targets        # zero erro; o jogo e o núcleo são forbid(unsafe_code)
SDL_VIDEODRIVER=offscreen ./target/release/giochi-neon-drift --quadro /tmp/q.bmp
SDL_VIDEODRIVER=offscreen ./target/release/giochi-neon-drift --frames 600 --json
./target/release/giochi-neon-drift --mapa boavista --tela corrida --passos 800 --piloto
```

O `--frames N` é o mesmo que `--quadros N`: existe para o smoke test headless
ler igual em qualquer jogo do repositório. Com `SDL_VIDEODRIVER=offscreen` e
`SDL_AUDIODRIVER=dummy` ele roda sem tela e sem placa de som — é o comando que o
CI usa.

```bash
# LAN: um host e um cliente, no mesmo PC (duas janelas ou dois offscreen)
giochi-neon-drift --lan-host --lan-porta 47800
giochi-neon-drift --lan-entrar 192.168.1.6:47800
giochi-neon-drift --lan-procurar            # descobre o lobby por mDNS

# aarch64 (Mali-G31/Panfrost, KMSDRM). Usa o SDL2 do sistema, não o bundled:
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR="$HOME/.cache/giochi-arm64/pkgconfig"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$HOME/.cache/giochi-arm64/cc-aarch64.sh"
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export RUSTFLAGS="-L $HOME/.cache/giochi-arm64/devlibs -L $HOME/.cache/giochi-arm64/sysroot/usr/aarch64-linux-gnu/lib"
cargo build --release --target aarch64-unknown-linux-gnu
file target/aarch64-unknown-linux-gnu/release/giochi-neon-drift   # ELF aarch64, stripped
```

O `CC_aarch64_unknown_linux_gnu` não é decoração: sem ele o `cc` do host é usado
para montar o assembly, e o link morre reclamando de arquitetura — mensagem que
não fala de `--target`.

## Rodar no aparelho

Cinco passos, e cada um responde uma pergunta diferente:

```bash
bash scripts/compilar-jogo-arm64.sh --jogo neon-drift        # 1. compila aarch64
node scripts/empacotar.mjs app/jogos/neon-drift              # 2. assina o .giochi
bash scripts/aparelho.sh instalar-pacote dist/pacotes/neon-drift.giochi  # 3. instala
bash scripts/aparelho.sh catalogo                            # 4. publica o cartão
bash scripts/aparelho.sh verificar                           # 5. prova que o painel desenha
```

1. **Compila** e deixa em `bin/giochi-neon-drift` (1 296 560 bytes, `NEEDED` =
   SDL2 + `SDL2_image` + `libgcc_s` + `libm` + `libc`). O script pergunta ao
   aparelho se ele tem `SDL2_ttf`, mas este jogo **não usa** — o texto é a fonte
   5×7 em dados. O tamanho quase dobrou na modernização: `laminar` + `mdns-sd`
   (descoberta de lobby) e as cópias embutidas dos dois `.obj`. Se o pacote
   apertar, a ordem de corte é: `mdns` (o cliente ainda entra por
   `--lan-entrar`), depois os modelos embutidos.
2. **Assina** (Ed25519) e recusa pacote sem `.sig` ou com `.sig` de tamanho
   errado. O empacotamento é reprodutível: dois do mesmo conteúdo dão o mesmo
   SHA-256.
3. **Instala** em `/user/apps/neon-drift` — e não em `/opt`, porque
   `distribution: store` significa "não faz parte da imagem assinada". É o que
   permite vender app GPL sem entregar chave. O `aparelho.sh` verifica a
   assinatura com a **chave do próprio aparelho**, não com uma do PC: verificar
   contra a chave errada produz um veredito, e veredito errado é pior que nenhum.
4. **Publica o cartão** (`catalogo`), senão o jogo fica **invisível sem erro**: o
   launcher lê `/opt/giochi/apps/index.json`, e um app instalado fora dele não
   vira cartão.
5. **Verifica** o *launcher*: ele precisa estar rodando e com o DRM aberto, e o
   `verificar` ainda captura um quadro dele e compara com a referência desenhada
   no PC. "O processo subiu" e "desenhou" são afirmações diferentes, e só a
   segunda interessa. Isso prova o painel, **não** o jogo — para o jogo, veja a
   captura logo abaixo.

#### Provar que o **jogo** desenha no aparelho

O KMSDRM é exclusivo: com o launcher rodando, um segundo processo recebe
`kmsdrm not available` e não gera BMP nenhum. A sequência é parar, capturar e
subir de novo — e a flag `/run/giochi/dev-free-console` é o que impede o
supervisor de ressuscitar o launcher no meio da captura:

```bash
ssh root@$APARELHO 'touch /run/giochi/dev-free-console
  kill $(pgrep -x giochi-launcher); sleep 2
  /user/apps/neon-drift/bin/giochi-neon-drift --quadro /tmp/jogo.bmp --passos 1200 --piloto
  echo "exit=$?"'
scp root@$APARELHO:/tmp/jogo.bmp .
bash scripts/aparelho.sh subir        # devolve o aparelho ao normal
```

Duas armadilhas, as duas custaram uma rodada perdida no X55:

- **não passe `--tamanho`** nessa captura. O KMSDRM ignora o pedido e usa o modo
  do painel (720×1280), mas a rotação que o jogo deduz sai do tamanho **pedido** —
  e o BMP sai inconsistente (a primeira captura do X55 deu 33 % de tela preta e
  foi descartada por isso; a segunda, sem `--tamanho`, deu 5,6 %).
- Esqueça a flag ligada e o supervisor fica sem subir o launcher: `aparelho.sh
  subir` é o passo que fecha o ciclo, não um extra opcional.

#### A instalação no X55, do jeito que foi feita

Feita em `10.55.0.2` (`giochi-system-x55`, SSH por `usb0`, chave já autorizada):

```bash
md5sum bin/giochi-neon-drift                     # 61c2d1e8db78314a814c186e01c347d7
node scripts/empacotar.mjs app/jogos/neon-drift  # 1 306 624 bytes
bash scripts/aparelho.sh instalar-pacote dist/pacotes/neon-drift.giochi
bash scripts/aparelho.sh catalogo
```

A assinatura saiu com a chave da **imagem** (`~/.cache/giochi-system/chaves-loja/
loja-privada.pem`) — sem o aviso "chave NÃO é a da imagem", que é o que faria o
aparelho recusar o pacote. O binário instalado em `/user/apps/neon-drift/bin/`
tem o mesmo md5 do `bin/` do repositório: o que roda no aparelho é o que foi
medido aqui.

Nos dois `.obj`, o aparelho **não precisa** dos arquivos: sem `assets/modelos` ao
lado do executável, o `modelos.rs` grava a cópia embutida em
`/tmp/giochi-neon-drift-modelos/` e carrega pelo mesmo `render.load_obj_mesh`.
Conferido no `--json` do X55: os dois carregaram do `include_str!`.

O que o aparelho reportou nas execuções instaladas: GLES 3 (`instancing: true`),
**20 batches** do `StaticBatch`, 88 postes / 6 cones / 4 portais desenhados por
`draw_instanced`, 9 texturas procedurais mais as **2 máscaras de luz**, de 56 a
60 desenhos e de 168 mil a 252 mil vértices conforme o mapa — e sempre
`recursos: {malhas:0, texturas:0, shaders:0}`. O `destruir()` não deixou nada
para trás nem no aparelho — as máscaras entram na mesma conta, e um
`destroy_texture` esquecido nelas apareceria aqui como `texturas: 2`. O fps
daí está na [tabela de desempenho](#desempenho-medido-nos-três-aparelhos).

### O que o aparelho precisa ter

| Item | Obrigatório? | Por quê |
| --- | --- | --- |
| `libSDL2-2.0.so.0` | **sim** | é a única biblioteca que o binário pede, além de `libgcc_s`, `libm` e `libc` |
| GLES 2.0 | **sim**, com fallback | tenta GLES 3 e cai para 2.0; sem GPU, o backend `software` do SDL2 também desenha |
| `/dev/dri` com KMSDRM | **sim** no aparelho | é por ali que o quadro chega ao painel, sem X11 e sem Wayland |
| `libasound.so.2` | **não** | sem ela o SDL2 não abre o dispositivo e o jogo **avisa** (`som: não abri o dispositivo … o jogo segue mudo`) em vez de morrer |
| `SDL2_ttf` e fonte TTF | **não** | o texto é a fonte 5×7 em dados (`crates/core/src/fonte.rs`) |
| `SDL2_image` e `libpng` | **não** | o leitor de BMP é escrito à mão e nenhuma textura vem de arquivo |

Seis linhas, e cinco delas são "não" — é o que faz o mesmo binário abrir no X55,
no X35H e no R35S sem uma linha de código por aparelho.

### Desempenho medido nos três aparelhos

| Aparelho | Painel | fps | O que limita |
| --- | --- | --- | --- |
| X35H (RK3566, Mali-G52) | 640×480 | **36,6 a 37,1 nos cinco mapas** | o conjunto: o `cobrinha` mede 49,3 no mesmo instante e `--so asfalto` 49,2 |
| X55 (RK3566, painel 720×1280) | 720×1280 | **36,6 a 37,8** depois da modernização (39,5 antes) | `1,04 ms + 0,154 ms por desenho + 16,7 ms de flip`, em série — ver abaixo |
| R35S (RK3326, Mali-G31) | 640×480 | **não medido** | — (a rodada de 09/11 só mediu quadro e HUD) |

Medido no X55 já instalado, `--frames 600`, `--sem-vsync`, launcher parado:

| Mapa | fps | ms de CPU | desenhos | vértices |
| --- | --- | --- | --- | --- |
| avenida | 36,9 | 10,30 | 57 | 167 796 |
| circuito | 37,8 | 9,79 | 60 | 251 568 |
| salar | 36,6 | 10,61 | 60 | 203 524 |

A conta fecha com o `ms_cpu` das três linhas, e é ela que explica o teto:
`1/(0,0103 s + 0,0167 s) = 37,0` contra 36,9 medidos; `1/(0,0098 + 0,0167) = 37,8`
contra 37,8; `1/(0,0106 + 0,0167) = 36,6` contra 36,6. Ou seja, o quadro é **CPU
seguida do flip**, em série: o `eglSwapBuffers` do KMSDRM bloqueia até o
page-flip do quadro anterior terminar no vblank, e esse tempo não é usado para
adiantar o próximo quadro. Cortar vértices (avenida 168 mil → circuito 252 mil,
um aumento de 50 %) não muda o fps, o que confirma que a GPU não é o limite.

O `--so`, que desenha um recorte, localiza o custo dentro do quadro:

| Recorte | fps | ms de CPU | desenhos | vértices |
| --- | --- | --- | --- | --- |
| `--so asfalto` (uma malha, quatro vértices) | **59,6** | 1,19 | 1 | 4 |
| `--so mundo` (só a geometria estática) | 41,3 | 5,97 | 32 | 158 968 |
| cena completa | 36,9 | 10,30 | 57 | 167 796 |

Duas conclusões, e as duas mudam o que fazer:

1. **O teto do aparelho é ~60 fps e não sobra folga nele.** Com uma malha só o
   X55 marca 59,6 fps — ou seja, 16,8 ms por quadro, exatamente um vblank, mesmo
   com `--sem-vsync` (o backend KMSDRM do SDL2 espera o page-flip de qualquer
   jeito). A partir daí o custo da cena não é absorvido pelo tempo do vblank: ele
   é **somado**, porque o flip só é pedido depois que o desenho termina e o
   processo então espera o próximo vblank inteiro. 60 fps "estáveis" com cena,
   nesse backend, exigiria apresentação assíncrona (dois swap chains ou page-flip
   `NONBLOCK`), que é mudança de engine, não de jogo.
2. **O custo escala com o número de desenhos, não com o de vértices.** Ajustando
   as duas primeiras linhas, `ms_cpu ≈ 1,04 + 0,154 ms × desenho`: com 32 desenhos
   prevê 5,97 (medido 5,97), e com 57 prevê 9,81. A cena completa mede **10,30** —
   os 0,5 ms de diferença são a amostragem das duas máscaras de luz, que os
   recortes `--so` não desenham (eles cortam os carros inteiros). São 159 mil
   vértices entre a primeira e a segunda linha e o custo acompanha os 31
   desenhos, não os vértices: cada `draw` custa ~154 µs no Cortex-A55 com
   Panfrost, e é por isso que o `StaticBatch` e o `draw_instanced` valem o que
   valem. O número não é particularidade de uma unidade: os 51 a 53 desenhos
   medidos no X35H na tabela abaixo dão ~8,9 ms, contra 9,45 a 9,96 medidos, no
   mesmo SoC com uma GPU diferente.

Medido no X35H em 10/11/2026, `--quadros 600`, sem o launcher segurando o DRM:

| Mapa | fps | ms de CPU | desenhos | partículas |
| --- | --- | --- | --- | --- |
| avenida | 37,0 | 9,70 | 51 | 663 |
| boavista | 36,7 | 9,86 | 52 | — |
| circuito | **37,1** | 9,45 | 53 | 682 |
| porto | 36,6 | 9,96 | 51 | — |
| salar | 35,6 | 9,77 | 53 | 671 |

**Os três mapas novos custam o mesmo que o antigo.** É o que a regra "cenário
novo não cria malha nova" compra: o circuito tem barreiras, torres de luz e
arquibancada a mais, e isso deu **2 desenhos**, não 20.

> A coluna `desenhos` desta tabela saiu do contador **antigo**, que perdia a
> chamada dos riscos de velocidade ([abaixo](#o-contador-de-desenhos-do-jogo-estava-errado-por-um)).
> Pelo contador da engine os mesmos quadros dão **um a mais** — e é esse que o
> `--json` reporta hoje.

Os quadros do aparelho, medidos nos BMPs: boavista 1882 cores/39,8 de média/0,45 %
quase preto · circuito 915/30,3/0,04 % · porto 1158/35,0/0,44 % · salar
749/35,4/0,04 %. Nenhum tem chapa chapada.

O número do X35H não é "o jogo é pesado": com **uma** malha só o jogo marca 49,2
e o `cobrinha` — outro jogo — marca 49,3 no mesmo minuto. O piso de apresentação
daquele painel naquele momento era ~49 fps, e o neon-drift fica ~5 ms acima dele.
`--sem-vsync` (37,4 contra 37,9) não muda nada, o que tira o vsync da conta.

**Correção de uma medição anterior:** a rodada de 09/11 registrou
"`GIOCHI_GLES=0` → 36,9, o mesmo número, logo não é GPU". A conclusão não vale,
porque **este jogo não lê `GIOCHI_GLES`** — essa variável é do launcher. As duas
execuções rodaram com a mesma GPU e a diferença de 1 fps era ruído. O que existe
aqui é `GIOCHI_GL_VERSION=2`, que pede GLES 2.0 em vez de 3.0 (ainda GPU, e o
jogo **não** tem caminho por software: ele desenha por GL direto, não pelo
renderer do SDL). Alguém que "desligue a GPU" com `GIOCHI_GLES=0` vai medir duas
vezes a mesma coisa.

## Cinco mapas

| `--mapa` | Pista | Desnível | Dificuldade | Cenário |
| --- | --- | --- | --- | --- |
| `avenida` (padrão) | 962 m, tudo plano | 0 m | 3 | cidade noturna: arcos de neon, passarelas, outdoors |
| `boavista` | 1 309 m, 2 voltas | **74 m** | 4 | serra: paredão de pedra, mata, guarda-corpo, cidade lá embaixo |
| `circuito` | **2 081 m** (o dobro da avenida) | 0 m | 5 | autódromo: barreiras, torres de luz, arquibancada, **3 rampas** |
| `porto` | **1 193 m**, **3 voltas** | 0 m | 3 | cais: contêineres, guindastes, mar com reflexos |
| `salar` | **2 188 m** | 0 m | 2 | planície de sal: pista larga, montanhas, **1 rampa** na reta de 500 m |

O mapa é **dado**, não código (`crates/core/src/mapa.rs`): pontos de controle,
largura mínima e máxima da pista, trechos de túnel, perfil de altura, **rampas**,
voltas, tempo limite, dificuldade e qual cenário construir. Acrescentar um sexto
é acrescentar uma constante e uma linha em `TODOS`.

A **dificuldade declarada não é enfeite**: o teste de promessa
(`cada_mapa_constroi_uma_pista_com_comprimento_utilizavel`) cobra curva fechada
de verdade (`>= 1,2 rad em 45 m`) só de quem declara `>= 3`. O salar promete
"larga e reta" e é medido por isso — antes o piso era fixo e reprovava o mapa
por ele ser exatamente o que promete.

A **descida** usa o Alto da Boavista (Rio de Janeiro) como referência de
*forma*, não como cópia: serpenteia para baixo, tem uma curva fechada de raio
pequeno, um trecho de túnel e uma reta longa onde o nitro vale. Onde a avenida
é plana de propósito, aqui a altura da pista é **real** e entra na física — a
gravidade puxa o carro na descida e o segura na subida (ver
[Relevo](#relevo-a-gravidade-entrou-na-física)).

### A largada: o fecho do laço usava a corda, não a tangente

Defeito relatado no aparelho: *"no mapa porto a posição do carrinho está
invertida, somente nesse mapa e somente o carro do player"*. Não era desenho —
era o carro **nascendo virado para trás**.

A pista é uma Catmull-Rom fechada, e a amostra que fecha o laço recebia como
direção o vetor `primeiro − anterior`, a **corda** entre o último ponto e o
primeiro. A tangente da spline em `t = 0` é `0,5·(p₂ − p₀)` e é a mesma no fim
do último segmento; a corda, ao contrário, é quase perpendicular à pista perto
da linha — e no porto apontava para o lado oposto, porque o traçado voltava
pelo mesmo trecho da reta em sentido contrário.

Duas correções, as duas medidas:

1. **fecho usa a tangente** (`pista.rs`): `amostras[0].direcao` em vez da corda;
2. **o porto ganhou volta pela rua de trás** (`mapa.rs`), em vez de descer e
   subir pela mesma reta.

O número que prova é o cosseno entre a direção 3 m antes e 3 m depois da linha:

| mapa | antes | depois |
| --- | --- | --- |
| avenida | 1,000 | 1,000 |
| **porto** | **−1,000** | **0,992** |
| salar | 0,997 | 0,997 |
| boavista | 0,251 | 0,251 |
| circuito | −0,765 | −0,765 |

`boavista` e `circuito` têm grampo na linha **por desenho** (a pista fecha em
"V" ali) e não apresentam o sintoma: o rumo inicial do carro vem de
`direcao_em(3,0)`, o trecho **depois** da linha, em `Corrida::com_mapa_e_config`
e em `reiniciar`. Duas tentativas de fechar esses dois "por dentro" criaram
quinas de 1,5–1,6 m e foram revertidas — o teste de raio mínimo
(`> 3,0 m`) reprovou as duas, que é o que ele existe para fazer.

Três testes fixam a regra: `a_direcao_e_continua_na_linha_de_largada_do_porto`
(cos > 0,9), `o_carro_nasce_encarando_o_trecho_depois_da_linha` (todos os mapas)
e o `#[ignore] imprime_direcao_na_linha_de_largada`, que é a ferramenta que
mede a tabela acima.

### A frase do título: desenha certo, e é a mesma nos dois lados

O relato incluía *"bug na frase «mapa porto noturno»"*. A frase é
`format!("MAPA  {}", mapa.nome.to_uppercase())` = `MAPA  PORTO NOTURNO`, e ela
**desenha inteira**: no X35H a linha ocupa x 230..409, y 160..170 — as mesmas
coordenadas do quadro gerado no PC —, a largura bate com
`fonte::largura_texto` (19 caracteres × 6 − 1 = 113 unidades, × 1,6 = 180,8 px)
e `todo_nome_de_mapa_tem_glifo` garante que cada letra existe na fonte 5×7.

O que existe é **build antiga no aparelho**: a instalação medida em
`/user/apps/neon-drift` era `2ecac141…`, de antes dos quatro modos de
dificuldade (era `9f0db73d…` uma rodada antes disso). Uma
tela que não muda e um binário velho são indistinguíveis de fora; o sha256 nas
duas pontas é o que separa os dois casos.

## Quatro dificuldades

O modo é **dado**, não código (`crates/core/src/dificuldade.rs`), e mexe em duas
coisas ao mesmo tempo — o ritmo dos adversários e, no mais difícil, o seu:

| `--dificuldade` | Adversários | Carro do jogador | O que se sente |
| --- | --- | --- | --- |
| `facil` | **×0,88** | ×1,00 | dá para errar a curva e ainda ganhar |
| `normal` (padrão) | ×1,00 | ×1,00 | a corrida balanceada de sempre |
| `dificil` | ×1,08 | ×1,00 | metade das freadas vira ultrapassagem perdida |
| `desafiante` | **×1,10** | **×0,95** | os outros andam mais e o seu carro anda **5 % menos** |

O `desafiante` é o pedido literal do dono do projeto: *"no modo desafiante o
carro do usuário será 5% mais lento que os outros carros"*. O fator do jogador
é `0,95` e entra no **teto** de velocidade (`Carro::teto`), não no motor — senão
o nitro emprestaria velocidade que o carro não sustenta e o limite virava
sugestão.

`fator_ia` é um campo **separado** de `habilidade`: a habilidade é clampada em
`0,72..=0,98` no construtor do adversário, então encostar a dificuldade nela
faria o difícil não ser mais difícil que o normal — o clamp comeria a diferença
em silêncio.

### Medido: a folga na linha de chegada

`--json` passou a expor `dificuldade`, `posicao` e `gap` (metros entre o melhor
adversário e o jogador; negativo = o jogador lidera). `posicao` sozinha não
serve: com o piloto ao volante ela é `1` nos quatro modos, e "1º" não distingue
ganhar por 500 m de ganhar por 100.

```bash
giochi-neon-drift --mapa avenida --dificuldade desafiante --tela corrida \
                  --piloto --passos 4000 --quadros 1 --json --sem-som
```

| `--mapa avenida` | fácil | normal | difícil | desafiante |
| --- | --- | --- | --- | --- |
| folga do jogador | **−511,8 m** | −327,1 m | −213,4 m | **−158,5 m** |
| tempo de prova | 41,5 s | 41,5 s | 41,5 s | **42,0 s** |

| `--mapa porto` (3 voltas) | fácil | normal | difícil | desafiante |
| --- | --- | --- | --- | --- |
| folga do jogador | −754,7 m | −397,9 m | −140,7 m | **−71,9 m** |

Duas leituras que saem daí:

1. **o tempo do jogador quase não muda** (41,5 → 42,0 s): dificuldade muda quem
   está à frente, não a física dele — exceto o desafiante, que muda o carro;
2. **a folga cai monotonicamente**, e o desafiante continua **ganhável** (71,9 m
   no porto ≈ 1,8 s a 40 m/s). Modo difícil que não se vence é modo quebrado.

Os números são **idênticos no PC e no X35H**, valor por valor, porque a física
roda em passo fixo e não lê relógio. É o que permite tratar um diagnóstico de PC
como diagnóstico de aparelho.

O teste que fixa isso é `a_dificuldade_aproxima_os_adversarios_de_verdade`
(`piloto.rs`): corre a prova nos quatro modos e exige que a folga caia, que a
queda do fácil ao desafiante passe de 100 m e que o desafiante siga acima do
piso de 1 s. Reverter o `fator_ia` reprova.

`o_carro_do_desafiante_anda_menos_que_o_do_normal` mede a outra metade na
**reta de teste a 300 m**: 14 s acelerando, razão de velocidade 0,95 ±0,02. A
primeira versão rodava na avenida e devolvia **103,2 %** — o desafiante, mais
lento, escapava da batida que o normal levava, e o teste media colisão com o
sinal trocado.

### Na tela

O título mostra o mapa **e** a dificuldade, com uma linha dizendo o que muda —
um modo que só se alcança por `--dificuldade` não existe para quem joga no
aparelho, e "desafiante" sem explicação faz o jogador sentir o carro lento e
chamar de bug:

```text
MAPA  PORTO NOTURNO
DESAFIANTE  ADVERSARIOS 10% MAIS RAPIDOS E VOCE 5% MAIS LENTO
ESQ/DIR  MAPA   CIMA/BAIXO  DIFICULDADE
```

Medido no X35H (640×480): a linha do mapa ocupa x 230..409 e a da dificuldade
x 75..564 — **as duas cabem** e o mesmo par de coordenadas sai no PC. Trocar é
na **vertical** no título (`←`/`→` continuam sendo o mapa).

## Texturas

Nove materiais, todos **gerados em código** (`crates/ui-sdl/src/texturas.rs`):
asfalto, concreto, rocha, mata, guarda-corpo, painel de neon, o xadrez da
largada e, desde a modernização, **faixa-pista** (tracejado e duas linhas) e
**grade-neon** (grade ortogonal das fitas de barreira). Nenhum arquivo de
imagem é obrigatório.

### Power-of-two, mipmap e `GL_REPEAT`

Todas são **64×64**, e isso não é escolha de resolução: o GLES 2.0 só aceita
`GL_REPEAT` e `glGenerateMipmap` em texturas cujo lado é potência de dois
(`texture.rs` da engine checa `is_power_of_two()` e cai para `CLAMP_TO_EDGE`
sem mipmap quando não é). Um lado de 100 px daria emenda visível e *aliasing*
a 30 m, sem erro de GL. O atlas de sprites é 128×128 pelo mesmo motivo — e o
único material que sai **sem** mipmap é o atlas, de propósito: nele cada célula
é vizinha de outra, e o filtro trilinear puxa o texel do vizinho.

As duas texturas novas são **decalques**: o fundo é o asfalto, e o que desenha
são as linhas. O teste de luminância trata as duas por uma regra própria (fração
de pixels claros > 5%) em vez da média > 90 dos materiais de superfície —
medir média nelas reprovaria uma textura correta.

Gerar em vez de carregar PNG não é preferência estética, é reduzir superfície de
falha. A primeira motivação era medida: na imagem de 07/11 do R35S o
`libSDL2_image` foi compilado sem `--enable-png` e `IMG_Load` respondia *"PNG
images are not supported"*. **A imagem de 09/11 corrigiu isso**, e a sincronização
com a engine 0.4.0 passou a usar o `Texture3D::from_file` — o binário ganhou
`libSDL2_image-2.0.so.0` no `NEEDED`. O que continua valendo é a **queda**: sem
arquivo externo, a textura sai da fórmula, e nenhuma das nove é obrigatória.

Os outros motivos, que também não mudaram: uma textura é uma fórmula (asfalto é
ruído com pedrisco, concreto são blocos com junta, mata é ruído em duas escalas
— vinte linhas cada), não há arquivo para empacotar nem caminho para faltar no
aparelho, e o `tint` continua valendo sem uma textura por cor.

As texturas nascem **neutras e claras**, e a cor vem do `tint` do material
(`amostra × tint`). É o que permite a mesma textura de concreto ser o muro
azulado da avenida e o paredão da serra. Textura escura × tint escuro = preto —
foi exatamente o que aconteceu na primeira tentativa, com 28 % do quadro quase
preto; a correção foi clarear as texturas, não o tint.

O sorteio é um xorshift com semente fixa: duas execuções geram bytes idênticos.
Sem isso a comparação de quadro PC × aparelho deixaria de valer.

E a **engine ganhou textura** para isto: o snapshot vendorizado em `vendor/`
não tinha criação de textura GL nem desenho texturizado, embora já tivesse
`uv` no vértice, shader `textured` e `set_texture`. Hoje tem
`create_texture_rgba`, `draw_mesh_textured`, `texture_size` e `destroy_texture`
(`vendor/giochi-engine-3d/src/render3d/texture.rs`). A engine original continua
intocada — atualizar é copiar de novo.

**Para pintar as suas:** [`TEXTURAS.md`](TEXTURAS.md) tem a ficha completa —
64×64 RGBA, cinza-claro neutro, 3 m por tile, faixa útil de luminância, a
tabela de qual malha usa qual material e as três armadilhas de BMP. O molde sai
do próprio jogo:

```bash
giochi-neon-drift --texturas /tmp/texturas.bmp   # as 7 lado a lado, 480x72
```

## Sprites: as partículas deixaram de ser quadrados

Oito formas num atlas de 128×128, também **geradas em código**
(`crates/ui-sdl/src/atlas.rs`): nuvem, gota, faísca, risco de nitro, risco de
chuva, halo, borrão de pneu e anel. Antes disto toda partícula era um
**quadrilátero sólido** — e num painel de 3,5" isso não lê como fumaça, lê como
quadrado.

| Forma | Onde aparece |
| --- | --- |
| `puff` | fumaça de pneu na derrapagem |
| `gota` | respingo de água no asfalto molhado |
| `faisca` | raspão na mureta e batida |
| `risco` | rastro de nitro no escapamento |
| `chuva` | gota caindo, alinhada à velocidade relativa |
| `halo` | mote de neon flutuando na cidade |
| `borrao` | marca de pneu deitada no chão |
| `anel` | (reservada) fragmento de vidro |

O atlas é **máscara de alfa**: RGB branco em toda parte, e quem desenha é o
canal `a`. A cor vem do `tint` e o desbotamento vem da faixa de vida, então uma
nuvem nasce branca, fica cinza e some sem precisar de uma textura por cor.

Cada célula vira uma **malha** cujo UV cobre só aquele retângulo
(`malha::quad_celula`), e o desenho usa `draw_instanced_textured`. A célula é
escolhida pela geometria, não por uniforme: é isso que permite as centenas de
cópias de um tipo saírem em **uma** chamada. O número de desenhos por quadro
não mudou — continua um por (tipo, faixa de vida).

O ganho de tamanho existe num lugar só: a `gota` é desenhada 1,30× maior, porque
o raio dela (0,72 do meio-quadrado) é menor que o quadrado cheio que ela
substituiu. Sem isso o respingo encolheria 28 % sem ninguém ter mexido no
tamanho dele.

### Usar a sua própria folha de sprites

Se você tem uma folha pronta (um `.bmp`), não precisa mexer em código:

```bash
GIOCHI_SPRITES=minha-folha.bmp GIOCHI_SPRITES_COLUNAS=8 giochi-neon-drift
```

- **`GIOCHI_SPRITES`** — caminho de um BMP **sem compressão, 24 ou 32 bits**.
- **`GIOCHI_SPRITES_COLUNAS`** — quantas colunas a folha tem (padrão 4). As
  linhas saem de `8 ÷ colunas`, então uma faixa 8×1 e uma grade 4×2 desenham
  igual.

A folha é **normalizada** para o mesmo 128×128 de quatro colunas: a célula *i*
vai para a posição da forma *i*. Nada do desenho sabe de onde ela veio, então
não há um segundo caminho de código que possa envelhecer diferente.

O RGB da sua folha é descartado (a cor vem do `tint`) e o que vale é o **alfa**.

A ficha de arte (grade 4×4, margem de 1e-4, por que o `gota` cresce 1,30×, BMP de
24 bits usando a luminância como máscara) está em [`TEXTURAS.md`](TEXTURAS.md). E
o arquivo para conferir é `--atlas /tmp/atlas.bmp`, que sai com fundo **magenta**:
alfa 0 sobre preto é indistinguível de "não desenhou".
Num BMP de 24 bits, que perdeu o alfa, a **luminância** vira máscara: branco
opaco, preto transparente — a convenção das folhas de efeito.

PNG **não** é aceito, e o motivo é do jogo, não da imagem do aparelho: o
`giochi-neon-drift` **não linka `SDL2_image`** (`NEEDED` = SDL2 + `libgcc_s` +
`libm` + `libc`), e o leitor de BMP é escrito à mão em `atlas.rs`. Aceitar PNG
seria uma dependência nova, num binário que hoje abre em qualquer imagem que
tenha SDL2. Converta para BMP — sai de qualquer editor.

Se o arquivo não abrir, não for BMP ou não couber no número de colunas, o jogo
**avisa** e cai no atlas procedural:

```text
neon-drift: folha de sprites ignorada — não consegui abrir /tmp/x.bmp: No such file or directory
neon-drift: atlas de sprites 128x128 [...] (origem: procedural)
```

O aviso existe porque a alternativa é pior: cair no procedural **em silêncio**
faz o usuário concluir que a folha dele não serve, quando o que falhou foi o
caminho.

```bash
giochi-neon-drift --atlas /tmp/atlas.bmp     # o atlas em BMP, fundo magenta
```

O `--atlas` respeita as duas variáveis: uma ferramenta que mostra o atlas
procedural enquanto o jogo desenha a folha do usuário mede outra coisa.

O fundo é magenta de propósito: alfa 0 sobre preto é indistinguível de "não
desenhou", e o arquivo mentiria sobre ter gerado as formas.

E a engine vendorizada ganhou o par que faltava: `BuiltinShader::TexturedInstanced`
(`shaders/textured_instanced.vert`) e
`RenderContext3D::draw_instanced_textured`. O fragmento é o mesmo do `Textured`,
então um sprite recebe exatamente a mesma luz que uma malha texturizada ao lado.

## Faísca no raspão, destroço na batida

Duas respostas diferentes para dois contatos diferentes:

| Situação | Sinal na física | Efeito |
| --- | --- | --- |
| encostou e continuou andando | `carro.raspando` | faíscas laranja contínuas, som de raspagem, **sem** zerar o combo |
| bateu de frente | `carro.impacto > 0,35` | rajada de **destroços 3D** (cubos girando em três eixos, com quique no chão) + tremor de câmera |

Separar os dois é o que faz o raspão ser divertido em vez de punitivo: antes
todo contato era `Evento::Bateu`, e raspar a mureta de leve zerava o combo como
uma batida a 200 km/h.

O tamanho da rajada é proporcional ao impacto — um raspão solta 2 ou 3 lascas,
uma batida forte solta 18. `--json` traz `"impacto"` e `"fragmentos"`; zero ali
é resposta legítima (não bateu), e é por isso que existe um teste que **prova**
que a rajada dispara — durante uma sessão inteira ela disparou zero vez e o
diagnóstico dizia `"fragmentos":0`, indistinguível de "não implementado".

## Relevo: a gravidade entrou na física

`Amostra.altura` carrega o perfil do mapa, e `Pista::inclinacao_em(s)` devolve a
rampa em radianos. O carro sente isso como aceleração ao longo da frente:

- descendo, acelera sozinho;
- subindo, perde velocidade;
- de ré, o sinal inverte (senão o carro aceleraria sem limite descendo de ré).

Uma armadilha medida: com a gravidade **real** (9,81 m/s²), uma rampa de 11 % dá
1,08 m/s² — menos que o atrito de rolagem constante, que era 4,5. O carro
**parava** no meio da descida. A correção foi dupla: gravidade arcade
(`9,81 × 3,5`, como todo jogo de corrida faz) e atrito de rolagem em 1,6.

### O nitro dava velocidade permanente

Achado ao medir o salar, o mapa mais aberto: a corrida fechava com média de
**78,2 m/s** contra um teto de **82**. Com tanque de 100, gasto de 30/s e recarga
de 8/s, o ciclo ativo do nitro é **21 %** — a média não podia passar de ~66.

A causa: o motor zera no topo (`sobra = 0`), mas isso só **impede acelerar mais**.
Nada trazia o carro de volta, e o arrasto com o acelerador preso é uma constante
de 0,55 m/s². Medido: **82 → 62 levava 36 s**, mais que uma volta inteira. O nitro
emprestava velocidade e ela nunca era devolvida.

A correção é uma parcela que só existe **acima** do topo:

```rust
let excesso = (vel - vel_topo).max(0.0) * FREIO_EXCESSO;   // 1,0 s⁻¹
let resistencias = arrasto + atrito_extra + excesso;
```

Ela não pode entrar no arrasto geral: lá também age **abaixo** do topo, e foi
exatamente esse erro que deixava o carro em 32,7 m/s numa versão anterior desta
conta (o comentário no código registra isso).

| Mapa | Média antes | depois |
| --- | --- | --- |
| salar | **78,2 m/s** | **67,1 m/s** |
| circuito | 63,6 m/s | **56,3 m/s** |
| boavista | 53,1 m/s | 50,6 m/s |
| avenida | 46,4 m/s | 46,4 m/s |
| porto | 32,9 m/s | 32,9 m/s |

`o_nitro_nao_deixa_velocidade_permanente` exige o carro de volta ao topo em 4 s;
revertendo a correção ele reprova com *"ainda estava a 79,8 m/s"*. E
`abaixo_do_topo_o_freio_de_excesso_nao_aparece` guarda o outro lado: a parcela é
zero em qualquer velocidade legal.

A descida assenta em **65–67 m/s** quando o nitro acaba: a gravidade sustenta
parte do excesso (`62 + 4,5 m/s² ÷ 1,0 s ≈ 66,5`), e é isso que faz a serra ter
caráter sem o carro passar a corrida acima do topo.

E as texturas de chão usam **UV pela posição no mundo**, não 0..1 por
quadrilátero: com UV por segmento, a textura reiniciaria a cada 4 m de serra e a
emenda apareceria como uma faixa correndo ao lado do carro.

## Rampas de salto: o carro sai do chão

Uma rampa **não** é um morrinho no relevo. Um morrinho é uma ladeira suave: a
física mantém o carro grudado no chão e ele nunca decola. Para o carro **sair do
chão** o piso tem de cair mais rápido do que a gravidade conseguiria trazê-lo de
volta — ou seja, um **corte seco** depois da subida, e isso não cabe num perfil
de `(fração, altura)` interpolado entre dois pontos.

Por isso a rampa é **dado declarado**, com `s` exato:

```rust
Rampa { fracao: 0.500, comprimento: 30.0, altura: 3.3 }
```

`RampaS::altura_em(s)` devolve a subida e **zero depois do topo** — é esse corte
que faz o carro voar. E o topo é **encaixado numa amostra** da pista
(`encaixar_em_amostra`): o asfalto é desenhado ligando a altura de amostras
vizinhas, e um topo no meio de um segmento faria o piso descer suave, de novo sem
decolagem. A decolagem é decidida por `pista.rampa_cruzada(s_anterior, s)`, não
por "o piso caiu".

No ar: não colide com mureta nem obstáculo, o drift não acumula e o nariz do
carro sobe e desce com `atan2(velocidade_vertical, velocidade)`. O voo entra na
física como fato, com `Decolou`/`Aterrissou` avisando a cena e o som.

Medido na rampa 1 do circuito a 62 m/s: decola com 11,6 m/s verticais, **pico de
6,8 m** e aterrissa **1,73 s** depois.

Duas constantes existem porque a física "correta" não é divertida:
`BONUS_SALTO = 1,7` (fisicamente `v·inclinação` daria 2,8 s de voo, que é longo
demais) e `GRAVIDADE_VOO = 15,0`, mais fraca que a `GRAVIDADE = 34,3` da ladeira.

### O `s` salta no ar, e isso quase creditou uma volta

Voando, o `localizar` pode trocar de referência ao atravessar o miolo de uma
curva: medido, o `s` saltou **96,76 m num único quadro** de 1/75 s. O progresso
da corrida passou a usar o **deslocamento real projetado na pista**
(`pos_anterior: Ponto`), que é imune a isso por construção — um teto arbitrário
seria o próximo número mágico a dar errado.

## O que o jogo tem

| Peça | Como está |
| --- | --- |
| Pista | **cinco mapas**: avenida (962 m, túnel de 157 m, curva de raio 20 m), descida (1 309 m, 74 m de desnível), **circuito (2 081 m com 3 rampas)**, porto (1 030 m, hairpin de raio 22 m) e salar (2 188 m, 1 rampa) |
| Salto | rampa é dado do mapa; o carro decola com **6,8 m de pico** e 1,7 s de voo, e no ar não colide nem acumula drift |
| Carro | física arcade: 62 m/s no asfalto, **82 m/s com nitro** (e o carro **volta** ao topo em ~3 s quando ele acaba), derrapagem que só existe com o freio de mão; rolagem e arrasto **freiam até zero e nunca empurram o carro para trás** |
| Drift | combo até ×5, pontos por segundo de derrapagem; bater zera o combo e mantém o que já foi somado |
| Adversários | três, com linha própria e freando na curva; ~9 s atrás de um piloto competente em duas voltas |
| Câmera | terceira pessoa com atraso exponencial, inclinação na derrapagem, tremor na batida e **FOV que abre no nitro** |
| HUD | volta, posição, tempo, velocímetro, marcha, barra de nitro, pontos e combo de drift |
| Efeitos | **7 tipos de partícula**, cada um com a sua forma no atlas de sprites (fumaça, respingo, faísca, nitro, chuva, poeira de neon, marca de pneu), raios de velocidade, clarão de nitro, **poça de neon com máscara de alfa** sob cada carro e **cone de farol** no asfalto — os dois acendem no drift — e **áudio sintetizado em runtime** (motor, derrapagem contínua e nitro) |
| Cenário | avenida: cidade noturna com **arcos de neon a cada 26 m**, três passarelas, outdoors luminosos, refletores na mureta, faixas de neon nas torres distantes e **pulos de luz em duas fases** no túnel |
| Telas | **menu do título** (mapa, dificuldade, qualidade, perfil, LAN e jogar, tudo no direcional) , corrida e resultado com "reiniciar" e "voltar ao menu" |

Nada aqui usa marca, carro ou logo real: os carros são caixas com proporções de
carro e o cenário é aritmética.

## O véu das telas de texto

O título e o resultado desenham texto claro **sobre a cidade**, e o céu da serra
chega a 39 de luminância média: um texto de 5×7 de 2 px por ponto sobre esse
fundo não se lê num painel de 3,5" sob luz do dia. Relatado pelo dono como
*"o texto da tela de entrada fica muito difícil de visualizar"*.

A correção é um quad preto translúcido de tela cheia entre o mundo e o texto —
o cenário continua girando atrás do menu (é o que dá vida à tela de entrada),
mas deixa de competir com as letras, e **na corrida o véu é zero**: o jogo
volta ao brilho normal.

| Fase | Alvo do véu |
| --- | --- |
| título | **0,62** |
| resultado | 0,50 |
| corrida | 0,00 |

O valor do título não é estética: 0,62 é o que leva o fundo local a 11 de
luminância com o texto em 184–189 — 12:1 de contraste, medido no PC e no painel
do X55.

### A transição: rampa com limite, não suavização exponencial

Suavização exponencial foi a primeira tentativa e falhou nas duas exigências ao
mesmo tempo: com τ = 0,12 s o véu pulava 26 % no primeiro quadro (pisca), e com
τ grande levava 0,7 s para sumir na largada (atrasa). O que ficou é uma rampa
com passo limitado:

```
passo = (alvo − atual).clamp(−dt/0,30, +dt/0,30)
```

São 0,044 por quadro a 75 Hz e a transição termina em 14 quadros: não pisca
porque o passo tem teto, não atrasa porque o teto é alto. `GIOCHI_VEU=0` desliga
o véu inteiro — é o A/B que mede o contraste.

### Medido: contraste 5,55:1 → 12,12:1

Os dois lados do A/B com o mesmo número de quadros — 1024×768 no PC e o painel
real do X55 (720×1280, girado). O texto é o mesmo pixel a pixel (ele é desenhado
**depois** do véu, então não escurece); o que muda é o fundo em volta:

| Medida | PC sem | PC com | X55 sem | X55 com |
| --- | --- | --- | --- | --- |
| luminância do texto (média) | 189,1 | 189,0 | 184,2 | **184,0** |
| fundo local (mediana) | 30 | 11 | 31 | **11** |
| contraste texto/fundo | 5,55:1 | **12,12:1** | 5,26:1 | **11,81:1** |

A cobertura é conferida em seis faixas horizontais de 128 px: todas escurecem
na mesma proporção (~0,40), inclusive as duas das pontas e os quatro cantos.
Esse teste é o que pegou o defeito abaixo.

### O que custa

Um quad de tela cheia com mistura alfa, e **só nas telas de texto** — na corrida
o desenho nem é emitido (`veu > 0,002` é falso):

| No X55 (painel 720×1280) | Sem véu | Com véu |
| --- | --- | --- |
| fps na tela de título | 36,2 | **29,8** |
| `ms_cpu` na tela de título | 10,20 | 12,08 |
| fps na corrida | 37 | **37** (inalterado) |
| chamadas de desenho na corrida | 57 | **57** (inalterado) |

São 6,4 fps a menos numa tela **estática**, onde fluidez não vale nada, para
trocar 5,26:1 de contraste por 11,81:1 — e a corrida, que é onde o quadro tem de
correr, não paga nada. Se algum dia o menu precisar de mais fôlego num aparelho
mais fraco, o caminho é baixar a escala de render **só na fase de título**, não
engrossar o véu.

### O defeito que a captura pegou: `hud_matriz` cobre um quarto da tela

A primeira versão desenhava o véu com `retangulo(0, 0, largura, altura)`, e um
retângulo de "tela cheia" por essa matriz cobria **512×384 de 1024×768** — o
quarto central, com as bordas intactas. A razão está no mapeamento: `hud_matriz`
acerta o **centro** de qualquer elemento no pixel pedido, mas o **tamanho** sai
pela metade, porque o NDC vai de −1 a +1 (duas unidades) e a escala dela entrega
uma. Todo o texto do HUD é calibrado nesse espaço — dobrá-lo faria as linhas se
sobreporem — então a matriz não se mexe; quem precisa dos cantos exatos da tela
usa `hud_matriz_tela`, que escala o quad unitário por `2·PROF·aspecto` na
largura e `2·PROF` na altura.

O mesmo erro existia no **flash de nitro**, desde sempre: ele também cobria só o
quarto central, e ninguém viu porque o alfa é 0,05. Corrigido junto.

E o teste que deveria ter pego isso mentia:

```rust
// errado: multiplica por 2 no lugar de dividir, e "prova" a cobertura
let largura_ndc = (m.data[0] / (PROFUNDIDADE_HUD * aspecto)) * 2.0;
```

Ele passou por anos afirmando 2,0 de NDC para um quad que entrega 1,0. O
substituto (`a_hud_matriz_dimensiona_pela_metade_do_pixel`) afirma o valor real,
e `a_matriz_de_tela_cheia_encosta_nos_quatro_cantos_do_ndc` projeta os cantos
pela `view_projection` da câmera do HUD de verdade e exige ±1 nos dois eixos —
em tela normal e com o painel girado 90° e 270°, onde a proporção vem do
**painel** e não do projeto.

## Controles: o controle é o caminho, o teclado é o atalho

O jogo é de handheld, então **tudo** se faz com o controle. O teclado continua
funcionando no PC, com as mesmas ações, mas não é o caminho principal — e até a
revisão de controles ele era o **único** para quatro coisas: LAN, escala de
render, overlay de desempenho e as opções do menu. Num aparelho sem teclado elas
eram inalcançáveis justamente onde mais importam.

### No aparelho (e em qualquer controle)

| Botão | Na corrida | No menu (título) |
| --- | --- | --- |
| A / gatilho direito | acelerar | **aceitar** a linha |
| B / gatilho esquerdo | frear / ré | — |
| analógico / direcional | virar, acelerar (↑) e frear (↓) | **↑↓** move o cursor |
| direcional ← / → | virar | **muda o valor** da linha |
| L1 / R1 | freio de mão (**é o que liga o drift**) | — |
| L2 / R2 | nitro | — |
| **START** | — | jogar / reiniciar sem passar pelo cursor |
| **X** | overlay de desempenho | overlay de desempenho |
| **Y** / SELECT | **volta ao menu** | sai do jogo |

`Y` na corrida volta ao menu, e não mata o jogo: no aparelho o polegar escorrega,
e o botão que fecha tudo tem de ser o do menu.

### O menu do título

```
> MAPA         [ AVENIDA CENTRAL ]
  DIFICULDADE  [ NORMAL ]
  QUALIDADE    [ 1.00 ]
  PERFIL       [ DESLIGADO ]
  LAN          [ DESLIGADA ]
  JOGAR
ESQ/DIR TROCA O MAPA
```

Seis linhas, um cursor e dois botões: ↑↓ escolhe a linha, ←→ muda o valor, A
aceita. A linha **LAN** cicla `DESLIGADA → HOSPEDAR → ENTRAR POR MDNS` e o A
executa — com a LAN no ar ela passa a mostrar o estado (`HOST 10.55.0.1:47800
(1)`), e o A desliga. A linha **QUALIDADE** é o `F3` com nome: `1.00 → 0.75 →
0.50`, e satura nas pontas.

A **dificuldade** explica a si mesma na linha de baixo ("adversários 12% mais
lentos"), porque "desafiante" sem explicação faz o jogador sentir o carro lento
e chamar de bug.

### Teclado (PC)

| Tecla | Ação |
| --- | --- |
| `W`/`↑` | acelerar |
| `S`/`↓` | frear (e ré, parado) |
| `A`/`D` ou `←`/`→` | virar (e navegar o menu) |
| `Espaço` | freio de mão — **é o que liga o drift** |
| `Shift` | nitro |
| `Enter` | jogar / reiniciar |
| `Esc` | voltar ao menu / sair |
| `F1` | overlay de desempenho (fps, ms, draw calls, rede) |
| `F3` | escala de render: 1,00 → **0,75** → 0,50 → 1,00 |
| `-` / `=` | muda o valor da linha do cursor no menu |
| `H` (menu) | hospedar corrida LAN + anunciar por mDNS |
| `J` (menu) | procurar lobby na LAN e entrar |

### Gamepad pelo SDL, com analógico e gatilhos

O evdev lê o controle cru do aparelho — é o caminho de produção no R35S, onde o
mapeamento do sistema já está certo. O `gamepad.rs` cobre o resto (PC, controle
USB genérico, SDL atualizado): abre o primeiro `GameController`, lê **estado** a
cada quadro (não evento — corrida precisa de "quanto", e um evento entre dois
passos do timestep fixo se perderia) e entrega:

| Entrada | Vai para |
| --- | --- |
| gatilho direito (`TriggerRight`) | `acelerar`, proporcional `0..1` |
| gatilho esquerdo (`TriggerLeft`) | `frear`, proporcional `0..1` |
| analógico esquerdo (`LeftX`) | `virar`, com zona morta de 0,16 **reescalada** |
| D-Pad | `CIMA`/`BAIXO`/`ESQUERDA`/`DIREITA` — os mesmos bits do evdev |
| A, B, X, Y | `ACELERAR`+`ACEITAR`, `FREAR`, `PERFIL`, `VOLTAR` |
| L1 / R1 / START / SELECT | freio de mão / nitro / `INICIAR` / `VOLTAR` |

**A mesma tecla faz a mesma coisa nos dois caminhos.** Até a revisão de
controles, o evdev tinha `A = acelerar` e o SDL `A = freio de mão`: o mesmo
botão, dois significados, conforme quem abrisse o controle. Hoje os dois falam
o vocabulário de **botão físico** (`evdev::bit_do_codigo`) e quem traduz botão
em intenção é `entrada_jogo::combinar_com_analogico`, onde as regras dão para
testar sem aparelho.

O direcional acende `CIMA`/`BAIXO`, e **não** `ACELERAR`/`FREAR`: se acendesse,
soltar o direcional apagaria o bit do A que o jogador continua segurando e o
carro pararia de acelerar sozinho na reta. A fusão lê `CIMA` como acelerar, e
há teste para isso.

Teclado, bits e analógico **somam com `max`** em cada canal, e o resultado é
limitado a 1: apertar acelerar na seta e no gatilho não dá o dobro de
aceleração. A fusão é função pura (`entrada_jogo::combinar_com_analogico`) e os
testes dela rodam sem controle plugado.

A zona morta **reescala** o resto do curso: sem isso o primeiro milímetro depois
dela salta de 0 para 0,16 e a correção fina de drift fica quadrada.

### Escala de render: o botão do fill-rate

Num painel de 720×1280 são 921 mil pixels por quadro, e um Mali gasta
proporcionalmente ao **pixel**, não ao triângulo. `F3` chama
`render.set_render_scale(Some(0.75))` — desenha em 540×960 e estica com
bilinear — e `0,50` para o caso extremo. O primeiro toque cai em **0,75** de
propósito: é o degrau que ainda deixa o HUD legível. O `--json` publica
`"escala"` e o overlay F1 mostra a mesma coisa, para o número da tela não mentir
sobre o que foi desenhado de verdade.

**Não espere ganho no X55.** Medido lá, com o jogo instalado: 1,00 → 36,9 fps,
0,75 → 36,9, 0,50 → 35,8. O motivo está na seção de desempenho acima: no X55 o
tempo de quadro é `1,04 ms + 0,154 ms por desenho + 16,7 ms de flip`, e nenhuma
dessas três parcelas depende de quantos pixels a cena ocupa — o `StaticBatch`
continua desenhando os mesmos 20 batches e a mesma contagem de desenhos. Num
aparelho em que o fragmento domine, o botão continua valendo; num aparelho
limitado pelo flip, o que valeria é apresentar um quadro enquanto o outro está no
scanner, e isso é o pipeline de apresentação da engine, não o jogo. O `F3` fica
porque é barato e honesto: se um dia o aparelho passar a ser limitado por pixel,
o botão já está lá — e enquanto não passar, o número medido diz que ele não ajuda.

## Revisão de controles: o teclado deixou de ser necessário

O jogo é de handheld, e por três versões quatro coisas **só** existiam no
teclado: LAN (`H`/`J`), escala de render (`F3`, `-`/`=`), overlay (`F1`) e as
opções do título (mapa e dificuldade nas setas). Num aparelho isso é o mesmo que
não existir — e é justamente onde a escala importa (fill-rate de Mali-G31) e
onde a LAN é o modo de jogar com outra pessoa.

A revisão fez quatro coisas:

1. **Menu do título** (`menu.rs`, puro e testável): seis linhas, cursor, dois
   botões. Mapa, dificuldade, qualidade, perfil, LAN e jogar.
2. **Botão para cada função**: `X` = overlay, `START` = jogar/reiniciar,
   `Y`/SELECT = voltar, `A` = aceitar.
3. **`Y` na corrida voltou a significar "menu"**, não "matar o processo". Antes,
   um dedo escorregado no botão ao lado do B fechava o jogo — e sem uma tecla
   `Esc` não havia como chegar ao menu para trocar a escala.
4. **O caminho do controle e o do evdev agora falam a mesma língua.**

### Os três defeitos que a revisão achou

- **`A` queria dizer duas coisas.** No evdev, `A` acendia `ACELERAR`; no SDL,
  `FREIO_MAO`. O mesmo botão, dois significados, conforme quem abrisse o
  controle — e nenhum teste pegava, porque cada caminho era testado sozinho.
  Hoje a máscara descreve o **botão físico** e quem traduz botão em intenção é a
  fusão, com teste.
- **O direcional acendia `ACELERAR`.** Parece inofensivo até o jogador segurar o
  `A`, tocar o direcional e soltar: o `fetch_and(!bit)` do evdev apagava
  `ACELERAR` e o carro parava de acelerar sozinho no meio da reta. O direcional
  acende `CIMA`/`BAIXO`, que a fusão também lê como acelerar/frear — e há teste
  para a sequência inteira (`segurar_o_direcional_nao_apaga_o_acelerar_do_a`).
- **Acento virava buraco.** A dica de dificuldade é "adversários 12% mais
  lentos"; a fonte 5×7 é ASCII maiúscula, e o `á` sem glifo avançava o cursor e
  desaparecia. O jogador lia **"RITMO DE FBRICA"**. A cena agora normaliza todo
  texto com `fonte::para_ascii` antes de desenhar, e a conversão é **1:1 em
  caracteres**, então a medida que o chamador fez com `largura_texto` continua
  valendo (texto descentralizado seria o próximo defeito).

### Como se prova que o controle faz tudo, sem controle

O laço transforma nível em **borda** (segurar não repete a ação), então `--teclas
down` move o cursor uma linha só. Para simular **toques**, dois pares de flags
com um `;` por toque:

```bash
B=./target/release/giochi-neon-drift

# o direcional do controle navega o menu
$B --frames 12 --botoes-ciclo "baixo;baixo" --json | jq -r .menu      # qualidade

# A na linha PERFIL liga o overlay; A em JOGAR começa a corrida
$B --frames 12 --botoes-ciclo "baixo;baixo;baixo;a" --json | jq -r .perfil   # true
$B --frames 12 --botoes-ciclo "cima;a" --json | jq -r .fase                  # Correndo

# a LAN: escolher HOSPEDAR com o direcional e executar com A
$B --frames 14 --botoes-ciclo "baixo;baixo;baixo;baixo;dir;a" --json | jq -r .lan  # Host

# X na corrida, Y de volta ao menu
$B --frames 14 --botoes-ciclo "start;x" --json | jq -r .perfil   # true
$B --frames 14 --botoes-ciclo "start;y" --json | jq -r .fase     # Titulo
```

O `--json` publica `menu` (a linha do cursor), `menu_linha`, `perfil`, `lan` e
`lan_acao`: sem esses campos o estado do menu só existiria na tela, e "o
direcional funciona" voltaria a ser fé.

## Como o teto de 75 fps é garantido

Não é um `sleep` no fim do quadro. O núcleo roda em **passo fixo de 75 Hz**: a
física, o drift e a corrida avançam em incrementos de `1/75 s`, e o laço só
desenha o que couber. Um aparelho que desenhe mais rápido que 75 Hz espera; um
que desenhe mais devagar perde quadros de desenho, **nunca de simulação**. A CLI
recusa `--fps-teto` fora de 1..75.

### O orçamento medido depois da modernização

`--frames 1200` em `--release`, offscreen, cinco pistas, com piloto e partículas
vivas: **75,0 fps em todas** (teto do relógio) e **0,37–0,44 ms de CPU por
quadro**. Contra os 16,7 ms de um quadro a 60 Hz, sobra ~97% do orçamento de
CPU. O número que importa para 60 FPS estáveis é esse, não o fps: com o teto em
75 o fps só diria "chegou no teto". No aparelho a conversa é outra — lá o mesmo
jogo gasta 9,8 a 10,6 ms de CPU e o flip cobra 16,7 ms sem sobrepor, o que dá os
~37 fps da [tabela de desempenho](#desempenho-medido-nos-três-aparelhos).

| Pista | fps (teto) | ms_cpu | draw calls |
| --- | --- | --- | --- |
| avenida | 75,0 | 0,40 | 57 |
| boavista | 75,0 | 0,40 | 56 |
| circuito | 75,0 | 0,43 | 57 |
| porto | 75,0 | 0,44 | 56 |
| salar | 75,0 | 0,37 | 56 |

O custo no **aparelho** é outro eixo, e não é nem o pixel nem o vértice: medido
no X55, 50 % mais vértices (avenida 168 mil → circuito 252 mil) e o desenho em
360×640 em vez de 720×1280 deram o mesmo fps (35,8 a 37,8 nos quatro casos). O que
manda lá é `ms_cpu + 16,7 ms de flip`, em série, e o `ms_cpu` anda com o número de
desenhos — a conta está na seção de desempenho dos aparelhos. O `F3` continua
existindo porque é o único botão que muda o custo por pixel, e num aparelho
limitado por fragmento é ele que libera; no X55 ele não tem o que liberar.

## Diagnóstico sem dedo no aparelho

| Comando | Para quê |
| --- | --- |
| `--quadro q.bmp` | desenha um quadro e sai (offscreen no PC) |
| `--tamanho 640x480` | painel simulado **no PC**; no aparelho o KMSDRM ignora o pedido e usa o modo do painel, o que produz BMP inconsistente — capture no aparelho **sem** o `--tamanho` |
| `--passos 1500` | adianta a simulação |
| `--piloto` | o piloto artificial dirige durante os `--passos` |
| `--tela corrida` | força uma tela |
| `--mapa porto` | escolhe o mapa (`avenida`, `boavista`, `circuito`, `porto`, `salar`) |
| `--dificuldade desafiante` | escolhe o modo (`facil`, `normal`, `dificil`, `desafiante`) |
| `--em 1240` | posiciona o carro em `s = 1240 m` já a 48 m/s — é como se cai **em cima** de uma rampa sem dirigir até ela |
| `--so asfalto_t` | desenha **uma** malha só (nome da malha; `GIOCHI_SEM` usa o nome da textura) |
| `--so mundo` | desenha toda a geometria estática e mais nada |
| `--teclas a,right,shift` | teclas **seguradas** |
| `--teclas-ciclo "down;down;a"` | toques de tecla: um `;` = um toque (aperta e solta) |
| `--botoes a,start` | botões de **controle** segurados (`a,b,x,y,l1,r1,l2,r2,start,select,cima,baixo,esq,dir`) |
| `--botoes-ciclo "baixo;baixo;esq"` | toques de botão: é como se prova, por script, que o controle navega o menu |
| `--quadros N` / `--frames N` | desenha N quadros e sai (mede fps quando há relógio). `--frames` é o nome que o smoke test headless do repositório usa |
| `--fps-teto 75` | baixa o limite de quadros (1 a 75; 75 é o padrão e o máximo) |
| `--sem-som` | não abre o dispositivo de áudio |
| `--sem-vsync` | pede troca de buffer sem espera (`GIOCHI_VSYNC=0`). No KMSDRM do aparelho **não tem efeito**: o `drmModePageFlip` continua esperando o campo, e é por isso que se mede comparando configurações, não buscando o "custo puro" |
| `--escala 0.75` | desenha em resolução menor e estica (Upscaler da engine; `GIOCHI_ESCALA`) |
| `--atlas a.bmp` | grava o atlas de sprites e sai, **sem abrir janela** |
| `--texturas t.bmp` | grava as 9 texturas lado a lado, **sem abrir janela** — o molde para pintar |
| `--lan-host` / `--lan-entrar IP[:porta]` | sobe/entra numa corrida LAN; `--lan-procurar` descobre o lobby por mDNS e `--lan-porta` troca a porta (controle = porta + 1) |
| `--prova-rotacao b.bmp` | três barras de cor conhecida em coordenadas de projeto, com posição prevista em número. Separa "o mundo girou errado" de "o HUD girou errado" sem olhar a tela |
| `--json` | estado da corrida em uma linha de máquina, com `fps`, `ms_cpu`, `desenhos`, `malhas`, `vertices`, `escala`, `lan`, `batches`, `recursos` (malhas/texturas/shaders **depois** do `destruir`), `mapa`, `altitude`, `impacto`, `fragmentos`, `largou`, `progresso`, `no_ar`, `v_vertical`, `altura_solo`, `nitro`, `nitro_usando`, `derrapando`, `dificuldade`, `posicao`, `gap` e a contagem de cada tipo de partícula. `desenhos`, `malhas` e `vertices` vêm do `frame_counters()` da engine |

`GIOCHI_PERFIL=1` liga o overlay F1 já no boot — é o que permite capturar um
`--quadro` com o overlay ligado sem tecla. `GIOCHI_MODELOS=<dir>` aponta para
outro diretório de `.obj`; sem arquivo, o modelo embutido no binário é gravado
num diretório temporário e carregado pelo **mesmo** `load_obj_mesh`.

`GIOCHI_SEM=carros,particulas,hud` desliga partes do quadro — e **só** essas três
chaves existem, mais `aura` e `farol` (a poça de neon sob os carros e o cone do
farol no asfalto, medidos abaixo em [A luz que os carros jogam no
chão](#a-luz-que-os-carros-jogam-no-chão)). `GIOCHI_SEM=mundo` não desliga nada e
não avisa: o nome da malha é outro (`asfalto`, `pedra`, …) e `--so mundo` é a
flag que desenha só a geometria estática. Confundir os dois já custou duas
execuções idênticas apresentadas como A/B. `GIOCHI_CULL=0`
desliga o descarte de faces (para comparar), e `GIOCHI_VEU=0` zera o véu das
telas de texto — é o A/B do [véu](#o-véu-das-telas-de-texto). Os três existem
para **medir**, não
para jogar: num painel de 720×1280 o quadro é dominado por pixel, e sem separar
as partes "está lento" não diz o que cortar.

O `--botoes-ciclo` é o par do `--teclas-ciclo` para o controle, e existe porque
**segurar não é apertar**: cada ação do menu é uma borda, então `--botoes baixo`
move o cursor **uma** linha. A lista com `;` simula os toques:

```bash
# três toques no direcional para baixo, e A na linha PERFIL
./target/release/giochi-neon-drift --frames 12 --botoes-ciclo "baixo;baixo;baixo;a" --json
# {"menu":"perfil","menu_linha":3,"perfil":true,...}
```

`--piloto` existe porque `--passos` sozinho media outra coisa: com o acelerador
preso, o carro batia no primeiro muro e ficava a 2,6 m/s encostado — e o quadro
capturado mostrava a linha de largada como se a corrida não tivesse começado.

O salto se confere numa linha, sem dirigir até a rampa — **com `--quadro`**, que
é o que faz o processo sair depois de imprimir:

```bash
giochi-neon-drift --mapa circuito --em 1240 --teclas up --passos 60 \
                  --quadro /tmp/j.bmp --json
# no_ar:true  v_vertical:8.43  s:1279.9   ← subindo
# passos 90:  no_ar:true  v_vertical:2.43   ← perto do ápice
# passos 120: no_ar:true  v_vertical:-3.57  ← caindo
```

`--em` também marca a largada (`marcar_largou`): sem isso o carro teleportado
apareceria com volta 0 e o diagnóstico mediria outra coisa.

**`--passos` sozinho não para o processo.** Ele adianta a simulação antes do
laço e o laço continua até `Esc` ou `--quadros`; o `--json` que sai no fim é do
estado **depois** da corrida inteira. Sem `--quadro` a medição do salto reporta
`no_ar:false` porque o carro já pousou — o número é verdadeiro e responde outra
pergunta.

`--so` existe porque "o quadro está estranho" não diz **qual** malha está
errada. Foi ele que mostrou o asfalto ausente com a malha tendo 2 560 vértices:
o quadrilátero do chão estava com a ordem dos cantos invertida e o descarte de
faces o removia em silêncio.

## `scripts/traco-voltas.mjs`: a linha do tempo das voltas

```bash
node scripts/traco-voltas.mjs --teclas up --ate 400 --passo 10
node scripts/traco-voltas.mjs --mapa boavista --teclas up
```

Varre `--passos` crescentes e imprime **só as transições** de volta, fase e
largada:

```text
  passos   tempo       s   volta  fase       o que mudou
      270     0.6     0.3      1  Correndo   largou false → true
```

Existe porque "registrou uma volta na partida" é um defeito de **linha do
tempo**, e um quadro parado não o mostra. Três decisões fazem dela uma medida e
não um palpite:

- **`--passos` é determinístico** — o passo é fixo em 1/75 s, sem relógio,
  então a varredura dá a mesma linha do tempo em qualquer máquina;
- **`--teclas up` e não `--piloto`** — o piloto acelera no mesmo passo em que a
  contagem acaba e **esconde** o defeito. O percurso tem de reproduzir o
  jogador;
- **sai com 1** se achar volta creditada antes de 5 s.

Foi ela que mostrou que a avenida e a Boavista tinham causas diferentes: a
avenida creditava a volta no primeiro metro, a Boavista em **1,1 s** com o carro
a 14 km/h. A regra de contagem hoje é **progresso acumulado com correção de
volta** — `s` sozinho oscila entre ~0 e ~comprimento quando o carro está perto
da linha, porque o último trecho passa geometricamente ao lado do primeiro.

## Efeitos: o que custa o quê, medido no X55

O aparelho de 720×1280 é **dominado por pixel**, e não por triângulo: cada
chamada de desenho custa ~0,15 ms de CPU, mas o que decide o quadro é a área
desenhada. Três medições mudaram o desenho da cena:

| Mudança | Antes | Depois |
| --- | --- | --- |
| descarte de faces ligado (`cull_face`) | 29,8 fps | **39,5 fps** |
| carro: 5 malhas × 4 carros em 8 chamadas | 20 desenhos | **8 desenhos** |
| cenário novo (arcos, passarelas, outdoors) | — | **0 chamada a mais** |

O `cull_face` foi o maior ganho isolado e a causa é uma armadilha: com
`blend` ligado no quadro inteiro, o Mali **não** rejeita a face de trás por
profundidade, então metade da cidade era sombreada para nada. Ligar o descarte
obrigou a acertar o sentido de quatro quads (aura, feixe, marca de pneu e
partícula) — e o defeito, quando errado, é silencioso: o quad simplesmente não
aparece.

O cenário novo entra nas malhas que já existiam (um arco é uma caixa em `muros`
e uma tira em `neon`). Uma malha por tipo de objeto custaria uma chamada por
tipo; um arco custa **zero**.

Medido no X55, com o jogo completo: **39 fps**, 45 desenhos, 770 partículas
vivas, 26,7 MB de pico de RAM. No X35H/R35S o painel tem 1/3 dos pixels.

## A luz que os carros jogam no chão

Duas coisas do carro se veem de longe à noite, e nenhuma delas é a lataria: a
**poça de neon** embaixo dele e o **cone do farol** à frente. Elas começaram como
dois quads de cor chapada — um retângulo de borda dura e uma faixa azul de
largura constante, que liam como adesivo no asfalto. Hoje são **máscaras de
alfa**: o RGB da textura é branco em toda a imagem e quem desenha é o alfa, que
cai sem degrau do ponto quente até a borda.

| Máscara | Fórmula | O que resolve |
| --- | --- | --- |
| `halo-neon` | `(1 - r)^1,2`, com `r` a distância normalizada ao centro | a poça vira **elipse** com a proporção do carro (2,5 × 4,2 m) e apaga na borda |
| `cone-farol` | `lateral × alcance`: meia-largura `0,45 + 0,55·v` e queda `(1 - v)^1,15` | o quad retangular vira **cone**: estreito no para-choque, aberto e apagado na ponta |

As duas sobem com `create_texture_from_rgba` em 64×64 (potência de dois, então
mipmap e `REPEAT` automáticos no GLES 2.0) e ficam **fora** de `texturas::todas()`:
aquelas são superfícies que o `GIOCHI_TEXTURAS` troca por arquivo pintado, e um
halo pintado à mão perderia exatamente a propriedade que importa. É a mesma
separação do atlas de sprites.

### O drift acende e esquenta

`tempo_drift` do núcleo alimenta `rampa_de_drift`, que satura em 0,45 s — um
toque de volante não pode acender a poça como um drift de três segundos. No
auge, `tinta_da_poca` faz duas coisas e **nenhuma delas custa um pixel**:

1. **acende**: alfa de 0,46 (piso) para 0,86;
2. **esquenta**: a cor do carro puxa 40 % para o branco, e a poça pulsa a ~5,9 Hz.

O pulso só existe durante o drift. Parado, um neon que pisca vira pisca-pisca e
o jogador procura um defeito no jogo.

Carro **no ar** não tem decalque nenhum: uma poça no asfalto sem emissor lê como
sombra errada, e sombra de luz é o defeito mais visível que existe num jogo
noturno. No cliente LAN o bit de drift do snapshot vale como auge (o protocolo
não carrega a duração, e inventar um relógio local dessincronizaria do host).

### Dois defeitos que estavam embaixo do tapete

Os dois só apareceram porque a máscara tornou a luz visível o bastante para
alguém olhar o decalque:

- **A normal do decalque apontava para o horizonte.** O `flat` e o `textured`
  calculam `0,35 + 0,65·lambert`, e a terceira coluna da matriz levava o `+Z` do
  quad para `+Z` do mundo em vez de `+Y`. A posição não mudava (os cantos têm
  `z = 0`), então nada quebrava: o fator travava em 0,54 e a poça saía **1,7×
  mais escura** que o tint pedia. O teste que já existia media a normal
  **geométrica** (produto vetorial dos eixos), que sempre deu `+Y`; a que o
  shader lê é outra. E o `feixe()` reimplementava a matriz inteira em vez de
  chamar o `achatar`: teria escapado da correção se não tivesse virado, nesta
  mesma mudança, uma chamada dele.
- **O decalque era cravado em `y = 0`.** O boavista desce de 74 m: lá a poça e o
  farol ficavam **enterrados** sob o asfalto e o mapa simplesmente não tinha luz
  embaixo do carro. Agora a altura é a do carro (`d[13]`) mais o offset.

### O que custa

| Item | Antes | Depois |
| --- | --- | --- |
| chamadas de desenho | 2 (poça e feixe, cor única) | **5** (poça: uma por cor de carro; feixe: uma para todos) |
| `ms_cpu` no X55 (avenida) | 9,39 | **10,30** |
| fps no X55 (avenida) | 38,1 | **36,9** |

A poça custa uma chamada **por cor** porque `draw_instanced_textured` tem um
`u_tint` por lote e cada carro tem a sua — e a cor é o que identifica o carro.
Duas defesas: `agrupar_por_tinta` funde carros de mesma cor numa chamada (no LAN
as cores vêm de uma paleta de quatro e se repetem), e `GIOCHI_SEM=aura` /
`GIOCHI_SEM=farol` desligam cada camada para medir. O `--so` não serve aqui
porque ele corta os carros inteiros.

O custo por pixel é o mesmo de antes: a máscara **não** aumentou o quad (2,5 ×
4,2 m e 2,6 × 7,5 m continuam os mesmos, e o comentário de 09/11 sobre os 9,6 m
que custaram 9 fps segue valendo como aviso).

## Som sintetizado, sem um `.wav` no repositório

Três sons, gerados em runtime a 22 050 Hz (`crates/ui-sdl/src/audio.rs`):

| Som | Como é feito |
| --- | --- |
| motor | dente-de-serra com frequência ligada ao RPM da marcha, mais ruído filtrado — a aspereza do dente-de-serra é o que o ouvido lê como motor |
| derrapagem | ruído com passa-banda ressonante, ligado ao deslize lateral; **contínuo** enquanto derrapa |
| nitro | varredura ascendente com ruído, somada ao motor |

Nenhum arquivo de áudio, e os três motivos já custaram caro neste projeto:

1. **Empacotamento** — um `.wav` é mais um caminho que pode faltar, e o sintoma
   seria silêncio, indistinguível de "o mixer não abriu".
2. **Licença** — som de motor baixado da internet é exatamente o conteúdo
   protegido que o projeto não usa. Sintetizar é original por construção.
3. **Memória** — 2 s de motor em 22 kHz são 88 KB; três sons disso num aparelho
   de 964 MB que divide RAM com o sistema.

Custa ~300 amostras por quadro a 75 Hz: nada. E os pesos vêm de `EstadoSom`, um
struct de sete campos — o áudio **não conhece o jogo**, então dá para testar a
síntese sem abrir dispositivo.

Sem `libasound` na imagem o SDL2 não abre a saída e o jogo **avisa** em vez de
morrer:

```text
som: não abri o dispositivo (...) — o jogo segue mudo
```

`--sem-som` faz o mesmo por escolha, e é o que o CI usa.

## As três armadilhas de matriz de quad

Um quad é uma matriz de modelo de 4×4, e dois campos dela enganam:

1. **Identidade não é o ponto de partida.** `Mat4::IDENTITY` deixa `m[1]` e
   `m[5]` em 1,0, e esses campos são a componente **Y** dos eixos X e Y. Quem
   monta a matriz campo a campo sem zerá-los cisalha o quad para cima: a aura
   do carro virou um painel vertical de 5 m, e o defeito passou por três
   rodadas como "o efeito está feio".
2. **O sentido do eixo decide se o quad existe.** Com `cull_face` ligado, uma
   normal que aponta para longe da câmera faz o desenho sumir sem erro nenhum.
   Há teste para os dois casos (`o_quad_deitado_do_carro_fica_com_a_normal_para_cima`,
   `a_normal_do_quad_deitado_aponta_para_cima`).
3. **Tamanho pedido não é tamanho desenhado.** `hud_matriz` posiciona certo e
   dimensiona pela metade, então "retângulo de tela cheia" cobre o quarto
   central. Tela cheia é `hud_matriz_tela`; o caso está contado em [O véu das
   telas de texto](#o-véu-das-telas-de-texto), com a captura que o expôs.

## Os carros corriam de ré — corrigido, com prova

Relatado pelo dono: *"parece que os carros estão correndo de ré."* Era exatamente
isso, e a causa era **180° entre o modelo e o rumo**:

| Onde | O que dizia |
| --- | --- |
| `core/carro.rs` — `direcao()` | `(sin θ, cos θ)` → com θ = 0 o carro **anda para +Z** |
| `ui-sdl/malha.rs` — `carro()` | faróis em `z = -2,14`, lanternas em `z = +2,14` → a **frente desenhada é -Z** |

Girar `(0,0,-1)` por `θ` em Y dá `(-sin θ, -cos θ)`, que é **menos** a
`direcao()`. A câmera fica atrás do movimento, então o jogador via os faróis
apontando para ele.

A correção é uma linha — `matriz_de_carro` gira por `θ + π` — e **não toca a
física nem a malha**. O teste que a sustenta,
`o_carro_desenhado_aponta_para_onde_ele_anda`, pega o centro dos vértices dos
faróis, aplica a matriz e compara com `direcao()`: revertendo o `+ π`, ele falha
com produto escalar `-1.000`.

E a prova em imagem, contando pixels na área do carro no mesmo comando
(`--tela corrida --piloto --passos 900`):

```text
antes     lanternas  1 px   ·  faróis 41 px    ← de ré
depois    lanternas 149 px  ·  faróis 54 px    ← de frente
```

![Carro antes e depois](../../../docs/imagens/neon-drift-carro-antes-depois.png)

## Arquitetura depois da modernização (engine 3D 0.4.0)

O jogo continua com a mesma divisão de antes — **núcleo puro** contra
**aparelho** —, mas o lado do aparelho ganhou quatro módulos com
responsabilidade única. A regra que os separa é a mesma de sempre: só entra
aqui o que não pode ser testado sem GPU, rede ou controle.

```text
crates/core/                 regras puras (sem SDL, sem janela, sem rede)
  carro.rs · drift.rs        física arcade + pontuação de drift, passo fixo
  pista.rs · mapa.rs         traçado, relevo, rampas, obstáculos, cenários
  corrida.rs · ia.rs         voltas, posição, desfecho, pilotos
  relogio.rs                 teto de quadros e o passo fixo de 75 Hz
  fonte.rs · hud.rs          texto 5×7 e formatação do HUD

crates/ui-sdl/               aparelho: GLES, entrada, áudio, rede
  main.rs                    laço, opções de linha de comando, destruição final
  cena3d.rs                  mundo, carros, partículas, câmera e HUD 3D
  malha.rs                   geometria procedural (cone, portal de queda)
  modelos.rs                 caminho dos .obj + cópia embutida de segurança
  texturas.rs                receitas procedurais POT 64×64 (+ arquivo)
  efeitos.rs                 partículas instanciadas
  rede.rs                    host-autoritativo LAN sobre `giochi-net`
  menu.rs                    menu do título (mapa, dificuldade, qualidade, perfil, LAN)
  gamepad.rs                 SDL GameController (eixos + botões)
  perfil.rs                  profiler da engine + overlay F1
  entrada_jogo.rs            fusão pura de teclado, bits e analógico + bordas
  audio.rs                   síntese de motor, drift e nitro
```

### 1. Física e carro: tudo dentro do `time.delta`

`Corrida::passo(&entrada, dt)` é a **única** porta de avanço da simulação, e o
laço só a chama com `dt = TIQUE = 1/75`. Aceleração, ângulo de derrapagem,
atrito, colisão, rampa e pontuação de drift são funções desse passo — nenhuma
delas vê `dt` de quadro. O cliente de LAN não simula o próprio carro: recebe
posição e a escreve no mesmo `Carro` que a câmera e as partículas leem, para
não existirem duas verdades.

O `Drift` premia o que a física cobra: pontos por `velocidade × derrapagem`,
combo até ×5 em 4 s, e recarga de nitro proporcional ao tempo de derrapagem.
Bater zera o combo em curso e **não** os pontos já garantidos.

### 2. Pista e batching: 20 malhas para o mundo inteiro

`construir_mundo` emite a geometria em **construtores por material**. Cada
construtor vira uma malha pelo `StaticBatchBuilder` + `StaticBatch::build` da
engine, que compõe as peças e registra o resultado no mesmo `mesh_count` /
`release_all` das outras malhas. Medido no build de release: **20 malhas
estáticas**, 56 a 57 chamadas de desenho no quadro inteiro (mundo + 4 carros +
poças e feixes + partículas + HUD).

| O que | Como | Por quê |
| --- | --- | --- |
| asfalto, concreto, pedra, mata, guarda-corpo, painel, xadrez | `BuiltinShader::Textured`, **uma** chamada por material | a cor vem do `u_tint`; a textura dá relevo |
| faixa central, fita de barreira | `Textured` com `u_tint` **pulsante** | tracejado e grade vêm da textura, o pulso vem do uniforme |
| postes, refletores, cones, portais | `draw_instanced`, **uma** chamada cada | mesma geometria N vezes; assar no mundo trocaria 1 chamada por N |
| lataria dos carros | `BuiltinShader::Phong` | é o único builtin com termo especular — o neon corre na tampa |
| vidro, rodas, faróis, lanternas | `draw_instanced` | cor única, sem brilho: não precisam de Phong |
| poça de neon sob cada carro | `draw_instanced_textured` com máscara de alfa, **uma chamada por cor** | a poça tem a cor do carro e o `u_tint` é por lote |
| feixe de farol no asfalto | `draw_instanced_textured`, **uma chamada para todos** | o farol é branco-frio em todos os carros |

### 3. Rede LAN: host-autoritativa, dois canais

`rede.rs` usa o `giochi-net` vendorizado com as features `laminar` e `mdns`:

| Dado | Canal | Garantia |
| --- | --- | --- |
| input do cliente | `Mensagem::Input` | UDP **sequenciado** (`laminar`) |
| posição/rotação | `Mensagem::Inimigos`, 15 Hz, filtrada por `host.definir_area()` | UDP **descartável** |
| largada, volta, checkpoint, resultado | `Mensagem::Global` | **confiável-ordenado** (`laminar`) |
| descoberta do lobby | mDNS `_giochi._udp.local.` | — |

O snapshot reaproveita o `SnapshotInimigo` do protocolo sem mudá-lo: `hp`
(u16) carrega o rumo quantizado em 65536 passos (0,0001 rad), `tipo` a
velocidade e `fase_chefe` três bits de estado (no ar, derrapando, nitro). O
cliente interpola entre as duas últimas amostras pelo caminho angular mais
curto e **segura na ponta** quando a rede atrasa — extrapolar posição faz o
carro atravessar a mureta e voltar.

Como o `laminar` dá garantia por canal e o snapshot escuta na porta anunciada,
o controle usa a **porta + 1**: um cliente deriva o endereço do único dado que
o mDNS oferece.

### O batimento que faltava

O cliente manda um `Global` de 6 bytes (`Ola{id}`) **uma vez por segundo,
sempre** — não só até o lobby chegar. O `laminar` fecha conexão ociosa, e
depois disso `send` enfileira num stream que ninguém entrega. Medido: sem o
batimento, o cliente parava de receber `Estado` ~5 s depois da largada e a
posição congelava na última mensagem — sintoma que parece "rede lagada", mas é
conexão morta. O teste `o_batimento_do_controle_e_mais_rapido_que_o_timeout_do_laminar`
prende o intervalo.

O tempo de corrida no cliente vem **só** do `Estado` (4 Hz): incrementá-lo
localmente faria o cronômetro andar com a rede parada, e o sintoma seria um HUD
que mente em vez de um cliente que parece travado. A contagem regressiva é a
única exceção — ela roda local para o número andar entre dois pacotes, e o host
a corrige no próximo.

A posição no pelotão é calculada no **host** e viaja no `Estado`. O cálculo
trata progresso não finito como "desconhecido": `NaN > x` é falso, e deixar a
comparação decidir dava **1º lugar** para o carro que o host não sabia onde
estava (`progresso_nao_finito_nao_vence_a_corrida`).

### 4. Câmera: spring-arm, boost e roll

`CameraJogo` integra um mola-amortecedor criticamente amortecido
(`1 − exp(−k·dt)`, estável com quadro perdido). O braço estica com a
velocidade **e** com o nitro (+1,1 m), o FOV abre no boost e o horizonte
tomba no drift (`roll` proporcional à derrapagem). A câmera do HUD é ortográfica
de fato e desenhada no painel, com a rotação do sistema aplicada como *roll* —
é o que mantém mundo e HUD concordando no X55 retrato.

### 5. Recursos: handles opacos, zero `unsafe`, zero vazamento

`#![forbid(unsafe_code)]` nos dois crates. Toda malha, textura e shader é um
handle `u64` da engine, criado no boot e liberado por `Cena3D::destruir`, que
chama `destroy_mesh`, `destroy_texture` e `destroy_shader` um a um e só depois
`release_all` (rede de segurança). O `--json` publica a contagem **depois** da
destruição:

```json
"recursos":{"malhas":0,"texturas":0,"shaders":0}
```

Um número diferente de zero no smoke test é vazamento, e o `destruir` também
imprime um `AVISO` no stderr quando isso acontece — não é preciso ler o JSON
para descobrir.

## Sincronização com a engine 0.4.0 (13/11/2026)

O upstream foi de `3785a33` (phase-18) para `0fa46ec` (release **0.4.0**), e
desta vez o crate 3D **mudou** — ao contrário da rodada anterior, em que o diff
do `giochi-engine-3d/` era vazio. O snapshot foi refeito por cópia de
`giochi-engine/src`, `giochi-engine/tests`, `giochi-engine-3d/src` e
`giochi-engine-3d/shaders`.

### Cinco adições que são **deste vendor**, não do upstream

O snapshot antigo tinha código que o upstream não tem, e que o jogo **não roda
sem**. Elas foram reaplicadas e cada uma carrega um comentário dizendo que é
adição local — senão a próxima sincronização apaga e o sintoma não aponta para
a cópia:

| Adição | Sem ela |
| --- | --- |
| `BuiltinShader::TexturedInstanced` + `textured_instanced.vert` | sprite com textura **e** instância não existe: o atlas não desenha |
| `draw_instanced_textured` | idem, do lado da `RenderContext3D` |
| `rebuild_instancing` | trocar de mapa no título deixa carros e partículas sem desenho (§ "trocar de mapa") |
| `create_texture_from_rgba_no_mipmap` | o mipmap do atlas puxa texel da célula vizinha e cada sprite ganha um risco reto |
| `texture::from_rgba8_with(..., mipmaps)` | o filtro `LINEAR_MIPMAP_LINEAR` sem mipmap gerado dá textura **preta**, sem erro de GL |

### O que a 0.4.0 trouxe e o jogo passou a usar

- **`Texture3D::from_file`** (PNG/BMP/JPG pelo SDL_image) → `GIOCHI_TEXTURAS=<dir>`
  permite trocar as nove texturas de superfície sem recompilar. O `NEEDED` do
  binário ganhou `libSDL2_image-2.0.so.0`: é dependência de **carga** — sem a
  `.so` o processo não sobe.
- **`LoadSurface` do SDL_image** → a folha de sprites passou a aceitar PNG e
  JPEG, não só BMP (`atlas::ler_com_sdl_image`). A regra de 24 bits continua:
  sem alfa, a **luminância** vira máscara.

### O que a 0.4.0 mudou e quebrou na cópia

Três quebras de API, todas resolvidas no jogo (não no upstream):

| Antes | Agora |
| --- | --- |
| `create_texture_rgba` | `create_texture_from_rgba` |
| `draw_mesh_textured(mesh, shader, model, camera, texture, tint)` | `(mesh, shader, **texture**, model, camera, tint)` — a textura subiu dois argumentos |
| `Texture3D.handle` (campo) | `Texture3D::gl_name()` |

## Rotação no X55: o mundo e o HUD giravam em sentidos opostos

Relato do dono: *"no x55 tá rolando erro de orientação, o jogo tá de ponta a
cabeça e as informações estão na vertical"*.

Duas causas, e a segunda só apareceu depois de a primeira ser corrigida.

### 1. O HUD era montado no projeto, não no painel

`camara_hud` recebia `1280×720` (o espaço de projeto) enquanto o alvo de desenho
era `720×1280` (o painel). O texto saía esticado 1,78× num eixo e 0,56× no
outro — e, como `hud_matriz` posicionava tudo em espaço de projeto sem girar
nada, no painel retrato o texto ficava **deitado**.

A correção tem três peças:

| Peça | O que faz |
| --- | --- |
| `camara_hud(tela)` | a câmera descreve o **alvo real de desenho**, então recebe o painel |
| `TelaHud { rotacao, projeto, painel }` | leva os dois espaços juntos, para ninguém confundir de novo |
| `girar_rect_hud` / `girar_ponto_hud` | delegam a `video::girar_rect` / `video::girar_ponto` — a **mesma** função do launcher |

Delegar não é economia de linha: a orientação do painel é fato do sistema, e dois
apps que discordam sobre ela mentem um sobre o outro. Foi assim que o launcher
foi verificado a 99,9 % no X55.

### 2. O `roll` do mundo estava com o sinal trocado

Com o HUD certo, a comparação passou a acusar **7 %** de acerto onde o mundo
sozinho dava 99,9 %. Os dois giravam, cada um coerente consigo mesmo, e mesmo
assim **180° em relação ao outro**.

O raciocínio que produziu o erro era este, e é plausível:

> girar a câmera por θ gira a imagem por −θ; `girar_rect(Ccw90)` gira a imagem
> em −90°; logo a câmera tem de girar +90°... **não**: tem de girar −90°.

A conclusão estava invertida, e nada no código dizia qual das duas era a certa.
A medida que decidiu foi **separar os dois lados**:

- `--prova-rotacao` desenha três barras de cor conhecida pelo caminho do
  `girar_rect`, com posição prevista em número. Elas caem onde o launcher manda
  — logo o HUD está certo, e o mundo é que discorda.
- Com o sinal do `roll` trocado, o mundo des-rotacionado volta ao projeto a
  **99,99 %**, e o quadro completo a **99,74 %**.

O sinal agora é uma função com nome, `roll_do_mundo`, e o teste
`o_roll_do_mundo_e_o_oposto_do_giro_do_hud` amarra a relação: `roll + giro == 0`.
Trocar o sinal reprova 2 testes — verificado.

### Medido no aparelho, com o binário instalado

```bash
bash scripts/verificar-rotacao-neon-drift.sh                 # no PC
bash scripts/verificar-rotacao-neon-drift.sh --aparelho      # ida e volta
```

```text
as três barras giram exatamente como o `girar_ponto` manda
mundo + HUD des-rotacionados: 99.53% idênticos (99.74% dentro de 4 níveis)
```

E o quadro do **X55** contra o do PC, procurando entre as 8 orientações do
diedro — a que vence é a única que faz sentido:

```text
rot90-cw         erro médio  1.0     ← a inversa correta
rot90-ccw        erro médio 37.2
identidade       erro médio 36.4
transposta       erro médio 27.1
```

Histograma (X55 des-rotacionado × PC): **84,99 %** exatos, **99,49 %** dentro de
4 níveis. O resíduo é o Mali-G52 × RX 580 (precisão de cor), não desenho.

### A armadilha que custou uma rodada inteira: BMP de 32 bits

Todas as medições desta fase estavam erradas, e o motivo não era o jogo: o
`gravar_quadro` usa `Surface::save_bmp` em RGBA32, isto é, **32 bpp** — e todos
os scripts de análise liam 24 bpp (3 bytes + alinhamento). O sintoma era cor
ciclando a cada pixel (`.0..0 B1..1 G2..2 R3..3`), "0 pixels vermelhos" num
quadro que visivelmente tinha vermelho, e diffs sem sentido.

A verificação que pega isso em dez segundos: **girar um quadro 90° e girar de
volta tem de dar 100,00 %**. Deu 0 % até o leitor ser corrigido.

E a segunda armadilha, esta de convenção: desfazer `Ccw90` **não** é "girar para
o outro lado". `girar_rect(Ccw90)` leva o projeto em `(y, W−x)`; a inversa leva
o painel em `(y, H−x)` com as medidas trocadas. Girar 90° para o lado errado
produz uma **transposta**, que num quadro quase simétrico passa despercebida.

## A engine vendorizada, e o que o phase-18 trouxe

O crate `vendor/giochi-engine-3d` é um **recorte** do
[GIOCHI-ENGINE-3D](https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D), copiado e
não referenciado por caminho. Atualizar é copiar de novo — e é de propósito: a
engine é outro repositório, com outra equipe e outro ritmo.

Revisão de 10/11/2026, comparando o snapshot com o `HEAD` do upstream
(`3785a33`, phase-18):

| Novidade do phase-18 | Aplica a este jogo? |
| --- | --- |
| `giochi-engine-3d` — o crate 3D | **não mudou uma linha** entre o phase-17 (`50f6857`, que é o snapshot) e o phase-18. Nada a sincronizar |
| C ABI v3 (`giochi_engine_frame_stats`) | **não**: o jogo não usa a C API |
| `[profile.release]` = `lto = "fat"` + `strip = "symbols"` | **já era**: `lto = true` é fat, e `strip = true`, `codegen-units = 1` e `opt-level = 3` já estavam lá |
| Codegen para a CPU do console (`-C target-cpu`) | **adotado como opção, desligado por padrão** — medido abaixo |
| Contadores de quadro da engine (`frame_counters`) | **adotado**, e corrigiu um bug: ver abaixo |

### O contador de desenhos do jogo estava errado por um

O `--json` reportava `desenhos` a partir de um contador próprio, incrementado à
mão em cada ponto de desenho. A engine já conta as chamadas **no ponto em que o
backend as emite**, atrás da feature `profiling`.

Os dois números, lado a lado no mesmo quadro:

```text
avenida    jogo=55  engine=56
circuito   jogo=56  engine=57
porto      jogo=59  engine=60
```

Sempre **um** a menos. O culpado: `hud_linhas_de_velocidade` emite um `draw_instanced` para
os riscos de velocidade e **nunca incrementava** — e o comentário logo abaixo da
chamada dizia "custa **uma** chamada de desenho". É a terceira vez que este
projeto tropeça na mesma classe (§30 registrou um contador que media a si mesmo):
**um contador que o autor precisa lembrar de atualizar mede intenção, não
realidade.**

O contador do jogo foi removido (campo, método, reset e doze `+= 1`), e `--json`
passou a ler `render.frame_counters()`:

```rust
let contadores = render.frame_counters();
// draw_calls, meshes_drawn, vertices_uploaded
```

Custo da feature `profiling`, medido no X35H em 5 corridas intercaladas de 400
quadros: mediana **37,2 fps sem** contra **37,0 com** — ruído, como se espera de
alguns incrementos de `u32` por chamada. Em troca, `--json` ganhou `malhas` e
`vertices`, que o jogo não sabia contar.

### O codegen para a CPU do console: medido, e o padrão é desligado

A engine passou a compilar para o core do console (`cortex-a35` + `+crc,+aes,+sha2`),
e a lição que ela registra é boa: `RUSTFLAGS` no ambiente faz o cargo **ignorar**
`rustflags` do `.cargo/config.toml` — e o script dela define `RUSTFLAGS` para o
sysroot. O build parecia configurado e compilava para ARMv8-A genérico sem nada
falhar. Aqui não há `.cargo/config.toml`; a flag entra em
`scripts/compilar-jogo-arm64.sh`, e foi verificada com um `RUSTC_WRAPPER` que
registra a linha de comando real do compilador.

O que a medição disse no X35H:

| | mediana de 5 corridas | binário |
| --- | --- | --- |
| sem `target-cpu` | **37,0 fps** | 675 584 B |
| `cortex-a35` + features | 36,8 fps | 701 712 B (**+3,9 %**) |

Mais lento **dentro do ruído**, maior de verdade, e sem ganho nenhum. O padrão
ficou sem `target-cpu`; `GIOCHI_TARGET_CPU=cortex-a35` liga o alvo conservador
para quem quiser. O mecanismo existe porque o número pode mudar com outra versão
de rustc, e o motivo de o padrão ser desligado está escrito ao lado dele no
script — não numa mensagem de commit.

#### Por que `cortex-a55` seria errado, mesmo sendo o core do X35H

O `bin/giochi-neon-drift` é o **mesmo arquivo** no R35S, no X35H e no X55 — é a
premissa do projeto desde o primeiro aparelho. Medido com `rustc --print cfg`:

```text
-C target-cpu=cortex-a35  → aes, crc, neon, sha2
-C target-cpu=cortex-a55  → aes, crc, neon, sha2, lse
```

`lse` são as atômicas ARMv8.1 (`ldadd`/`swpal`). O Cortex-A35 do R35S é
ARMv8.0-A e **não decodifica nenhuma delas**: o binário morreria com `SIGILL` na
primeira atômica, que num jogo é um `Arc::clone` do primeiro quadro. O
`/proc/cpuinfo` confirma a diferença entre os aparelhos:

```text
X35H (RK3566, Cortex-A55)  ... crc32 atomics fphp asimdhp cpuid asimdrdm lrcpc dcpop asimddp
R35S (RK3326, Cortex-A35)  ... crc32 cpuid
```

Por isso o script **recusa** `+lse` com erro, em vez de aceitar e produzir um
binário que só falha no aparelho mais antigo.

## O que este jogo **não** faz

- Não tem save nem placar persistente.
- **Não prediz** o próprio carro no cliente de LAN: o cliente manda input e
  interpola o que o host devolve. Em LAN o RTT é de 1–4 ms; predição resolveria
  um problema que não existe e criaria dessincronização sem forma de reproduzir.
- Não usa NAT traversal nem relay: **LAN apenas**, como o `giochi-net` declara.
- Não usa `SDL2_ttf`: o texto é uma fonte 5×7 em dados (`crates/core/src/fonte.rs`),
  porque o aparelho pode não ter a biblioteca e um jogo que depende dela não
  abre.

## Estrutura

```text
crates/core/     regras puras: mapa, pista, carro, drift, IA, corrida, fonte,
                 HUD, relógio e o piloto artificial. Sem SDL, sem janela e sem
                 arquivo: 146 testes.
crates/ui-sdl/   cena 3D, efeitos, malhas, texturas e atlas de sprites
                 procedurais (ou folha externa em BMP), áudio, evdev, gamepad,
                 rede LAN, menu do título, perfil e o laço. É a única parte que
                 precisa de tela: 211 testes.
vendor/          cópia isolada da GIOCHI-ENGINE-3D 0.4.0 e do `giochi-net`
                 (features `laminar` e `mdns`). A engine original é outro
                 repositório e **não** é editada por este app; as duas adições
                 locais estão listadas na seção de sincronização.
assets/modelos/  `carro.obj` e `portal.obj`. Gerados por
                 `scripts/gerar-modelos.mjs`; o jogo carrega por
                 `render.load_obj_mesh` e, se o arquivo faltar no aparelho, usa
                 uma cópia embutida no binário — mesmo caminho de carga nos dois
                 casos.
scripts/         `gerar-modelos.mjs`: regera os dois `.obj` com winding
                 anti-horário para fora. Rodar depois de mudar proporções.
TEXTURAS.md      guia de arte: tamanho, canais, escala no mundo, faixa de
                 luminância, as três armadilhas de BMP e onde mexer para cada
                 mudança.
```

### O que roda onde

| Comando | Precisa de | Prova |
| --- | --- | --- |
| `cargo test --workspace` | nada | 313 testes: física, pista, rede (codificação/interpolação), texturas, fonte |
| `--frames N` + `offscreen` | nada | o laço desenha N quadros e o `--json` publica fps, draw calls e `recursos` zerados |
| `--quadro q.bmp` | GLES (offscreen serve) | um quadro real em disco, para medir luminância e conferir orientação |
| `--lan-host` / `--lan-entrar` | soquete UDP | duas instâncias conversam; o cliente recebe posição e o host vê o id |
| `--target aarch64-unknown-linux-gnu` | sysroot ARM + SDL2 do sistema | o binário do aparelho, `NEEDED` = SDL2 + SDL2_image + libc/libm/libgcc_s |
