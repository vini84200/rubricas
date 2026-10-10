// Resumo do aluno, depois do último critério, e o selo de sincronia do topo.
import { html } from '../../../vendor/preact-htm.js';
import { rotulo, tom, pontos, maximo, ehRoteiro, pesoDe } from '../../nota.js';
import { num } from '../../util.js';
import { FichaOnde } from './ficha.js';
import { respostaDe } from './navegacao.js';
import { EfeitosDoAluno } from './efeitos.js';

function LinhaResumo({ item, correcao, aoIr }) {
  const c = item.criterio;
  const r = respostaDe(correcao, item);
  const p = pontos(c, r);
  return html`
    <li>
      <button type="button" class="resumo-item" data-tom=${tom(c, r)} onClick=${aoIr}>
        <span class="resumo-ponto" aria-hidden="true"></span>
        <span class="resumo-criterio">${c.nome}</span>
        <span class="resumo-pontos">${p === null ? '' : `${num(p * pesoDe(c))} / ${num(maximo(c) * pesoDe(c))}`}</span>
        <span class="resumo-marca">${p === null ? 'sem marcação' : rotulo(c, r)}</span>
        ${r?.comentario && html`<span class="resumo-comentario">“${r.comentario}”</span>`}
      </button>
    </li>
  `;
}

/** `efeitos`: as props de `EfeitosDoAluno`, menos avaliação e correção. */
export function Resumo({ aluno, avaliacao, itens, correcao, resultado, desfazer, aoIrPara, aoComentarGeral, efeitos }) {
  const faltam = resultado.total - resultado.respondidos;
  const varias = avaliacao.rubricas.length > 1;
  const mudou = resultado.efeitos.length > 0 && Math.abs(resultado.nota - resultado.antes) > 1e-9;
  return html`
    <article class="ficha resumo entra-frente">
      <${FichaOnde} desfazer=${desfazer} posicao=${aluno.nome} />
      <p class="resumo-nota" data-parcial=${!resultado.concluida}>
        <strong>${num(resultado.nota)}</strong><span> de ${num(avaliacao.nota_maxima)}</span>
        ${mudou && html`<span class="resumo-antes">${num(resultado.antes)} antes dos efeitos</span>`}
      </p>
      ${faltam > 0 && (resultado.encerrada
        ? html`<p class="resumo-faltam">Correção encerrada por um efeito: ${faltam === 1 ? 'o critério sem marcação não precisa' : `os ${faltam} critérios sem marcação não precisam`} ser marcados.</p>`
        : html`<p class="resumo-faltam">${faltam === 1 ? 'Falta 1 critério' : `Faltam ${faltam} critérios`}: a nota conta zero neles.</p>`)}
      <${EfeitosDoAluno} avaliacao=${avaliacao} correcao=${correcao} ...${efeitos} />
      ${avaliacao.rubricas.map((ra, n) => html`
        ${varias && html`
          <p class="resumo-rubrica">
            <span>${ra.rubrica.titulo}</span>
            <span>${num(resultado.rubricas[n].pontos)} / ${num(resultado.rubricas[n].maximo)}</span>
          </p>
        `}
        <ul class="resumo-lista">
          ${itens.map((it, pos) => it.origem === ra.origem && !ehRoteiro(it.criterio)
            && html`<${LinhaResumo} item=${it} correcao=${correcao} aoIr=${() => aoIrPara(pos)} />`)}
        </ul>
      `)}
      <label class="campo comentario">
        <span>Comentário geral</span>
        <textarea rows="2" placeholder="Opcional" value=${correcao?.comentario_geral || ''} onInput=${(e) => aoComentarGeral(e.target.value)}></textarea>
      </label>
    </article>
  `;
}

export function Sincronia({ estado }) {
  let texto = 'Salvo';
  let tipo = 'ok';
  if (estado.erro) {
    texto = 'Veja o aviso';
    tipo = 'erro';
  } else if (estado.pendentes && (estado.falhando || !estado.online)) {
    texto = `Sem conexão, ${estado.pendentes} por enviar`;
    tipo = 'espera';
  } else if (estado.pendentes) {
    texto = 'Salvando…';
    tipo = 'indo';
  }
  return html`<span class="sincronia" data-tipo=${tipo} role="status">${texto}</span>`;
}
