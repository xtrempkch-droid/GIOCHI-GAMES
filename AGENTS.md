# AGENTS.md — Giochi Games

> Handoff técnico e **regras** deste repositório. Leia antes de continuar.
> Criado em 15/11/2026.
>
> **Quatro regras, e elas vêm antes de qualquer tarefa:**
> **R1** repositório alheio é **somente leitura** ([§2](#2-r1--repositório-alheio-é-somente-leitura)) ·
> **R2** **qualquer alteração** fora daqui vira **issue** ([§3](#3-r2--qualquer-alteração-fora-daqui-vira-issue)) ·
> **R3** o foco é a `GIOCHI-ENGINE-3D`, outras plataformas em segundo plano ([§4](#4-r3--o-foco-é-a-giochi-engine-3d-outras-plataformas-em-segundo-plano)) ·
> **R4** o alvo é **handheld**; o PC **não** é plataforma de entrega ([§4.1](#41-r4--alvo-handheld-não-pc))
>
> Lê-se o que é dos outros. Escreve-se só aqui. Precisa de mudar algo lá fora?
> Issue, com reprodução — nunca o commit.
>
> **Estes jogos não são para PC.** São para handhelds — `aarch64`, KMSDRM, painel
> pequeno, controle na mão. `x86_64` é a bancada onde se mede e se testa; um
> binário de PC que funcione não é entrega, e um item não fecha por ele. A regra
> inteira, com o que é permitido e o que não é, está no [§4.1](#41-r4--alvo-handheld-não-pc).
>
> O código deste repositório cita `AGENTS.md §12`, `§23`, `§24`, `§25`, `§31`,
> `§32` — números do documento do `GIOCHI-APP` **antes** de ele ser compactado e
> renumerado. O [§5](#5-as-citações-agentsmd-nn-espalhadas-no-código) explica
> quais migraram e quais apontam hoje para outro assunto.

## 0. Estado em uma tela

| Jogo | Versão | Engine vendorizada | Testes (`cargo test`) | Binário aarch64 |
| --- | --- | --- | --- | --- |
| `cobrinha` | 1.1.0 | 0.5.0 (`f12c09b`) | **72** | 2 913 008 B |
| `neon-drift` | 1.0.1 | 0.4.0 (`88741aa`) | **385** | 1 315 560 B |
| `giochi-tower` | 0.1.0 | 0.3.1 (`33a3b92`) | **266** | 2 068 024 B |
| `giochi-defender` | 0.2.0 | 0.3.1 (`cd94ce6`) | **331** | 2 454 416 B |

Todos os quatro são **Rust + SDL2 + GLES 2.0**, alvo `aarch64-unknown-linux-gnu`
(consoles Linux com KMSDRM). O `x86_64` é a **bancada de medição**, não um alvo —
ver [R4](#41-r4--alvo-handheld-não-pc). Cada jogo é um workspace Cargo próprio:
não há workspace na raiz e não deve haver — os jogos partilham a engine, não o
`Cargo.lock`.

---

## 1. Objetivo e fronteira

Todos os repositórios abaixo são **somente leitura**, sem exceção. A coluna
"como trato" diz o que fazer quando algo lá precisa de mudar.

| Repositório | Função | Como trato |
| --- | --- | --- |
| **Giochi Games** (este) | Os quatro jogos. É aqui que o trabalho acontece | **edito** |
| **GIOCHI-ENGINE-3D** | **A engine.** Fonte canónica de `giochi-engine`, `giochi-engine-3d`, `giochi-net` | leio; issue (R2) |
| GIOCHI-ENGINE | Engine anterior (0.1.0), fora do caminho dos jogos | leio; issue (R2) |
| GIOCHI-SYSTEM | Imagens Linux ARM64 por dispositivo (Buildroot) | leio; issue (R2) |
| GIOCHI-OS | Junção sistema + apps, imagem final por aparelho | leio; issue (R2) |
| GIOCHI-APP | Apps do sistema: launcher, forge, store, painel | leio; issue (R2) |
| `giochi-engine` | Clone separado da engine 0.1.0, em minúsculas | leio; issue (R2) |

Cada pasta é um **repositório Git individual**. A comunicação entre eles é sempre
por **artefacto declarado explicitamente** (ficheiro de release, catálogo,
`metadata.json`), nunca por caminho relativo presumido.

A engine chega aos jogos por **cópia vendorizada** (`vendor/` em cada jogo), que é
o que torna a regra R1 verificável: o jogo não tem como alcançar o repositório da
engine sem alguém copiar de propósito.

---

## 2. R1 — repositório alheio é **somente leitura**

**Todo repositório que não é este é somente leitura.** `GIOCHI-ENGINE-3D`,
`GIOCHI-ENGINE`, `giochi-engine` (minúsculas — é um clone separado, com o seu
próprio `HEAD`), `GIOCHI-SYSTEM`, `GIOCHI-OS`, `GIOCHI-APP` e qualquer outro:
lê-se, não se escreve. Nem para "corrigir rapidamente", nem para "experimentar e
reverter depois".

Precisou de uma alteração lá? **Abre-se uma issue** ([R2](#3-r2--qualquer-alteração-fora-daqui-vira-issue)) e a equipa dona trata. Não se implementa, não se
"deixa pronto no branch", não se manda patch aplicado.

Isto não é burocracia: o `vendor/` de cada jogo é uma cópia de um **commit
específico** da engine. Uma alteração no repositório da engine muda o HEAD de um
sítio que quatro jogos consomem por cópia, e o defeito aparece num jogo que
ninguém tocou semanas depois.

### Como se verifica (obrigatório antes de dizer "acabei")

```bash
# todos de uma vez, antes e depois da sessão
for r in GIOCHI-ENGINE-3D GIOCHI-ENGINE giochi-engine GIOCHI-SYSTEM GIOCHI-OS GIOCHI-APP; do
  printf '%-20s %s  %s\n' "$r" \
    "$(git -C ../$r rev-parse --short HEAD)" \
    "$(git -C ../$r status --porcelain | wc -l) sujo(s)"
done
```

A coluna dos ficheiros sujos tem de dar **o mesmo número** no fim, e o `HEAD` tem
de ser o mesmo. Se mudou, algo escreveu lá.

**O baseline não é zero, é o que se mediu no início.** Estes repositórios têm
trabalho em curso das equipas donas: medido em 15/11/2026, `GIOCHI-APP` estava
com 42 ficheiros modificados e `GIOCHI-ENGINE` com 7, nenhum deles meu. Comparar
contra zero daria alarme falso; comparar contra o início da sessão é o que
distingue "a equipa está a trabalhar" de "eu escrevi ali".

Corolário: se um comando deste repositório grava em `dist/` do repositório
alheio (o `aparelho.sh` grava capturas), confirme que a pasta está no
`.gitignore` de lá — `GIOCHI-APP/.gitignore` cobre `dist/`, e é por isso que uma
sessão de instalação não aparece como alteração.

### Eles mexem ao mesmo tempo

Estes repositórios **não estão parados à espera**: as equipas donas editam-nos
enquanto trabalhamos aqui. Medido em 15/11/2026, o `GIOCHI-APP/AGENTS.md` mudou
às 15:37 — durante a sessão que escrevia este documento.

Duas consequências que já custaram confusão:

- um `git status` que muda no meio da sessão pode ser **deles**, não nosso. Por
  isso o baseline é por ficheiro e por hora, não só uma contagem;
- nunca se conclui "o ficheiro diz X" a partir de uma leitura antiga. O
  `GIOCHI-ENGINE-3D` mudou de `f12c09b` para `622e8cc` **durante** esta mesma
  sessão, e um número citado antes já estava velho depois. Releia antes de agir,
  e cite o commit junto do número.

O precedente é o §25 do documento antigo, e está citado no
`giochi-tower/README.md`: *"NÃO EDITAR (regra R1)"*. A prática que funcionou foi
medir o `git status` **antes e depois** de cada sessão.

### O que R1 permite

| Permitido | Proibido |
| --- | --- |
| **Ler** o código da engine (é a documentação de referência) | Editar, criar ou apagar ficheiro em repositório alheio |
| `git log`/`git show`/`git diff` para consultar | `git commit`, `git push`, `git checkout --` lá |
| Copiar de novo para o `vendor/` de um jogo | "Corrigir só aqui para desbloquear" |
| **Abrir issue** com reprodução e proposta (R2) | Deixar alteração feita "para depois reverter" |
| Declarar um patch local **no jogo** (ver §8) | Deixar patch local sem declarar |

Escrever num repositório alheio inclui o que parece inofensivo: apagar um
temporário, `git clean`, corrigir um comentário, `cargo fmt`, gerar um
`Cargo.lock`. Se o comando pode mudar o `status` daquele repositório, não corre
lá — corre-se numa **cópia** (o `vendor/` deste repositório existe para isso) ou
não se corre.

### A exceção, e o preço dela

Um jogo pode precisar de um comportamento que a engine não tem. O `neon-drift`
tem cinco adições locais ao `vendor/` (shader com textura e instância, atlas sem
mipmap, etc.) e o `cobrinha` tem uma (upscale aceita escala > 1,0).

Repare que a exceção é **dentro deste repositório**: o patch vive no `vendor/` do
jogo, que é nosso. A engine continua intacta, e a issue aberta para ela é o que
faz o patch deixar de ser necessário um dia.

Quando não houver alternativa, o preço é **declarar nos três lugares**:

1. no `vendor/Cargo.toml` do jogo, em comentário;
2. no `README.md` do jogo, numa tabela "adições deste vendor" com o sintoma de
   cada uma quando ausente;
3. no próprio código da adição, num comentário que diga que é local.

Sem os três, a próxima sincronização do `vendor/` apaga a adição. Foi o que
obrigou a reaplicar à mão o patch de supersampling do `cobrinha` quando a cópia
subiu de 0.2.2 para 0.5.0 — a alteração estava declarada (foi por isso que se
soube o que repor), e mesmo assim o trabalho foi manual.

---

## 3. R2 — qualquer alteração fora daqui vira **issue**

Não é só defeito. **Qualquer** coisa que precise de ser mudada num repositório
que não este — bug, melhoria, texto errado, campo que falta no manifesto — vira
**issue** para a equipa dona. O repositório dela é somente leitura
([R1](#2-r1--repositório-alheio-é-somente-leitura)); a issue é o canal.

O relatório tem de conter, no mínimo:

| Campo | Conteúdo |
| --- | --- |
| **Sintoma** | o que se vê, em uma frase |
| **Reprodução** | comando exato, sem "passos vagos" |
| **Medido** | número antes × depois, ou o erro literal |
| **Causa raiz** | ficheiro e linha; se não se sabe, dizer que não se sabe |
| **Proposta** | diff ou descrição; opcional, mas encurta a conversa |
| **Impacto** | quantos jogos/consolas herdam aquilo |

E onde **não** se escreve:

| Não | Por quê |
| --- | --- |
| `GIOCHI-ENGINE-3D/src/…` | R1 — é somente leitura |
| mensagem de commit no repositório alheio | é o mesmo que editar, com passos extra |
| "corrigi provisoriamente no vendor e depois vejo" | o provisório fica |
| "apliquei o patch e avisei na issue" | a equipa dona decide; patch aplicado é alteração feita |

O trabalho **não fica bloqueado** por R2: o que o jogo precisa resolve-se **no
jogo** (com a exceção declarada do §2). O que R2 proíbe é a alteração ir parar ao
repositório de outra equipa sem passar pelo canal dela.

E uma issue aberta **fecha o item daqui**. Não se espera a correção do outro lado
para terminar a tarefa deste lado: registou-se o que falta, com reprodução, e a
tarefa local está entregue.

### Template (copiar)

````markdown
## Sintoma
<uma frase>

## Reprodução
```bash
<comando exato>
```

## Medido
antes:  <número / erro literal>
depois: <número / erro literal>

## Causa raiz
`<repo>/<ficheiro>:<linha>` — <o que a linha faz de errado>

## Impacto
- <jogo A>: <sintoma>
- <jogo B>: <sintoma>

## Proposta
<diff ou descrição>
````

---

## 4. R3 — o foco é a **GIOCHI-ENGINE-3D**; outras plataformas em segundo plano

Quando houver de escolher onde gastar esforço, a ordem é esta:

| Prioridade | O quê |
| --- | --- |
| 1 | **GIOCHI-ENGINE-3D** — a engine que os quatro jogos consomem |
| 2 | **os quatro jogos** deste repositório (defeito que o jogador vê) |
| 3 | **GIOCHI-SYSTEM / GIOCHI-OS** — a plataforma, quando o defeito é dela |
| 4 | GIOCHI-APP, GIOCHI-ENGINE (0.1.0), afinações de plataforma |

Consequências práticas:

- Uma decisão que sirva à engine vale mais do que a mesma decisão presa num
  jogo: se a correção é genérica, ela pertence à engine — e por R1 isso quer
  dizer **issue com proposta**, não commit.
- Não se abre trabalho de plataforma (imagem, kernel, launcher, otimização de
  sistema) enquanto houver um jogo a falhar no aparelho.
- Ports para outras plataformas (outros SoCs, outros sistemas, outras linguagens)
  ficam atrás das quatro linhas acima. A engine declara o alvo primário
  `aarch64-unknown-linux-gnu` com KMSDRM; `x86_64` é o ambiente de
  desenvolvimento, e serve para medir, não para entregar.
- "Funciona no PC" não fecha um item. O alvo é o aparelho, porque o que falha
  lá (KMSDRM, dono do DRM, `libpng` ausente, vblank de 60 Hz) não aparece
  no PC.

### 4.1 R4 — alvo handheld, não PC

**Os jogos deste repositório são para handhelds. Não são para PC, e não viram.**

A frase não é preferência de gosto: é o que decide o que conta como pronto, o
que entra num README e onde o esforço é gasto. Um jogo que só abre no PC não
está entregue; um que abre no aparelho está.

| O que o alvo é | Detalhe |
| --- | --- |
| **arquitetura** | `aarch64-unknown-linux-gnu` — o binário versionado em `bin/` é sempre ARM64 |
| **vídeo** | KMSDRM, `/dev/dri` **exclusivo**, sem X11 e sem Wayland |
| **entrada** | **controle na mão**, e só ele: sem teclado, sem mouse, sem tela de toque |
| **painel** | 3,5" a 5", frequentemente girado (retrato 720×1280 no X55) |
| **GPU** | Mali (Panfrost) com GLES 2.0 como piso |
| **energia** | bateria; o que esquenta ou drena reprova mesmo que o fps agrade |

| O PC (`x86_64`) **é** | O PC **não é** |
| --- | --- |
| bancada de `cargo test`, `cargo clippy` e `cargo build` | plataforma de entrega |
| gerador de quadro offscreen (`SDL_VIDEODRIVER=offscreen`) | alvo de pacote, instalador ou loja |
| A/B de desempenho (`--json`, `--benchmark`) | prova de que "está pronto" |
| ensaio dos caminhos do aparelho (`GIOCHI_INPUT_DIR`, `GIOCHI_MAPA_BOTOES`, `GIOCHI_VEU=0`) | desculpa para código que só existe no PC |
| lugar de onde se faz `scp`/`ssh` para o aparelho | o que o jogador vê |

Consequências que valem como regra:

- **Não se cria artefato de PC**: nada de instalador, `.desktop`, AppImage,
  flatpak, pacote de distribuição, alvo Windows/macOS. Um binário `x86_64` no
  repositório é erro — o `bin/` é `aarch64`, e é `strip`ado. Conferido nos
  quatro jogos: `file */bin/giochi-*` responde `ARM aarch64` em todos, e o
  `NEEDED` é só `libSDL2*`, `libc`, `libm` e `libgcc_s`.
- **Documentação de PC é rotulada como bancada.** `cargo run` no README existe
  para medir; tem de estar dito que é desenvolvimento. Captura de tela do PC é
  diagnóstico, não aceitação.
- **Um caminho de código que só funciona no PC não serve.** O teclado é atalho
  de desenvolvimento; toda ação precisa de botão equivalente — foi o que a
  revisão de controles do `neon-drift` teve de corrigir depois do fato (LAN,
  escala de render e overlay eram inalcançáveis num handheld).
- **Aceitação é medida no aparelho**, com o binário instalado: `--quadro` no
  painel real, `--json` de uma corrida real, e o `giochi-launcher` do ciclo de
  vida. Foi assim que se pegaram o HUD girado, o `libpng` ausente e o teto de
  59,6 fps por vblank.
- **Fallback de PC é fallback, não caminho principal.** O `neon-drift` mantém o
  caminho do SDL porque ele também é o degrau 2 do contrato de entrada num
  aparelho cujo evdev não abra — não porque o PC seja alvo.
- **Nada de "depois eu adapto para o aparelho".** Se a decisão depende de
  `x86_64` (float de 80 bits, `mmap` diferente, tamanho de página, `libc` de
  desktop), ela não vale como decisão até rodar no alvo.

#### Auditoria: o que ainda depende de teclado ou mouse

Medido em 17/11/2026, procurando ação sem botão equivalente. Uma linha por jogo,
com o arquivo e a linha da pendência — é o que a próxima sessão precisa para não
repetir a busca.

| Jogo | Situação | Pendência |
| --- | --- | --- |
| `neon-drift` | **completo no controle**: menu do título (mapa, dificuldade, escala, perfil, LAN, jogar), corrida e resultado | — (teclado é bancada, e cada tecla tem botão) |
| `giochi-tower` | **paridade**: ataque, duas habilidades, item rápido, loja e pausa têm botão (`src/engine_bridge/input.rs`) | — (teclado é alternativa, não caminho único) |
| `cobrinha` | jogo no controle: direcional, A/START, B/SELECT | **overlay de diagnóstico só no `F1`** — `crates/ui-sdl/src/main.rs:647`, sem botão equivalente |
| `giochi-defender` | órbita no direcional e no analógico direito; sair pelo `B` no menu | **zoom só na roda do mouse** — `src/main.rs:893`; `camera.zoom` não é chamado por nenhum caminho de controle |

As duas pendências são do mesmo tipo: uma capacidade que existe e não tem como
ser alcançada com o aparelho na mão. Elas **não** foram corrigidas nesta sessão
porque mudam controles já documentados dos dois jogos e pedem verificação no
aparelho; ficam registradas aqui e no README de cada um.

E o corolário que fecha a regra: **um pedido para "deixar no PC" é bancada,
não escopo.** O dono pode pedir uma cópia local para experimentar; ela vive fora
do repositório, não é versionada e não muda o alvo.

---

## 5. As citações `AGENTS.md §NN` espalhadas no código

O código tem comentários do tipo `(AGENTS.md §24)` ou `(regra R1 do §25)`. Esses
números apontam para o documento que vivia em `GIOCHI-APP`, quando os jogos
moravam lá. **Não use esse ficheiro como referência direta hoje**: ele continua a
existir, mas foi compactado (commit `f66a92e`, "AGENTS.md compactado") — passou
de 45 secções para 18, e o conteúdo foi **renumerado**, não preservado nos mesmos
números.

Há dois desfechos, e os dois aparecem no código:

| Caso | Exemplo | Onde está hoje |
| --- | --- | --- |
| conteúdo **migrou** | `giochi-defender/src/input_evdev.rs` cita `§12` para "entrada por evdev, não por SDL gamepad" | `GIOCHI-APP/AGENTS.md` **§6.3**, com o mesmo título |
| número **reutilizado** para outra coisa | `neon-drift/cena3d.rs` e `giochi-tower/{menu,render}.rs` citam `§12` para rotação de HUD e rótulo de menu que anunciava a ação errada | o `§12` de hoje chama-se "Ambientes medidos" e é uma tabela de SoCs — **assunto diferente** |

O segundo caso é o perigoso: a citação resolve, mas para o sítio errado. Os
números antigos só existem em `git log`:

```bash
git -C ../GIOCHI-APP show 391a3f1:AGENTS.md | sed -n '760p'   # §12 antigo
```

Regra: **não reescrever essas citações em massa.** Elas carregam o número da
medição original (e a data), que é a única pista de qual decisão é. Se tocar num
ficheiro que tem uma citação:

- a citação fica;
- se o assunto for explicado no `README.md` do jogo, acrescentar a referência ao
  lado (`AGENTS.md §24 · ver README "Texturas"`);
- **R1 continua a significar o mesmo**: o `vendor/` é cópia, não se edita.

Distribuição atual, fora do `vendor/`: `giochi-tower` 10, `giochi-defender` 4,
`neon-drift` 3, `cobrinha` **0** (foi reescrito de raiz e não herdou citações).
Note que `GDD §12` **não** é a mesma coisa — é o documento de design do jogo, e
esse continua válido.

---

## 6. Estrutura

Há **dois** layouts, e não é dívida a resolver agora — são decisões tomadas em
momentos diferentes, e as duas funcionam:

| Layout | Jogos | Por quê |
| --- | --- | --- |
| **dois crates** (`crates/core` + `crates/ui-sdl`) | `cobrinha`, `neon-drift` | `core` não linka SDL: os testes de regra correm sem ecrã |
| **crate único** (`src/` com módulos) | `giochi-tower`, `giochi-defender` | mais antigos, já separam por módulo (`game/`, `net/`, `ui/`) dentro do mesmo crate |

```text
< jogo >/
├── Cargo.toml          workspace do jogo; `exclude = ["vendor"]`
├── metadata.json       catálogo do aparelho (title, version, entrypoint…)
├── README.md           o relatório do jogo: defeitos, medições, decisões
├── bin/giochi-<nome>   binário aarch64 instalado no aparelho
├── crates/{core,ui-sdl}/   (só cobrinha e neon-drift)
│   ou src/{game,net,ui}/   (só giochi-tower e giochi-defender)
├── vendor/             engine (COPIA — ver R1)
├── assets/             modelos, shaders, texturas
└── scripts/            geradores `.mjs` (cobrinha, neon-drift, giochi-tower)
```

Nem todos têm tudo: o `giochi-defender` não tem `scripts/` (tem `benches/`), e
os `assets/` variam de conteúdo. O que **todos** têm é `Cargo.toml`,
`metadata.json`, `README.md`, `bin/` e `vendor/` — se faltar um destes cinco,
algo está errado com a instalação, não com a documentação.

A regra não é o número de crates: **regra de jogo sem SDL, plataforma com SDL**.
Num crate único isso é por módulo; em dois crates, por dependência. Sintoma que
só aparece com janela aberta pertence à camada de plataforma, em qualquer dos
dois casos.

---

## 7. Comandos

```bash
# testar um jogo (é o comando de verificação; rodar antes de dizer "acabou")
cd <jogo> && cargo test

# gerar um quadro sem tela — bancada, não entrega (R4)
SDL_VIDEODRIVER=offscreen SDL_AUDIODRIVER=dummy \
  ./target/release/giochi-<nome> --quadro /tmp/q.bmp --json

# compilar para o aparelho (aarch64, pkg-config cruzado) — ESTE é o que se entrega
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR=$HOME/.cache/giochi-arm64/pkgconfig
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$HOME/.cache/giochi-arm64/cc-aarch64.sh
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export RUSTFLAGS="-L $HOME/.cache/giochi-arm64/devlibs -L $HOME/.cache/giochi-arm64/sysroot/usr/aarch64-linux-gnu/lib"
cargo build --release --target aarch64-unknown-linux-gnu
cp target/aarch64-unknown-linux-gnu/release/giochi-<nome> bin/
strip bin/giochi-<nome>        # o binário versionado é stripado

# conferir as dependências do binário (tem de ser só libSDL2*/libc/libm/libgcc_s)
aarch64-linux-gnu-objdump -p bin/giochi-<nome> | grep NEEDED
```

**`sdl2-sys` em build bundled falha neste ambiente** (o CMake 4 recusa a
`CMakeLists.txt` antiga do SDL). Quando o `vendor/` for compilado sozinho:

```bash
cargo build --manifest-path vendor/Cargo.toml --no-default-features --features sdl2-system
```

Os quatro jogos pedem `sdl2-system` — nos de crate único no `Cargo.toml` da raiz;
nos de dois crates, em `crates/ui-sdl/Cargo.toml`. O problema do CMake só aparece
ao compilar o workspace do `vendor/` **diretamente**, porque aí ninguém pediu a
feature e vale o `default` (`sdl2-bundled`).

---

## 8. Vendor: atualizar e o que nunca fazer

Atualizar a engine de um jogo é **copiar de novo**, de propósito, e num commit
que diz isso:

```bash
git -C ../GIOCHI-ENGINE-3D archive <commit> giochi-engine giochi-engine-3d \
  | tar -x -C vendor/
```

| Regra | Motivo |
| --- | --- |
| Copiar por `git archive`/`rsync`, nunca por edição à mão | é o que mantém o `vendor/` auditável contra um commit |
| `--exclude target`, e nunca versionar `vendor/target` | cache de build não é fonte |
| Reescrever `vendor/Cargo.toml` (versão, `rust-version`, `glam`/`glow`) | o snapshot precisa de fechar com o código que chama — ver §9 |
| Reaplicar as adições locais declaradas (§2) | sem isso a sincronização apaga-as |
| Rodar `cargo test` do jogo depois | é o único sinal de que a cópia serve |

---

## 9. Verificar é medir, não afirmar

O padrão da casa, e o que os `README.md` dos jogos fazem: **número antes,
número depois**, com o comando ao lado. Sem isso não há correção — há opinião.

| Afirmação | Como se prova aqui |
| --- | --- |
| "o quadro mudou" | `--quadro x.bmp` (todos) + contagem de cores distintas / % de pixels diferentes |
| "o carro saiu do lugar" | `neon-drift --json`: `s`, `velocidade`, `largou`, `progresso` |
| "a LAN funciona" | dois processos reais, host + cliente, e ler o estado dos dois |
| "esta peça custa X ms" | `--json` no `cobrinha`/`neon-drift` (traz `fps`, `ms_cpu`, `desenhos`, `vertices`); `--benchmark` no `giochi-tower`/`giochi-defender` |
| "o defeito não volta" | teste que **falha** no código antigo; provar a falha, não só o passar |

As interfaces de medição **não são as mesmas** nos quatro jogos: os dois de dois
crates expõem `--json` (uma linha de máquina) e os dois de crate único expõem
`--benchmark` (relatório de FPS). Antes de escrever um comando num README,
confira qual o jogo tem — um `--json` copiado para o `giochi-tower` é um erro de
argumento, não uma medição.

Corolário: um teste que passa contra o código com o defeito **não** testa o
defeito. Antes de fechar uma correção, desligar a correção e ver o teste falhar.
Foi assim que se descobriu que o teste de ida-e-volta do `giochi-net` só usava
coordenadas positivas — passava com o bug presente.

Ferramentas que já falharam neste ambiente: `pkill`/`killall` (proibidos; usar
`kill <PID>`), `PIL`/ImageMagick ausentes (BMP reinterpretado à mão), `sdl2`
bundled (§7). Não vale a pena insistir nelas.

---

## 10. O que este repositório não faz

- **Não** publica a imagem do aparelho — isso é `GIOCHI-OS`/`GIOCHI-SYSTEM`.
- **Não** tem o código da engine — só cópias vendorizadas, de commits nomeados.
- **Não** é workspace único: cada jogo tem `Cargo.lock` próprio, de propósito,
  para uma atualização de engine num jogo não arrastar os outros.
- **Não** escreve em repositório alheio (R1), nem para corrigir um defeito de um
  caractere, nem para apagar um temporário. Qualquer alteração que precise de ser
  feita lá fora vira **issue** com reprodução (R2) — o repositório dos outros é
  somente leitura.
- **Não** entrega para o PC, e **não** é um projeto de PC: os quatro jogos são
  para handhelds (R4). O PC é bancada de medição e de teste sem ecrã — nada de
  instalador, `.desktop`, AppImage, flatpak ou alvo Windows/macOS.
- **Não** aceita "funciona no PC" como pronto. Aceitação é `--quadro` no painel
  real e `--json` de uma corrida no aparelho (R3 e R4).

---

## 11. Issues abertas noutros repositórios

Registar aqui o que saiu daqui por R2, para a próxima sessão não repetir o
diagnóstico nem abrir issue duplicada.

| Repositório | Tema | Estado |
| --- | --- | --- |
| `GIOCHI-APP` | `.giochi` não leva `config/` nem `assets/`; `giochi-tower` instalado pela loja não abre | corpo pronto, **a submeter** |
| `GIOCHI-SYSTEM` | `/run/giochi` nunca é criada; o `touch .../dev-free-console` do README do `neon-drift` falha e o DRM não é libertado | corpo pronto, **a submeter** |

Os corpos completos estão fora do repositório, na pasta de sessão do agente
(`files/issue-GIOCHI-APP-pacote-sem-runtime.md` e
`files/issue-GIOCHI-SYSTEM-run-giochi-ausente.md`). Cada um segue os seis campos
da [R2](#3-r2--qualquer-alteração-fora-daqui-vira-issue).

### Abrir issue depende de um clique humano

Medido em 15/11/2026: a criação pelo MCP do GitHub **autentica** (um `POST` para
uma issue inexistente responde 404, não 401), mas o servidor exige confirmação
num formulário do VS Code. O agente renderiza o formulário e o processo pára ali:
`list_issues` continua a devolver **0**. Não há caminho programático — nem `gh`
(não está instalado), nem `curl` com token.

Consequência prática: o entregável de R2 é **o corpo da issue**, não a issue. Se
o clique não vier na sessão, o texto tem de ficar onde a próxima sessão o ache.

A equipa do `GIOCHI-APP` chegou à mesma conclusão no mesmo dia e resolveu-a à
maneira dela: `GIOCHI-APP/docs/issues-para-outros-repos.md` guarda os corpos que
ela deve aos vizinhos (quatro, em 15/11/2026), com a tabela de gravidade. Vale a
pena ler antes de abrir algo para `GIOCHI-SYSTEM` ou `GIOCHI-OS` — pode já estar
lá, e issue duplicada queima a atenção de quem a recebe.

### Workaround enquanto a correção não vem

Manual, no aparelho, e **não se versiona** — é precisamente o que R1 proíbe fazer
no repositório dos outros:

```bash
# giochi-tower instalado pela loja: falta o que ele lê do disco
cd <jogos>/giochi-tower && tar -czf /tmp/d.tgz config assets
scp /tmp/d.tgz root@$APARELHO:/tmp/
ssh root@$APARELHO 'cd /user/apps/giochi-tower && tar -xzf /tmp/d.tgz'

# DRM preso pelo launcher no ciclo de captura
ssh root@$APARELHO 'mkdir -p /run/giochi && touch /run/giochi/dev-free-console
  kill $(pgrep -x giochi-launcher)'
# ...e no fim, sempre:
bash scripts/aparelho.sh subir
```

Quando a issue for resolvida a montante, a sincronização do `vendor/` (ou a
imagem nova) é que tira este registo daqui — e a linha da tabela muda de estado.
