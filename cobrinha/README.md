# Cobrinha 3D

Jogo para os consoles Giochi, renderizado pela **GIOCHI-ENGINE 3D** com SDL2 e
OpenGL ES. O tabuleiro, as paredes, a cobra e a fruta são geometria 3D. O HUD,
as partículas, as texturas e o hot reload vêm da engine 0.5.

A cópia da engine usada pelo jogo fica em [`vendor/`](./vendor/), isolada do
repositório compartilhado `GIOCHI-ENGINE-3D`. O snapshot agora corresponde à
**0.5.0**, commit `f12c09b`, e inclui as fases 19–24 (hot reload, devtools, UI,
partículas, animação e física). Atualizações da equipe da engine não alteram
esta cópia automaticamente.

**Esta cópia tem uma alteração local, declarada para não se perder numa
resincronização:** `render3d/upscale.rs` e `render3d/renderer.rs` foram
estendidos para permitir **supersampling** (`MAX_SCALE` 1,0 → 2,0, com
`UNITY_SCALE` como identidade). O motivo e as medidas estão em
[Resolução](#resolução-800600-de-alvo-num-painel-de-640480). A 0.5.0 já traz o
outro lado do passe de upscale — a reposição do `glDepthMask` — que a 0.2.2 não
tinha e que era o **bug gráfico** descrito abaixo.

O desenho tem limite rígido configurável de **1 a 75 FPS** (padrão: 75); o
núcleo também limita o ritmo a 75. A simulação usa passo fixo, independente da
frequência de desenho. O jogo solicita GLES 3.0 por padrão e tenta GLES 2.0 se
não conseguir criar o contexto. GLES 2.0 é o requisito mínimo declarado para os
aparelhos Giochi.

## O bug gráfico corrigido

A 0.2.2 vendorizada desligava o `glDepthMask` no passe de upscale e **nunca o
religava**. O `glClear(GL_DEPTH_BUFFER_BIT)` do quadro seguinte era mascarado, o
depth buffer guardava a geometria do quadro anterior e, a partir do **segundo**
quadro, objetos mais distantes deixavam de aparecer — a cena parecia se
auto-ocultar. O primeiro quadro saía perfeito, então uma captura `--quadro`
isolada não pegava o defeito; era preciso jogar.

A 0.5.0 repõe o mask depois do blit (`upscale.rs`) e o `GlState::apply` o liga
em todo início de quadro (`context/state.rs`), que é a segunda rede de
segurança. O jogo mantém o supersampling **e** ganhou um teste de regressão:

```bash
cargo test -p giochi-cobrinha --test upscale_segundo_quadro
```

O teste desenha dois quadros a 1,25 (o padrão do jogo, não um valor de teste) e
exige que o cubo mais distante do segundo quadro apareça. Contra a 0.2.2 ele
falha com "o segundo quadro escalado está vazio"; contra a 0.5.0 passa.

## O que a engine 0.5 trouxe para o jogo

| Novidade da engine | Onde o Cobrinha usa |
| --- | --- |
| HUD `GlesHud` (egui sobre GLES 2.0) | `hud.rs` — pontos, recorde, estado e diagnóstico F1 |
| Partículas 3D (`BillboardParticleSystem`) | `efeitos.rs` — estouro da fruta comida, explosão da morte e brilho da fruta |
| Texturas (`create_texture_from_rgba` + `draw_mesh_textured`) | `recursos.rs` desenha a casca/cabo/folha da maçã em 128×128, sem PNG no pacote |
| Malha OBJ (`load_obj_mesh`) | `assets/modelos/maca.obj` — maçã low-poly gerada por `scripts/gerar-modelos.mjs` |
| Hot reload (`FileWatcher` + `ShaderReloader`) | `recarga.rs` — recarrega `assets/shaders/fruta.*` e o OBJ no mesmo handle |
| Profiler (`FrameCounters`) | HUD detalhado (F1) e campos `draws`/`vertices` no `--json` |
| Static batching e instancing | tabuleiro, paredes, corpo da cobra e dígitos do placar 3D |
| Upscaling | supersampling 1,25 (800×600 num painel 640×480), agora com depth correto |

Não são usados, e o motivo é o desenho do jogo — não falta de integração:

- **Física (Rapier 2D/3D)**: a cobrinha é de grade e passo discreto; um mundo
  físico não muda a regra e custaria ~2 MB no binário.
- **Rede (`giochi-net`)**: o jogo é de um jogador só.
- **C ABI**: o jogo é Rust nativo; a ABI serve a consumidores C.
- **UI 2D/animação de sprites**: o caminho 3D não usa `sdl2::render`
  (ADR-037); o HUD egui cobre o que a UI 2D faria.

## Resolução: 800×600 de alvo num painel de 640×480

O jogo **supersampleia** por padrão: o quadro é desenhado em **800×600** e
apresentado em 640×480. Foi pedido do dono do projeto — *"é um jogo leve, acho
que por ele em 800×600 daria para manter acima dos 60 fps ou 45 pelo menos"* —
e o número caiu exatamente no fator 1,25 (640 × 1,25 = 800).

```bash
giochi-cobrinha                 # padrão: alvo 800x600, apresenta no painel
giochi-cobrinha --escala 1      # desenha na resolução da tela
giochi-cobrinha --escala 2      # 1280x960 de alvo (4x o custo de fill)
```

`--escala` aceita **0,25 a 2,0**; abaixo de 1 reduz (mais FPS), acima
supersampleia (mais nitidez). O padrão é `1.25`.

### O que mudou na engine vendorizada

`Upscaler` só sabia **reduzir**: `MAX_SCALE` era `1.0` e `set_render_scale`
filtrava por `< MAX_SCALE`. Agora são `2.0`, existe `UNITY_SCALE = 1.0` como "sem
escala" (o filtro passou a ser `!= 1.0`) e `sanitise_scale` devolve **1,0** para
valor inválido — antes devolvia `MAX_SCALE`, que com o teto novo significaria
supersampling dobrado a partir de um `NaN`.

O caminho de desenho é o **mesmo** nos dois sentidos: alvo `800x600` → passe
final com filtro linear → `640x480`. Não há um segundo caminho de desenho para
envelhecer diferente.

### Medições anteriores (snapshot 0.2.2)

> As medições abaixo foram feitas antes da 0.5.0, sem HUD egui e sem
> partículas. O caminho de render é o mesmo, mas o custo do HUD (atlas de fonte
> e alguns draw calls) não está incluído. Meça de novo no aparelho com
> `--fps-teto 75 --quadros 600 --json`: o campo `draws` diz quantas draw calls
> o quadro custou.

```text
--escala 1.0   49,3 fps    521 cores distintas no quadro
--escala 1.25  49,2 fps  4 604 cores distintas no quadro
```

**Custo: nenhum mensurável** (−0,2 %, dentro do ruído), e o ganho é real: os
dois quadros diferem em **13,41 %** dos pixels, e as cores distintas vão de 521
para 4 604 — é a antialiasing das bordas, que antes eram degrau puro.

No PC, com HUD, partículas e textura (build debug, 640×480 apresentado a partir
de 800×600), o laço fica em **~74 FPS** com teto de 75; o número de referência
do aparelho precisa ser medido de novo.

### `--quadro` lia o alvo intermediário

Com escala de render o `drawable_size()` do renderer é o **alvo** (800×600), e
`--quadro` gravava um BMP de 800×600 chamando `read_pixels` de uma janela de
640×480 — região fora da tela. A captura usa o tamanho da **janela**: o BMP sai
sempre do tamanho do que é apresentado. `--quadro` com `--quadros N` captura o
**último** dos N quadros, que é o que permite ver o depth corrigido e as
partículas.

## Construir e testar

```bash
# Na raiz do repositório
bash scripts/compilar-jogo-arm64.sh --jogo cobrinha

# Testes do jogo (72 no total)
cd app/jogos/cobrinha
cargo test --workspace
cargo clippy --workspace --all-targets

# Build sem egui/hot reload: placar 3D de cubos, binário menor
cargo build -p giochi-cobrinha --no-default-features

# Testes do snapshot da engine (GLES/offscreen)
cd vendor
SDL_VIDEODRIVER=offscreen cargo test -p giochi-engine-3d \
  --no-default-features --features hud,sdl2-system \
  --test upscale --test instancing --test static_batch --test mesh_pipeline \
  --test lighting --test context_smoke --test textures_3d --test obj_models \
  --test hud_render
```

O script de cross-compile resolve o SDL2 do aparelho. Não compile nem altere
arquivos no repositório separado da engine; a dependência local está em
`vendor/`. O `vendor/assets/` é o mínimo que os testes da engine leem
(`checker.png`); os assets do jogo ficam em `assets/`.

No aarch64, o release com HUD/egui/hot reload mede **2,9 MB** e continua a
precisar só de `libSDL2` (verificado com `readelf -d`). Com
`--no-default-features` — placar 3D de cubos, sem egui e sem `notify` — mede
**0,64 MB**. O custo do HUD é quase todo o atlas de fontes do egui; num pacote
apertado, é a primeira coisa a cortar.

## Diagnóstico

```bash
giochi-cobrinha --versao
giochi-cobrinha --fps-teto 75 --quadros 150 --json
giochi-cobrinha --escala 1.25 --quadro /tmp/cobrinha.bmp   # alvo 800x600
giochi-cobrinha --quadros 240 --quadro /tmp/fim.bmp        # último quadro
giochi-cobrinha --tamanho 640x480 --quadro /tmp/cobrinha.bmp
giochi-cobrinha --tamanho 720x1280 --quadro /tmp/cobrinha-retrato.bmp
giochi-cobrinha --sem-hud --quadro /tmp/placar3d.bmp       # placar 3D
giochi-cobrinha --perfil --quadro /tmp/diag.bmp            # HUD detalhado
giochi-cobrinha --sem-textura --quadro /tmp/cru.bmp        # fruta em cor sólida
GIOCHI_GL_VERSION=2 giochi-cobrinha
GIOCHI_ROTATE=ccw giochi-cobrinha
GIOCHI_HOT_RELOAD=1 giochi-cobrinha                        # vigia em release
```

`--fps-teto` aceita apenas valores entre 1 e 75. `--quadros` termina após o
número solicitado; `--json` relata quadros, FPS médio observado, GLES ativo,
instancing, draw calls, vértices, partículas, HUD e escala. `--quadro` grava o
back buffer antes do swap. `--tamanho` simula a dimensão do painel e evita usar
a orientação do sistema de teste.

`GIOCHI_ASSETS` aponta a raiz de assets; sem ela a procura é pelo cwd, pelo
diretório do executável e por `bin/../assets`. Se o OBJ não carregar, o jogo
avisa e usa a esfera embutida; se o shader não compilar, usa o `textured`
embutido. O jogo não deixa de abrir por causa de um asset.

## Arquitetura

```text
crates/core
├── jogo.rs       regras, colisão, crescimento e RNG determinístico
├── relogio.rs    simulação em passo fixo e limite de 75 FPS
└── entrada.rs    códigos evdev → ações

crates/ui-sdl
├── main.rs       SDL, contexto GLES, laço limitado e CLI
├── cena3d.rs     tabuleiro em static batches, malha OBJ/texturizada e instancing
├── efeitos.rs    partículas 3D (comida, morte e brilho)
├── hud.rs        placar e diagnóstico em egui/GLES; sem a feature, placar 3D
├── recarga.rs    hot reload de shader e OBJ (opt-in em release)
├── recursos.rs   procura de assets e textura procedural da maçã
├── video.rs      orientação declarada pelo sistema
└── evdev.rs      leitura em thread separada e fila SPSC

assets/
├── modelos/maca.obj
└── shaders/fruta.vert|frag

scripts/gerar-modelos.mjs   gera o OBJ (node)

vendor
├── assets/                 checker/player usados pelos testes da engine
├── giochi-engine
└── giochi-engine-3d
```

O tabuleiro imutável é enviado em dois lotes estáticos; segmentos da cobra e
segmentos do placar usam instancing quando o contexto oferece suporte. Sem
instancing, o jogo desenha cada mesh individualmente. As regras e a entrada
evdev permanecem independentes da renderização.

O HUD é desenhado depois da cena e antes do `end_frame`, dentro do alvo de
render: com supersampling o texto também é reduzido no passe final. O primeiro
quadro roda o egui duas vezes porque o primeiro `run` monta o atlas de fontes;
sem isso o HUD piscaria ausente no arranque.

## Controles

| Entrada | Ação |
| --- | --- |
| direcional (544–547) | muda a direção |
| A (304) / START (315) | começa ou recomeça |
| B (305) / SELECT (314) | sai |
| setas, Espaço, Esc, Q (PC) | controles equivalentes |
| F1 (PC) | abre/fecha o diagnóstico do HUD |

Até três mudanças de direção podem ficar enfileiradas para que viradas rápidas
sejam aplicadas em passos sucessivos, sem aceitar uma inversão de 180°.
