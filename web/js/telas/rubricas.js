// Rubricas: lista e editor. Cada rubrica é um arquivo rubricas/<id>.toml.
import { html, useState, useEffect } from '../../vendor/preact-htm.js';
import { api } from '../api.js';
import { TIPOS, maximo, maximoRubrica, ehRoteiro, contarCriterios, resumoItens } from '../nota.js';
import { ir, rota, num, campo, lerNum, chave, cls, pts, guarda } from '../util.js';
import { useDados, Estado, Titulo, Vazio, ListaArquivadas, AvisoArquivada, arquivar, Compartilhar } from './comum.js';
import { sessao, pode } from '../sessao.js';

function LinhaRubrica(r) {
  return html`
    <li>
      <a class="linha" href=${`#${rota('rubricas', r.id)}`}>
        <span class="linha-titulo">${r.titulo}</span>
        <span class="linha-meta">${resumoItens(r)}, ${pts(maximoRubrica(r))}</span>
      </a>
    </li>
  `;
}

export function ListaRubricas() {
  const { dados, ...estado } = useDados('/rubricas');
  const ativas = dados?.rubricas.filter((r) => !r.arquivada) || [];
  return html`
    <div class="tela">
    <${Titulo} titulo="Rubricas">
      ${sessao.pode('criar') && html`<a class="botao primario" href="#/rubricas/nova">Nova rubrica</a>`}
    <//>
    <${Estado} ...${estado} />
    ${dados?.problemas.map((p) => html`<p class="aviso erro" role="alert">Arquivo ignorado — ${p}</p>`)}
    ${dados && !dados.rubricas.length && html`
      <${Vazio} titulo=${sessao.pode('criar') ? 'Nenhuma rubrica ainda' : 'Nenhuma rubrica compartilhada com você'}>
        <p class="suave">Monte uma aqui ou coloque arquivos <code>.toml</code> na pasta <code>rubricas</code>.</p>
      <//>
    `}
    ${ativas.length > 0 && html`<ul class="lista">${ativas.map(LinhaRubrica)}</ul>`}
    ${dados && html`<${ListaArquivadas} itens=${dados.rubricas.filter((r) => r.arquivada)} linha=${LinhaRubrica} />`}
    </div>
  `;
}

// ----- rascunho: a rubrica em edição, com números como texto e chaves locais -----

const MODELOS = {
  2: ['Atende', 'Não atende'],
  3: ['Completo', 'Parcial', 'Insuficiente'],
  4: ['Excelente', 'Bom', 'Parcial', 'Insuficiente'],
  5: ['Excelente', 'Bom', 'Regular', 'Fraco', 'Insuficiente'],
};

function niveisDoModelo(n, max) {
  return MODELOS[n].map((rotulo, i) => ({
    _k: chave(),
    id: '',
    rotulo,
    descritor: '',
    pontos: campo(Math.round((max * (n - 1 - i) * 100) / (n - 1)) / 100),
  }));
}

function criterioNovo() {
  return { _k: chave(), id: '', nome: '', descricao: '', peso: '1', escala: { tipo: 'niveis', niveis: niveisDoModelo(4, 3) } };
}

function passoNovo() {
  return { _k: chave(), tipo: 'roteiro', id: '', nome: '', descricao: '' };
}

function paraRascunho(r) {
  return {
    titulo: r.titulo,
    descricao: r.descricao || '',
    criterios: r.criterios.map((c) => {
      if (ehRoteiro(c)) return { _k: chave(), tipo: 'roteiro', id: c.id, nome: c.nome, descricao: c.descricao || '' };
      const e = c.escala;
      let escala;
      if (e.tipo === 'niveis') {
        escala = { tipo: 'niveis', niveis: e.niveis.map((n) => ({ _k: chave(), id: n.id, rotulo: n.rotulo, descritor: n.descritor || '', pontos: campo(n.pontos) })) };
      } else if (e.tipo === 'binaria') {
        escala = { tipo: 'binaria', pontos: campo(e.pontos) };
      } else if (e.tipo === 'livre') {
        escala = { tipo: 'livre', max: campo(e.max), passo: e.passo ? campo(e.passo) : '' };
      } else {
        escala = { tipo: 'ponto_unico', esperado: e.esperado, max: campo(e.max) };
      }
      return { _k: chave(), id: c.id, nome: c.nome, descricao: c.descricao || '', peso: campo(c.peso ?? 1), escala };
    }),
  };
}

function doRascunho(r) {
  return {
    titulo: r.titulo.trim(),
    descricao: r.descricao.trim(),
    criterios: r.criterios.map((c) => {
      if (ehRoteiro(c)) return { tipo: 'roteiro', id: c.id, nome: c.nome.trim(), descricao: c.descricao.trim() };
      const e = c.escala;
      let escala;
      if (e.tipo === 'niveis') {
        escala = { tipo: 'niveis', niveis: e.niveis.map((n) => ({ id: n.id, rotulo: n.rotulo.trim(), descritor: n.descritor.trim(), pontos: lerNum(n.pontos) })) };
      } else if (e.tipo === 'binaria') {
        escala = { tipo: 'binaria', pontos: lerNum(e.pontos) };
      } else if (e.tipo === 'livre') {
        escala = { tipo: 'livre', max: lerNum(e.max) };
        if (String(e.passo).trim()) escala.passo = lerNum(e.passo);
      } else {
        escala = { tipo: 'ponto_unico', esperado: e.esperado.trim(), max: lerNum(e.max) };
      }
      return { id: c.id, nome: c.nome.trim(), descricao: c.descricao.trim(), peso: lerNum(c.peso), escala };
    }),
  };
}

/** Primeiro número que não dá para ler, para avisar antes de mandar ao servidor. */
function numeroInvalido(rubrica) {
  for (const c of rubrica.criterios) {
    if (ehRoteiro(c)) continue;
    const nome = c.nome || 'sem nome';
    if (!Number.isFinite(c.peso)) return `Critério “${nome}”: o peso precisa ser um número`;
    const e = c.escala;
    const valores = e.tipo === 'niveis' ? e.niveis.map((n) => n.pontos) : e.tipo === 'binaria' ? [e.pontos] : [e.max, ...(e.passo === undefined ? [] : [e.passo])];
    if (valores.some((v) => !Number.isFinite(v))) return `Critério “${nome}”: preencha os pontos com números`;
  }
  return '';
}

function maximoRascunho(c) {
  if (ehRoteiro(c)) return 0;
  try {
    const m = maximo({ escala: doRascunho({ titulo: '', descricao: '', criterios: [c] }).criterios[0].escala });
    return Number.isFinite(m) ? m : 0;
  } catch {
    return 0;
  }
}

/** Troca o tipo de escala guardando a anterior: voltar a ela devolve o que estava preenchido. */
function trocarTipo(c, tipo) {
  const guardadas = { ...(c._guardadas || {}), [c.escala.tipo]: c.escala };
  let escala = guardadas[tipo];
  if (!escala) {
    const max = maximoRascunho(c) || 1;
    const m = campo(max);
    if (tipo === 'niveis') escala = { tipo, niveis: niveisDoModelo(4, max) };
    else if (tipo === 'binaria') escala = { tipo, pontos: m };
    else if (tipo === 'livre') escala = { tipo, max: m, passo: '' };
    else escala = { tipo, esperado: c.descricao || '', max: m };
  }
  return { ...c, escala, _guardadas: guardadas };
}

function mover(lista, i, delta) {
  const j = i + delta;
  if (j < 0 || j >= lista.length) return lista;
  const copia = [...lista];
  [copia[i], copia[j]] = [copia[j], copia[i]];
  return copia;
}

function Numero({ valor, aoMudar, rotulo, ...resto }) {
  return html`<input class="numero" inputmode="decimal" aria-label=${rotulo} value=${valor} onInput=${(e) => aoMudar(e.target.value)} ...${resto} />`;
}

function EditorNiveis({ escala, aoMudar, max }) {
  const niveis = escala.niveis;
  const mudar = (i, campo, valor) => aoMudar({ ...escala, niveis: niveis.map((n, j) => (j === i ? { ...n, [campo]: valor } : n)) });
  return html`
    <div class="niveis">
      ${niveis.map((n, i) => html`
        <div class="nivel" key=${n._k}>
          <input class="nivel-rotulo" placeholder="Rótulo" aria-label="Rótulo do nível" value=${n.rotulo} onInput=${(e) => mudar(i, 'rotulo', e.target.value)} />
          <${Numero} rotulo="Pontos do nível" valor=${n.pontos} aoMudar=${(v) => mudar(i, 'pontos', v)} />
          <div class="nivel-botoes">
            <button type="button" class="icone" title="Subir" aria-label="Subir nível" disabled=${i === 0} onClick=${() => aoMudar({ ...escala, niveis: mover(niveis, i, -1) })}>↑</button>
            <button type="button" class="icone" title="Descer" aria-label="Descer nível" disabled=${i === niveis.length - 1} onClick=${() => aoMudar({ ...escala, niveis: mover(niveis, i, 1) })}>↓</button>
            <button type="button" class="icone" title="Remover" aria-label="Remover nível" disabled=${niveis.length <= 2} onClick=${() => aoMudar({ ...escala, niveis: niveis.filter((_, j) => j !== i) })}>×</button>
          </div>
          <textarea class="nivel-descritor" rows="2" placeholder="O que caracteriza este nível" aria-label="Descritor do nível"
            value=${n.descritor} onInput=${(e) => mudar(i, 'descritor', e.target.value)}></textarea>
        </div>
      `)}
      <div class="niveis-acoes">
        <button type="button" class="botao pequeno" onClick=${() => aoMudar({ ...escala, niveis: [...niveis, { _k: chave(), id: '', rotulo: '', descritor: '', pontos: '0' }] })}>Adicionar nível</button>
        <span class="suave">Recomeçar com</span>
        ${[3, 4, 5].map((n) => html`<button type="button" class="botao-texto" onClick=${() => aoMudar({ ...escala, niveis: niveisDoModelo(n, max || 1) })}>${n} níveis</button>`)}
      </div>
    </div>
  `;
}

function EditorCriterio({ c, i, ordem, total, aberto, alternar, aoMudar, aoMover, aoRemover, aoDuplicar, podeRemover }) {
  const max = maximoRascunho(c);
  const e = c.escala;
  const mudarEscala = (campo, valor) => aoMudar({ ...c, escala: { ...e, [campo]: valor } });
  return html`
    <section class=${cls('criterio', aberto && 'aberto')}>
      <div class="criterio-linha">
      <button type="button" class="criterio-cabeca" aria-expanded=${aberto} onClick=${alternar}>
        <span class="criterio-ordem">${ordem}</span>
        <span class="criterio-nome">${c.nome || 'Critério sem nome'}</span>
        <span class="criterio-resumo">${TIPOS[e.tipo]}, ${pts(max)}</span>
      </button>
        <span class="criterio-setas">
          <button type="button" class="icone" title="Subir" aria-label=${`Subir “${c.nome || 'sem nome'}”`} disabled=${i === 0} onClick=${() => aoMover(-1)}>↑</button>
          <button type="button" class="icone" title="Descer" aria-label=${`Descer “${c.nome || 'sem nome'}”`} disabled=${i === total - 1} onClick=${() => aoMover(1)}>↓</button>
        </span>
      </div>
      ${aberto && html`
        <div class="criterio-corpo">
          <label class="campo">
            <span>Nome do critério</span>
            <input required value=${c.nome} placeholder="Modularização" onInput=${(ev) => aoMudar({ ...c, nome: ev.target.value })} />
          </label>
          <label class="campo">
            <span>O que observar</span>
            <textarea rows="2" value=${c.descricao} placeholder="Aparece no cartão durante a avaliação" onInput=${(ev) => aoMudar({ ...c, descricao: ev.target.value })}></textarea>
          </label>
          <fieldset class="campo">
            <legend>Como avaliar</legend>
            <div class="segmentos">
              ${Object.entries(TIPOS).map(([tipo, nome]) => html`
                <label class=${cls('segmento', e.tipo === tipo && 'marcado')}>
                  <input type="radio" name=${`tipo-${c._k}`} checked=${e.tipo === tipo} onChange=${() => aoMudar(trocarTipo(c, tipo))} />
                  ${nome}
                </label>
              `)}
            </div>
          </fieldset>
          ${e.tipo === 'niveis' && html`<${EditorNiveis} escala=${e} max=${max} aoMudar=${(escala) => aoMudar({ ...c, escala })} />`}
          ${e.tipo === 'binaria' && html`
            <label class="campo curto"><span>Pontos se atender</span><${Numero} rotulo="Pontos se atender" valor=${e.pontos} aoMudar=${(v) => mudarEscala('pontos', v)} /></label>
          `}
          ${e.tipo === 'livre' && html`
            <div class="par">
              <label class="campo curto"><span>Nota máxima</span><${Numero} rotulo="Nota máxima" valor=${e.max} aoMudar=${(v) => mudarEscala('max', v)} /></label>
              <label class="campo curto"><span>Passo</span><${Numero} rotulo="Passo" valor=${e.passo} placeholder="automático" aoMudar=${(v) => mudarEscala('passo', v)} /></label>
            </div>
          `}
          ${e.tipo === 'ponto_unico' && html`
            <label class="campo">
              <span>Desempenho esperado</span>
              <textarea rows="2" required value=${e.esperado} onInput=${(ev) => mudarEscala('esperado', ev.target.value)}></textarea>
            </label>
            <label class="campo curto"><span>Nota máxima</span><${Numero} rotulo="Nota máxima" valor=${e.max} aoMudar=${(v) => mudarEscala('max', v)} /></label>
          `}
          <details class="avancado">
            <summary>Peso</summary>
            <label class="campo curto">
              <span>Multiplica os pontos deste critério</span>
              <${Numero} rotulo="Peso" valor=${c.peso} aoMudar=${(v) => aoMudar({ ...c, peso: v })} />
            </label>
          </details>
          <div class="criterio-acoes">
            <button type="button" class="botao pequeno" onClick=${aoDuplicar}>Duplicar</button>
            <button type="button" class="botao pequeno perigo" disabled=${!podeRemover} onClick=${aoRemover}>Remover</button>
          </div>
        </div>
      `}
    </section>
  `;
}

function EditorPasso({ c, i, total, aberto, alternar, aoMudar, aoMover, aoRemover }) {
  return html`
    <section class=${cls('criterio', 'passo', aberto && 'aberto')}>
      <div class="criterio-linha">
      <button type="button" class="criterio-cabeca" aria-expanded=${aberto} onClick=${alternar}>
        <span class="criterio-ordem" aria-label="Passo de roteiro">▸</span>
        <span class="criterio-nome">${c.nome || 'Passo sem nome'}</span>
        <span class="criterio-resumo">roteiro, não vale nota</span>
      </button>
        <span class="criterio-setas">
          <button type="button" class="icone" title="Subir" aria-label=${`Subir “${c.nome || 'sem nome'}”`} disabled=${i === 0} onClick=${() => aoMover(-1)}>↑</button>
          <button type="button" class="icone" title="Descer" aria-label=${`Descer “${c.nome || 'sem nome'}”`} disabled=${i === total - 1} onClick=${() => aoMover(1)}>↓</button>
        </span>
      </div>
      ${aberto && html`
        <div class="criterio-corpo">
          <label class="campo">
            <span>Passo</span>
            <input required value=${c.nome} placeholder="Rode com entrada vazia" onInput=${(ev) => aoMudar({ ...c, nome: ev.target.value })} />
          </label>
          <label class="campo">
            <span>Instruções</span>
            <textarea rows="4" value=${c.descricao} placeholder=${'Execute `python main.py` e aperte Enter.\nDeve pedir a entrada de novo.'}
              onInput=${(ev) => aoMudar({ ...c, descricao: ev.target.value })}></textarea>
            <small class="suave">Aceita <code>\`código\`</code>, blocos <code>\`\`\`</code> com botão de copiar, listas com <code>-</code>, <strong>**negrito**</strong> e <em>*itálico*</em>.</small>
          </label>
          <div class="criterio-acoes">
            <button type="button" class="botao pequeno perigo" onClick=${aoRemover}>Remover</button>
          </div>
        </div>
      `}
    </section>
  `;
}

export function EditorRubrica({ id }) {
  const { dados, erro: erroCarga, carregando, recarregar } = useDados(id ? `/rubricas/${encodeURIComponent(id)}` : null);
  const [r, setR] = useState(() => (id ? null : { titulo: '', descricao: '', criterios: [criterioNovo()] }));
  const [abertos, setAbertos] = useState(() => new Set());
  const [sujo, setSujo] = useState(false);
  const [erro, setErro] = useState('');
  const [ocupado, setOcupado] = useState(false);
  const [salvo, setSalvo] = useState(false);
  // À parte do rascunho: arquivar não mexe nas alterações por salvar.
  const [arquivada, setArquivada] = useState(false);

  // A navegação pergunta antes de sair enquanto houver alterações não salvas.
  useEffect(() => {
    guarda.ativa = sujo;
    return () => {
      guarda.ativa = false;
    };
  }, [sujo]);

  /** Troca o rascunho pelo que veio do servidor, mantendo abertos os mesmos critérios. */
  function receber(rubrica) {
    const posicoes = r ? r.criterios.map((c, i) => (abertos.has(c._k) ? i : -1)).filter((i) => i >= 0) : [];
    const rascunho = paraRascunho(rubrica);
    setR(rascunho);
    setAbertos(new Set(posicoes.map((i) => rascunho.criterios[i]?._k).filter(Boolean)));
    setSujo(false);
  }

  useEffect(() => {
    if (dados) {
      receber(dados);
      setArquivada(!!dados.arquivada);
    } else if (!id && r) {
      setAbertos(new Set([r.criterios[0]._k]));
    }
  }, [dados]);

  useEffect(() => {
    if (!sujo) return undefined;
    const avisar = (e) => {
      e.preventDefault();
      e.returnValue = '';
    };
    window.addEventListener('beforeunload', avisar);
    return () => window.removeEventListener('beforeunload', avisar);
  }, [sujo]);

  function mudar(novo) {
    guarda.ativa = true; // já, sem esperar o próximo desenho da tela
    setR(novo);
    setSujo(true);
    setSalvo(false);
  }

  const mudarCriterios = (criterios) => mudar({ ...r, criterios });

  function alternar(k) {
    const s = new Set(abertos);
    if (s.has(k)) s.delete(k);
    else s.add(k);
    setAbertos(s);
  }

  function adicionar(c = criterioNovo()) {
    mudarCriterios([...r.criterios, c]);
    setAbertos(new Set([...abertos, c._k]));
  }

  function duplicar(i) {
    const origem = r.criterios[i];
    const copia = structuredClone(origem);
    copia._k = chave();
    copia.id = '';
    copia.nome = origem.nome ? `${origem.nome} (cópia)` : '';
    if (copia.escala?.niveis) copia.escala.niveis.forEach((n) => { n._k = chave(); });
    const lista = [...r.criterios];
    lista.splice(i + 1, 0, copia);
    mudarCriterios(lista);
    setAbertos(new Set([...abertos, copia._k]));
  }

  async function salvar(e) {
    e?.preventDefault();
    const corpo = doRascunho(r);
    const problema = numeroInvalido(corpo);
    if (problema) {
      setErro(problema);
      return;
    }
    setOcupado(true);
    setErro('');
    try {
      const gravada = id ? await api('PUT', `/rubricas/${encodeURIComponent(id)}`, corpo) : await api('POST', '/rubricas', corpo);
      guarda.ativa = false;
      setSujo(false);
      setSalvo(true);
      if (!id) ir(rota('rubricas', gravada.id), true);
      else receber(gravada);
    } catch (err) {
      setErro(err.message);
    }
    setOcupado(false);
  }

  async function apagar() {
    if (!confirm(`Apagar a rubrica “${r.titulo}”? Avaliações que já a usam mantêm a cópia delas.`)) return;
    try {
      await api('DELETE', `/rubricas/${encodeURIComponent(id)}`);
      guarda.ativa = false;
      setSujo(false);
      ir('/rubricas');
    } catch (err) {
      setErro(err.message);
    }
  }

  async function alternarArquivada() {
    setErro('');
    try {
      await arquivar(`/rubricas/${encodeURIComponent(id)}`, !arquivada);
      setArquivada(!arquivada);
    } catch (err) {
      setErro(err.message);
    }
  }

  async function copiar() {
    try {
      const corpo = doRascunho(r);
      corpo.titulo = `${corpo.titulo} (cópia)`;
      const nova = await api('POST', '/rubricas', corpo);
      ir(rota('rubricas', nova.id));
    } catch (err) {
      setErro(err.message);
    }
  }

  if (id && (carregando || erroCarga)) {
    return html`
    <div class="tela">
      <${Titulo} titulo="Rubrica" voltar=${{ href: '#/rubricas', texto: 'Rubricas' }} />
      <${Estado} carregando=${carregando} erro=${erroCarga} recarregar=${recarregar} />
    </div>
  `;
  }
  if (!r) return null;

  // Quem só pode ver enxerga tudo aberto e nada editável.
  const soLeitura = !!id && !pode(dados, 'editar');
  const soCriterios = contarCriterios(r);
  const total = r.criterios.reduce((s, c) => s + maximoRascunho(c) * (Number.isFinite(lerNum(c.peso)) ? lerNum(c.peso) : 0), 0);

  return html`
    <div class="tela">
    <${Titulo} titulo=${id ? r.titulo || 'Rubrica' : 'Nova rubrica'} voltar=${{ href: '#/rubricas', texto: 'Rubricas' }}>
      ${id && sessao.pode('criar') && html`<button type="button" class="botao" onClick=${copiar} disabled=${sujo}>Duplicar</button>`}
      ${id && !arquivada && pode(dados, 'arquivar') && html`<button type="button" class="botao" onClick=${alternarArquivada}>Arquivar</button>`}
      ${id && pode(dados, 'apagar') && html`<button type="button" class="botao perigo" onClick=${apagar}>Apagar</button>`}
    <//>
    ${soLeitura && html`<p class="aviso">${sessao.pode('criar') ? 'Você pode ver esta rubrica e usá-la nas suas avaliações, mas não alterá-la. Para adaptar, duplique.' : 'Você pode ver esta rubrica, mas não alterá-la.'}</p>`}
    ${arquivada && pode(dados, 'arquivar') && html`<${AvisoArquivada} texto="Rubrica arquivada: fica fora da lista e da escolha de rubricas nas avaliações. Quem já a usa continua igual." aoDesarquivar=${alternarArquivada} />`}
    <form class="formulario editor" onSubmit=${salvar}>
      <fieldset class="limpo" disabled=${soLeitura}>
      <label class="campo">
        <span>Título</span>
        <input required maxlength="120" placeholder="Trabalho final: código" value=${r.titulo} onInput=${(e) => mudar({ ...r, titulo: e.target.value })} />
      </label>
      <label class="campo">
        <span>Descrição</span>
        <textarea rows="2" value=${r.descricao} onInput=${(e) => mudar({ ...r, descricao: e.target.value })}></textarea>
      </label>

      <h2>Critérios e roteiro</h2>
      <p class="suave">Cada passo de roteiro aparece na avaliação logo antes do critério que vem depois dele.</p>
      <div class="criterios">
        ${r.criterios.map((c, i) => {
          const props = {
            c, i, total: r.criterios.length, aberto: soLeitura || abertos.has(c._k), alternar: () => alternar(c._k),
            aoMudar: (novo) => mudarCriterios(r.criterios.map((x) => (x._k === c._k ? novo : x))),
            aoMover: (d) => mudarCriterios(mover(r.criterios, i, d)),
            aoRemover: () => mudarCriterios(r.criterios.filter((x) => x._k !== c._k)),
          };
          return ehRoteiro(c)
            ? html`<${EditorPasso} key=${c._k} ...${props} />`
            : html`<${EditorCriterio} key=${c._k} ...${props} ordem=${r.criterios.slice(0, i + 1).filter((x) => !ehRoteiro(x)).length} podeRemover=${soCriterios > 1} aoDuplicar=${() => duplicar(i)} />`;
        })}
      </div>
      <div class="acoes-lista">
        <button type="button" class="botao" onClick=${() => adicionar()}>Adicionar critério</button>
        <button type="button" class="botao" onClick=${() => adicionar(passoNovo())}>Adicionar passo de roteiro</button>
      </div>

      </fieldset>
      ${!soLeitura && html`<div class="barra-salvar">
        <span class="barra-info">
          ${resumoItens(r)}, ${pts(total)}
          ${sujo && html`<span class="pendente"> — alterações não salvas</span>`}
          ${salvo && !sujo && html`<span class="ok"> — salvo</span>`}
        </span>
        ${erro && html`<span class="erro" role="alert">${erro}</span>`}
        <button class="botao primario" disabled=${ocupado || (!sujo && !!id)}>${ocupado ? 'Salvando…' : id ? 'Salvar' : 'Criar rubrica'}</button>
      </div>`}
    </form>
    ${id && pode(dados, 'compartilhar') && html`<${Compartilhar} caminho=${`/rubricas/${encodeURIComponent(id)}`} pasta="rubricas" />`}
    </div>
  `;
}
