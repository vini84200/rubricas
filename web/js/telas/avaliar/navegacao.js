// Por onde a avaliação anda: itens (critérios e passos de roteiro) de um aluno.
// Funções puras, sem tela.
import { ehRoteiro, pontos, marcacaoRapida, rotulo } from '../../nota.js';

export function respostaDe(correcao, item) {
  return correcao?.respostas?.[item.origem]?.[item.criterio.id] || null;
}

function semResposta(itens, correcao, i) {
  return pontos(itens[i].criterio, respostaDe(correcao, itens[i])) === null;
}

/** Onde parar: critério sem marcação, ou passo de roteiro que leva a um. Passos
    de critérios já marcados ficam para trás. */
export function primeiroSemResposta(itens, correcao, de = 0) {
  for (let i = de; i < itens.length; i += 1) {
    let alvo = i;
    while (alvo < itens.length && ehRoteiro(itens[alvo].criterio)) alvo += 1;
    if (alvo < itens.length && semResposta(itens, correcao, alvo)) return i;
  }
  return itens.length;
}

/** Passos de roteiro logo antes do item `i`. */
export function passosAntes(itens, i) {
  let j = i;
  while (j > 0 && ehRoteiro(itens[j - 1].criterio)) j -= 1;
  return itens.slice(j, i).map((it) => it.criterio);
}

/** Critérios contam "3 de 8"; passos de roteiro não entram na conta. */
export function contarCriterios(itens, ate = itens.length) {
  return itens.slice(0, ate).filter((it) => !ehRoteiro(it.criterio)).length;
}

export function mesmaMarcacao(a, b) {
  return !!a && !!b && ['tipo', 'nivel', 'atende', 'nota', 'desempenho'].every((k) => a[k] === b[k]);
}

/** Rótulo e marcação do que o deslize lateral marca. */
export function rapido(criterio, positivo) {
  const marcacao = marcacaoRapida(criterio, positivo);
  return { texto: rotulo(criterio, marcacao), marcacao };
}
