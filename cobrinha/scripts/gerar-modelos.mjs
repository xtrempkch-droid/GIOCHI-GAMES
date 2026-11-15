#!/usr/bin/env node
// Gera assets/modelos/maca.obj — a maçã low-poly do Cobrinha.
//
// O modelo é um OBJ de verdade (v/vt/vn e faces triangulares) para exercitar o
// caminho de malha arbitrária da engine 0.4. A textura é procedural, gerada pelo
// próprio jogo em `texturas.rs`; este arquivo só carrega as coordenadas.
//
// Uso: node scripts/gerar-modelos.mjs
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const raiz = join(dirname(fileURLToPath(import.meta.url)), '..');
const destino = join(raiz, 'assets', 'modelos', 'maca.obj');
mkdirSync(dirname(destino), { recursive: true });

const vertices = [];
const uvs = [];
const normais = [];
const faces = [];

function v(x, y, z) { vertices.push([x, y, z]); return vertices.length; }
function vt(u, w) { uvs.push([u, w]); return uvs.length; }
function vn(x, y, z) {
  const l = Math.hypot(x, y, z) || 1;
  normais.push([x / l, y / l, z / l]);
  return normais.length;
}
function f(a, b, c) {
  // Cada argumento é [posição, uv, normal]; a face guarda os três índices.
  faces.push([
    [a[0], b[0], c[0]],
    [a[1], b[1], c[1]],
    [a[2], b[2], c[2]],
  ]);
}

// --- corpo: esfera UV achatada, com a cova do cabo no topo -------------------
const aneis = 14;
const segmentos = 24;
const grade = [];

for (let i = 0; i <= aneis; i++) {
  const theta = (Math.PI * i) / aneis;
  const y = Math.cos(theta);
  const r = Math.sin(theta);
  const linha = [];
  for (let j = 0; j <= segmentos; j++) {
    const phi = (2 * Math.PI * j) / segmentos;
    const sx = r * Math.cos(phi) * (1.0 + 0.10 * Math.max(0, 1 - y * y));
    const sz = r * Math.sin(phi) * (1.0 + 0.10 * Math.max(0, 1 - y * y));
    const cova = 0.16 * Math.exp(-6.0 * (sx * sx + sz * sz)) * Math.max(0, y);
    const sy = 0.92 * y - cova;
    const u = 0.02 + 0.58 * (0.5 + Math.atan2(sz, sx) / (2 * Math.PI));
    const w = 0.02 + 0.86 * (0.5 - y * 0.5);
    linha.push({
      pos: v(sx, sy, sz),
      uv: vt(u, w),
      n: vn(sx, sy * 1.12, sz),
    });
  }
  grade.push(linha);
}

// Quads entre anéis (os polos ficam como leques, sem triângulos degenerados).
for (let i = 1; i < aneis; i++) {
  for (let j = 0; j < segmentos; j++) {
    const a = grade[i][j];
    const b = grade[i][j + 1];
    const c = grade[i + 1][j + 1];
    const d = grade[i + 1][j];
    f([a.pos, a.uv, a.n], [b.pos, b.uv, b.n], [c.pos, c.uv, c.n]);
    f([a.pos, a.uv, a.n], [c.pos, c.uv, c.n], [d.pos, d.uv, d.n]);
  }
}
// Leque do topo (anel 0 é um único ponto repetido).
for (let j = 0; j < segmentos; j++) {
  const a = grade[0][j];
  const b = grade[1][j];
  const c = grade[1][j + 1];
  f([a.pos, a.uv, a.n], [b.pos, b.uv, b.n], [c.pos, c.uv, c.n]);
}
// Leque do fundo.
for (let j = 0; j < segmentos; j++) {
  const a = grade[aneis][j];
  const b = grade[aneis - 1][j + 1];
  const c = grade[aneis - 1][j];
  f([a.pos, a.uv, a.n], [b.pos, b.uv, b.n], [c.pos, c.uv, c.n]);
}

// --- cabo: cilindro curto, mapeado na faixa castanha da textura ---------------
const lados = 8;
const alturaBase = 0.78;
const alturaTopo = 1.12;
const raioCabo = 0.035;
const anelBase = [];
const anelTopo = [];
for (let j = 0; j <= lados; j++) {
  const phi = (2 * Math.PI * j) / lados;
  const x = Math.cos(phi) * raioCabo + 0.015;
  const z = Math.sin(phi) * raioCabo;
  const u = 0.68 + 0.28 * (j / lados);
  anelBase.push({ pos: v(x, alturaBase, z), uv: vt(u, 0.04), n: vn(x, 0, z) });
  anelTopo.push({ pos: v(x * 0.7 + 0.03, alturaTopo, z * 0.7), uv: vt(u, 0.18), n: vn(x, 0, z) });
}
for (let j = 0; j < lados; j++) {
  const a = anelBase[j];
  const b = anelBase[j + 1];
  const c = anelTopo[j + 1];
  const d = anelTopo[j];
  f([a.pos, a.uv, a.n], [b.pos, b.uv, b.n], [c.pos, c.uv, c.n]);
  f([a.pos, a.uv, a.n], [c.pos, c.uv, c.n], [d.pos, d.uv, d.n]);
}
// Tampa superior e inferior, com a mesma faixa de textura.
const topoCentro = v(0.03, alturaTopo, 0);
const topoCentroUv = vt(0.82, 0.10);
const topoCentroN = vn(0, 1, 0);
const baseCentro = v(0.015, alturaBase, 0);
const baseCentroUv = vt(0.82, 0.04);
const baseCentroN = vn(0, -1, 0);
for (let j = 0; j < lados; j++) {
  const t0 = anelTopo[j];
  const t1 = anelTopo[j + 1];
  f([topoCentro, topoCentroUv, topoCentroN], [t1.pos, t1.uv, t1.n], [t0.pos, t0.uv, t0.n]);
  const b0 = anelBase[j];
  const b1 = anelBase[j + 1];
  f([baseCentro, baseCentroUv, baseCentroN], [b0.pos, b0.uv, b0.n], [b1.pos, b1.uv, b1.n]);
}

// --- folha: dois triângulos, dos dois lados ---------------------------------
const base = v(0.05, 1.02, 0.0);
const baseUv = vt(0.70, 0.36);
const baseN = vn(0, 1, 0);
const larga1 = v(0.16, 1.06, -0.09);
const larga1Uv = vt(0.78, 0.28);
const larga2 = v(0.18, 1.06, 0.10);
const larga2Uv = vt(0.78, 0.48);
const ponta = v(0.40, 1.12, 0.01);
const pontaUv = vt(0.96, 0.38);
const normalFolha = vn(0, 1, 0.1);
for (const [a, b, c, au, bu, cu] of [
  [base, larga1, ponta, baseUv, larga1Uv, pontaUv],
  [base, ponta, larga2, baseUv, pontaUv, larga2Uv],
  [base, ponta, larga1, baseUv, pontaUv, larga1Uv],
  [base, larga2, ponta, baseUv, larga2Uv, pontaUv],
]) {
  f([a, au, normalFolha], [b, bu, normalFolha], [c, cu, normalFolha]);
}

// --- escrita ---------------------------------------------------------------
const linhas = ['# maçã low-poly do Cobrinha — gerado por scripts/gerar-modelos.mjs'];
for (const [x, y, z] of vertices) linhas.push(`v ${x.toFixed(5)} ${y.toFixed(5)} ${z.toFixed(5)}`);
for (const [u, w] of uvs) linhas.push(`vt ${u.toFixed(5)} ${w.toFixed(5)}`);
for (const [x, y, z] of normais) linhas.push(`vn ${x.toFixed(5)} ${y.toFixed(5)} ${z.toFixed(5)}`);
linhas.push('g maca');
for (const [[pa, pb, pc], [ua, ub, uc], [na, nb, nc]] of faces) {
  linhas.push(`f ${pa}/${ua}/${na} ${pb}/${ub}/${nb} ${pc}/${uc}/${nc}`);
}
writeFileSync(destino, linhas.join('\n') + '\n');
console.log(`${destino}: ${vertices.length} vértices, ${faces.length} triângulos`);
