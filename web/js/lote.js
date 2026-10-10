// Lotes da fila de correções: o que um aluno teve de alterado e ainda não chegou
// ao servidor. Funções puras, usadas pela fila (api.js) e pela tela de avaliar.
//
// Formato: { respostas: { "<origem>\n<criterio>": resposta|null }, comentario_geral?: string,
//           efeitos?: { "<tipo>": aplicado|null }, dispensadas?: { "<tipo>": dispensa|null } }

const SEP = '\n';

export function chaveItem(origem, criterio) {
  return origem + SEP + criterio;
}

export function separar(chave) {
  return chave.split(SEP);
}

/** Aplica um lote sobre uma correção (cópia). Resposta `null` apaga a marcação. */
export function aplicarLote(correcao, lote) {
  const c = structuredClone(correcao || { respostas: {} });
  c.respostas ||= {};
  if (!lote) return c;
  for (const [k, resposta] of Object.entries(lote.respostas || {})) {
    const [origem, criterio] = separar(k);
    if (resposta === null) {
      if (c.respostas[origem]) delete c.respostas[origem][criterio];
    } else {
      (c.respostas[origem] ||= {})[criterio] = resposta;
    }
  }
  if (lote.comentario_geral !== undefined) c.comentario_geral = lote.comentario_geral;
  for (const campo of MAPAS) {
    for (const [id, v] of Object.entries(lote[campo] || {})) {
      if (v === null) {
        if (c[campo]) delete c[campo][id];
      } else {
        (c[campo] ||= {})[id] = v;
      }
    }
  }
  return c;
}

/** Os mapas por id de tipo de efeito, que andam juntos com as respostas. */
export const MAPAS = ['efeitos', 'dispensadas'];

const igual = (a, b) => JSON.stringify(a) === JSON.stringify(b);

/** O que sobra de `atual` depois que `enviado` foi aceito: só sai o que não mudou
    durante o envio. `null` quando não sobra nada. */
export function descontar(atual, enviado) {
  if (!atual) return null;
  const respostas = { ...atual.respostas };
  for (const [c, r] of Object.entries(enviado.respostas || {})) {
    if (igual(respostas[c], r)) delete respostas[c];
  }
  const resto = { respostas };
  if (atual.comentario_geral !== undefined && atual.comentario_geral !== enviado.comentario_geral) {
    resto.comentario_geral = atual.comentario_geral;
  }
  for (const campo of MAPAS) {
    const mapa = { ...atual[campo] };
    for (const [id, v] of Object.entries(enviado[campo] || {})) {
      if (igual(mapa[id], v)) delete mapa[id];
    }
    if (Object.keys(mapa).length) resto[campo] = mapa;
  }
  return tamanho(resto) ? resto : null;
}

/** Quantas alterações o lote carrega. */
export function tamanho(lote) {
  const mapas = MAPAS.reduce((n, campo) => n + Object.keys(lote[campo] || {}).length, 0);
  return Object.keys(lote.respostas || {}).length + (lote.comentario_geral !== undefined ? 1 : 0) + mapas;
}
