// Peças usadas por várias telas.
import { html, useState, useEffect, useCallback } from '../../vendor/preact-htm.js';
import { api } from '../api.js';

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
        ${recarregar && html`<button class="botao" onClick=${recarregar}>Tentar de novo</button>`}
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
