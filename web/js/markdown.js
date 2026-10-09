// Markdown mínimo para descrições e passos de roteiro: parágrafos, listas,
// `código`, blocos ``` (com botão de copiar), **negrito** e *itálico*.
// Gera nós do Preact, nunca HTML cru: texto da rubrica não vira marcação.
import { html, useState } from '../vendor/preact-htm.js';
import { cls } from './util.js';

const INLINE = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*\s][^*]*\*)/g;

function inline(texto) {
  return texto.split(INLINE).map((parte, i) => {
    if (i % 2 === 0) return parte;
    if (parte.startsWith('`')) return html`<code>${parte.slice(1, -1)}</code>`;
    if (parte.startsWith('**')) return html`<strong>${parte.slice(2, -2)}</strong>`;
    return html`<em>${parte.slice(1, -1)}</em>`;
  });
}

function linhas(lista) {
  return lista.flatMap((l, i) => (i ? [html`<br />`, inline(l)] : [inline(l)]));
}

function Codigo({ texto }) {
  const [copiado, setCopiado] = useState(false);
  async function copiar() {
    try {
      await navigator.clipboard.writeText(texto);
      setCopiado(true);
      setTimeout(() => setCopiado(false), 1500);
    } catch {
      /* sem permissão de área de transferência: o texto continua selecionável */
    }
  }
  return html`
    <div class="md-codigo">
      <pre><code>${texto}</code></pre>
      <button type="button" class="md-copiar" onClick=${copiar}>${copiado ? 'Copiado' : 'Copiar'}</button>
    </div>
  `;
}

const ITEM = /^\s*([-*]|\d+[.)])\s+(.*)$/;

function blocos(texto) {
  const saida = [];
  const ls = String(texto || '').replace(/\r/g, '').split('\n');
  let i = 0;
  while (i < ls.length) {
    const l = ls[i];
    if (l.trim().startsWith('```')) {
      const corpo = [];
      i += 1;
      while (i < ls.length && !ls[i].trim().startsWith('```')) corpo.push(ls[i++]);
      i += 1;
      saida.push(html`<${Codigo} texto=${corpo.join('\n')} />`);
    } else if (ITEM.test(l)) {
      const numerada = /^\s*\d/.test(l);
      const itens = [];
      while (i < ls.length && ITEM.test(ls[i])) itens.push(ls[i++].match(ITEM)[2]);
      const lis = itens.map((t) => html`<li>${inline(t)}</li>`);
      saida.push(numerada ? html`<ol>${lis}</ol>` : html`<ul>${lis}</ul>`);
    } else if (!l.trim()) {
      i += 1;
    } else {
      const par = [];
      while (i < ls.length && ls[i].trim() && !ITEM.test(ls[i]) && !ls[i].trim().startsWith('```')) par.push(ls[i++]);
      saida.push(html`<p>${linhas(par)}</p>`);
    }
  }
  return saida;
}

export function Markdown({ texto, classe }) {
  return html`<div class=${cls('md', classe)}>${blocos(texto)}</div>`;
}
