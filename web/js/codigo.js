// Código do aluno (números de linha, destaque de Python) e o relatório das checagens.
// Usado ao lado do cartão na tela de avaliar e na tela "Acompanhar".
import { html, useState, useEffect, useRef } from '../vendor/preact-htm.js';
import Prism from '../vendor/prism.js';
import { api } from './api.js';
import { diferencas } from './diff.js';
import { cls, plural, guarda } from './util.js';

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
  // Depois de editar, o servidor devolve o estado novo: entra direto na tela.
  const trocar = (dados) => {
    cache.set(chave, { dados, versao: null });
    setEstado({ dados, erro: null });
  };
  return { ...estado, trocar };
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

function numeros(texto) {
  const n = texto.replace(/\n$/, '').split('\n').length;
  return Array.from({ length: n }, (_, i) => `${i + 1}\n`).join('');
}

function Destacado({ texto, caminho }) {
  const destacado = destacar(texto, caminho);
  return destacado === null ? html`<code>${texto}</code>` : html`<code dangerouslySetInnerHTML=${{ __html: destacado }}></code>`;
}

function Fonte({ texto, caminho, tamanho, href }) {
  if (texto === null || texto === undefined) {
    return html`<p class="codigo-binario">Arquivo binário ou grande demais para mostrar (${Math.ceil(tamanho / 1024)} KB). <a href=${href} download>Baixar</a></p>`;
  }
  return html`
    <div class="codigo-fonte">
      <pre class="codigo-numeros" aria-hidden="true">${numeros(texto)}</pre>
      <pre class="codigo-texto"><${Destacado} texto=${texto} caminho=${caminho} /></pre>
    </div>
  `;
}

/** Insere texto onde está o cursor sem perder o desfazer (Ctrl+Z) do navegador. */
function inserir(area, texto) {
  area.focus();
  // Com texto vazio, `insertText` não apaga a seleção: aí vale `delete`.
  const feito = texto ? document.execCommand('insertText', false, texto) : document.execCommand('delete');
  if (!feito) area.setRangeText(texto, area.selectionStart, area.selectionEnd, 'end');
}

/** Editor leve: um textarea transparente sobre o código destacado. Tab indenta,
    Enter mantém a indentação (e acrescenta depois de `:`), Ctrl+S salva. */
function Editor({ caminho, inicial, aoSalvar, aoCancelar }) {
  const [texto, setTexto] = useState(inicial);
  const [ocupado, setOcupado] = useState(false);
  const [erro, setErro] = useState('');
  const area = useRef(null);
  const fundo = useRef(null);
  const gutter = useRef(null);
  const mudou = texto !== inicial;

  useEffect(() => {
    area.current?.focus();
  }, []);
  // Trocar de aluno ou de tela com a edição por salvar pergunta antes.
  useEffect(() => {
    guarda.ativa = mudou;
    return () => {
      guarda.ativa = false;
    };
  }, [mudou]);

  function rolar() {
    const t = area.current;
    fundo.current.scrollTop = t.scrollTop;
    fundo.current.scrollLeft = t.scrollLeft;
    gutter.current.scrollTop = t.scrollTop;
  }

  async function salvar() {
    if (ocupado) return;
    if (!mudou) {
      aoCancelar();
      return;
    }
    setOcupado(true);
    setErro('');
    try {
      guarda.ativa = false;
      await aoSalvar(texto);
    } catch (e) {
      guarda.ativa = true;
      setErro(e.message);
      setOcupado(false);
    }
  }

  function aoTeclar(e) {
    // As teclas do editor não chegam aos atalhos da tela de avaliar.
    e.stopPropagation();
    const t = area.current;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      salvar();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      if (!mudou) aoCancelar();
    } else if (e.key === 'Tab' && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      if (!e.shiftKey) {
        inserir(t, '    ');
      } else {
        const cursor = t.selectionStart;
        const inicio = t.value.lastIndexOf('\n', cursor - 1) + 1;
        const espacos = t.value.slice(inicio).match(/^ {1,4}/)?.[0].length || 0;
        if (espacos) {
          t.setSelectionRange(inicio, inicio + espacos);
          inserir(t, '');
          // O cursor recua junto com a linha, em vez de ir para o começo dela.
          const novo = Math.max(inicio, cursor - espacos);
          t.setSelectionRange(novo, novo);
        }
      }
      setTexto(t.value);
    } else if (e.key === 'Enter' && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
      e.preventDefault();
      const linha = t.value.slice(t.value.lastIndexOf('\n', t.selectionStart - 1) + 1, t.selectionStart);
      let recuo = linha.match(/^[ \t]*/)[0];
      if (/:\s*(#.*)?$/.test(linha)) recuo += '    ';
      inserir(t, `\n${recuo}`);
      setTexto(t.value);
    }
  }

  return html`
    <div class="editor">
      <div class="editor-corpo">
        <pre class="codigo-numeros" ref=${gutter} aria-hidden="true">${numeros(`${texto}\n`)}</pre>
        <div class="editor-camadas">
          <pre class="codigo-texto editor-fundo" ref=${fundo} aria-hidden="true"><${Destacado} texto=${`${texto}\n`} caminho=${caminho} /></pre>
          <textarea class="editor-area" ref=${area} value=${texto} wrap="off" spellcheck="false" autocapitalize="off" autocomplete="off" autocorrect="off"
            aria-label=${`Editar ${caminho}`} onInput=${(e) => setTexto(e.target.value)} onScroll=${rolar} onKeyDown=${aoTeclar}></textarea>
        </div>
      </div>
      <div class="editor-barra">
        <span class="editor-dica">O original do aluno fica guardado. <kbd>Ctrl</kbd>+<kbd>S</kbd> salva.</span>
        ${erro && html`<span class="erro" role="alert">${erro}</span>`}
        <button type="button" class="botao pequeno editor-cancelar" disabled=${ocupado} onClick=${() => (!mudou || confirm('Descartar a edição?')) && aoCancelar()}>Cancelar</button>
        <button type="button" class="botao pequeno primario" disabled=${ocupado} onClick=${salvar}>${ocupado ? 'Salvando…' : 'Salvar e checar de novo'}</button>
      </div>
    </div>
  `;
}

/** Um arquivo: ver, ver o original (se o professor editou), editar e desfazer. */
function Arquivo({ arquivo, href, caminhoApi, podeEditar, aoMudar }) {
  const [verOriginal, setVerOriginal] = useState(false);
  const [editando, setEditando] = useState(false);
  const [erro, setErro] = useState('');
  const texto = verOriginal && arquivo.editado ? arquivo.original : arquivo.texto;
  const editavel = podeEditar && typeof arquivo.texto === 'string';

  async function salvar(novo) {
    const dados = await api('PUT', caminhoApi, new Blob([novo], { type: 'text/plain;charset=utf-8' }));
    setEditando(false);
    setVerOriginal(false);
    aoMudar(dados);
  }

  async function desfazer() {
    if (!confirm(`Voltar ${arquivo.caminho} ao que o aluno enviou? A sua edição se perde.`)) return;
    try {
      aoMudar(await api('DELETE', caminhoApi));
      setVerOriginal(false);
      setErro('');
    } catch (e) {
      setErro(e.message);
    }
  }

  if (editando) {
    return html`<${Editor} caminho=${arquivo.caminho} inicial=${arquivo.texto} aoSalvar=${salvar} aoCancelar=${() => setEditando(false)} />`;
  }
  return html`
    ${(editavel || arquivo.editado) && html`
      <div class="codigo-barra">
        ${arquivo.editado && html`<span class="codigo-editado">${verOriginal ? 'Original do aluno' : 'Editado'}</span>`}
        ${arquivo.editado && html`<button type="button" class="codigo-acao" onClick=${() => setVerOriginal(!verOriginal)}>${verOriginal ? 'Ver editado' : 'Ver original'}</button>`}
        ${arquivo.editado && podeEditar && html`<button type="button" class="codigo-acao" onClick=${desfazer}>Desfazer edição</button>`}
        ${erro && html`<span class="erro">${erro}</span>`}
        ${editavel && html`<button type="button" class="codigo-acao principal" onClick=${() => setEditando(true)}>Editar</button>`}
      </div>
    `}
    <${Fonte} texto=${texto} caminho=${arquivo.caminho} tamanho=${arquivo.tamanho} href=${href} />
  `;
}

/** Abas por arquivo e o arquivo escolhido. Com `aoMudar`, os arquivos podem ser editados. */
export function Codigo({ avaliacao, aluno, arquivos, aoMudar }) {
  const lista = ordenar(arquivos || []);
  const [escolhido, setEscolhido] = useState(null);
  const atual = lista.find((a) => a.caminho === escolhido) || lista[0];
  if (!atual) return html`<p class="codigo-vazio">Sem arquivos.</p>`;
  const caminhoApi = (a) => `/avaliacoes/${encodeURIComponent(avaliacao)}/submissoes/${encodeURIComponent(aluno)}/${a.caminho.split('/').map(encodeURIComponent).join('/')}`;
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
      <${Arquivo} key=${atual.caminho} arquivo=${atual} href=${`api${caminhoApi(atual)}`} caminhoApi=${caminhoApi(atual)}
        podeEditar=${!!aoMudar} aoMudar=${aoMudar} />
    </div>
  `;
}

const SINAL = { passou: '✓', falhou: '✗', erro: '!' };

/** Uma saída com as linhas e os trechos que diferem do outro lado coloridos. */
function Saida({ linhas, lado }) {
  return html`<pre class=${cls('saida-diff', lado)}>${linhas.map((l) => html`<span class=${cls('diff-linha', l.mudou && 'mudou')}>${
    l.mudou && !l.partes.length ? html`<mark>⏎</mark>` : l.partes.map((p) => (p.mudou ? html`<mark>${p.texto}</mark>` : p.texto))
  }</span>`)}</pre>`;
}

function Caso({ c }) {
  // Saída cortada no começo ("…") não alinha com a esperada: aí vai sem cores.
  const d = !c.passou && c.obtido && !c.obtido.startsWith('…') && diferencas(c.esperado, c.obtido);
  return html`
    <li class=${cls('caso', c.passou ? 'passou' : 'falhou')}>
      <p><strong>${c.nome}</strong>${!c.passou && html` — ${c.motivo}`}</p>
      ${!c.passou && html`
        <div class="caso-par">
          <div><span>Entrada</span><pre>${c.entrada || '(vazia)'}</pre></div>
          <div><span>Esperado</span>${d ? html`<${Saida} linhas=${d.esperado} lado="esperado" />` : html`<pre>${c.esperado}</pre>`}</div>
          <div><span>Obtido</span>${d ? html`<${Saida} linhas=${d.obtido} lado="obtido" />` : html`<pre>${c.obtido || '(nada)'}</pre>`}</div>
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
  if (sub?.edicao) avisos.push({ tipo: 'editado', texto: `Código editado por ${sub.edicao.por}`, dica: 'As checagens usam a versão editada; o original do aluno fica guardado.' });
  if (ead.plagio[aluno]) avisos.push({ tipo: 'plagio', texto: `⚠ ${textoPlagio(ead.plagio[aluno], alunos)}` });
  if (sub && ead.pipeline && !ead.relatorios[aluno]?.atual) avisos.push({ tipo: 'espera', texto: 'Checagens aguardando o executor' });
  return avisos;
}

/** Código e checagens de um aluno, num painel só. */
/** `semAvisos`: quando a tela já mostra os avisos do aluno em outro lugar. */
export function PainelCodigo({ avaliacao, aluno, ead, alunos, grande, aoMudar, semAvisos }) {
  const sub = ead?.submissoes[aluno];
  const rel = ead?.relatorios[aluno];
  const versao = `${sub?.alterada_em || 0}:${sub?.edicao?.em || 0}:${rel ? `${rel.atual}:${rel.checagens.map((c) => c.resumo).join('|')}` : ''}`;
  const { dados, erro, trocar } = useSubmissao(sub ? avaliacao : null, aluno, versao);
  // Depois de uma edição, o painel mostra o código novo na hora e a tela recarrega
  // o resumo (a checagem volta a "aguardando o executor").
  const mudou = aoMudar && ((d) => {
    trocar(d);
    aoMudar();
  });
  const avisos = semAvisos ? [] : avisosEad(ead, aluno, alunos || []).filter((a) => a.tipo !== 'espera');
  return html`
    <div class=${cls('painel-codigo', grande && 'grande')}>
      ${avisos.length > 0 && html`<ul class="ead-avisos">${avisos.map((a) => html`<li data-tipo=${a.tipo} title=${a.dica}>${a.texto}</li>`)}</ul>`}
      ${ead?.pipeline && sub && html`
        <section class="painel-checagens" aria-label="Checagens">
          <${Checagens} relatorio=${dados?.relatorio} aguardando=${ead.aguardando > 0} />
        </section>
      `}
      ${sub
        ? erro && !dados
          ? html`<p class="codigo-vazio">${erro.message}</p>`
          : dados
            ? html`<${Codigo} key=${aluno} avaliacao=${avaliacao} aluno=${aluno} arquivos=${dados.arquivos} aoMudar=${mudou} />`
            : html`<p class="codigo-vazio" aria-busy="true">Carregando o código…</p>`
        : html`<p class="codigo-vazio">Este aluno não tem submissão.</p>`}
      ${sub && html`<p class="codigo-rodape">${plural(sub.arquivos, 'arquivo', 'arquivos')} · recebido ${new Date(sub.alterada_em * 1000).toLocaleString('pt-BR', { dateStyle: 'short', timeStyle: 'short' })}</p>`}
    </div>
  `;
}
