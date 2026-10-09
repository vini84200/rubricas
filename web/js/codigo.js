// Código do aluno (números de linha, destaque de Python) e o relatório das checagens.
// Usado ao lado do cartão na tela de avaliar e na tela "Acompanhar".
import { html, useState, useEffect } from '../vendor/preact-htm.js';
import Prism from '../vendor/prism.js';
import { api } from './api.js';
import { cls, plural } from './util.js';

const cache = new Map();

/** Código e relatório de um aluno. Mostra o que já está em memória e atualiza
    em seguida; `versao` muda quando a submissão ou o relatório mudam. */
export function useSubmissao(avaliacao, aluno, versao) {
  const chave = avaliacao && aluno ? `${avaliacao}\n${aluno}` : null;
  const [estado, setEstado] = useState(() => ({ dados: chave ? cache.get(chave)?.dados || null : null, erro: null }));
  useEffect(() => {
    if (!chave) {
      setEstado({ dados: null, erro: null });
      return undefined;
    }
    const guardado = cache.get(chave);
    setEstado({ dados: guardado?.dados || null, erro: null });
    if (guardado && guardado.versao === versao) return undefined;
    let vivo = true;
    api('GET', `/avaliacoes/${encodeURIComponent(avaliacao)}/submissoes/${encodeURIComponent(aluno)}`)
      .then((dados) => {
        cache.set(chave, { dados, versao });
        if (vivo) setEstado({ dados, erro: null });
      })
      .catch((erro) => vivo && setEstado((e) => ({ dados: e.dados, erro })));
    return () => {
      vivo = false;
    };
  }, [chave, versao]);
  return estado;
}

/** Python primeiro, depois o resto em ordem alfabética. */
function ordenar(arquivos) {
  const py = (a) => (a.caminho.endsWith('.py') ? 0 : 1);
  return [...arquivos].sort((a, b) => py(a) - py(b) || a.caminho.localeCompare(b.caminho));
}

function destacar(texto, caminho) {
  if (!caminho.endsWith('.py')) return null;
  try {
    return Prism.highlight(texto, Prism.languages.python, 'python');
  } catch {
    return null;
  }
}

function Fonte({ arquivo, href }) {
  if (arquivo.texto === null || arquivo.texto === undefined) {
    return html`<p class="codigo-binario">Arquivo binário ou grande demais para mostrar (${Math.ceil(arquivo.tamanho / 1024)} KB). <a href=${href} download>Baixar</a></p>`;
  }
  const linhas = arquivo.texto.replace(/\n$/, '').split('\n').length;
  const destacado = destacar(arquivo.texto, arquivo.caminho);
  return html`
    <div class="codigo-fonte">
      <pre class="codigo-numeros" aria-hidden="true">${Array.from({ length: linhas }, (_, i) => `${i + 1}\n`).join('')}</pre>
      ${destacado === null
        ? html`<pre class="codigo-texto"><code>${arquivo.texto}</code></pre>`
        : html`<pre class="codigo-texto"><code dangerouslySetInnerHTML=${{ __html: destacado }}></code></pre>`}
    </div>
  `;
}

/** Abas por arquivo e o arquivo escolhido. */
export function Codigo({ avaliacao, aluno, arquivos }) {
  const lista = ordenar(arquivos || []);
  const [escolhido, setEscolhido] = useState(null);
  const atual = lista.find((a) => a.caminho === escolhido) || lista[0];
  if (!atual) return html`<p class="codigo-vazio">Sem arquivos.</p>`;
  const href = (a) => `api/avaliacoes/${encodeURIComponent(avaliacao)}/submissoes/${encodeURIComponent(aluno)}/${a.caminho.split('/').map(encodeURIComponent).join('/')}`;
  return html`
    <div class="codigo">
      ${lista.length > 1 && html`
        <div class="codigo-abas" role="tablist" aria-label="Arquivos">
          ${lista.map((a) => html`
            <button type="button" role="tab" aria-selected=${a === atual} class=${cls('codigo-aba', a === atual && 'atual')} onClick=${() => setEscolhido(a.caminho)}>${a.caminho}</button>
          `)}
        </div>
      `}
      ${lista.length === 1 && html`<p class="codigo-nome">${atual.caminho}</p>`}
      <${Fonte} key=${atual.caminho} arquivo=${atual} href=${href(atual)} />
    </div>
  `;
}

const SINAL = { passou: '✓', falhou: '✗', erro: '!' };

function Caso({ c }) {
  return html`
    <li class=${cls('caso', c.passou ? 'passou' : 'falhou')}>
      <p><strong>${c.nome}</strong>${!c.passou && html` — ${c.motivo}`}</p>
      ${!c.passou && html`
        <div class="caso-par">
          <div><span>Entrada</span><pre>${c.entrada || '(vazia)'}</pre></div>
          <div><span>Esperado</span><pre>${c.esperado}</pre></div>
          <div><span>Obtido</span><pre>${c.obtido || '(nada)'}</pre></div>
        </div>
        ${c.erro && html`<div class="caso-erro"><span>Fim do erro</span><pre>${c.erro}</pre></div>`}
      `}
    </li>
  `;
}

/** As checagens do aluno: uma linha por checagem; tocar abre o detalhe. */
export function Checagens({ relatorio, aguardando }) {
  if (!relatorio) {
    return html`<p class="checagens-vazio">${aguardando ? 'Aguardando o executor.' : 'Ainda sem checagens.'}</p>`;
  }
  const { relatorio: r, atual } = relatorio;
  return html`
    <div class="checagens">
      ${!atual && html`<p class="checagens-velho">A submissão ou o pipeline mudou: estas checagens são da versão anterior.</p>`}
      ${r.checagens.map((c) => {
        // O que passou não tem o que ver ("All checks passed!", "2 passed").
        const temDetalhe = c.situacao !== 'passou' && (c.casos?.some((x) => !x.passou) || c.saida);
        const linha = html`
          <span class="checagem-sinal" aria-hidden="true">${SINAL[c.situacao]}</span>
          <span class="checagem-nome">${c.nome}</span>
          <span class="checagem-resumo">${c.situacao === 'erro' ? `erro de execução: ${c.resumo}` : c.resumo}</span>
        `;
        if (!temDetalhe) return html`<div class="checagem" data-situacao=${c.situacao}><div class="checagem-linha">${linha}</div></div>`;
        return html`
          <details class="checagem" data-situacao=${c.situacao} open=${c.situacao !== 'passou' && r.checagens.length === 1}>
            <summary class="checagem-linha">${linha}</summary>
            ${c.casos?.length > 0 && html`<ul class="casos">${c.casos.filter((x) => !x.passou).map((x) => html`<${Caso} c=${x} />`)}</ul>`}
            ${c.saida && html`<pre class="checagem-saida">${c.saida}</pre>`}
          </details>
        `;
      })}
    </div>
  `;
}

/** "87% parecido com Bruno Lima" a partir do resumo do pacote. */
export function textoPlagio(p, alunos) {
  if (!p) return '';
  const pc = Math.round(p.similaridade * 100);
  const outro = p.com.startsWith('referencias/')
    ? `a referência ${p.com.slice('referencias/'.length)}`
    : alunos.find((a) => a.id === p.com)?.nome || p.com;
  return `${pc}% parecido com ${outro}`;
}

/** Avisos do aluno: entrega, mudança depois da correção, plágio, executor. */
export function avisosEad(ead, aluno, alunos) {
  if (!ead) return [];
  const sub = ead.submissoes[aluno];
  const avisos = [];
  if (!sub) avisos.push({ tipo: 'sem', texto: 'Sem entrega no Moodle' });
  if (sub?.mudou) avisos.push({ tipo: 'mudou', texto: 'A submissão mudou depois da correção' });
  if (ead.plagio[aluno]) avisos.push({ tipo: 'plagio', texto: `⚠ ${textoPlagio(ead.plagio[aluno], alunos)}` });
  if (sub && ead.pipeline && !ead.relatorios[aluno]?.atual) avisos.push({ tipo: 'espera', texto: 'Checagens aguardando o executor' });
  return avisos;
}

/** Código e checagens de um aluno, num painel só. */
export function PainelCodigo({ avaliacao, aluno, ead, alunos, grande }) {
  const sub = ead?.submissoes[aluno];
  const rel = ead?.relatorios[aluno];
  const versao = `${sub?.alterada_em || 0}:${rel ? `${rel.atual}:${rel.checagens.map((c) => c.resumo).join('|')}` : ''}`;
  const { dados, erro } = useSubmissao(sub ? avaliacao : null, aluno, versao);
  const avisos = avisosEad(ead, aluno, alunos || []).filter((a) => a.tipo !== 'espera');
  return html`
    <div class=${cls('painel-codigo', grande && 'grande')}>
      ${avisos.length > 0 && html`<ul class="ead-avisos">${avisos.map((a) => html`<li data-tipo=${a.tipo}>${a.texto}</li>`)}</ul>`}
      ${ead?.pipeline && sub && html`
        <section class="painel-checagens" aria-label="Checagens">
          <${Checagens} relatorio=${dados?.relatorio} aguardando=${ead.aguardando > 0} />
        </section>
      `}
      ${sub
        ? erro && !dados
          ? html`<p class="codigo-vazio">${erro.message}</p>`
          : dados
            ? html`<${Codigo} key=${aluno} avaliacao=${avaliacao} aluno=${aluno} arquivos=${dados.arquivos} />`
            : html`<p class="codigo-vazio" aria-busy="true">Carregando o código…</p>`
        : html`<p class="codigo-vazio">Este aluno não tem submissão.</p>`}
      ${sub && html`<p class="codigo-rodape">${plural(sub.arquivos, 'arquivo', 'arquivos')} · recebido ${new Date(sub.alterada_em * 1000).toLocaleString('pt-BR', { dateStyle: 'short', timeStyle: 'short' })}</p>`}
    </div>
  `;
}
