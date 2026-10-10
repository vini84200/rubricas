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

/** nota = nota_maxima × Σ peso_r × (pontos_r ÷ máximo_r) ÷ Σ peso_r, e depois os efeitos.
    `antes` é a nota só das rubricas; `encerrada`, se um efeito aplicado dispensa os critérios. */
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
  const antes = somaPesos > 0 ? (avaliacao.nota_maxima * soma) / somaPesos : 0;
  const e = aplicarEfeitos(avaliacao.efeitos || [], correcao?.efeitos || {}, antes, avaliacao.nota_maxima);
  return {
    rubricas, nota: e.nota, antes, efeitos: e.ids, encerrada: e.encerrada, respondidos, total,
    concluida: e.encerrada || (total > 0 && respondidos === total),
  };
}

// ---------- Efeitos sobre a nota. Espelha src/efeitos.rs — mude os dois juntos. ----------

const valorDoTipo = (tipo) => {
  const o = tipo.operacao;
  return o.tipo === 'multiplicar' ? o.fator : o.tipo === 'fixar' || o.tipo === 'teto' ? o.nota : o.pontos;
};

/** O tipo deixa o valor para quem aplica digitar? */
export const efeitoAberto = (tipo) => valorDoTipo(tipo) === 'na_hora';
export const porUnidade = (tipo) => tipo.operacao.tipo === 'por_unidade';

function valorDe(tipo, aplicado) {
  const v = valorDoTipo(tipo);
  return v === 'na_hora' ? aplicado.valor : v;
}

/** O que falta para o efeito valer neste tipo; null se está tudo certo. */
export function problemaEfeito(tipo, aplicado) {
  if (efeitoAberto(tipo)) {
    const v = aplicado.valor;
    if (!Number.isFinite(v)) return 'Falta o valor';
    const op = tipo.operacao.tipo;
    if ((op === 'fixar' || op === 'teto') && v < 0) return 'A nota não pode ser negativa';
    if (op === 'multiplicar' && v < 0) return 'O fator não pode ser negativo';
    if (!aplicado.justificativa?.trim()) return 'Escreva a justificativa';
  }
  if (porUnidade(tipo)) {
    const q = aplicado.quantidade;
    if (!Number.isFinite(q)) return 'Falta a quantidade';
    if (q < 0) return 'A quantidade precisa ser zero ou mais';
  }
  return null;
}

function totalPorUnidade(tipo, pontos, quantidade) {
  const total = pontos * quantidade;
  const limite = tipo.operacao.limite;
  if (!Number.isFinite(limite)) return total;
  return pontos < 0 ? Math.max(total, limite) : Math.min(total, limite);
}

/** Ordem fixa: multiplicar → somar e por unidade → teto; fixar vence; no fim, entre 0 e a máxima. */
export function aplicarEfeitos(tipos, aplicados, base, maxima) {
  let fator = 1;
  let soma = 0;
  let teto = Infinity;
  let fixa = null;
  let encerrada = false;
  const ids = [];
  for (const t of tipos) {
    const a = aplicados[t.id];
    if (!a || problemaEfeito(t, a)) continue;
    const v = valorDe(t, a);
    const op = t.operacao.tipo;
    if (op === 'multiplicar') fator *= v;
    else if (op === 'somar') soma += v;
    else if (op === 'por_unidade') soma += totalPorUnidade(t, v, a.quantidade ?? 0);
    else if (op === 'teto') teto = Math.min(teto, v);
    else if (op === 'fixar') fixa = fixa === null ? v : Math.min(fixa, v);
    else continue;
    encerrada ||= !!t.encerra;
    ids.push(t.id);
  }
  if (!ids.length) return { nota: base, encerrada, ids };
  const nota = fixa !== null ? fixa : Math.min(base * fator + soma, teto);
  return { nota: Math.min(Math.max(nota, 0), maxima), encerrada, ids };
}

const sinalNum = (x) => (x < 0 ? `−${num(-x)}` : `+${num(x)}`);
export const UNIDADES = { dia: ['dia', 'dias'], hora: ['hora', 'horas'] };
export const unidadeDe = (tipo) => UNIDADES[tipo.operacao.unidade || 'dia'];

/** "Desconto por atraso (2 dias): −2". Com a justificativa, se houver. */
export function textoEfeito(tipo, aplicado) {
  const v = valorDe(tipo, aplicado) ?? 0;
  const op = tipo.operacao.tipo;
  let base;
  if (op === 'fixar') base = `${tipo.titulo}: nota ${num(v)}`;
  else if (op === 'multiplicar') base = `${tipo.titulo}: ${sinalNum((v - 1) * 100)}%`;
  else if (op === 'somar') base = `${tipo.titulo}: ${sinalNum(v)}`;
  else if (op === 'por_unidade') {
    const q = aplicado.quantidade ?? 0;
    base = `${tipo.titulo} (${num(q)} ${unidadeDe(tipo)[q === 1 ? 0 : 1]}): ${sinalNum(totalPorUnidade(tipo, v, q))}`;
  } else base = `${tipo.titulo}: nota até ${num(v)}`;
  const j = aplicado.justificativa?.trim();
  return j ? `${base}. ${j}` : base;
}

/** O que o tipo faz, sem aplicar: "−1 por dia, até −5", "nota 0", "valor na hora". */
export function descreverOperacao(tipo) {
  const o = tipo.operacao;
  const v = valorDoTipo(tipo);
  const valor = (f) => (v === 'na_hora' ? 'valor digitado na hora' : f(v));
  if (o.tipo === 'fixar') return valor((x) => `nota ${num(x)}`);
  if (o.tipo === 'multiplicar') return valor((x) => `${sinalNum((x - 1) * 100)}% na nota`);
  if (o.tipo === 'somar') return valor((x) => `${sinalNum(x)} na nota`);
  if (o.tipo === 'teto') return valor((x) => `nota até ${num(x)}`);
  const limite = Number.isFinite(o.limite) ? `, até ${sinalNum(o.limite)}` : '';
  return `${valor((x) => sinalNum(x))} por ${unidadeDe(tipo)[0]}${limite}`;
}

/** "AAAA-MM-DDTHH:MM" → minutos, sem fuso (prazo e Moodle usam o mesmo relógio). */
export function minutos(texto) {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(texto || '');
  if (!m) return null;
  const [a, me, d, h, mi] = m.slice(1).map(Number);
  const t = Date.UTC(a, me - 1, d, h, mi);
  const v = new Date(t);
  if (v.getUTCMonth() !== me - 1 || v.getUTCDate() !== d || h > 23 || mi > 59) return null;
  return t / 60000;
}

/** Quantas unidades de atraso: cada período começado conta inteiro; dentro da
    tolerância (minutos), não é atraso. 0 se no prazo ou sem as datas. */
export function atraso(prazo, tolerancia, envio, unidade = 'dia') {
  const p = minutos(prazo);
  const e = minutos(envio);
  if (p === null || e === null) return 0;
  const passou = e - p;
  if (passou <= (tolerancia || 0)) return 0;
  return Math.ceil(passou / (unidade === 'hora' ? 60 : 1440));
}

/**
 * As sugestões de efeito para um aluno: os tipos com `sugerir` cujo sinal apareceu,
 * que não estão aplicados e não foram dispensados com um sinal igual ou maior.
 * `plagio` é o par mais parecido acima do limiar; `envio`, a data do envio no Moodle.
 */
export function sugestoesEfeitos(avaliacao, correcao, { plagio, envio }) {
  const lista = [];
  for (const t of avaliacao.efeitos || []) {
    if (!t.sugerir || correcao?.efeitos?.[t.id]) continue;
    let sinal = 0;
    let aplicado = {};
    if (t.sugerir === 'plagio' && plagio) sinal = plagio.similaridade;
    if (t.sugerir === 'atraso') {
      sinal = atraso(avaliacao.prazo, avaliacao.tolerancia, envio, t.operacao.unidade);
      if (porUnidade(t)) aplicado = { quantidade: sinal };
    }
    if (!(sinal > 0)) continue;
    const dispensa = correcao?.dispensadas?.[t.id];
    if (dispensa && sinal <= dispensa.sinal + 1e-9) continue;
    lista.push({ tipo: t, sinal, aplicado, completo: !efeitoAberto(t) });
  }
  return lista;
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

/**
 * As opções de marcar com um toque (níveis e sim/não), da melhor para a pior, com
 * a tecla que escolhe cada uma: a pior é 1. Vazio para nota livre e ponto único,
 * que pedem um número.
 */
export function opcoesDe(criterio) {
  const e = criterio.escala;
  if (e.tipo === 'niveis') {
    const ordem = niveisOrdenados(criterio);
    return ordem.map((n, i) => {
      const marcacao = { tipo: 'nivel', nivel: n.id };
      return { marcacao, rotulo: n.rotulo, pontos: n.pontos, tom: tom(criterio, marcacao), descritor: n.descritor, tecla: ordem.length - i };
    });
  }
  if (e.tipo === 'binaria') {
    return [true, false].map((atende) => ({
      marcacao: { tipo: 'binaria', atende },
      rotulo: atende ? 'Atende' : 'Não atende',
      pontos: atende ? e.pontos : 0,
      tom: atende ? 'sim' : 'nao',
      tecla: atende ? 2 : 1,
    }));
  }
  return [];
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
