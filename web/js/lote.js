// Lotes da fila de correções: o que um aluno teve de alterado e ainda não chegou
// ao servidor. Funções puras, usadas pela fila (api.js) e pela tela de avaliar.
//
// Formato: { respostas: { "<origem>\n<criterio>": resposta|null }, comentario_geral?: string }

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
  return c;
}

/** O que sobra de `atual` depois que `enviado` foi aceito: só sai o que não mudou
    durante o envio. `null` quando não sobra nada. */
export function descontar(atual, enviado) {
  if (!atual) return null;
  const respostas = { ...atual.respostas };
  for (const [c, r] of Object.entries(enviado.respostas || {})) {
    if (JSON.stringify(respostas[c]) === JSON.stringify(r)) delete respostas[c];
  }
  const resto = { respostas };
  if (atual.comentario_geral !== undefined && atual.comentario_geral !== enviado.comentario_geral) {
    resto.comentario_geral = atual.comentario_geral;
  }
  return Object.keys(respostas).length || resto.comentario_geral !== undefined ? resto : null;
}

/** Quantas alterações o lote carrega. */
export function tamanho(lote) {
  return Object.keys(lote.respostas || {}).length + (lote.comentario_geral !== undefined ? 1 : 0);
}
