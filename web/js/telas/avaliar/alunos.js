// Quem avaliar: a ordem dos alunos para as setas e o "próximo", e a lista com busca.
import { html, useState, useEffect, useRef } from '../../../vendor/preact-htm.js';
import { num, cls, semAcento } from '../../util.js';

/**
 * Com submissões do Moodle, quem não entregou fica de fora: as setas, o "próximo
 * aluno" e a lista pulam essa pessoa, a não ser que o avaliador peça para incluir.
 */
export function useAlunos({ alunos, ead, resultados, atual }) {
  const [incluir, setIncluir] = useState(false);
  const temEntregas = !!ead && Object.keys(ead.submissoes).length > 0;
  const semEntrega = (a) => temEntregas && !ead.submissoes[a.id];
  // Quem está aberto entra mesmo sem entrega, para as setas saberem de onde partir.
  const navegaveis = alunos.filter((a) => incluir || !semEntrega(a) || a.id === atual);
  const posicao = navegaveis.findIndex((a) => a.id === atual);
  const deslocado = (d) => navegaveis[(posicao + d + navegaveis.length) % navegaveis.length];
  let proximoPendente = null;
  for (let k = 1; k < navegaveis.length && !proximoPendente; k += 1) {
    if (!resultados[deslocado(k).id]?.concluida) proximoPendente = deslocado(k);
  }
  return {
    navegaveis,
    posicao,
    vizinho: deslocado,
    proximoPendente,
    /** Por onde começar quando o endereço não diz o aluno. */
    primeiroPendente: () => navegaveis.find((a) => !resultados[a.id].concluida) || navegaveis[0] || alunos[0],
    filtro: { semEntrega, total: alunos.filter(semEntrega).length, incluir, setIncluir },
  };
}

export function Alunos({ alunos, resultados, atual, aoEscolher, filtro }) {
  const [busca, setBusca] = useState('');
  const [soFaltam, setSoFaltam] = useState(false);
  const lista = useRef(null);
  useEffect(() => {
    lista.current?.querySelector('.aluno.atual')?.scrollIntoView({ block: 'center' });
  }, [atual]);
  const termo = semAcento(busca.trim());
  const visiveis = alunos.filter((a) => {
    if (soFaltam && resultados[a.id].concluida && a.id !== atual) return false;
    if (!filtro.incluir && filtro.semEntrega(a) && a.id !== atual) return false;
    return !termo || semAcento(a.nome).includes(termo) || semAcento(a.id).includes(termo);
  });
  const feitos = alunos.filter((a) => resultados[a.id].concluida).length;
  return html`
    <div class="alunos">
      <div class="alunos-topo">
      <input class="alunos-busca" type="search" placeholder="Buscar aluno" aria-label="Buscar aluno"
        value=${busca} onInput=${(e) => setBusca(e.target.value)}
        onKeyDown=${(e) => { if (e.key === 'Enter' && visiveis.length) aoEscolher(visiveis[0].id); }} />
      <label class="alunos-filtro">
        <input type="checkbox" checked=${soFaltam} onChange=${(e) => setSoFaltam(e.target.checked)} />
        Só quem falta (${alunos.length - feitos})
      </label>
      ${filtro.total > 0 && html`
        <label class="alunos-filtro">
          <input type="checkbox" checked=${filtro.incluir} onChange=${(e) => filtro.setIncluir(e.target.checked)} />
          Incluir quem não entregou (${filtro.total})
        </label>
      `}
      </div>
      <ul class="alunos-lista" ref=${lista}>
        ${visiveis.map((a) => {
          const r = resultados[a.id];
          const estado = r.concluida ? 'feito' : r.respondidos ? 'meio' : 'nada';
          return html`
            <li>
              <button type="button" class=${cls('aluno', a.id === atual && 'atual')} data-estado=${estado}
                aria-current=${a.id === atual ? 'true' : null} onClick=${() => aoEscolher(a.id)}>
                <span class="aluno-marca" aria-hidden="true">${r.concluida ? '✓' : ''}</span>
                <span class="aluno-nome">${a.nome}</span>
                <span class="aluno-nota">${r.concluida ? num(r.nota) : r.respondidos ? `${r.respondidos}/${r.total}` : filtro.semEntrega(a) ? html`<span class="suave">sem entrega</span>` : ''}</span>
              </button>
            </li>
          `;
        })}
        ${!visiveis.length && html`<li class="suave alunos-vazio">Ninguém com esse nome.</li>`}
      </ul>
    </div>
  `;
}
