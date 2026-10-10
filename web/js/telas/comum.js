// Peças usadas por várias telas.
import { html, useState, useEffect, useCallback, useRef } from '../../vendor/preact-htm.js';
import { api } from '../api.js';
import { sessao, descreverPapel } from '../sessao.js';
import { cls, guarda, rota } from '../util.js';

/** Busca um recurso e acompanha carregamento e erro. */
export function useDados(caminho) {
  const [estado, setEstado] = useState({ dados: null, erro: null, carregando: true });
  const carregar = useCallback(async () => {
    if (!caminho) {
      setEstado({ dados: null, erro: null, carregando: false });
      return;
    }
    setEstado((e) => ({ ...e, carregando: true }));
    try {
      setEstado({ dados: await api('GET', caminho), erro: null, carregando: false });
    } catch (e) {
      setEstado({ dados: null, erro: e, carregando: false });
    }
  }, [caminho]);
  useEffect(() => {
    carregar();
  }, [carregar]);
  return { ...estado, recarregar: carregar };
}

/**
 * Uma ação que fala com o servidor: `rodar(fn)` marca `ocupado` enquanto ela
 * corre e guarda a mensagem do erro. Devolve o que `fn` devolveu, ou `undefined`
 * se ela falhou.
 */
export function useAcao() {
  const [ocupado, setOcupado] = useState(false);
  const [erro, setErro] = useState('');
  async function rodar(fn) {
    setOcupado(true);
    setErro('');
    try {
      return await fn();
    } catch (e) {
      setErro(e.message);
      return undefined;
    } finally {
      setOcupado(false);
    }
  }
  return { rodar, ocupado, erro, setErro };
}

/** Com edição por salvar, sair da tela (ou fechar a página) pergunta antes. */
export function useGuarda(sujo) {
  useEffect(() => {
    guarda.ativa = sujo;
    return () => {
      guarda.ativa = false;
    };
  }, [sujo]);
  useEffect(() => {
    if (!sujo) return undefined;
    const avisar = (e) => {
      e.preventDefault();
      e.returnValue = '';
    };
    window.addEventListener('beforeunload', avisar);
    return () => window.removeEventListener('beforeunload', avisar);
  }, [sujo]);
}

/** Telas de tela inteira (avaliar, acompanhar): a página não rola, quem se move é o conteúdo. */
export function useTelaInteira() {
  useEffect(() => {
    document.documentElement.classList.add('avaliando');
    return () => document.documentElement.classList.remove('avaliando');
  }, []);
}

export function useMedia(consulta) {
  const [vale, setVale] = useState(() => window.matchMedia(consulta).matches);
  useEffect(() => {
    const m = window.matchMedia(consulta);
    const f = () => setVale(m.matches);
    m.addEventListener('change', f);
    return () => m.removeEventListener('change', f);
  }, [consulta]);
  return vale;
}

export function Estado({ carregando, erro, recarregar }) {
  if (carregando) return html`<p class="suave" aria-busy="true">Carregando…</p>`;
  if (erro) {
    return html`
      <div class="aviso erro" role="alert">
        <p>${erro.message}</p>
        ${recarregar && erro.status !== 403 && html`<button class="botao" onClick=${recarregar}>Tentar de novo</button>`}
      </div>
    `;
  }
  return null;
}

export function Titulo({ voltar, titulo, children }) {
  return html`
    <div class="titulo-pagina">
      <div>
        ${voltar && html`<a class="voltar" href=${voltar.href}>‹ ${voltar.texto}</a>`}
        <h1>${titulo}</h1>
      </div>
      <div class="titulo-acoes">${children}</div>
    </div>
  `;
}

export function Vazio({ titulo, children }) {
  return html`<div class="vazio"><p class="vazio-titulo">${titulo}</p>${children}</div>`;
}

/** Barra de progresso de alunos avaliados. */
export function Progresso({ concluidos, iniciados = 0, total }) {
  const pc = (n) => (total ? `${(100 * n) / total}%` : '0%');
  return html`
    <div class="progresso" role="img" aria-label=${`${concluidos} de ${total} alunos avaliados`}>
      <span class="progresso-feito" style=${{ width: pc(concluidos) }}></span>
      <span class="progresso-meio" style=${{ width: pc(iniciados) }}></span>
    </div>
  `;
}

/** Botão que escolhe um arquivo e chama `aoEscolher(arquivo)`; fica ocupado até ela terminar. */
export function BotaoArquivo({ accept, rotulo, rotuloOcupado, title, aoEscolher }) {
  const [enviando, setEnviando] = useState(false);
  async function escolher(e) {
    const arquivo = e.target.files?.[0];
    e.target.value = '';
    if (!arquivo) return;
    setEnviando(true);
    try {
      await aoEscolher(arquivo);
    } finally {
      setEnviando(false);
    }
  }
  return html`
    <label class=${cls('botao', 'pequeno', enviando && 'ocupado')} title=${title}>
      <input type="file" accept=${accept} class="escondido" disabled=${enviando} onChange=${escolher} />
      ${enviando ? rotuloOcupado : rotulo}
    </label>
  `;
}

/** Arquiva (`true`) ou desarquiva o recurso em `caminho` (`/turmas/…`). */
export function arquivar(caminho, arquivada) {
  return api('PUT', `${caminho}/arquivada`, { arquivada });
}

/** Os itens arquivados ficam fechados no fim da lista, a um toque de distância. */
export function ListaArquivadas({ itens, linha }) {
  if (!itens.length) return null;
  return html`
    <details class="arquivadas">
      <summary>Arquivadas (${itens.length})</summary>
      <ul class="lista">${itens.map(linha)}</ul>
    </details>
  `;
}

/** Faixa no alto de um item arquivado, com o desfazer à mão. */
export function AvisoArquivada({ texto, aoDesarquivar }) {
  return html`
    <p class="aviso">
      ${texto}
      <button class="botao pequeno" onClick=${aoDesarquivar}>Desarquivar</button>
    </p>
  `;
}

/** Papel padrão ao dar acesso: numa avaliação, quase sempre um monitor que vai corrigir. */
const PAPEL_PADRAO = { avaliacoes: 'avaliador', rubricas: 'leitor', turmas: 'leitor' };

/**
 * Quem tem acesso ao item e com que papel; quem pode compartilhar muda aqui
 * mesmo, sem sair da tela. `caminho` é `/avaliacoes/<id>` (ou rubrica, turma).
 */
export function Compartilhar({ caminho, pasta }) {
  const [aberto, setAberto] = useState(false);
  const [dados, setDados] = useState(null);
  const [usuarios, setUsuarios] = useState([]);
  const [novo, setNovo] = useState('');
  const [papel, setPapel] = useState(PAPEL_PADRAO[pasta] || '');
  const { rodar, ocupado, erro, setErro } = useAcao();

  useEffect(() => {
    if (!aberto || dados) return;
    Promise.all([api('GET', `${caminho}/acessos`), api('GET', '/usuarios')])
      .then(([d, u]) => {
        setDados(d);
        setUsuarios(u);
        if (!d.papeis.includes(papel)) setPapel(d.papeis[0]);
      })
      .catch((e) => setErro(e.message));
  }, [aberto]);

  async function mudar(metodo, usuario, corpo) {
    const r = await rodar(() => api(metodo, `${caminho}${rota('acessos', usuario)}`, corpo));
    if (r) setDados((d) => ({ ...d, acessos: r.acessos }));
    return !!r;
  }

  async function dar(e) {
    e.preventDefault();
    if (novo && (await mudar('PUT', novo, { papel }))) setNovo('');
  }

  const eu = sessao.get().usuario;
  const comAcesso = new Set((dados?.acessos || []).map((a) => a.usuario.toLowerCase()));
  // Admin já vê tudo, e o executor não corrige: não faz sentido oferecê-los.
  const candidatos = usuarios.filter((u) => !comAcesso.has(u.nome.toLowerCase()) && u.papel !== 'admin' && u.papel !== 'executor');
  const opcoesPapel = (atual) => dados.papeis.map((p) => html`<option value=${p} selected=${p === atual}>${descreverPapel(p)}</option>`);

  return html`
    <details class="compartilhar" open=${aberto} onToggle=${(e) => setAberto(e.target.open)}>
      <summary>Compartilhar</summary>
      ${!dados && !erro && html`<p class="suave" aria-busy="true">Carregando…</p>`}
      ${dados && html`
        <ul class="acessos">
          ${dados.acessos.map((a) => html`
            <li>
              <span class="acesso-nome">${a.usuario}${a.usuario === eu ? ' (você)' : ''}</span>
              <select aria-label=${`Papel de ${a.usuario}`} disabled=${ocupado}
                onChange=${(e) => mudar('PUT', a.usuario, { papel: e.target.value })}>${opcoesPapel(a.papel)}</select>
              <button type="button" class="icone" title="Tirar o acesso" aria-label=${`Tirar o acesso de ${a.usuario}`} disabled=${ocupado}
                onClick=${() => mudar('DELETE', a.usuario)}>×</button>
            </li>
          `)}
          ${!dados.acessos.length && html`<li class="suave">Ninguém além dos admins tem acesso.</li>`}
        </ul>
        ${candidatos.length > 0
          ? html`
            <form class="acesso-novo" onSubmit=${dar}>
              <select aria-label="Pessoa" value=${novo} onChange=${(e) => setNovo(e.target.value)}>
                <option value="">Dar acesso a…</option>
                ${candidatos.map((u) => html`<option value=${u.nome}>${u.nome}</option>`)}
              </select>
              <select aria-label="Papel" onChange=${(e) => setPapel(e.target.value)}>${opcoesPapel(papel)}</select>
              <button class="botao pequeno primario" disabled=${!novo || ocupado}>Dar acesso</button>
            </form>
          `
          : html`<p class="suave">Não há mais ninguém a quem dar acesso (admins já veem tudo). Novos usuários são criados por um admin.</p>`}
      `}
      ${erro && html`<p class="erro" role="alert">${erro}</p>`}
    </details>
  `;
}

/** Folha que sobe de baixo, com o foco preso dentro enquanto aberta. */
export function Folha({ titulo, subtitulo, aoFechar, alta, espera = 0, children }) {
  const pega = useRef(null);
  const caixa = useRef(null);
  const aberta = useRef(performance.now());
  // Quando a folha abre por um toque, um toque duplo não pode fechá-la nem acertar,
  // sem querer, uma opção que acabou de subir sob o dedo: por `espera` ms ela
  // ignora cliques. Aberta por deslize ou botão, responde na hora.
  const recente = () => performance.now() - aberta.current < espera;
  const fecharPeloFundo = () => {
    if (!recente()) aoFechar();
  };
  const segurarClique = (e) => {
    if (recente()) {
      e.stopPropagation();
      e.preventDefault();
    }
  };

  // Enquanto aberta, a folha segura o foco; ao fechar, devolve a quem o tinha.
  useEffect(() => {
    const antes = document.activeElement;
    caixa.current?.focus();
    return () => antes?.focus?.();
  }, []);

  function aoTeclar(e) {
    if (e.key !== 'Tab') return;
    const focaveis = [...caixa.current.querySelectorAll('button, input, textarea, select, a[href]')].filter((el) => !el.disabled);
    if (!focaveis.length) return;
    const [primeiro, ultimo] = [focaveis[0], focaveis[focaveis.length - 1]];
    const fora = !caixa.current.contains(document.activeElement) || document.activeElement === caixa.current;
    if (e.shiftKey && (fora || document.activeElement === primeiro)) {
      e.preventDefault();
      ultimo.focus();
    } else if (!e.shiftKey && (fora || document.activeElement === ultimo)) {
      e.preventDefault();
      primeiro.focus();
    }
  }

  function aoDescer(e) {
    pega.current = e.clientY;
  }
  function aoSoltar(e) {
    if (pega.current !== null && e.clientY - pega.current > 40) aoFechar();
    pega.current = null;
  }
  return html`
    <div class="folha-fundo" onClick=${fecharPeloFundo}></div>
    <section class=${cls('folha', alta && 'alta')} role="dialog" aria-modal="true" aria-label=${titulo} tabindex="-1" ref=${caixa} onKeyDown=${aoTeclar} onClickCapture=${segurarClique}>
      <header class="folha-topo" onPointerDown=${aoDescer} onPointerUp=${aoSoltar}>
        <span class="folha-pega" aria-hidden="true"></span>
        <h2>${titulo}</h2>
        <button type="button" class="icone" aria-label="Fechar" onClick=${aoFechar}>×</button>
      </header>
      <div class="folha-corpo">
        ${subtitulo && html`<div class="folha-intro">${subtitulo}</div>`}
        ${children}
      </div>
    </section>
  `;
}
