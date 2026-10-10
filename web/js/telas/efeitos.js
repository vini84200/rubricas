// Catálogo de tipos de efeito sobre a nota. Só para ver: os tipos se criam e se
// mudam pela API em TOML (a skill do Claude ensina o formato).
import { html } from '../../vendor/preact-htm.js';
import { useDados, Estado, Titulo, Vazio } from './comum.js';
import { resumoEfeito } from './avaliacoes.js';

export function TelaEfeitos() {
  const { dados, ...estado } = useDados('/efeitos');
  return html`
    <div class="tela">
      <${Titulo} titulo="Efeitos sobre a nota" voltar=${{ href: '#/avaliacoes', texto: 'Avaliações' }} />
      <p class="suave pagina-intro">
        Um efeito muda a nota final de um aluno depois da conta das rubricas: zero por plágio, desconto por atraso, bônus.
        Cada avaliação escolhe os tipos que usa (em Editar) e guarda uma cópia deles. Quem avalia aplica no resumo do aluno.
      </p>
      <${Estado} ...${estado} />
      ${dados?.problemas.map((p) => html`<p class="aviso">${p}</p>`)}
      ${dados && !dados.efeitos.length && html`
        <${Vazio} titulo="Nenhum tipo de efeito ainda">
          <p class="suave">Crie com <code>PUT /api/efeitos/ID.toml</code>, ou peça ao Claude com a skill <code>rubricas</code>.</p>
        <//>
      `}
      ${dados?.efeitos.length > 0 && html`
        <ul class="lista">
          ${dados.efeitos.map((t) => html`
            <li class="linha">
              <span class="linha-titulo">${t.titulo}</span>
              <span class="linha-meta">${resumoEfeito(t)}</span>
              ${t.descricao && html`<span class="linha-meta">${t.descricao}</span>`}
              <span class="linha-meta"><code>${t.id}</code></span>
            </li>
          `)}
        </ul>
        <p class="suave">Para criar ou mudar um tipo, use a API em TOML (<code>GET/PUT /api/efeitos/ID.toml</code>) ou peça ao Claude com a skill <code>rubricas</code>.</p>
      `}
    </div>
  `;
}
