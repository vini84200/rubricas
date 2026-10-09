// Pontuação. Espelha src/modelo.rs — mude os dois juntos.
import { num, plural } from './util.js';

export const TIPOS = {
  binaria: 'Sim ou não',
  niveis: 'Níveis',
  livre: 'Nota livre',
  ponto_unico: 'Ponto único',
};

export const DESEMPENHOS = { abaixo: 'Abaixo', atende: 'Atende', acima: 'Acima' };

/** Passo de roteiro: orienta quem avalia, não recebe marcação nem conta na nota. */
export function ehRoteiro(criterio) {
  return criterio.tipo === 'roteiro';
}

/** O servidor omite o peso quando é 1. */
export function pesoDe(criterio) {
  return criterio.peso ?? 1;
}

/** Maior pontuação possível do critério, antes do peso. Zero para passos de roteiro. */
export function maximo(criterio) {
  const e = criterio.escala;
  if (!e) return 0;
  if (e.tipo === 'binaria') return e.pontos;
  if (e.tipo === 'niveis') return Math.max(0, ...e.niveis.map((n) => n.pontos));
  return e.max;
}

/** Pontos de uma resposta, antes do peso; null se não combina com a escala. */
export function pontos(criterio, resposta) {
  if (!resposta || ehRoteiro(criterio) || !criterio.escala) return null;
  const e = criterio.escala;
  if (e.tipo === 'binaria' && resposta.tipo === 'binaria') return resposta.atende ? e.pontos : 0;
  if (e.tipo === 'niveis' && resposta.tipo === 'nivel') {
    const n = e.niveis.find((x) => x.id === resposta.nivel);
    return n ? n.pontos : null;
  }
  if ((e.tipo === 'livre' && resposta.tipo === 'nota') || (e.tipo === 'ponto_unico' && resposta.tipo === 'ponto_unico')) {
    const v = resposta.nota;
    return Number.isFinite(v) && v >= 0 && v <= e.max + 1e-9 ? v : null;
  }
  return null;
}

/** nota = nota_maxima × Σ peso_r × (pontos_r ÷ máximo_r) ÷ Σ peso_r */
export function calcular(avaliacao, correcao) {
  const rubricas = [];
  let respondidos = 0;
  let total = 0;
  let soma = 0;
  let somaPesos = 0;
  for (const ra of avaliacao.rubricas) {
    const respostas = correcao?.respostas?.[ra.origem] || {};
    let pts = 0;
    let max = 0;
    for (const c of ra.rubrica.criterios) {
      if (ehRoteiro(c)) continue;
      total += 1;
      max += pesoDe(c) * maximo(c);
      const p = pontos(c, respostas[c.id]);
      if (p !== null) {
        respondidos += 1;
        pts += pesoDe(c) * p;
      }
    }
    if (max > 0 && ra.peso > 0) {
      soma += ra.peso * (pts / max);
      somaPesos += ra.peso;
    }
    rubricas.push({ pontos: pts, maximo: max });
  }
  const nota = somaPesos > 0 ? (avaliacao.nota_maxima * soma) / somaPesos : 0;
  return { rubricas, nota, respondidos, total, concluida: total > 0 && respondidos === total };
}

/** Critérios e passos de roteiro de todas as rubricas, na ordem em que aparecem. */
export function itens(avaliacao) {
  const lista = [];
  for (const ra of avaliacao.rubricas) {
    for (const criterio of ra.rubrica.criterios) {
      lista.push({ origem: ra.origem, rubrica: ra.rubrica.titulo, criterio });
    }
  }
  return lista;
}

export function niveisOrdenados(criterio) {
  return [...criterio.escala.niveis].sort((a, b) => b.pontos - a.pontos);
}

/** O que um deslize para a direita (positivo) ou esquerda (negativo) marca. */
export function marcacaoRapida(criterio, positivo) {
  const e = criterio.escala;
  if (e.tipo === 'binaria') return { tipo: 'binaria', atende: positivo };
  if (e.tipo === 'niveis') {
    const ordem = niveisOrdenados(criterio);
    return { tipo: 'nivel', nivel: (positivo ? ordem[0] : ordem[ordem.length - 1]).id };
  }
  if (e.tipo === 'livre') return { tipo: 'nota', nota: positivo ? e.max : 0 };
  return { tipo: 'ponto_unico', desempenho: positivo ? 'atende' : 'abaixo', nota: positivo ? e.max : 0 };
}

/** Texto curto de uma marcação: "Bom", "Atende", "2,5". */
export function rotulo(criterio, resposta) {
  if (!resposta) return '';
  const e = criterio.escala;
  if (resposta.tipo === 'binaria') return resposta.atende ? 'Atende' : 'Não atende';
  if (resposta.tipo === 'nivel') return e.niveis?.find((n) => n.id === resposta.nivel)?.rotulo || '?';
  if (resposta.tipo === 'ponto_unico') return DESEMPENHOS[resposta.desempenho] || '?';
  return num(resposta.nota);
}

/** 'sim' (pontuação máxima), 'nao' (zero ou mínima), 'meio' ou '' (sem resposta). */
export function tom(criterio, resposta) {
  const p = pontos(criterio, resposta);
  if (p === null) return '';
  const max = maximo(criterio);
  if (p >= max - 1e-9) return 'sim';
  const min = criterio.escala.tipo === 'niveis' ? Math.min(...criterio.escala.niveis.map((n) => n.pontos)) : 0;
  return p <= min + 1e-9 ? 'nao' : 'meio';
}

/** Tem algo entre o mínimo e o máximo? Decide se o deslize para cima oferece níveis. */
export function temIntermediarios(criterio) {
  const e = criterio.escala;
  return e.tipo === 'livre' || e.tipo === 'ponto_unico' || (e.tipo === 'niveis' && e.niveis.length > 2);
}

export function passoDe(escala) {
  if (escala.passo > 0) return escala.passo;
  if (escala.max <= 2) return 0.1;
  if (escala.max <= 5) return 0.25;
  return 0.5;
}

export function maximoRubrica(rubrica) {
  return rubrica.criterios.reduce((s, c) => s + pesoDe(c) * (maximo(c) || 0), 0);
}

export function contarCriterios(rubrica) {
  return rubrica.criterios.filter((c) => !ehRoteiro(c)).length;
}

/** "4 critérios" ou "4 critérios, 3 passos de roteiro". */
export function resumoItens(rubrica) {
  const n = contarCriterios(rubrica);
  const passos = rubrica.criterios.length - n;
  return `${plural(n, 'critério', 'critérios')}${passos ? `, ${plural(passos, 'passo de roteiro', 'passos de roteiro')}` : ''}`;
}
