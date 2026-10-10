// node --test testes/*.test.mjs
// nota.js espelha src/modelo.rs: estes casos fixam a conta do lado do navegador.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { calcular, pontos, tom, opcoesDe, marcacaoRapida } from '../web/js/nota.js';
import { arredondar, num } from '../web/js/util.js';

const niveis = {
  id: 'c1',
  nome: 'Modularização',
  escala: {
    tipo: 'niveis',
    niveis: [
      { id: 'parcial', rotulo: 'Parcial', pontos: 1 },
      { id: 'otimo', rotulo: 'Ótimo', pontos: 3 },
      { id: 'nada', rotulo: 'Nada', pontos: 0 },
    ],
  },
};
const binaria = { id: 'c2', nome: 'Compila', peso: 2, escala: { tipo: 'binaria', pontos: 1 } };
const livre = { id: 'c3', nome: 'Relatório', escala: { tipo: 'livre', max: 5 } };
const passo = { id: 'p1', tipo: 'roteiro', nome: 'Rode o programa' };

const avaliacao = {
  nota_maxima: 10,
  rubricas: [
    { origem: 'a', peso: 1, rubrica: { criterios: [passo, niveis, binaria] } },
    { origem: 'b', peso: 3, rubrica: { criterios: [livre] } },
  ],
};

test('pontos: cada escala com a marcação dela; o resto não vale', () => {
  assert.equal(pontos(niveis, { tipo: 'nivel', nivel: 'parcial' }), 1);
  assert.equal(pontos(niveis, { tipo: 'nivel', nivel: 'sumiu' }), null);
  assert.equal(pontos(binaria, { tipo: 'binaria', atende: false }), 0);
  assert.equal(pontos(binaria, { tipo: 'nivel', nivel: 'otimo' }), null);
  assert.equal(pontos(livre, { tipo: 'nota', nota: 5 }), 5);
  assert.equal(pontos(livre, { tipo: 'nota', nota: 5.5 }), null);
  assert.equal(pontos(passo, { tipo: 'nota', nota: 1 }), null);
});

test('tom: máximo é sim, mínimo é não, o resto é meio', () => {
  assert.equal(tom(niveis, { tipo: 'nivel', nivel: 'otimo' }), 'sim');
  assert.equal(tom(niveis, { tipo: 'nivel', nivel: 'nada' }), 'nao');
  assert.equal(tom(niveis, { tipo: 'nivel', nivel: 'parcial' }), 'meio');
  assert.equal(tom(livre, null), '');
});

test('calcular: sem correção, nada respondido e nota zero', () => {
  const r = calcular(avaliacao, null);
  assert.deepEqual([r.respondidos, r.total, r.nota, r.concluida], [0, 3, 0, false]);
});

test('calcular: média das rubricas pelos pesos, passos de roteiro fora da conta', () => {
  const correcao = {
    respostas: {
      a: { c1: { tipo: 'nivel', nivel: 'otimo' }, c2: { tipo: 'binaria', atende: false } },
      b: { c3: { tipo: 'nota', nota: 2.5 } },
    },
  };
  const r = calcular(avaliacao, correcao);
  // Rubrica a: 3 de 3 + 2×1 = 3/5; rubrica b: 2,5/5. (1 × 0,6 + 3 × 0,5) ÷ 4 × 10 = 5,25.
  assert.deepEqual(r.rubricas, [{ pontos: 3, maximo: 5 }, { pontos: 2.5, maximo: 5 }]);
  assert.equal(r.concluida, true);
  assert.equal(arredondar(r.nota), 5.25);
});

test('opcoesDe: do melhor ao pior nível, com a pior na tecla 1', () => {
  const o = opcoesDe(niveis);
  assert.deepEqual(o.map((x) => [x.rotulo, x.tecla, x.tom]), [['Ótimo', 3, 'sim'], ['Parcial', 2, 'meio'], ['Nada', 1, 'nao']]);
  assert.deepEqual(o[0].marcacao, { tipo: 'nivel', nivel: 'otimo' });
  assert.deepEqual(opcoesDe(binaria).map((x) => [x.rotulo, x.tecla, x.pontos]), [['Atende', 2, 1], ['Não atende', 1, 0]]);
  assert.deepEqual(opcoesDe(livre), []);
});

test('marcacaoRapida: direita é o máximo, esquerda o mínimo', () => {
  assert.deepEqual(marcacaoRapida(niveis, true), { tipo: 'nivel', nivel: 'otimo' });
  assert.deepEqual(marcacaoRapida(niveis, false), { tipo: 'nivel', nivel: 'nada' });
  assert.deepEqual(marcacaoRapida(livre, false), { tipo: 'nota', nota: 0 });
});

test('arredondar: duas casas, meio para cima, mesmo com o fio do ponto flutuante', () => {
  assert.equal(arredondar(1.005), 1.01);
  assert.equal(arredondar(1.025), 1.03);
  assert.equal(arredondar(2.675), 2.68);
  assert.equal(arredondar(-1.005), -1.01);
  assert.equal(arredondar(7.123), 7.12);
  assert.ok(Object.is(arredondar(-0.001), 0));
  assert.equal(num(7.5), '7,5');
  assert.equal(num(NaN), '0');
});
