// Diferenças entre a saída esperada e a obtida de um caso, para colorir as duas.

/** Linhas como o executor compara: sem espaços no fim nem linhas em branco no fim. */
function linhas(texto) {
  const ls = (texto || '').split('\n').map((l) => l.trimEnd());
  while (ls.length && !ls[ls.length - 1]) ls.pop();
  return ls;
}

/** Maior subsequência comum: para cada item de `a` e de `b`, se ele é comum aos dois.
    Grande demais para a tabela, compara posição a posição. */
function comuns(a, b) {
  const n = a.length, m = b.length;
  if (n * m > 4_000_000) return [a.map((x, i) => x === b[i]), b.map((x, j) => x === a[j])];
  const t = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) t[i][j] = a[i] === b[j] ? t[i + 1][j + 1] + 1 : Math.max(t[i + 1][j], t[i][j + 1]);
  }
  const emA = new Array(n).fill(false), emB = new Array(m).fill(false);
  for (let i = 0, j = 0; i < n && j < m;) {
    if (a[i] === b[j]) { emA[i++] = emB[j++] = true; }
    else if (t[i + 1][j] >= t[i][j + 1]) i++;
    else j++;
  }
  return [emA, emB];
}

/** Junta caracteres vizinhos com a mesma marca em trechos. */
function trechos(chars, comum) {
  const partes = [];
  chars.forEach((c, i) => {
    const mudou = !comum[i];
    const ultima = partes[partes.length - 1];
    if (ultima && ultima.mudou === mudou) ultima.texto += c;
    else partes.push({ texto: c, mudou });
  });
  return partes;
}

/** As linhas de cada lado, cada uma em trechos marcados como mudados ou não. */
export function diferencas(esperado, obtido) {
  const a = linhas(esperado), b = linhas(obtido);
  const [emA, emB] = comuns(a, b);
  const igual = (texto) => ({ mudou: false, partes: [{ texto, mudou: false }] });
  const lado = { esperado: [], obtido: [] };
  let i = 0, j = 0;
  while (i < a.length || j < b.length) {
    if (emA[i] && emB[j]) {
      lado.esperado.push(igual(a[i++]));
      lado.obtido.push(igual(b[j++]));
      continue;
    }
    // Um bloco de linhas que mudou: emparelha as do esperado com as do obtido.
    const ia = i, jb = j;
    while (i < a.length && !emA[i]) i++;
    while (j < b.length && !emB[j]) j++;
    for (let k = 0; k < Math.max(i - ia, j - jb); k++) {
      const x = [...(ia + k < i ? a[ia + k] : '')], y = [...(jb + k < j ? b[jb + k] : '')];
      let [cx, cy] = comuns(x, y);
      // Pouco em comum e nenhuma contida na outra: picotar só confunde, marca inteira.
      const iguais = cx.filter(Boolean).length;
      if (2 * iguais < Math.max(x.length, y.length) && iguais < Math.min(x.length, y.length)) [cx, cy] = [[], []];
      if (ia + k < i) lado.esperado.push({ mudou: true, partes: trechos(x, cx) });
      if (jb + k < j) lado.obtido.push({ mudou: true, partes: trechos(y, cy) });
    }
  }
  return lado;
}
