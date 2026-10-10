// node --test testes/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { aplicarLote, descontar, chaveItem, tamanho } from '../web/js/lote.js';

const k = chaveItem('rubrica', 'c1');
const nivel = (n) => ({ tipo: 'nivel', nivel: n });

test('aplicarLote marca, troca e apaga sem mexer na correção original', () => {
  const original = { respostas: { rubrica: { c1: nivel('bom'), c2: nivel('ruim') } } };
  const c = aplicarLote(original, { respostas: { [k]: nivel('otimo'), [chaveItem('rubrica', 'c2')]: null }, comentario_geral: 'ok' });
  assert.deepEqual(c, { respostas: { rubrica: { c1: nivel('otimo') } }, comentario_geral: 'ok' });
  assert.deepEqual(original.respostas.rubrica.c1, nivel('bom'));
});

test('aplicarLote parte do vazio e cria a rubrica quando preciso', () => {
  assert.deepEqual(aplicarLote(null, null), { respostas: {} });
  assert.deepEqual(aplicarLote(undefined, { respostas: { [k]: nivel('bom') } }), { respostas: { rubrica: { c1: nivel('bom') } } });
});

test('descontar tira o que foi enviado e não mudou no caminho', () => {
  const enviado = { respostas: { [k]: nivel('bom') }, comentario_geral: 'a' };
  assert.equal(descontar(enviado, enviado), null);
  assert.equal(descontar(null, enviado), null);
});

test('descontar guarda o que mudou durante o envio', () => {
  const outro = chaveItem('rubrica', 'c2');
  const enviado = { respostas: { [k]: nivel('bom') }, comentario_geral: 'a' };
  const atual = { respostas: { [k]: nivel('otimo'), [outro]: null }, comentario_geral: 'b' };
  assert.deepEqual(descontar(atual, enviado), atual);
  assert.deepEqual(descontar({ respostas: { [k]: nivel('bom'), [outro]: null } }, enviado), { respostas: { [outro]: null } });
  assert.deepEqual(atual.respostas[k], nivel('otimo'));
});

test('tamanho conta respostas e o comentário geral', () => {
  assert.equal(tamanho({ respostas: {} }), 0);
  assert.equal(tamanho({ respostas: { [k]: null }, comentario_geral: '' }), 2);
});

test('lotes levam efeitos e dispensas, que se aplicam, se descontam e contam', () => {
  const lote = { respostas: {}, efeitos: { atraso: { quantidade: 2 }, bonus: null }, dispensadas: { plagio: { sinal: 0.8 } } };
  const c = aplicarLote({ respostas: {}, efeitos: { bonus: { valor: 1 } } }, lote);
  assert.deepEqual(c.efeitos, { atraso: { quantidade: 2 } });
  assert.deepEqual(c.dispensadas, { plagio: { sinal: 0.8 } });
  assert.equal(tamanho(lote), 3);
  assert.equal(descontar(lote, lote), null);
  const mudou = { ...lote, efeitos: { ...lote.efeitos, atraso: { quantidade: 3 } } };
  assert.deepEqual(descontar(mudou, lote), { respostas: {}, efeitos: { atraso: { quantidade: 3 } } });
});
