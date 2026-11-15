#!/usr/bin/env node
// Gera os modelos `.obj` do Neon Drift: lataria do carro e portal/checkpoint.
//
// ## Por que gerar em vez de desenhar à mão
//
// Os dois modelos são loft/varredura com parâmetros (seções do carro, raio do
// anel). Gerar mantém a **orientação das faces** consistente entre peças, que é
// o defeito invisível deste tipo de arquivo: com winding invertido a peça
// continua aparecendo (você vê o interior do lado oposto) e só a iluminação
// denuncia. O teste `a_lataria_tem_normais_para_fora` em `modelos.rs` confere o
// resultado, então este script e o teste se cobrem.
//
// ## Convenções
//
// * carro: nariz em **-Z**, teto em **+Y**, largura em X (igual a `malha.rs`);
// * portal: anel no plano **XY**, normal **+Z**; o jogo gira para ficar em pé
//   sobre a pista;
// * toda face é **triângulo** e o winding é anti-horário visto de **fora**
//   (GL_CCW, o padrão do `RenderContext3D`).
//
// Uso: node scripts/gerar-modelos.mjs

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const raiz = join(dirname(fileURLToPath(import.meta.url)), "..");
const destino = join(raiz, "assets", "modelos");
mkdirSync(destino, { recursive: true });

/** Escreve um OBJ: vértices e faces já trianguladas, sem `vn`/`vt`. */
function escrever(nome, comentario, vertices, faces) {
  const linhas = [comentario, "# gerado por scripts/gerar-modelos.mjs — não editar à mão"];
  for (const [x, y, z] of vertices) {
    linhas.push(`v ${x.toFixed(4)} ${y.toFixed(4)} ${z.toFixed(4)}`);
  }
  for (const face of faces) {
    linhas.push(`f ${face.join(" ")}`);
  }
  writeFileSync(join(destino, nome), linhas.join("\n") + "\n");
  console.log(`${nome}: ${vertices.length} vértices, ${faces.length} triângulos`);
}

/** Triangula um quadrilátero mantendo o winding: (a,b,c,d) → (a,b,c) + (a,c,d). */
function quad(a, b, c, d) {
  return [
    [a, b, c],
    [a, c, d],
  ];
}

// ────────────────────────────────────────────────────────────
// Carro: sete seções ao longo de Z, quatro cantos cada
// ────────────────────────────────────────────────────────────
{
  // [z, meia-largura, y do assoalho, y do teto]
  const secoes = [
    [-2.2, 0.38, 0.3, 0.42],
    [-1.7, 0.78, 0.22, 0.6],
    [-0.9, 0.96, 0.2, 0.76],
    [0.0, 0.96, 0.2, 0.8],
    [1.0, 0.93, 0.2, 0.82],
    [1.9, 0.8, 0.24, 0.88],
    [2.2, 0.55, 0.32, 0.8],
  ];

  const vertices = [];
  const aneis = [];
  for (const [z, hw, yb, yt] of secoes) {
    // Ordem anti-horária vista de +Z: base-esq, base-dir, topo-dir, topo-esq.
    const base = vertices.length + 1;
    vertices.push([-hw, yb, z], [hw, yb, z], [hw, yt, z], [-hw, yt, z]);
    aneis.push([base, base + 1, base + 2, base + 3]);
  }

  const faces = [];
  for (let i = 0; i + 1 < aneis.length; i++) {
    const a = aneis[i];
    const b = aneis[i + 1];
    for (let k = 0; k < 4; k++) {
      const k2 = (k + 1) % 4;
      // (a_k, a_k+1, b_k+1, b_k) é o winding que aponta **para fora** do tubo
      // quando o anel é anti-horário visto de +Z. A ordem trocada — que é a
      // intuitiva — aponta para dentro e some com a face sob `cull_face`.
      faces.push(...quad(a[k], a[k2], b[k2], b[k]));
    }
  }
  // Tampa do nariz (-Z): horário visto de +Z = anti-horário visto de -Z.
  faces.push(...quad(aneis[0][0], aneis[0][3], aneis[0][2], aneis[0][1]));
  // Tampa da traseira (+Z).
  faces.push(...quad(aneis.at(-1)[0], aneis.at(-1)[1], aneis.at(-1)[2], aneis.at(-1)[3]));

  escrever(
    "carro.obj",
    "# Neon Drift — lataria low-poly (nariz em -Z, teto em +Y)",
    vertices,
    faces,
  );
}

// ────────────────────────────────────────────────────────────
// Portal: toro no plano XY + duas colunas
// ────────────────────────────────────────────────────────────
{
  const MAJOR = 20;
  const MINOR = 6;
  const R = 4.6;
  const r = 0.28;

  const vertices = [];
  const faces = [];

  for (let i = 0; i < MAJOR; i++) {
    const u = (2 * Math.PI * i) / MAJOR;
    for (let j = 0; j < MINOR; j++) {
      const v = (2 * Math.PI * j) / MINOR;
      const raio = R + r * Math.cos(v);
      vertices.push([raio * Math.cos(u), raio * Math.sin(u), r * Math.sin(v)]);
    }
  }
  const idx = (i, j) => (i % MAJOR) * MINOR + (j % MINOR) + 1;
  for (let i = 0; i < MAJOR; i++) {
    for (let j = 0; j < MINOR; j++) {
      // (i,j) → (i+1,j) → (i+1,j+1) tem normal **para fora** do toro:
      // cross(d/du, d/dv) aponta na direção radial.
      faces.push(...quad(idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)));
    }
  }

  /** Caixa com as seis faces para fora (winding por face, não por convenção). */
  function caixa(cx, cy, cz, hx, hy, hz) {
    const base = vertices.length + 1;
    vertices.push(
      [cx - hx, cy - hy, cz - hz], // 0
      [cx + hx, cy - hy, cz - hz], // 1
      [cx + hx, cy + hy, cz - hz], // 2
      [cx - hx, cy + hy, cz - hz], // 3
      [cx - hx, cy - hy, cz + hz], // 4
      [cx + hx, cy - hy, cz + hz], // 5
      [cx + hx, cy + hy, cz + hz], // 6
      [cx - hx, cy + hy, cz + hz], // 7
    );
    const v = (n) => base + n;
    faces.push(...quad(v(4), v(5), v(6), v(7))); // +Z
    faces.push(...quad(v(1), v(0), v(3), v(2))); // -Z
    faces.push(...quad(v(0), v(4), v(7), v(3))); // -X
    faces.push(...quad(v(5), v(1), v(2), v(6))); // +X
    faces.push(...quad(v(5), v(1), v(0), v(4))); // -Y
    faces.push(...quad(v(7), v(6), v(2), v(3))); // +Y
  }

  caixa(-R - 0.55, 0.5, 0, 0.45, 0.5, 0.45);
  caixa(R + 0.55, 0.5, 0, 0.45, 0.5, 0.45);

  escrever(
    "portal.obj",
    "# Neon Drift — portal/checkpoint (anel no plano XY, normal +Z)",
    vertices,
    faces,
  );
}
