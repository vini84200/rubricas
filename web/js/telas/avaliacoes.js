// Avaliações: uma ou mais rubricas atreladas a uma turma, e as notas que saem delas.
import { html, useState, useEffect } from '../../vendor/preact-htm.js';
import { api, carregarPacote } from '../api.js';
import { calcular, maximoRubrica, resumoItens, textoEfeito, descreverOperacao, atraso } from '../nota.js';
import { ir, rota, num, campo, lerNum, plural, cls, pts } from '../util.js';
import { useDados, useAcao, Estado, Titulo, Vazio, Progresso, ListaArquivadas, AvisoArquivada, BotaoArquivo, arquivar, Compartilhar } from './comum.js';
import { sessao, pode } from '../sessao.js';
import { textoPlagio } from '../codigo.js';

function LinhaAvaliacao(a) {
  return html`
    <li class="linha-avaliacao">
      <a class="linha" href=${`#${rota('avaliacoes', a.id)}`}>
        <span class="linha-titulo">${a.titulo}</span>
        <span class="linha-meta">${a.turma}, ${a.concluidos} de ${plural(a.alunos, 'aluno', 'alunos')}</span>
        <${Progresso} concluidos=${a.concluidos} iniciados=${a.iniciados} total=${a.alunos} />
      </a>
      ${pode(a, 'corrigir') && html`<a class="botao primario" href=${`#${rota('avaliar', a.id)}`}>Avaliar</a>`}
    </li>
  `;
}

export function ListaAvaliacoes() {
  const { dados, ...estado } = useDados('/avaliacoes');
  const ativas = dados?.filter((a) => !a.arquivada) || [];
  return html`
    <div class="tela">
    <${Titulo} titulo="Avaliações">
      ${sessao.pode('criar') && html`<a class="botao primario" href="#/avaliacoes/nova">Nova avaliação</a>`}
    <//>
    <${Estado} ...${estado} />
    ${dados && !dados.length && !sessao.pode('criar') && html`
      <${Vazio} titulo="Nenhuma avaliação compartilhada com você">
        <p class="suave">Quando alguém der acesso a você numa avaliação, ela aparece aqui.</p>
      <//>
    `}
    ${dados && !dados.length && sessao.pode('criar') && html`
      <${Vazio} titulo="Nenhuma avaliação ainda">
        <p class="suave">Uma avaliação junta uma turma e as rubricas que você vai usar para corrigir. Você precisa de pelo menos uma <a href="#/rubricas">rubrica</a> e uma <a href="#/turmas">turma</a>.</p>
      <//>
    `}
    ${ativas.length > 0 && html`<ul class="lista">${ativas.map(LinhaAvaliacao)}</ul>`}
    ${dados && html`<${ListaArquivadas} itens=${dados.filter((a) => a.arquivada)} linha=${LinhaAvaliacao} />`}
    </div>
  `;
}

/** O corpo do PUT de uma avaliação como está, para quem muda só uma parte dela. */
function corpoDe(a) {
  return {
    titulo: a.titulo,
    turma: a.turma,
    nota_maxima: a.nota_maxima,
    rubricas: a.rubricas.map((r) => ({ origem: r.origem, peso: r.peso })),
    efeitos: (a.efeitos || []).map((t) => t.id),
    prazo: a.prazo || null,
    tolerancia: a.tolerancia || 0,
  };
}

/** "−1 por dia, até −3 · sugerido pelo atraso · encerra a correção" */
export function resumoEfeito(t) {
  const partes = [descreverOperacao(t)];
  if (t.sugerir === 'plagio') partes.push('sugerido pelo plágio');
  if (t.sugerir === 'atraso') partes.push('sugerido pelo atraso');
  if (t.encerra) partes.push('encerra a correção');
  if (t.feedback === false) partes.push('não vai ao aluno');
  return partes.join(' · ');
}

export function FormAvaliacao({ id }) {
  const rubricas = useDados('/rubricas');
  const turmas = useDados('/turmas');
  const catalogo = useDados('/efeitos');
  const atual = useDados(id ? rota('avaliacoes', id) : null);
  const [f, setF] = useState(id ? null : { titulo: '', turma: '', nota_maxima: '10', escolhidas: {}, efeitos: [], prazo: '', tolerancia: '' });
  const { rodar, ocupado, erro, setErro } = useAcao();

  useEffect(() => {
    if (atual.dados) {
      const a = atual.dados.avaliacao;
      setF({
        titulo: a.titulo,
        turma: a.turma,
        nota_maxima: campo(a.nota_maxima),
        escolhidas: Object.fromEntries(a.rubricas.map((r) => [r.origem, campo(r.peso)])),
        efeitos: (a.efeitos || []).map((t) => t.id),
        prazo: a.prazo || '',
        tolerancia: a.tolerancia ? String(a.tolerancia) : '',
      });
    }
  }, [atual.dados]);

  useEffect(() => {
    const ativas = turmas.dados?.filter((t) => !t.arquivada);
    if (f && !f.turma && ativas?.length === 1) setF({ ...f, turma: ativas[0].id });
  }, [turmas.dados, f]);

  const carregando = rubricas.carregando || turmas.carregando || atual.carregando || catalogo.carregando;
  const erroCarga = rubricas.erro || turmas.erro || atual.erro || catalogo.erro;
  const volta = id ? { href: `#${rota('avaliacoes', id)}`, texto: 'Notas' } : { href: '#/avaliacoes', texto: 'Avaliações' };
  if (carregando || erroCarga || !f) {
    return html`<div class="tela"><${Titulo} titulo=${id ? 'Editar avaliação' : 'Nova avaliação'} voltar=${volta} /><${Estado} carregando=${carregando} erro=${erroCarga} /></div>`;
  }

  // Arquivadas só aparecem se já estão escolhidas, para não sumirem da avaliação.
  const turmasVisiveis = turmas.dados.filter((t) => !t.arquivada || t.id === f.turma);
  // A turma de uma avaliação compartilhada pode não estar compartilhada com quem edita.
  if (f.turma && atual.dados && !turmasVisiveis.some((t) => t.id === f.turma)) turmasVisiveis.push(atual.dados.turma);
  // Rubricas da pasta, mais as que a avaliação já usa e foram apagadas de lá.
  const opcoes = rubricas.dados.rubricas.filter((r) => !r.arquivada || r.id in f.escolhidas);
  for (const ra of atual.dados?.avaliacao.rubricas || []) {
    if (!opcoes.some((r) => r.id === ra.origem)) opcoes.push({ ...ra.rubrica, id: ra.origem, _soCopia: true });
  }
  const ordem = Object.keys(f.escolhidas);
  // Tipos do catálogo, mais os que a avaliação já usa e saíram de lá.
  const tiposEfeito = [...catalogo.dados.efeitos];
  for (const t of atual.dados?.avaliacao.efeitos || []) {
    if (!tiposEfeito.some((x) => x.id === t.id)) tiposEfeito.push({ ...t, _soCopia: true });
  }
  const comAtraso = tiposEfeito.some((t) => t.sugerir === 'atraso' && f.efeitos.includes(t.id));
  const alternarEfeito = (tid) => setF({ ...f, efeitos: f.efeitos.includes(tid) ? f.efeitos.filter((x) => x !== tid) : [...f.efeitos, tid] });

  function alternar(rid) {
    const escolhidas = { ...f.escolhidas };
    if (rid in escolhidas) delete escolhidas[rid];
    else escolhidas[rid] = '1';
    setF({ ...f, escolhidas });
  }

  function salvar(e) {
    e.preventDefault();
    const corpo = {
      titulo: f.titulo,
      turma: f.turma,
      nota_maxima: lerNum(f.nota_maxima),
      rubricas: ordem.map((origem) => ({ origem, peso: lerNum(f.escolhidas[origem]) })),
      efeitos: f.efeitos.filter((tid) => tiposEfeito.some((t) => t.id === tid)),
      prazo: f.prazo || null,
      tolerancia: f.tolerancia.trim() ? Number(f.tolerancia) : 0,
    };
    if (!Number.isFinite(corpo.nota_maxima) || corpo.rubricas.some((r) => !Number.isFinite(r.peso))) {
      setErro('Preencha a nota máxima e os pesos com números');
      return;
    }
    if (!Number.isInteger(corpo.tolerancia) || corpo.tolerancia < 0) {
      setErro('A tolerância é um número inteiro de minutos');
      return;
    }
    rodar(async () => {
      const a = await (id ? api('PUT', rota('avaliacoes', id), corpo) : api('POST', '/avaliacoes', corpo));
      ir(rota('avaliacoes', a.id));
    });
  }

  return html`
    <div class="tela">
    <${Titulo} titulo=${id ? 'Editar avaliação' : 'Nova avaliação'} voltar=${volta} />
    <form class="formulario" onSubmit=${salvar}>
      <label class="campo">
        <span>Título</span>
        <input required maxlength="120" placeholder="Trabalho final" value=${f.titulo} onInput=${(e) => setF({ ...f, titulo: e.target.value })} />
      </label>
      <div class="par">
        <label class="campo">
          <span>Turma</span>
          <select required value=${f.turma} onChange=${(e) => setF({ ...f, turma: e.target.value })}>
            <option value="" disabled>Escolha a turma</option>
            ${turmasVisiveis.map((t) => html`<option value=${t.id}>${t.id} (${t.alunos.length})${t.arquivada ? ', arquivada' : ''}</option>`)}
          </select>
        </label>
        <label class="campo curto">
          <span>Nota máxima</span>
          <input class="numero" inputmode="decimal" required value=${f.nota_maxima} onInput=${(e) => setF({ ...f, nota_maxima: e.target.value })} />
        </label>
      </div>
      ${!turmasVisiveis.length && html`<p class="aviso">${turmas.dados.length ? 'Todas as turmas estão arquivadas.' : 'Nenhuma turma cadastrada.'} <a href="#/turmas/nova">Criar uma turma</a></p>`}

      <fieldset class="campo">
        <legend>Rubricas</legend>
        ${!opcoes.length && html`<p class="aviso">${rubricas.dados.rubricas.length ? 'Todas as rubricas estão arquivadas.' : 'Nenhuma rubrica cadastrada.'} <a href="#/rubricas/nova">Montar uma rubrica</a></p>`}
        <ul class="escolhas">
          ${opcoes.map((r) => {
            const marcada = r.id in f.escolhidas;
            return html`
              <li class=${cls('escolha', marcada && 'marcada')}>
                <label class="escolha-rotulo">
                  <input type="checkbox" checked=${marcada} onChange=${() => alternar(r.id)} />
                  <span>
                    <span class="linha-titulo">${r.titulo}</span>
                    <span class="linha-meta">${resumoItens(r)}, ${pts(maximoRubrica(r))}${r._soCopia ? ' (só existe a cópia desta avaliação)' : ''}${r.arquivada ? ' (arquivada)' : ''}</span>
                  </span>
                </label>
                ${marcada && ordem.length > 1 && html`
                  <label class="escolha-peso">
                    <span>Peso</span>
                    <input class="numero" inputmode="decimal" value=${f.escolhidas[r.id]}
                      onInput=${(e) => setF({ ...f, escolhidas: { ...f.escolhidas, [r.id]: e.target.value } })} />
                  </label>
                `}
              </li>
            `;
          })}
        </ul>
        ${ordem.length > 1 && html`<small class="suave">A nota final é a média das rubricas ponderada pelos pesos, na escala da nota máxima.</small>`}
        <small class="suave">A avaliação guarda uma cópia de cada rubrica: editar a rubrica depois não muda correções já feitas.</small>
      </fieldset>

      <fieldset class="campo">
        <legend>Efeitos sobre a nota</legend>
        ${!tiposEfeito.length && html`<p class="suave">Nenhum tipo de efeito no catálogo. Eles são criados pela API em TOML (a skill do Claude ensina): zero por plágio, desconto por atraso, bônus. <a href="#/efeitos">Ver o catálogo</a></p>`}
        ${tiposEfeito.length > 0 && html`
          <ul class="escolhas">
            ${tiposEfeito.map((t) => {
              const marcada = f.efeitos.includes(t.id);
              return html`
                <li class=${cls('escolha', marcada && 'marcada')}>
                  <label class="escolha-rotulo">
                    <input type="checkbox" checked=${marcada} onChange=${() => alternarEfeito(t.id)} />
                    <span>
                      <span class="linha-titulo">${t.titulo}</span>
                      <span class="linha-meta">${resumoEfeito(t)}${t._soCopia ? ' (só existe a cópia desta avaliação)' : ''}</span>
                    </span>
                  </label>
                </li>
              `;
            })}
          </ul>
          <small class="suave">Quem avalia aplica no resumo do aluno; o plágio e o atraso aparecem como sugestão. <a href="#/efeitos">Ver o catálogo</a></small>
        `}
        ${comAtraso && html`
          <div class="par">
            <label class="campo">
              <span>Prazo de entrega</span>
              <input type="datetime-local" value=${f.prazo} onInput=${(e) => setF({ ...f, prazo: e.target.value })} />
            </label>
            <label class="campo curto">
              <span>Tolerância (min)</span>
              <input class="numero" inputmode="numeric" placeholder="0" value=${f.tolerancia} onInput=${(e) => setF({ ...f, tolerancia: e.target.value })} />
            </label>
          </div>
          <small class="suave">O atraso sai da data de envio da planilha de notas do Moodle: cada 24 h começadas depois do prazo contam um dia. Envie a planilha junto com o ZIP, na tela da avaliação.</small>
        `}
      </fieldset>
      ${erro && html`<p class="erro" role="alert">${erro}</p>`}
      <div class="acoes">
        <button class="botao primario" disabled=${ocupado || !ordem.length || !f.turma}>${id ? 'Salvar' : 'Criar avaliação'}</button>
        <a class="botao" href=${volta.href}>Cancelar</a>
      </div>
    </form>
    </div>
  `;
}

export function Resultados({ id }) {
  const [pacote, setPacote] = useState(null);
  const [erro, setErro] = useState(null);
  const { rodar, erro: aviso } = useAcao();

  async function carregar() {
    try {
      setPacote(await carregarPacote(id));
      setErro(null);
    } catch (e) {
      setErro(e);
    }
  }
  useEffect(() => {
    carregar();
  }, [id]);

  if (erro) return html`<div class="tela"><${Titulo} titulo="Avaliação" voltar=${{ href: '#/avaliacoes', texto: 'Avaliações' }} /><${Estado} erro=${erro} recarregar=${carregar} /></div>`;
  if (!pacote) return html`<${Estado} carregando=${true} />`;

  const { avaliacao: a, turma, correcoes, desatualizadas, ead } = pacote;
  const efeitosDesatualizados = pacote.efeitos_desatualizados || [];
  const comEfeitos = (a.efeitos || []).length > 0;
  const linhas = turma.alunos.map((aluno) => ({ aluno, r: calcular(a, correcoes[aluno.id]) }));
  const concluidos = linhas.filter((l) => l.r.concluida).length;
  const iniciados = linhas.filter((l) => !l.r.concluida && l.r.respondidos > 0).length;
  const notas = linhas.filter((l) => l.r.concluida).map((l) => l.r.nota);
  const media = notas.length ? notas.reduce((s, n) => s + n, 0) / notas.length : null;
  const varias = a.rubricas.length > 1;

  const caminho = rota('avaliacoes', id);

  function atualizarCopias() {
    const texto = desatualizadas.length
      ? 'Atualizar a cópia a partir da pasta? Marcações em critérios ou níveis que deixaram de existir são desconsideradas.'
      : 'Atualizar a cópia dos tipos de efeito a partir do catálogo? As notas com esses efeitos mudam.';
    if (!confirm(texto)) return;
    rodar(async () => {
      await api('PUT', caminho, { ...corpoDe(a), atualizar: desatualizadas, atualizar_efeitos: efeitosDesatualizados });
      carregar();
    });
  }

  const alternarArquivada = () => rodar(async () => {
    await arquivar(caminho, !pacote.arquivada);
    carregar();
  });

  function apagar() {
    if (!confirm(`Apagar a avaliação “${a.titulo}” e todas as correções dela?`)) return;
    rodar(async () => {
      await api('DELETE', caminho);
      ir('/avaliacoes');
    });
  }

  const csv = `api${caminho}/notas.csv`;
  return html`
    <div class="tela">
    <${Titulo} titulo=${a.titulo} voltar=${{ href: '#/avaliacoes', texto: 'Avaliações' }}>
      ${pode(pacote, 'corrigir') && html`<a class="botao primario" href=${`#${rota('avaliar', id)}`}>Avaliar</a>`}
    <//>
    ${pacote.offline && html`<p class="aviso">Sem conexão: mostrando a última cópia guardada neste aparelho.</p>`}
    ${aviso && html`<p class="aviso erro" role="alert">${aviso}</p>`}
    ${pacote.arquivada && pode(pacote, 'arquivar') && html`<${AvisoArquivada} texto="Avaliação arquivada: fica fora da lista e o executor não roda mais as checagens dela. As notas continuam aqui." aoDesarquivar=${alternarArquivada} />`}
    ${(desatualizadas.length > 0 || efeitosDesatualizados.length > 0) && pode(pacote, 'editar') && html`
      <p class="aviso">
        ${desatualizadas.length > 0 && `${desatualizadas.length === 1 ? 'Uma rubrica mudou' : 'Rubricas mudaram'} na pasta depois de entrar nesta avaliação. `}
        ${efeitosDesatualizados.length > 0 && `${efeitosDesatualizados.length === 1 ? 'Um tipo de efeito mudou' : 'Tipos de efeito mudaram'} no catálogo. `}
        As correções continuam usando a cópia antiga.
        <button class="botao pequeno" onClick=${atualizarCopias}>Atualizar cópia</button>
      </p>
    `}
    <div class="resumo-avaliacao">
      <p>${turma.id}. ${concluidos} de ${plural(turma.alunos.length, 'aluno avaliado', 'alunos avaliados')}${media !== null ? html`, média ${num(media)} de ${num(a.nota_maxima)}` : ''}.</p>
      <${Progresso} concluidos=${concluidos} iniciados=${iniciados} total=${turma.alunos.length} />
    </div>

    ${(ead || pode(pacote, 'editar')) && html`<${Entregas} id=${id} pacote=${pacote} aoMudar=${carregar} editar=${pode(pacote, 'editar')} />`}

    <div class="acoes acoes-notas">
      ${pode(pacote, 'exportar') && html`
        <a class="botao pequeno" href=${csv} download>Baixar notas (CSV)</a>
        <a class="botao pequeno" href=${`${csv}?br=1`} download>CSV para Excel em português</a>
        <${PlanilhaMoodle} id=${id} />
      `}
      ${pode(pacote, 'editar') && html`<a class="botao pequeno" href=${`#${rota('avaliacoes', id, 'editar')}`}>Editar</a>`}
      ${!pacote.arquivada && pode(pacote, 'arquivar') && html`<button class="botao pequeno" onClick=${alternarArquivada}>Arquivar</button>`}
      ${pode(pacote, 'apagar') && html`<button class="botao pequeno perigo" onClick=${apagar}>Apagar</button>`}
    </div>

    <div class="rolagem">
      <table class="tabela notas">
        <thead>
          <tr>
            <th>Aluno</th>
            ${ead && html`<th>Entrega</th>`}
            ${varias && a.rubricas.map((r) => html`<th class="numerico">${r.rubrica.titulo}</th>`)}
            ${comEfeitos && html`<th class="numerico">Antes</th><th>Efeitos</th>`}
            <th class="numerico">Nota</th>
          </tr>
        </thead>
        <tbody>
          ${linhas.map(({ aluno, r }) => html`
            <tr class="clicavel" onClick=${() => ir(rota('avaliar', id, aluno.id))}>
              <td>
                <a href=${`#${rota('avaliar', id, aluno.id)}`} onClick=${(e) => e.stopPropagation()}>${aluno.nome}</a>
                <span class="linha-meta">${aluno.id}</span>
              </td>
              ${ead && html`<td><${Entrega} ead=${ead} aluno=${aluno.id} alunos=${turma.alunos} /></td>`}
              ${varias && r.rubricas.map((p) => html`<td class="numerico">${r.respondidos ? `${num(p.pontos)} / ${num(p.maximo)}` : ''}</td>`)}
              ${comEfeitos && html`
                <td class="numerico">${r.efeitos.length && r.respondidos ? num(r.antes) : ''}</td>
                <td class="efeitos-celula">${r.efeitos.map((tid) => {
                  const t = a.efeitos.find((x) => x.id === tid);
                  const ap = correcoes[aluno.id].efeitos[tid];
                  return html`<span class="selo" title=${ap.justificativa || null}>${textoEfeito(t, { ...ap, justificativa: '' })}</span>`;
                })}</td>
              `}
              <td class="numerico">
                ${r.concluida
                  ? html`<strong>${num(r.nota)}</strong>`
                  : r.respondidos
                    ? html`<span class="parcial">${num(r.nota)}<small> ${r.respondidos} de ${r.total}</small></span>`
                    : html`<span class="suave">—</span>`}
              </td>
            </tr>
          `)}
        </tbody>
      </table>
    </div>
    ${pode(pacote, 'compartilhar') && !pacote.offline && html`<${Compartilhar} caminho=${caminho} pasta="avaliacoes" />`}
    </div>
  `;
}

/**
 * Recebe a planilha de notas baixada da tarefa do Moodle e baixa de volta a mesma
 * planilha preenchida, pronta para "Carregar planilha de notas".
 */
function PlanilhaMoodle({ id }) {
  const [msg, setMsg] = useState(null);

  async function enviar(arquivo) {
    setMsg(null);
    try {
      const r = await api('POST', rota('avaliacoes', id, 'planilha-moodle'), arquivo);
      const link = document.createElement('a');
      link.href = URL.createObjectURL(new Blob([r.csv], { type: 'text/csv;charset=utf-8' }));
      link.download = `${arquivo.name.replace(/\.csv$/i, '')}-com-notas.csv`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(link.href), 10000);
      const avisos = [];
      if (r.incompletas.length) avisos.push(`Correção incompleta, nota não preenchida (fica a que estiver no Moodle): ${r.incompletas.join(', ')}.`);
      if (r.fora_da_planilha.length) avisos.push(`Avaliados que não estão na planilha: ${r.fora_da_planilha.join(', ')}.`);
      if (r.sem_aluno.length) avisos.push(`Nomes do Moodle que não casaram com ninguém da turma: ${r.sem_aluno.join(', ')}.`);
      if (r.bloqueadas.length) avisos.push(`Nota travada no Moodle, não mexida: ${r.bloqueadas.join(', ')}.`);
      const convertidas = r.convertida_para ? ` (convertidas para a nota máxima do Moodle, ${num(r.convertida_para)})` : '';
      setMsg({
        ok: true,
        texto: `${plural(r.preenchidas, 'nota preenchida', 'notas preenchidas')}${convertidas}. No Moodle, envie o arquivo baixado em “Carregar planilha de notas” da tarefa.`,
        avisos,
      });
    } catch (err) {
      setMsg({ ok: false, texto: err.message, avisos: [] });
    }
  }

  return html`
    <${BotaoArquivo} accept=".csv,text/csv" rotulo="Preencher planilha do Moodle" rotuloOcupado="Preenchendo…" aoEscolher=${enviar}
      title="A planilha que a tarefa do Moodle baixa em “Baixar planilha de notas”" />
    ${msg && html`
      <div class="planilha-moodle" role="status">
        <p class=${msg.ok ? 'ok' : 'erro'}>${msg.texto}</p>
        ${msg.avisos.map((t) => html`<p class="pendente">${t}</p>`)}
      </div>
    `}
  `;
}

/** Situação da entrega de um aluno, numa célula da tabela de notas. */
function Entrega({ ead, aluno, alunos }) {
  const sub = ead.submissoes[aluno];
  if (!sub) return html`<span class="suave">—</span>`;
  const rel = ead.relatorios[aluno];
  const ok = rel?.atual ? rel.checagens.filter((c) => c.situacao === 'passou').length : null;
  return html`
    <span class="entrega">
      <span title="Entregou">✓</span>
      ${ok !== null && html`<span class="entrega-checagens" title="Checagens que passaram">${ok}/${rel.checagens.length}</span>`}
      ${sub.mudou && html`<span class="selo meio" title="A submissão mudou depois da correção">mudou</span>`}
      ${ead.plagio[aluno] && html`<span class="selo nao" title=${textoPlagio(ead.plagio[aluno], alunos)}>${Math.round(ead.plagio[aluno].similaridade * 100)}%</span>`}
    </span>
  `;
}

function quando(segundos) {
  const min = Math.round((Date.now() / 1000 - segundos) / 60);
  if (min < 2) return 'agora há pouco';
  if (min < 60) return `há ${min} min`;
  if (min < 48 * 60) return `há ${Math.round(min / 60)} h`;
  return `há ${Math.round(min / 1440)} dias`;
}

/** O que o servidor fez com o ZIP, numa frase. */
function frasesDoZip(r) {
  const partes = [`${plural(r.recebidas, 'submissão nova ou alterada', 'submissões novas ou alteradas')}`];
  if (r.iguais) partes.push(`${plural(r.iguais, 'igual à anterior', 'iguais às anteriores')}`);
  if (r.sem_dono.length) partes.push(`${plural(r.sem_dono.length, 'nome sem dono', 'nomes sem dono')}`);
  if (r.edicoes_descartadas.length) partes.push(`${plural(r.edicoes_descartadas.length, 'edição sua descartada', 'edições suas descartadas')} (o aluno mandou código novo)`);
  if (r.ignorados.length) partes.push(`${plural(r.ignorados.length, 'arquivo fora do padrão ignorado', 'arquivos fora do padrão ignorados')}`);
  return `${partes.join(', ')}.`;
}

/** E com a planilha de notas, só pelas datas de envio. */
function frasesDaPlanilha(r) {
  let texto = `Planilha do Moodle: ${plural(r.com_data, 'data de envio registrada', 'datas de envio registradas')}`;
  if (r.atrasados !== null) texto += `, ${plural(r.atrasados, 'enviada depois do prazo', 'enviadas depois do prazo')}`;
  texto += '.';
  if (r.ilegiveis.length) texto += ` Data ilegível: ${r.ilegiveis.join(', ')}.`;
  if (r.sem_aluno.length) texto += ` Sem aluno na turma: ${r.sem_aluno.join(', ')}.`;
  return texto;
}

/** Envio do ZIP do Moodle (e da planilha de notas, pelas datas de envio), quem
    ficou sem dono e a fila do executor. */
function Entregas({ id, pacote, aoMudar, editar }) {
  const { ead, avaliacao } = pacote;
  const alunos = pacote.turma.alunos;
  const [msg, setMsg] = useState(null);
  const tiposAtraso = (avaliacao.efeitos || []).filter((t) => t.sugerir === 'atraso');
  const envios = pacote.envios || {};

  // O ZIP começa com "PK"; o resto é tratado como a planilha de notas.
  async function enviar(arquivos) {
    setMsg(null);
    const textos = [];
    let ok = true;
    for (const arquivo of arquivos) {
      try {
        const zip = (await arquivo.slice(0, 2).text()) === 'PK';
        const r = await api('POST', rota('avaliacoes', id, zip ? 'submissoes' : 'envios'), arquivo);
        textos.push(zip ? frasesDoZip(r) : frasesDaPlanilha(r));
      } catch (err) {
        ok = false;
        textos.push(`${arquivo.name}: ${err.message}`);
      }
    }
    setMsg({ ok, texto: textos.join(' ') });
    aoMudar();
  }

  async function associar(chave, aluno) {
    if (!aluno) return;
    try {
      await api('POST', rota('avaliacoes', id, 'submissoes', 'sem-dono', chave), { aluno });
      aoMudar();
    } catch (err) {
      setMsg({ ok: false, texto: err.message });
    }
  }

  const botao = editar && html`
    <${BotaoArquivo} accept=".zip,application/zip,.csv,text/csv" varios rotulo=${ead ? 'Enviar novo ZIP ou planilha' : 'Enviar submissões do Moodle (ZIP)'}
      rotuloOcupado="Enviando…" aoEscolherVarios=${enviar}
      title="O ZIP de “Baixar todas as submissões” e, para o atraso, a planilha de “Baixar planilha de notas”. Os dois juntos também vão." />
  `;
  const atrasados = avaliacao.prazo && tiposAtraso.length
    ? alunos.filter((al) => atraso(avaliacao.prazo, avaliacao.tolerancia, envios[al.id], tiposAtraso[0].operacao.unidade) > 0).length
    : null;
  const linhaAtraso = tiposAtraso.length > 0 && html`
    <br /><span class=${!avaliacao.prazo || !Object.keys(envios).length ? 'pendente' : 'suave'}>
      ${!avaliacao.prazo
        ? 'Sem prazo: defina o prazo em Editar para sugerir o desconto por atraso'
        : !Object.keys(envios).length
          ? 'Sem datas de envio: envie a planilha de notas do Moodle para sugerir o atraso'
          : `${plural(Object.keys(envios).length, 'data de envio', 'datas de envio')}, ${plural(atrasados, 'depois do prazo', 'depois do prazo')}`}
    </span>
  `;
  if (!ead) {
    return html`
      <div class="entregas vazia">
        ${botao}
        ${linhaAtraso && html`<p>${linhaAtraso}</p>`}
        ${msg && html`<p class=${msg.ok ? 'ok' : 'erro'} role="status">${msg.texto}</p>`}
      </div>
    `;
  }
  const entregaram = Object.keys(ead.submissoes).length;
  const comDono = new Set(Object.keys(ead.submissoes));
  return html`
    <section class="entregas" aria-label="Entregas do Moodle">
      <div class="entregas-topo">
        <p>
          <strong>${entregaram} de ${plural(alunos.length, 'aluno entregou', 'alunos entregaram')}</strong>
          ${ead.pipeline && html`<br /><span class=${ead.aguardando ? 'pendente' : 'suave'}>
            ${ead.aguardando ? `${plural(ead.aguardando, 'submissão aguardando', 'submissões aguardando')} o executor` : ead.plagio_pendente ? 'Plágio aguardando o executor' : 'Checagens em dia'}
            ${(ead.aguardando > 0 || ead.plagio_pendente) && (ead.executor_visto_em ? ` (visto ${quando(ead.executor_visto_em)})` : ' (nenhum executor conectado desde que o servidor subiu)')}
          </span>`}
          ${ead.erro_pipeline && html`<br /><span class="erro">${ead.erro_pipeline}</span>`}
          ${linhaAtraso}
        </p>
        <div class="acoes">
          ${botao}
          <a class="botao pequeno" href=${`#${rota('avaliacoes', id, 'checagens')}`}>Checagens e plágio</a>
          <a class="botao pequeno" href="#/acompanhar" title="Abra no monitor: ele segue o aluno que você abrir no celular">Acompanhar no monitor</a>
        </div>
      </div>
      ${msg && html`<p class=${msg.ok ? 'ok' : 'erro'} role="status">${msg.texto}</p>`}
      ${ead.sem_dono.length > 0 && editar && html`
        <div class="sem-dono">
          <p><strong>Sem dono:</strong> estes nomes do Moodle não casaram com ninguém da turma. Escolha o aluno; a associação vale para os próximos ZIPs.</p>
          <ul>
            ${ead.sem_dono.map((s) => html`
              <li>
                <span>${s.nome} <small class="suave">(${plural(s.arquivos, 'arquivo', 'arquivos')})</small></span>
                <select aria-label=${`Aluno para ${s.nome}`} value="" onChange=${(e) => associar(s.chave, e.target.value)}>
                  <option value="">Escolha o aluno…</option>
                  ${alunos.map((a) => html`<option value=${a.id}>${a.nome}${comDono.has(a.id) ? ' (já entregou)' : ''}</option>`)}
                </select>
              </li>
            `)}
          </ul>
        </div>
      `}
    </section>
  `;
}
