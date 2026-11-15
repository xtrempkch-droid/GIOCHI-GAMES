# Texturas e sprites do Neon Drift — guia para desenhar as suas

> Este documento é para quem vai **pintar** textura e sprite do jogo. Ele diz o
> tamanho exato, o que cada pixel significa e onde o arquivo entra. Os números
> aqui não são recomendação: são os que o código lê hoje, e mudá-los sem mudar o
> código faz a textura aparecer esticada, tremida ou invisível.

## Formatos: BMP sempre funciona, PNG depende da imagem

O R35S de 07/11 tinha o `libSDL2_image` compilado **sem** `--enable-png` e o
`IMG_Load` respondia *"PNG images are not supported"* (`AGENTS.md` §24). A
imagem de 09/11 corrigiu o `defconfig` e passou a trazer `libpng16.so.16`.

Por isso o jogo **aceita os dois** e cai para o procedural quando o arquivo não
carrega — nunca em silêncio, sempre com o motivo no log.

| Onde | Formato aceito | Variável |
| --- | --- | --- |
| texturas de superfície | `png`, `bmp`, `jpg`, `jpeg` | `GIOCHI_TEXTURAS=<dir>` |
| sprites de partícula | `png`, `bmp`, `jpg`, `jpeg` | `GIOCHI_SPRITES=<arquivo>` |
| captura e conferência | BMP | `--texturas`, `--atlas`, `--quadro` |

A ordem de preferência é **PNG primeiro** (`texturas::EXTENSOES`): quem exporta
o mesmo material nos dois formatos quer o PNG, que não perde qualidade. Se o
aparelho não decodificar PNG, o jogo avisa e usa o procedural — não quebra.

```bash
GIOCHI_TEXTURAS=~/minhas-texturas giochi-neon-drift      # asfalto.png, rocha.png, ...
GIOCHI_SPRITES=~/minha-folha.png giochi-neon-drift       # folha 4x2 ou 8x1
```

O log na abertura diz de onde veio cada coisa:

```text
neon-drift: 9 textura(s) [...] 3 de arquivo · 6 procedurais, 64 x 64 de receita
neon-drift: máscaras de luz 'halo-neon' e 'cone-farol' (64 x 64, 32768 bytes, só alfa)
neon-drift: atlas de sprites 128x128 [...] (origem: folha externa (PNG/JPEG/BMP via SDL_image))
```

---

## Parte 1 — Texturas de superfície

### A ficha técnica

| Item | Valor | Por quê |
| --- | --- | --- |
| tamanho | **64×64 px** | `texturas::LADO`; potência de dois é exigência do GLES 2.0 para `REPEAT` — 96×96 funciona no Mesa e falha em driver embarcado |
| canais | **RGBA8**, 4 bytes por pixel | 16 384 bytes por textura |
| escala no mundo | **1 textura = 3,0 m** | `malha::METROS_POR_TILE` |
| endereçamento | `REPEAT` | o chão tem 2 081 m: a textura repete ~694 vezes |
| filtro | `LINEAR` + mipmap (`LINEAR_MIPMAP_LINEAR`) | potência de dois é o que o GLES 2.0 exige para gerar mip e usar `REPEAT`; o mip é o que impede o asfalto de tremer ao longe |
| cor | **cinza-claro e neutro** | a cor final é `amostra × tint` (ver abaixo) |
| UV | **por posição no mundo** | não 0..1 por quadrilátero |

### A regra do cinza-claro, e por que ela existe

A textura **não** traz a cor do material: ela traz o **relevo**, e o `tint` traz a
cor. `final = amostra × tint`.

Isso foi aprendido errado na primeira tentativa. As texturas nasceram escuras, no
tom do material (asfalto a 0,06), e o `tint` também era escuro: escuro × escuro =
preto. A serra ficou com **28 % do quadro quase preto**. A correção foi clarear a
**textura**, não o `tint` — porque o `tint` é o que muda de mapa para mapa e a
textura é compartilhada.

Consequência prática para quem pinta:

- **faixa útil: 0,45 a 0,95 de luminância.** Abaixo de 0,35 o material afunda no
  preto noturno; acima de 0,98 ele estoura e perde o desenho.
- **contraste dentro da textura** é o que vira relevo. Um asfalto liso de 0,70
  uniforme desenha um plano chapado; granulado entre 0,55 e 0,80 desenha
  asfalto.
- **nunca pinte a cor do material.** Verde de mata na textura × tint verde =
  verde duas vezes, e o resultado é uma massa preta esverdeada.

Os `tint` em uso hoje (`cena3d.rs`):

| Material | `tint` | Textura |
| --- | --- | --- |
| asfalto | `0.155 0.160 0.215` | `asfalto` |
| concreto | `0.44 0.46 0.56` | `concreto` |
| pedra | `0.52 0.50 0.48` | `rocha` |
| mata | `0.34 0.60 0.34` | `mata` |
| guarda-corpo | `0.62 0.64 0.70` | `guarda_corpo` |
| painel de neon | `1.0 0.55 0.92` | `painel` |
| xadrez da largada | `0.95 0.95 1.0` | `xadrez` |

### As nove texturas e onde cada uma aparece

| Textura | O que desenha | Regra de desenho |
| --- | --- | --- |
| `asfalto` | o piso inteiro | granulado fino + emendas ocasionais; **sem faixa pintada** (a faixa central é geometria separada) |
| `concreto` | mureta, laje de passarela, parede de túnel, torre | manchas largas e juntas a cada 16 px; é o material mais visto de perto |
| `rocha` | paredão da serra | estratos horizontais irregulares, claros e escuros alternados |
| `mata` | copa de árvore vista de longe | massas verdes com pontos claros (reflexo de folha); **não é folha**, é copa |
| `guarda_corpo` | chapa do guarda-corpo da serra | nervura vertical a cada 8 px, sombra de um lado e luz do outro — passando depressa é metade da sensação de velocidade |
| `painel` | outdoor de neon | gradiente diagonal + grade fina; o `tint` é forte, então aqui a textura deve ser **clara** |
| `xadrez` | faixa de largada | 8×8 quadrados alternados, puro preto e branco |
| `faixa-pista` | tracejado central e faixas de rampa | **decalque**: fundo quase preto (é o asfalto) e traço branco; o tracejado vem da textura, não de geometria |
| `grade-neon` | fita de advertência no topo das barreiras | **decalque**: grade ortogonal branca a cada 16 px; ortogonal e não diagonal porque a fita é rasante e a diagonal se perde na perspectiva |

### As duas máscaras de luz (e por que não estão na lista acima)

A poça de neon embaixo de cada carro e o cone do farol no asfalto usam textura,
mas **não** são materiais: o RGB é branco em toda a imagem e quem desenha é o
**alfa**. Elas ficam fora de `texturas::todas()` e o `GIOCHI_TEXTURAS` não as
alcança — um halo pintado à mão perderia a única propriedade que importa (o alfa
cair do ponto quente até a borda sem degrau), e o sintoma de trocá-las por um
material é conhecido: a poça vira um retângulo de borda dura.

| Máscara | Fórmula | O que ela desenha |
| --- | --- | --- |
| `halo-neon` | `(1 − r)^1,2` | elipse da luz indireta sob o carro; `r` é a distância normalizada ao centro, e como o quad tem a proporção do carro a elipse sai alongada no sentido certo |
| `cone-farol` | `lateral × alcance` | cone do farol: meia-largura `0,45 + 0,55·v` (estreito no para-choque, aberto na ponta) e queda `(1 − v)^1,15` |

As duas são 64×64 (potência de dois, então mipmap e `REPEAT` automáticos) e
sobem por `create_texture_from_rgba`, como as outras. O **expoente baixo** é
proposital: luz de neon não tem bico, ela some devagar; um expoente alto
desenharia um ponto brilhante no meio do carro e nada em volta, que lê como
adesivo.

A cor vem do `u_tint` do desenho: a poça usa a cor do carro — e no drift ela
puxa 40 % para o branco e o alfa sobe de 0,46 para 0,86 —, e o farol usa um
branco-frio fixo para todos. Ver a seção "A luz que os carros jogam no chão" no
[README](./README.md#a-luz-que-os-carros-jogam-no-chão).

### O UV vem do mundo, não do quadrilátero

`chao3` calcula o UV da **posição no mundo** dividida pelo `tile`. Com UV 0..1 por
segmento, a textura reiniciaria a cada 4 m de asfalto e a emenda apareceria como
uma faixa correndo ao lado do carro. É por isso que o piso é **uma** malha de
2 560 vértices em vez de 480 quadriláteros independentes.

`caixa_texturizada` faz o mesmo por face (`largura / tile`), e `parede3` estica ao
longo da tira. Quem pinta não precisa saber disso — só precisa saber que a
textura é **contínua por 3 m** e que a escala é sempre a mesma em qualquer malha.

### Como conferir o que você fez

```bash
giochi-neon-drift --texturas /tmp/texturas.bmp
```

Sai uma tira horizontal com as **nove** texturas de 64×64 na ordem de
`texturas::todas()`, 4 px de separação e fundo cinza-escuro (616×72 com as nove). É o molde: pinte
dentro da célula de 64×64 e a densidade já está certa.

Para olhar uma superfície no jogo, isole a malha:

```bash
giochi-neon-drift --mapa boavista --tela corrida --piloto --passos 600 \
                  --so asfalto_t --quadro /tmp/asfalto.bmp
```

O `--so` aceita `asfalto_t`, `concreto_t`, `pedra_t`, `mata_t`, `guarda_t`,
`painel_t`, `xadrez_t` (nome da malha) e `asfalto`, `concreto`… (nome da textura,
via `GIOCHI_SEM`).

### Onde mexer no código

| Quero… | Arquivo | O que fazer |
| --- | --- | --- |
| mudar o desenho de uma textura | `crates/ui-sdl/src/texturas.rs` | reescrever a função (`asfalto()`, `mata()`…) |
| mudar a cor de um material | `crates/ui-sdl/src/cena3d.rs` | a constante `ASFALTO`, `CONCRETO`… |
| mudar a escala no mundo | `crates/ui-sdl/src/malha.rs` | `METROS_POR_TILE` — afeta **todas** as texturas |
| acrescentar uma textura | `texturas.rs` + o `struct TexturasGpu` em `cena3d.rs` | função nova, campo novo, `criar()` e um braço no laço de desenho |
| mudar o halo da poça ou o cone do farol | `crates/ui-sdl/src/texturas.rs` | `halo_neon()` e `cone_farol()` — são máscaras de **alfa**, e o RGB tem de ficar branco |
| mudar o tamanho ou o brilho da luz do carro | `crates/ui-sdl/src/cena3d.rs` | `POCA_*`, `FEIXE_*`, `tinta_da_poca` e `rampa_de_drift` |
| trocar a geometria que a usa | `crates/ui-sdl/src/malha.rs` | `chao3`, `caixa_texturizada`, `parede3` |

**Orçamento:** textura não custa chamada de desenho — custa área de tela. As nove
somadas ocupam 147 456 bytes na GPU (as duas máscaras entram com 32 768) e
nenhum draw call extra **por material**. O que pesa é **quantos pixels do
quadro** cada superfície cobre: o asfalto é o quadro inteiro embaixo do carro, e
por isso ele é o material com maior orçamento de detalhe.

As máscaras são a exceção à regra "textura não custa desenho", e é bom saber
por quê: o `u_tint` do desenho instanciado vale para o lote inteiro, então a
poça de cada carro custa uma chamada (uma por cor). `agrupar_por_tinta` funde as
cores repetidas, e no LAN isso acontece de verdade — a paleta tem quatro cores
para até oito carros.

---

## Parte 2 — Sprites de partícula

### A ficha técnica

| Item | Valor |
| --- | --- |
| atlas | **128×128 px** |
| grade | **4×4 células** de 32×32 (só as 8 primeiras são usadas) |
| canais | **RGBA8** |
| RGB | **branco em toda parte** (`255,255,255`) |
| alfa | é o que desenha |
| margem interna | **1e-4** de encolhimento por célula |

### Branco, não cinza: a cor vem do `tint`

Uma nuvem de fumaça nasce branca, fica cinza e some — sem uma textura por cor. O
que muda é o **tint** da instância e a **faixa de vida** (o alfa de desenho), não
o pixel. Se você pintar RGB colorido, o tint multiplica e a cor sai dobrada.

O RGB **não** é lixo: ele existe para que a folha externa de 24 bits possa ser
lida (ver Parte 3). Internamente só o `A` importa.

### A margem de 1e-4, e o risco que ela evita

O retângulo de UV de cada célula é encolhido 1e-4 para dentro. Sem isso, a
amostragem `LINEAR` na borda puxa o texel da **célula vizinha** e cada sprite
ganha um risco reto atravessando a forma — visível como uma linha de 1 px que
aparece e some conforme a partícula gira.

Ao pintar uma célula, mantenha ~1 px de margem transparente em volta. É a mesma
proteção pelo lado da arte.

### As oito formas

| Índice | Forma | Onde aparece |
| --- | --- | --- |
| 0 | `puff` | fumaça de pneu no drift — a única que **desbota** |
| 1 | `gota` | respingo de chuva no asfalto |
| 2 | `faisca` | raspão na mureta e batida |
| 3 | `risco` | rastro de nitro (esticado ao longo da velocidade) |
| 4 | `chuva` | gota caindo (esticada na velocidade relativa) |
| 5 | `halo` | mote de neon flutuando na cidade |
| 6 | `borrao` | marca de pneu deitada no chão |
| 7 | `anel` | reservada (fragmento de vidro) |

A ordem é **contrato**: o índice é o que a partícula guarda no estado e o que o
desenho usa para escolher a célula. Trocar duas formas de lugar troca a arte de
todos os efeitos que as usam.

### A forma certa para o espaço certo

- **redonda e cheia** (`puff`, `borrao`, `halo`): desenhada a 1,0× — já encosta na
  borda da célula.
- **redonda e pequena** (`gota`): o raio útil é **0,72 do meio-quadrado**, e o
  desenho a amplia **1,30×** para compensar. Se você pintar uma gota que já ocupa
  a célula inteira, ela sai 30 % maior que as outras.
- **alongada** (`risco`, `chuva`, `faisca`): o jogo estica o quadrilátero ao longo
  da velocidade, então pinte-a **fina e centrada**; o alongamento é do shader.

### Como conferir

```bash
giochi-neon-drift --atlas /tmp/atlas.bmp
```

O fundo sai **magenta**, de propósito: alfa 0 sobre preto é indistinguível de
"não desenhou". Se a célula aparecer magenta inteira, a forma está vazia; se
aparecer com borda dura, você esqueceu de suavizar o alfa.

Prova de que o sprite está sendo **amostrado** (e não desenhado como quadrado):

```bash
GIOCHI_SEM=mundo,carros,hud giochi-neon-drift --tela corrida --piloto \
    --passos 400 --quadro /tmp/part.bmp
```

Um quadrilátero sólido dá 2 ou 3 níveis de luminância e ~0 % de rampa. O atlas
procedural, medido nesse comando, dá **114 níveis e 86,2 % de pixels em rampa**
(o número exato varia com o quadro; o que importa é a ordem de grandeza — dezenas
de níveis, não dois). Se o seu dá 3, o alfa está binário.

### Onde mexer no código

| Quero… | Arquivo |
| --- | --- |
| mudar o desenho de uma forma | `crates/ui-sdl/src/atlas.rs`, a função `alfa_da_forma` |
| mudar o ganho de tamanho | `atlas.rs`, `Forma::ganho` |
| mudar o UV da célula | `atlas.rs`, `Forma::retangulo` |
| trocar qual forma cada partícula usa | `crates/ui-sdl/src/particulas.rs`, `Marca` |
| mudar cor, gravidade, arrasto | `particulas.rs`, a tabela por tipo |

---

## Parte 3 — Usar a sua própria folha de sprites

```bash
GIOCHI_SPRITES=minha-folha.bmp GIOCHI_SPRITES_COLUNAS=8 giochi-neon-drift
```

O jogo lê o BMP, **normaliza** para o mesmo atlas 128×128 de 4×4 e segue como se
fosse o procedural. Depois da normalização, `retangulo()`, `quad_celula()` e o
desenho instanciado não sabem de onde os pixels vieram — não existe um segundo
caminho de desenho para envelhecer diferente.

### Formato do arquivo

| Item | Aceito | Recusado |
| --- | --- | --- |
| formato | BMP `BITMAPINFOHEADER` (`40` bytes) | qualquer outro cabeçalho |
| compressão | **nenhuma** (`BI_RGB`) | `BI_RLE8`, `BI_RLE4`, `BI_BITFIELDS` |
| bits | **24 ou 32** | 8, 4, 1 |
| altura | positiva (de baixo para cima) ou **negativa** (de cima para baixo) | zero |
| alinhamento | o leitor trata as linhas alinhadas em 4 bytes | — |
| grade | `colunas` × `ceil(8 / colunas)` | mais formas que células |

`GIOCHI_SPRITES_COLUNAS` padrão é **4**. Uma tira 8×1 (`256×32`) entra igual a
uma grade 4×2 (`128×64`): a célula *i* da folha vai para a posição da forma *i*.

### 24 bits não tem alfa — a luminância é a máscara

Num BMP de 32 bits o alfa vem junto. Num de 24 bits não existe, e a
**luminância** vira opacidade: **branco opaco, preto transparente** — a convenção
das folhas de efeito. Sem isso, todo sprite de uma folha 24 bits sairia como
retângulo cheio.

Pratique com fundo preto e desenhe a forma em branco. Escala de cinza no meio vira
semi-transparência, que é o que faz a borda da fumaça desbotar em vez de recortar.

### Quando a folha é ignorada, o motivo aparece

```text
neon-drift: folha de sprites ignorada — não consegui abrir /tmp/x.bmp: No such file or directory
neon-drift: atlas de sprites 128x128 [...] (origem: procedural)
```

Cair em silêncio faria você concluir que a sua folha não serve, quando o que
falhou foi o caminho, o formato ou o número de colunas.

E `--atlas` respeita as duas variáveis: ferramenta que mostra o procedural
enquanto o jogo desenha a folha do usuário mede outra coisa.

### Três armadilhas de BMP, todas com teste

| Armadilha | Sintoma |
| --- | --- |
| altura **negativa** = de cima para baixo | folha de cabeça para baixo (invisível num puff, óbvio num risco) |
| linhas alinhadas em 4 bytes | imagem **cisalhada**, cada linha deslocada um pouco mais |
| BMP comprimido | aceitar e ler como cru desenha lixo |

---

## Parte 4 — O que **não** existe hoje, para não perder tempo

Não é falta de aviso: está declarado aqui para ninguém procurar o recurso.

- **As máscaras de luz não aceitam arquivo.** `GIOCHI_TEXTURAS` troca as nove
  superfícies, mas o halo da poça e o cone do farol são recusados de propósito:
  elas são gradientes de alfa, não desenhos. Ver a seção das máscaras acima.
- **Não há folha de sprite com mais de 8 formas.** `Forma::TODAS` tem 8 e o atlas
  é 4×4; uma folha com 16 formas entraria nas células certas, mas o jogo só sabe
  pedir as 8.
- **Não há textura animada** (sprite sheet de 2 quadros, por exemplo). A célula é
  geometria (UV embutido no quadrilátero), não uniforme — animar exigiria trocar
  o UV por quadro.

> Dois itens que estavam nesta lista **deixaram de ser verdade** na modernização
> para a engine 0.4.0, e ficam registrados para quem leu a versão antiga: textura
> de superfície **pode** vir de arquivo (`GIOCHI_TEXTURAS`, acima) e **há
> mipmap** — todas as superfícies são potência de dois, que é o que o GLES 2.0
> exige para `LINEAR_MIPMAP_LINEAR` e `REPEAT`; o atlas de sprites é a exceção
> deliberada, sem mipmap, para uma célula não puxar o texel da vizinha.

---

## Resumo de uma linha por arquivo

```text
texturas.rs   64x64 RGBA: 9 materiais (cor vem do tint) + 2 máscaras de alfa
atlas.rs      128x128, grade 4x4 de 32px, RGB branco, alfa desenha — 8 formas
malha.rs      METROS_POR_TILE = 3.0; chao3/caixa_texturizada/parede3 fazem o UV
cena3d.rs     os tints (ASFALTO, CONCRETO, ...), os decalques de luz e qual malha usa qual textura
particulas.rs qual forma cada tipo de partícula usa, e cor/gravidade/arrasto
```
