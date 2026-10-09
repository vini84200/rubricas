// EAD: o pipeline de checagens e o plágio de uma avaliação (só leitura: o
// pipeline é escrito pela API em texto, com a skill do Claude), e a tela
// "Acompanhar", que segue no monitor o aluno aberto em outro aparelho.
import { html, useState, useEffect } from '../../vendor/preact-htm.js';
import { api } from '../api.js';
import { PainelCodigo } from '../codigo.js';
import { rota, plural, cls } from '../util.js';
import { useDados, Estado, Titulo, Vazio } from './comum.js';

const TIPOS = { casos: 'Casos de entrada e saída', pytest: 'Testes (pytest)', estilo: 'Estilo (ruff)', comando: 'Comando' };

function detalhe(c) {
  if (c.tipo === 'casos') return plural(c.casos.length, 'caso', 'casos');
  if (c.tipo === 'pytest') return c.testes;
  if (c.tipo === 'estilo') return [c.regras?.length ? `regras ${c.regras.join(', ')}` : 'regras padrão', c.formato && 'formatação'].filter(Boolean).join(', ');
  return c.comando;
}

function nomeDe(alunos, id) {
  if (id.startsWith('referencias/')) return `Referência: ${id.slice('referencias/'.length)}`;
  return alunos.find((a) => a.id === id)?.nome || id;
}

export function TelaChecagens({ id }) {
  const enc = encodeURIComponent(id);
  const pipeline = useDados(`/pipelines/${enc}`);
  const plagio = useDados(`/avaliacoes/${enc}/plagio`);
  const pacote = useDados(`/avaliacoes/${enc}`);
  const [todos, setTodos] = useState(false);
  const volta = { href: `#${rota('avaliacoes', id)}`, texto: 'Notas' };
  const carregando = pipeline.carregando || plagio.carregando || pacote.carregando;
  const erro = pipeline.erro || plagio.erro || pacote.erro;
  if (carregando || erro) {
    return html`<div class="tela"><${Titulo} titulo="Checagens e plágio" voltar=${volta} /><${Estado} carregando=${carregando} erro=${erro} /></div>`;
  }
  const p = pipeline.dados;
  const alunos = pacote.dados.turma.alunos;
  const res = plagio.dados.resultado;
  const limiar = plagio.dados.limiar;
  const pares = (res?.pares || []).filter((x) => x.similaridade > 0);
  const visiveis = todos ? pares : pares.slice(0, 15);

  return html`
    <div class="tela">
    <${Titulo} titulo="Checagens e plágio" voltar=${volta} />
    <p class="suave pagina-intro">${pacote.dados.avaliacao.titulo}. As checagens rodam fora do servidor, no executor (<code>rubricas executor</code>), e viram sugestões no cartão de avaliar. Quem decide é sempre você.</p>

    <h2 class="secao">Pipeline</h2>
    ${!p.toml && html`
      <${Vazio} titulo="Esta avaliação ainda não tem pipeline">
        <p class="suave">Peça ao Claude Code (skill <code>rubricas</code>) para montar um a partir da especificação, ou grave <code>pipelines/${id}/pipeline.toml</code> na pasta de dados.</p>
      <//>
    `}
    ${p.erro && html`<p class="aviso erro" role="alert">${p.erro}</p>`}
    ${p.pipeline && html`
      <p class=${p.aguardando ? 'pendente' : 'suave'}>
        ${p.aguardando ? `${plural(p.aguardando, 'trabalho aguardando', 'trabalhos aguardando')} o executor.` : 'Tudo checado.'}
        ${p.executor_visto_em ? ` Executor visto às ${new Date(p.executor_visto_em * 1000).toLocaleTimeString('pt-BR', { timeStyle: 'short' })}.` : p.aguardando ? ' Nenhum executor conectado desde que o servidor subiu.' : ''}
      </p>
      <div class="rolagem">
        <table class="tabela">
          <thead><tr><th>Checagem</th><th>Tipo</th><th>Sugere</th></tr></thead>
          <tbody>
            ${p.pipeline.checagens.map((c) => html`
              <tr>
                <td><strong>${c.nome}</strong><span class="linha-meta">${detalhe(c)}</span></td>
                <td>${TIPOS[c.tipo]}</td>
                <td>${c.criterio || html`<span class="suave">—</span>`}</td>
              </tr>
            `)}
          </tbody>
        </table>
      </div>
      <p class="suave">Arquivo do aluno: ${p.pipeline.arquivo ? html`renomeado para <code>${p.pipeline.arquivo}</code>` : 'o único .py da submissão'}. Limite: ${p.pipeline.tempo} s por execução, ${p.pipeline.memoria} de memória, imagem <code>${p.pipeline.imagem}</code>.</p>
    `}
    ${p.anexos.length > 0 && html`
      <p class="anexos"><strong>Anexos:</strong> ${p.anexos.map((a, i) => html`${i ? ', ' : ' '}<a href=${`api/pipelines/${enc}/${a.caminho.split('/').map(encodeURIComponent).join('/')}`} download>${a.caminho}</a>`)}</p>
    `}
    ${p.toml && html`<details class="fonte-toml"><summary>Ver pipeline.toml</summary><pre>${p.toml}</pre></details>`}

    <h2 class="secao">Plágio</h2>
    ${!p.pipeline?.plagio && html`<p class="suave">O pipeline não pede checagem de plágio. Ela é ligada com <code>plagio = { limiar = 0.6 }</code> no pipeline.toml.</p>`}
    ${p.pipeline?.plagio && !res && html`<p class="suave">O JPlag ainda não rodou nesta avaliação.</p>`}
    ${res?.erro && html`<div class="aviso erro" role="alert"><p>O JPlag não rodou: ${res.erro.split('\n')[0]}</p><details><summary>Detalhes</summary><pre class="checagem-saida">${res.erro}</pre></details></div>`}
    ${res && !res.erro && html`
      <p class="suave">${pares.length ? `Os pares mais parecidos. Acima de ${Math.round((limiar || 0) * 100)}%, o cartão de avaliar mostra um alerta.` : 'Nenhum par com semelhança.'}</p>
      ${pares.length > 0 && html`
        <div class="rolagem">
          <table class="tabela plagio">
            <thead><tr><th>Submissão</th><th>Parecida com</th><th class="numerico">Semelhança</th></tr></thead>
            <tbody>
              ${visiveis.map((x) => html`
                <tr class=${cls(limiar && x.similaridade >= limiar && 'acima')}>
                  <td>${nomeDe(alunos, x.a)}</td>
                  <td>${nomeDe(alunos, x.b)}</td>
                  <td class="numerico">${Math.round(x.similaridade * 100)}%</td>
                </tr>
              `)}
            </tbody>
          </table>
        </div>
        ${pares.length > visiveis.length && html`<button class="botao-texto" onClick=${() => setTodos(true)}>Mostrar os ${pares.length} pares</button>`}
      `}
      <div class="acoes">
        <a class="botao pequeno" href=${`api/avaliacoes/${enc}/plagio.jplag`} download>Baixar o relatório .jplag</a>
        <a class="botao-texto" href="https://jplag.github.io/JPlag/" target="_blank" rel="noopener">Abrir o visualizador oficial do JPlag</a>
      </div>
    `}
    </div>
  `;
}

/** No monitor: o código e as checagens do aluno que o mesmo usuário abriu em
    outro aparelho. Troca sozinha quando ele troca de aluno. */
export function Acompanhar() {
  const [foco, setFoco] = useState(undefined); // undefined: conectando; null: nada aberto
  const [conectado, setConectado] = useState(true);
  const [pacote, setPacote] = useState(null);
  const [erro, setErro] = useState('');

  useEffect(() => {
    const fonte = new EventSource('api/foco/eventos');
    fonte.addEventListener('foco', (e) => {
      setConectado(true);
      try {
        setFoco(JSON.parse(e.data));
      } catch {
        /* evento malformado: espera o próximo */
      }
    });
    fonte.onopen = () => setConectado(true);
    // O navegador reconecta sozinho; enquanto isso, a tela avisa.
    fonte.onerror = () => setConectado(false);
    return () => fonte.close();
  }, []);

  // Recarrega o pacote a cada troca de aluno e, enquanto ele está aberto, a cada
  // 30 s: checagens que o executor terminar aparecem sem mexer no monitor.
  useEffect(() => {
    if (!foco) return undefined;
    let vivo = true;
    const carregar = () => api('GET', `/avaliacoes/${encodeURIComponent(foco.avaliacao)}`)
      .then((p) => {
        if (!vivo) return;
        setPacote(p);
        setErro('');
      })
      .catch((e) => vivo && setErro(e.message));
    carregar();
    const t = setInterval(carregar, 30000);
    return () => {
      vivo = false;
      clearInterval(t);
    };
  }, [foco?.avaliacao, foco?.aluno]);

  useEffect(() => {
    document.documentElement.classList.add('avaliando');
    return () => document.documentElement.classList.remove('avaliando');
  }, []);

  const aluno = foco && pacote?.avaliacao.id === foco.avaliacao ? pacote.turma.alunos.find((a) => a.id === foco.aluno) : null;
  return html`
    <div class="acompanhar">
      <header class="acompanhar-topo">
        <a class="av-sair" href="#/avaliacoes" aria-label="Sair do acompanhamento">‹</a>
        <div class="acompanhar-titulo">
          ${aluno
            ? html`<h1>${aluno.nome}</h1><p>${pacote.avaliacao.titulo} · ${aluno.id}</p>`
            : html`<h1>Acompanhar</h1><p>Mostra o código do aluno que você abrir em outro aparelho.</p>`}
        </div>
        <span class=${cls('acompanhar-sinal', conectado ? 'ok' : 'fora')} role="status">${conectado ? 'Conectado' : 'Reconectando…'}</span>
      </header>
      <main class="acompanhar-corpo">
        ${erro && html`<p class="aviso erro">${erro}</p>`}
        ${foco === undefined && html`<p class="acompanhar-espera" aria-busy="true">Conectando…</p>`}
        ${foco === null && html`
          <div class="acompanhar-espera">
            <p><strong>Nenhum aluno aberto.</strong></p>
            <p>Entre com o mesmo usuário no celular e abra uma avaliação. Esta tela segue o aluno que estiver aberto lá.</p>
          </div>
        `}
        ${foco && !aluno && !erro && html`<p class="acompanhar-espera" aria-busy="true">Carregando…</p>`}
        ${aluno && !pacote.ead && html`<p class="acompanhar-espera">Esta avaliação não tem submissões do Moodle.</p>`}
        ${aluno && pacote.ead && html`<${PainelCodigo} grande avaliacao=${foco.avaliacao} aluno=${aluno.id} ead=${pacote.ead} alunos=${pacote.turma.alunos} />`}
      </main>
    </div>
  `;
}
