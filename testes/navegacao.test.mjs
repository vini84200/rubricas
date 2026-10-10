// node --test testes/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { primeiroSemResposta, passosAntes, contarCriterios, mesmaMarcacao } from '../web/js/telas/avaliar/navegacao.js';

const criterio = (id) => ({ id, nome: id, escala: { tipo: 'binaria', pontos: 1 } });
const passo = (id) => ({ id, nome: id, tipo: 'roteiro' });
// p1, c1, p2, p3, c2, c3
const itens = [passo('p1'), criterio('c1'), passo('p2'), passo('p3'), criterio('c2'), criterio('c3')].map((c) => ({ origem: 'r', rubrica: 'R', criterio: c }));
const marcado = (...ids) => ({ respostas: { r: Object.fromEntries(ids.map((id) => [id, { tipo: 'binaria', atende: true }])) } });

test('primeiroSemResposta para no roteiro de quem falta, ou no resumo', () => {
  assert.equal(primeiroSemResposta(itens, null), 0);
  assert.equal(primeiroSemResposta(itens, marcado('c1')), 2);
  assert.equal(primeiroSemResposta(itens, marcado('c1', 'c2')), 5);
  assert.equal(primeiroSemResposta(itens, marcado('c1', 'c2', 'c3')), itens.length);
  // A partir de um ponto: o que ficou para trás não conta.
  assert.equal(primeiroSemResposta(itens, null, 2), 2);
  assert.equal(primeiroSemResposta(itens, marcado('c2'), 2), 5);
});

test('passosAntes junta os passos seguidos logo antes do item', () => {
  assert.deepEqual(passosAntes(itens, 4).map((c) => c.id), ['p2', 'p3']);
  assert.deepEqual(passosAntes(itens, 1).map((c) => c.id), ['p1']);
  assert.deepEqual(passosAntes(itens, 5), []);
});

test('contarCriterios ignora passos de roteiro', () => {
  assert.equal(contarCriterios(itens), 3);
  assert.equal(contarCriterios(itens, 5), 2);
});

test('mesmaMarcacao compara o que define a marcação, não o comentário', () => {
  assert.ok(mesmaMarcacao({ tipo: 'binaria', atende: true, comentario: 'x' }, { tipo: 'binaria', atende: true }));
  assert.ok(!mesmaMarcacao({ tipo: 'binaria', atende: true }, { tipo: 'binaria', atende: false }));
  assert.ok(!mesmaMarcacao(null, { tipo: 'binaria', atende: true }));
});
