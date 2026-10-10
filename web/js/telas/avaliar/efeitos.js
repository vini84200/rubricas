// Efeitos sobre a nota, no resumo do aluno: as sugestões (plágio, atraso), os
// efeitos aplicados e um botão para cada tipo que ainda não está.
import { html, useState, useEffect, useRef } from '../../../vendor/preact-htm.js';
import { textoEfeito, efeitoAberto, porUnidade, unidadeDe, problemaEfeito } from '../../nota.js';
import { campo, lerNum } from '../../util.js';

/** Valor, quantidade e justificativa de um efeito, antes de aplicar. */
function FormEfeito({ tipo, inicial, aoAplicar, aoCancelar }) {
  const [valor, setValor] = useState(campo(inicial?.valor));
  const [quantidade, setQuantidade] = useState(campo(inicial?.quantidade));
  const [justificativa, setJustificativa] = useState(inicial?.justificativa || '');
  const [erro, setErro] = useState('');
  const primeiro = useRef(null);
  useEffect(() => primeiro.current?.focus(), []);
  const aberto = efeitoAberto(tipo);

  function aplicar(e) {
    e.preventDefault();
    const aplicado = {};
    if (aberto) aplicado.valor = lerNum(valor);
    if (porUnidade(tipo)) aplicado.quantidade = lerNum(quantidade);
    if (justificativa.trim()) aplicado.justificativa = justificativa.trim();
    const problema = problemaEfeito(tipo, aplicado);
    if (problema) {
      setErro(problema);
      return;
    }
    aoAplicar(aplicado);
  }

  const op = tipo.operacao.tipo;
  const rotuloValor = op === 'multiplicar' ? 'Fator' : op === 'fixar' || op === 'teto' ? 'Nota' : 'Pontos';
  return html`
    <form class="efeito-form" onSubmit=${aplicar} onKeyDown=${(e) => e.key === 'Escape' && (e.stopPropagation(), aoCancelar())}>
      <p class="efeito-titulo">${tipo.titulo}</p>
      ${tipo.descricao && html`<p class="suave">${tipo.descricao}</p>`}
      <div class="par">
        ${aberto && html`
          <label class="campo curto">
            <span>${rotuloValor}</span>
            <input ref=${primeiro} class="numero" inputmode="decimal" value=${valor} onInput=${(e) => setValor(e.target.value)}
              placeholder=${op === 'somar' ? '−1 ou 0,5' : ''} />
          </label>
        `}
        ${porUnidade(tipo) && html`
          <label class="campo curto">
            <span>${unidadeDe(tipo)[1][0].toUpperCase()}${unidadeDe(tipo)[1].slice(1)}</span>
            <input ref=${aberto ? null : primeiro} class="numero" inputmode="decimal" value=${quantidade} onInput=${(e) => setQuantidade(e.target.value)} />
          </label>
        `}
      </div>
      <label class="campo">
        <span>Justificativa${aberto ? '' : ' (opcional)'}</span>
        <input ref=${aberto || porUnidade(tipo) ? null : primeiro} value=${justificativa} onInput=${(e) => setJustificativa(e.target.value)}
          placeholder=${tipo.feedback === false ? 'Fica só aqui, não vai ao aluno' : 'Vai ao aluno no feedback do Moodle'} />
      </label>
      ${erro && html`<p class="erro" role="alert">${erro}</p>`}
      <div class="acoes">
        <button class="botao primario pequeno">Aplicar</button>
        <button type="button" class="botao pequeno" onClick=${aoCancelar}>Cancelar</button>
      </div>
    </form>
  `;
}

/** Aplica a sugestão; a que pede valor abre o formulário já preenchido. */
export function aceitarSugestao(s, aoAplicar, aoAbrir) {
  if (s.completo) aoAplicar(s.tipo.id, s.aplicado);
  else aoAbrir({ id: s.tipo.id, inicial: s.aplicado });
}

/**
 * `sugestoes`: de `sugestoesEfeitos`, cada uma com o `motivo` para mostrar.
 * `aberto`/`aoAbrir`: o tipo cujo formulário está aberto (a tecla G abre o de uma
 * sugestão que pede valor).
 */
export function EfeitosDoAluno({ avaliacao, correcao, sugestoes, comTecla, aberto, aoAbrir, aoAplicar, aoTirar, aoDispensar }) {
  const tipos = avaliacao.efeitos || [];
  if (!tipos.length) return null;
  const aplicados = correcao?.efeitos || {};
  const sugeridos = new Set(sugestoes.map((s) => s.tipo.id));
  const livres = tipos.filter((t) => !aplicados[t.id] && !sugeridos.has(t.id));
  const tipoAberto = tipos.find((t) => t.id === aberto?.id);


  return html`
    <section class="efeitos" aria-label="Efeitos sobre a nota">
      <h3 class="efeitos-titulo">Efeitos sobre a nota</h3>
      ${sugestoes.length > 0 && html`
        <ul class="efeitos-lista">
          ${sugestoes.map((s, i) => html`
            <li class="efeito sugerido">
              <span class="efeito-texto"><strong>Sugestão:</strong> ${s.completo ? textoEfeito(s.tipo, s.aplicado) : s.tipo.titulo}
                <span class="suave"> (${s.motivo})</span></span>
              <span class="efeito-acoes">
                <button type="button" class="botao primario pequeno" onClick=${() => aceitarSugestao(s, aoAplicar, aoAbrir)}>
                  Aplicar${comTecla && i === 0 ? html` <kbd>G</kbd>` : ''}
                </button>
                <button type="button" class="botao pequeno" onClick=${() => aoDispensar(s.tipo.id, s.sinal)}
                  title="O aviso some; volta só se o sinal piorar">Dispensar</button>
              </span>
            </li>
          `)}
        </ul>
      `}
      ${tipos.some((t) => aplicados[t.id]) && html`
        <ul class="efeitos-lista">
          ${tipos.filter((t) => aplicados[t.id]).map((t) => {
            const a = aplicados[t.id];
            const problema = problemaEfeito(t, a);
            return html`
              <li class="efeito aplicado" data-problema=${problema ? 'sim' : null}>
                <span class="efeito-texto">${problema ? `${t.titulo}: não conta (${problema.toLowerCase()})` : textoEfeito(t, a)}
                  ${a.por && html`<span class="suave"> · ${a.por}</span>`}
                  ${t.encerra && !problema && html`<span class="suave"> · encerra a correção</span>`}
                  ${t.feedback === false && html`<span class="suave"> · não vai ao aluno</span>`}</span>
                <span class="efeito-acoes">
                  ${(efeitoAberto(t) || porUnidade(t) || a.justificativa) && html`
                    <button type="button" class="botao pequeno" onClick=${() => aoAbrir({ id: t.id, inicial: a })}>Alterar</button>
                  `}
                  <button type="button" class="botao pequeno" onClick=${() => aoTirar(t.id)}>Tirar</button>
                </span>
              </li>
            `;
          })}
        </ul>
      `}
      ${tipoAberto
        ? html`<${FormEfeito} key=${tipoAberto.id} tipo=${tipoAberto} inicial=${aberto.inicial}
            aoAplicar=${(a) => aoAplicar(tipoAberto.id, a)} aoCancelar=${() => aoAbrir(null)} />`
        : livres.length > 0 && html`
          <div class="efeitos-livres">
            ${livres.map((t) => html`
              <button type="button" class="botao pequeno" onClick=${() => aoAbrir({ id: t.id })}>+ ${t.titulo}</button>
            `)}
          </div>
        `}
    </section>
  `;
}
