// O que acompanha o cartão: os botões de baixo e, na tela larga, a lista de teclas.
import { html } from '../../../vendor/preact-htm.js';
import { rapido } from './navegacao.js';

/** As teclas, na lateral da tela larga. */
export function Teclas({ comSugestao }) {
  return html`
    <dl class="teclas">
      <dt><kbd>←</kbd> <kbd>→</kbd></dt><dd>mínimo e máximo</dd>
      <dt><kbd>1</kbd>–<kbd>9</kbd></dt><dd>escolher o nível</dd>
      <dt><kbd>↑</kbd></dt><dd>ir às opções</dd>
      <dt><kbd>↓</kbd></dt><dd>voltar um critério</dd>
      <dt><kbd>S</kbd></dt><dd>pular sem marcar</dd>
      <dt><kbd>C</kbd></dt><dd>comentar</dd>
      <dt><kbd>Z</kbd></dt><dd>desfazer a última marcação</dd>
      ${comSugestao && html`<dt><kbd>G</kbd></dt><dd>aceitar a sugestão</dd>`}
      <dt><kbd>Enter</kbd></dt><dd>próximo aluno, no resumo</dd>
      <dt><kbd>[</kbd> <kbd>]</kbd></dt><dd>trocar de aluno</dd>
    </dl>
  `;
}

/** Os botões de baixo: os mesmos gestos do cartão, para quem prefere tocar. */
export function Rodape({ modo, criterio, rotuloOpcoes, largo, aoMarcar, aoOpcoes, aoSeguirPasso, resumo }) {
  if (modo === 'roteiro') {
    return html`
      <footer class="av-botoes um">
        <button type="button" class="av-botao seguir" onClick=${() => aoSeguirPasso('direita')}>
          <span class="av-botao-texto">Seguir ›</span>
        </button>
      </footer>
    `;
  }
  if (modo === 'resumo') {
    const { concluida, proximo, aoCompletar, aoProximo, hrefNotas } = resumo;
    return html`
      <footer class="av-botoes um">
        ${!concluida
          ? html`<button type="button" class="av-botao seguir" onClick=${aoCompletar}>Completar o que falta</button>`
          : proximo
            ? html`<button type="button" class="av-botao seguir" onClick=${aoProximo}>
                <span class="av-botao-texto">Próximo aluno: ${proximo.nome}</span>
              </button>`
            : html`<a class="av-botao seguir" href=${hrefNotas}>Turma toda avaliada. Ver notas</a>`}
      </footer>
    `;
  }
  const pos = rapido(criterio, true);
  const neg = rapido(criterio, false);
  return html`
    <footer class="av-botoes">
      <button type="button" class="av-botao nao" onClick=${() => aoMarcar(neg.marcacao, 'esquerda')}>
        <span class="av-botao-sinal" aria-hidden="true">✗</span>
        <span class="av-botao-texto">${neg.texto}</span>
      </button>
      <button type="button" class="av-botao meio" onClick=${() => aoOpcoes('botao')} aria-haspopup=${largo ? null : 'dialog'}>
        <span class="av-botao-sinal" aria-hidden="true">︿</span>
        <span class="av-botao-texto">${rotuloOpcoes}</span>
      </button>
      <button type="button" class="av-botao sim" onClick=${() => aoMarcar(pos.marcacao, 'direita')}>
        <span class="av-botao-sinal" aria-hidden="true">✓</span>
        <span class="av-botao-texto">${pos.texto}</span>
      </button>
    </footer>
  `;
}
