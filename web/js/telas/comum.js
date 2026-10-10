// Peças usadas por várias telas.
import { html, useState, useEffect, useCallback } from '../../vendor/preact-htm.js';
import { api } from '../api.js';
import { sessao, descreverPapel } from '../sessao.js';

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
  const [erro, setErro] = useState('');
  const [ocupado, setOcupado] = useState(false);

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
    setOcupado(true);
    setErro('');
    try {
      const r = await api(metodo, `${caminho}/acessos/${encodeURIComponent(usuario)}`, corpo);
      setDados((d) => ({ ...d, acessos: r.acessos }));
      return true;
    } catch (e) {
      setErro(e.message);
      return false;
    } finally {
      setOcupado(false);
    }
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
