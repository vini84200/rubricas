// node --test testes/*.test.mjs
// nota.js espelha src/modelo.rs: estes casos fixam a conta do lado do navegador.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { calcular, pontos, tom, opcoesDe, marcacaoRapida, aplicarEfeitos, problemaEfeito, textoEfeito, atraso, minutos, sugestoesEfeitos } from '../web/js/nota.js';
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

// Efeitos: os mesmos casos de src/efeitos.rs.
const atrasoT = { id: 'atraso', titulo: 'Atraso', sugerir: 'atraso', operacao: { tipo: 'por_unidade', pontos: -1, limite: -3 } };
const plagioT = { id: 'plagio', titulo: 'Plágio', sugerir: 'plagio', encerra: true, feedback: false, operacao: { tipo: 'fixar', nota: 0 } };
const bonusT = { id: 'bonus', titulo: 'Bônus', operacao: { tipo: 'somar', pontos: 'na_hora' } };
const metadeT = { id: 'metade', titulo: 'Metade', operacao: { tipo: 'multiplicar', fator: 0.5 } };
const tiposEf = [bonusT, atrasoT, metadeT, plagioT];
const digitado = (valor) => ({ valor, justificativa: 'Apresentou em aula' });

test('aplicarEfeitos: ordem fixa, limite, e a nota entre zero e a máxima', () => {
  assert.deepEqual(aplicarEfeitos(tiposEf, {}, 8, 10), { nota: 8, encerrada: false, ids: [] });
  const r = aplicarEfeitos(tiposEf, { bonus: digitado(1), atraso: { quantidade: 2 }, metade: {} }, 8, 10);
  assert.deepEqual([r.nota, r.ids], [3, ['bonus', 'atraso', 'metade']]);
  assert.equal(aplicarEfeitos(tiposEf, { atraso: { quantidade: 9 } }, 8, 10).nota, 5);
  assert.equal(aplicarEfeitos(tiposEf, { atraso: { quantidade: 9 } }, 2, 10).nota, 0);
  assert.equal(aplicarEfeitos(tiposEf, { bonus: digitado(3) }, 9, 10).nota, 10);
  const z = aplicarEfeitos(tiposEf, { bonus: digitado(3), plagio: {} }, 9, 10);
  assert.deepEqual([z.nota, z.encerrada], [0, true]);
  // o que não combina com o tipo fica de fora
  assert.equal(aplicarEfeitos(tiposEf, { bonus: { valor: 1 }, atraso: {}, sumiu: { quantidade: 1 } }, 7, 10).nota, 7);
  assert.equal(problemaEfeito(bonusT, { valor: 1 }), 'Escreva a justificativa');
});

test('calcular: efeito que encerra conclui sem marcar nada', () => {
  const r = calcular({ ...avaliacao, efeitos: tiposEf }, { respostas: {}, efeitos: { plagio: {} } });
  assert.deepEqual([r.nota, r.respondidos, r.concluida, r.encerrada], [0, 0, true, true]);
});

test('textoEfeito: o mesmo texto que vai ao Moodle', () => {
  assert.equal(textoEfeito(atrasoT, { quantidade: 2 }), 'Atraso (2 dias): −2');
  assert.equal(textoEfeito(atrasoT, { quantidade: 5 }), 'Atraso (5 dias): −3');
  assert.equal(textoEfeito(atrasoT, { quantidade: 1 }), 'Atraso (1 dia): −1');
  assert.equal(textoEfeito(plagioT, {}), 'Plágio: nota 0');
  assert.equal(textoEfeito(bonusT, digitado(0.5)), 'Bônus: +0,5. Apresentou em aula');
  assert.equal(textoEfeito(metadeT, {}), 'Metade: −50%');
});

test('atraso: dia começado conta inteiro, dentro da tolerância não conta', () => {
  const prazo = '2026-10-06T23:59';
  assert.equal(atraso(prazo, 10, '2026-10-07T00:05'), 0);
  assert.equal(atraso(prazo, 10, '2026-10-07T00:15'), 1);
  assert.equal(atraso(prazo, 0, '2026-10-08T00:15'), 2);
  assert.equal(atraso(prazo, 0, '2026-10-07T01:59', 'hora'), 2);
  assert.equal(atraso(prazo, 0, '2026-10-06T10:00'), 0);
  assert.equal(atraso(null, 0, '2026-10-07T00:15'), 0);
  assert.equal(minutos('2026-02-29T00:00'), null);
});

test('sugestoesEfeitos: aparece com o sinal, some aplicada ou dispensada, volta se piorar', () => {
  const av = { ...avaliacao, efeitos: tiposEf, prazo: '2026-10-06T23:59' };
  const sinais = { plagio: { com: 'x', similaridade: 0.87 }, envio: '2026-10-08T10:00' };
  const ids = (c, s = sinais) => sugestoesEfeitos(av, c, s).map((x) => [x.tipo.id, x.sinal, x.aplicado]);
  assert.deepEqual(ids(null), [['atraso', 2, { quantidade: 2 }], ['plagio', 0.87, {}]]);
  assert.deepEqual(ids(null, {}), []);
  assert.deepEqual(ids({ efeitos: { plagio: {} } }), [['atraso', 2, { quantidade: 2 }]]);
  const dispensada = { dispensadas: { plagio: { sinal: 0.87 }, atraso: { sinal: 2 } } };
  assert.deepEqual(ids(dispensada), []);
  assert.deepEqual(ids(dispensada, { ...sinais, plagio: { similaridade: 0.9 } }), [['plagio', 0.9, {}]]);
});
