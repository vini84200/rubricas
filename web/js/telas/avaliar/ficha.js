// O cartão do meio da tela: um critério, um passo de roteiro ou o resumo do aluno.
import { html } from '../../../vendor/preact-htm.js';
import { rotulo, tom, pontos, temIntermediarios } from '../../nota.js';
import { Markdown } from '../../markdown.js';
import { num, cls } from '../../util.js';
import { useDeslize } from './gestos.js';
import { rapido } from './navegacao.js';

/** Linha do alto do cartão: o desfazer da última marcação ou, sem ele, um rótulo. */
export function FichaOnde({ desfazer, rotulo: texto, posicao }) {
  return html`
    <p class="ficha-onde">
      ${desfazer
        ? html`<button type="button" class="ficha-desfazer" onClick=${desfazer.acao}>Desfazer “${desfazer.texto}”</button>`
        : texto}
      <span>${posicao}</span>
    </p>
  `;
}

export function temDescricao(criterio) {
  return !!criterio.descricao || criterio.escala?.tipo === 'ponto_unico';
}

/** O que observar no critério e, no ponto único, o desempenho esperado. */
export function DescricaoCriterio({ criterio: c }) {
  return html`
    ${c.descricao && html`<${Markdown} texto=${c.descricao} />`}
    ${c.escala?.tipo === 'ponto_unico' && html`<p><strong>Esperado:</strong> ${c.escala.esperado}</p>`}
  `;
}

/** O artigo que desliza, com os carimbos de cada lado. */
function Moldura({ classe, papel, entrada, deslize, sim, nao, children }) {
  return html`
    <article class=${cls('ficha', classe, `entra-${entrada}`)} ref=${deslize.ref} aria-roledescription=${papel} ...${deslize.eventos}>
      <span class="carimbo sim" aria-hidden="true">${sim}</span>
      <span class="carimbo nao" aria-hidden="true">${nao}</span>
      <span class="carimbo livre" aria-hidden="true"></span>
      ${children}
    </article>
  `;
}

/**
 * Passo de roteiro: sem marcação; deslizar para os lados ou tocar segue.
 * `controle.current` recebe `{ voar }`, para a tela soltar o cartão ao seguir.
 */
export function FichaPasso({ item, posicao, entrada, desfazer, aoSeguir, aoBaixo, controle }) {
  const c = item.criterio;
  const deslize = useDeslize({ aoDireita: () => aoSeguir('direita'), aoEsquerda: () => aoSeguir('esquerda'), aoBaixo, aoToque: () => aoSeguir('direita') });
  controle.current = deslize;
  return html`
    <${Moldura} classe="passo" papel="passo de roteiro" entrada=${entrada} deslize=${deslize} sim="Seguir" nao="Seguir">
      <${FichaOnde} desfazer=${desfazer} rotulo=${html`<span class="ficha-rotulo-passo">Roteiro</span>`} posicao=${posicao} />
      <h2 class="ficha-nome">${c.nome}</h2>
      ${c.descricao && html`<${Markdown} classe="ficha-texto" texto=${c.descricao} />`}
      <div class="ficha-dicas" aria-hidden="true">
        <span class="dica cima">toque ou deslize para seguir</span>
      </div>
    <//>
  `;
}

/** Critério: direita marca o máximo, esquerda o mínimo, cima (ou um toque) abre as opções. */
export function FichaCriterio({ item, resposta, posicao, mostrarRubrica, entrada, comentario, desfazer, passos, sugestao, aoMarcar, aoCima, aoBaixo, controle }) {
  const c = item.criterio;
  const pos = rapido(c, true);
  const neg = rapido(c, false);
  const intermediarios = temIntermediarios(c);
  const deslize = useDeslize({
    aoDireita: () => aoMarcar(pos.marcacao, 'direita'),
    aoEsquerda: () => aoMarcar(neg.marcacao, 'esquerda'),
    aoCima,
    aoBaixo,
    aoToque: () => aoCima('toque'),
  });
  controle.current = deslize;
  return html`
    <${Moldura} papel="cartão de critério" entrada=${entrada} deslize=${deslize} sim=${pos.texto} nao=${neg.texto}>
      <${FichaOnde} desfazer=${desfazer} rotulo=${mostrarRubrica && html`<span class="ficha-rubrica">${item.rubrica}</span>`} posicao=${posicao} />
      ${passos.length > 0 && html`
        <ul class="ficha-passos" aria-label="Passos de roteiro antes deste critério">
          ${passos.map((p) => html`<li>${p.nome}</li>`)}
        </ul>
      `}
      <h2 class="ficha-nome">${c.nome}</h2>
      ${sugestao && html`
        <button type="button" class="ficha-sugestao" data-tom=${sugestao.tom} onClick=${sugestao.aceitar}>
          <span>Sugestão: <strong>${sugestao.texto}</strong> <span class="ficha-sugestao-motivo">(${sugestao.motivo})</span></span>
          <span class="ficha-sugestao-acao">${sugestao.tecla ? html`<kbd>G</kbd>` : 'Aceitar'}</span>
        </button>
      `}
      <div class="ficha-texto"><${DescricaoCriterio} criterio=${c} /></div>
      ${resposta && html`
        <p class="ficha-marcado" data-tom=${tom(c, resposta)}>
          Marcado: <strong>${rotulo(c, resposta)}</strong>
          ${resposta.tipo !== 'nota' && html` (${num(pontos(c, resposta) ?? 0)} pt)`}
        </p>
      `}
      ${comentario && html`<p class="ficha-comentario">“${comentario}”</p>`}
      <div class="ficha-dicas" aria-hidden="true">
        <span class="dica nao">‹ ${neg.texto}</span>
        <span class="dica cima">${intermediarios ? (c.escala.tipo === 'niveis' ? `${c.escala.niveis.length} níveis` : 'nota') : 'comentar'}<i>︿</i></span>
        <span class="dica sim">${pos.texto} ›</span>
      </div>
      <span class="solte" aria-hidden="true">${intermediarios ? 'Solte para ver as opções' : 'Solte para comentar'}</span>
    <//>
  `;
}
