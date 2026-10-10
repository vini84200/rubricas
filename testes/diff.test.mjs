// node --test testes/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { diferencas } from '../web/js/diff.js';

// Linha como texto, com [trechos marcados] entre colchetes e "!" no começo das linhas que mudaram.
const ver = (linhas) => linhas.map((l) => (l.mudou ? '!' : ' ') + l.partes.map((p) => (p.mudou ? `[${p.texto}]` : p.texto)).join(''));

test('saídas iguais não marcam nada', () => {
  const d = diferencas('5\n10\n', '5\n10\n');
  assert.deepEqual(ver(d.esperado), [' 5', ' 10']);
  assert.deepEqual(ver(d.obtido), [' 5', ' 10']);
});

test('linha trocada marca só os caracteres diferentes', () => {
  const d = diferencas('Soma: 5\nfim\n', 'Soma= 5\nfim\n');
  assert.deepEqual(ver(d.esperado), ['!Soma[:] 5', ' fim']);
  assert.deepEqual(ver(d.obtido), ['!Soma[=] 5', ' fim']);
});

test('linha que sobra ou falta fica marcada inteira', () => {
  const d = diferencas('a\nc\n', 'a\nb\nc\nd\n');
  assert.deepEqual(ver(d.esperado), [' a', ' c']);
  assert.deepEqual(ver(d.obtido), [' a', '![b]', ' c', '![d]']);
  const f = diferencas('a\nb\n', 'a\n');
  assert.deepEqual(ver(f.esperado), [' a', '![b]']);
  assert.deepEqual(ver(f.obtido), [' a']);
});

test('espaços no fim e linhas em branco no fim não contam, como no executor', () => {
  const d = diferencas('5\n', '5   \n\n\n');
  assert.deepEqual(ver(d.obtido), [' 5']);
});

test('linhas pouco parecidas ficam marcadas inteiras, sem picotar', () => {
  const d = diferencas('Total de votos: 10\n', 'Resultado final\n');
  assert.deepEqual(ver(d.esperado), ['![Total de votos: 10]']);
  assert.deepEqual(ver(d.obtido), ['![Resultado final]']);
});

test('linha que falta não é comparada com a linha igual seguinte', () => {
  const d = diferencas('Soma: 5\nSoma: 6\n', 'Soma: 6\n');
  assert.deepEqual(ver(d.esperado), ['![Soma: 5]', ' Soma: 6']);
  assert.deepEqual(ver(d.obtido), [' Soma: 6']);
});

test('espaços a mais no começo aparecem, mesmo em linha curta', () => {
  const d = diferencas('5\n', '  5\n');
  assert.deepEqual(ver(d.esperado), ['!5']);
  assert.deepEqual(ver(d.obtido), ['![  ]5']);
});
