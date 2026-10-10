// Opções do critério: níveis, nota, comentário. Ficam na lateral da tela larga
// ou numa folha que sobe de baixo no celular.
import { html, useState } from '../../../vendor/preact-htm.js';
import { opcoesDe, passoDe, DESEMPENHOS } from '../../nota.js';
import { num, campo, lerNum, cls } from '../../util.js';

function CampoNota({ max, passo, inicial, aoAplicar, foco }) {
  const [texto, setTexto] = useState(inicial ?? '');
  const valor = lerNum(texto);
  const valido = Number.isFinite(valor) && valor >= 0 && valor <= max + 1e-9;
  const arredondar = (v) => Math.round(v / passo) * passo;
  const atalhos = [...new Set([0, 0.25, 0.5, 0.75, 1].map((f) => Math.round(arredondar(max * f) * 1000) / 1000))];
  const aplicar = () => valido && aoAplicar(valor);
  return html`
    <div class="nota">
      <div class="nota-atalhos">
        ${atalhos.map((v) => html`<button type="button" class="pastilha" onClick=${() => aoAplicar(v)}>${num(v)}</button>`)}
      </div>
      <div class="nota-linha">
        <input class="nota-valor" inputmode="decimal" aria-label="Nota" placeholder=${`0 a ${num(max)}`} ref=${foco}
          value=${texto} onInput=${(e) => setTexto(e.target.value)}
          onKeyDown=${(e) => { if (e.key === 'Enter') { e.preventDefault(); aplicar(); } }} />
        <span class="suave">de ${num(max)}</span>
        <button type="button" class="botao primario" disabled=${!valido} onClick=${aplicar}>Aplicar</button>
      </div>
      <input class="nota-barra" type="range" min="0" max=${max} step=${passo} aria-label="Ajustar nota"
        value=${Number.isFinite(valor) ? valor : 0} onInput=${(e) => setTexto(campo(Number(e.target.value)))} />
    </div>
  `;
}

const marcada = (resposta, m) => (m.tipo === 'nivel' ? resposta?.nivel === m.nivel : resposta?.atende === m.atende);

/** Níveis e sim/não: uma lista para tocar, com a tecla de cada um na tela larga. */
function Lista({ criterio, resposta, comTeclas, aoMarcar }) {
  return html`
    <ol class="opcoes-niveis">
      ${opcoesDe(criterio).map((o) => html`
        <li>
          <button type="button" class=${cls('opcao', marcada(resposta, o.marcacao) && 'marcada')} data-tom=${o.tom} onClick=${() => aoMarcar(o.marcacao)}>
            ${comTeclas && html`<kbd>${o.tecla}</kbd>`}
            <span class="opcao-rotulo">${o.rotulo}</span>
            <span class="opcao-pontos">${num(o.pontos)} pt</span>
            ${o.descritor && html`<span class="opcao-descritor">${o.descritor}</span>`}
          </button>
        </li>
      `)}
    </ol>
  `;
}

/** Ponto único: abaixo, atende ou acima; abaixo pede quanto vale o entregue. */
function PontoUnico({ escala: e, resposta, foco, aoMarcar }) {
  const [abaixo, setAbaixo] = useState(resposta?.tipo === 'ponto_unico' && resposta.desempenho === 'abaixo');
  return html`
    <div class="tres">
      ${Object.entries(DESEMPENHOS).map(([d, nome]) => html`
        <button type="button" data-tom=${d === 'abaixo' ? 'nao' : 'sim'}
          class=${cls('opcao', 'curta', (resposta?.desempenho === d || (d === 'abaixo' && abaixo)) && 'marcada')}
          onClick=${() => (d === 'abaixo' ? setAbaixo(true) : aoMarcar({ tipo: 'ponto_unico', desempenho: d, nota: e.max }))}>
          <span class="opcao-rotulo">${nome}</span>
          ${d !== 'abaixo' && html`<span class="opcao-pontos">${num(e.max)} pt</span>`}
        </button>
      `)}
    </div>
    ${abaixo && html`
      <p class="suave">Quanto vale o que foi entregue?</p>
      <${CampoNota} max=${e.max} passo=${passoDe(e)} foco=${foco}
        inicial=${resposta?.desempenho === 'abaixo' ? campo(resposta.nota) : ''}
        aoAplicar=${(nota) => aoMarcar({ tipo: 'ponto_unico', desempenho: 'abaixo', nota })} />
    `}
  `;
}

export function Opcoes({ item, resposta, comentario, aoMarcar, aoComentar, aoLimpar, aoPular, comTeclas, focoNota, focoComentario }) {
  const c = item.criterio;
  const e = c.escala;
  let corpo;
  if (e.tipo === 'livre') {
    corpo = html`<${CampoNota} max=${e.max} passo=${passoDe(e)} foco=${focoNota}
      inicial=${resposta?.tipo === 'nota' ? campo(resposta.nota) : ''} aoAplicar=${(nota) => aoMarcar({ tipo: 'nota', nota })} />`;
  } else if (e.tipo === 'ponto_unico') {
    corpo = html`<${PontoUnico} escala=${e} resposta=${resposta} foco=${focoNota} aoMarcar=${aoMarcar} />`;
  } else {
    corpo = html`<${Lista} criterio=${c} resposta=${resposta} comTeclas=${comTeclas} aoMarcar=${aoMarcar} />`;
  }
  return html`
    <div class="opcoes">
      ${corpo}
      <label class="campo comentario">
        <span>Comentário para o aluno</span>
        <textarea rows="2" ref=${focoComentario} value=${comentario} placeholder="Opcional" onInput=${(ev) => aoComentar(ev.target.value)}></textarea>
      </label>
      <div class="opcoes-rodape">
        <button type="button" class="botao-texto" onClick=${aoPular}>Pular sem marcar</button>
        ${resposta && html`<button type="button" class="botao-texto" onClick=${aoLimpar}>Limpar marcação</button>`}
      </div>
    </div>
  `;
}
