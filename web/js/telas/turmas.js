// Turmas: listas de alunos lidas de turmas/<nome>.csv.
import { html, useState } from '../../vendor/preact-htm.js';
import { api } from '../api.js';
import { ir, rota, plural } from '../util.js';
import { useDados, Estado, Titulo, Vazio, ListaArquivadas, AvisoArquivada, arquivar, Compartilhar } from './comum.js';
import { sessao, pode } from '../sessao.js';

function LinhaTurma(t) {
  return html`
    <li>
      <a class="linha" href=${`#${rota('turmas', t.id)}`}>
        <span class="linha-titulo">${t.id}</span>
        <span class="linha-meta">${t.alunos.length ? plural(t.alunos.length, 'aluno', 'alunos') : 'arquivo ilegível'}</span>
      </a>
    </li>
  `;
}

export function ListaTurmas() {
  const { dados, ...estado } = useDados('/turmas');
  const ativas = dados?.filter((t) => !t.arquivada) || [];
  return html`
    <div class="tela">
    <${Titulo} titulo="Turmas">
      ${sessao.pode('criar') && html`<a class="botao primario" href="#/turmas/nova">Nova turma</a>`}
    <//>
    <${Estado} ...${estado} />
    ${dados && !dados.length && html`
      <${Vazio} titulo=${sessao.pode('criar') ? 'Nenhuma turma ainda' : 'Nenhuma turma compartilhada com você'}>
        <p class="suave">Cole uma lista de alunos ou coloque um CSV com as colunas matrícula e nome na pasta <code>turmas</code>.</p>
      <//>
    `}
    ${ativas.length > 0 && html`<ul class="lista">${ativas.map(LinhaTurma)}</ul>`}
    ${dados && html`<${ListaArquivadas} itens=${dados.filter((t) => t.arquivada)} linha=${LinhaTurma} />`}
    </div>
  `;
}

function CampoLista({ csv, setCsv }) {
  async function aoEscolher(e) {
    const arquivo = e.target.files?.[0];
    if (!arquivo) return;
    const bytes = await arquivo.arrayBuffer();
    try {
      setCsv(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
    } catch {
      // Planilhas antigas exportam em Latin-1 / Windows-1252.
      setCsv(new TextDecoder('windows-1252').decode(bytes));
    }
  }
  return html`
    <label class="campo">
      <span>Lista de alunos</span>
      <textarea rows="10" spellcheck="false" placeholder=${'matricula,nome\n00123456,Ana Souza\n00234567,Bruno Lima'}
        value=${csv} onInput=${(e) => setCsv(e.target.value)}></textarea>
      <small class="suave">Uma linha por aluno. Aceita vírgula, ponto e vírgula ou tabulação, com ou sem cabeçalho, e a exportação de participantes do Moodle.</small>
    </label>
    <label class="campo">
      <span>Ou carregue um arquivo CSV</span>
      <input type="file" accept=".csv,.txt,text/csv,text/plain" onChange=${aoEscolher} />
    </label>
  `;
}

export function NovaTurma() {
  const [nome, setNome] = useState('');
  const [csv, setCsv] = useState('');
  const [erro, setErro] = useState('');
  const [ocupado, setOcupado] = useState(false);

  async function salvar(e) {
    e.preventDefault();
    setOcupado(true);
    setErro('');
    try {
      const t = await api('POST', '/turmas', { nome, csv });
      ir(rota('turmas', t.id));
    } catch (err) {
      setErro(err.message);
      setOcupado(false);
    }
  }

  return html`
    <div class="tela">
    <${Titulo} titulo="Nova turma" voltar=${{ href: '#/turmas', texto: 'Turmas' }} />
    <form class="formulario" onSubmit=${salvar}>
      <label class="campo">
        <span>Nome da turma</span>
        <input required maxlength="80" placeholder="INF01202-D-2026-2" value=${nome} onInput=${(e) => setNome(e.target.value)} />
        <small class="suave">Vira o nome do arquivo em <code>turmas/</code>.</small>
      </label>
      <${CampoLista} csv=${csv} setCsv=${setCsv} />
      ${erro && html`<p class="erro" role="alert">${erro}</p>`}
      <div class="acoes">
        <button class="botao primario" disabled=${ocupado || !csv.trim()}>Criar turma</button>
        <a class="botao" href="#/turmas">Cancelar</a>
      </div>
    </form>
    </div>
  `;
}

export function DetalheTurma({ id }) {
  const { dados, recarregar, ...estado } = useDados(`/turmas/${encodeURIComponent(id)}`);
  const [trocando, setTrocando] = useState(false);
  const [csv, setCsv] = useState('');
  const [erro, setErro] = useState('');

  async function substituir(e) {
    e.preventDefault();
    setErro('');
    try {
      await api('PUT', `/turmas/${encodeURIComponent(id)}`, { csv });
      setTrocando(false);
      setCsv('');
      recarregar();
    } catch (err) {
      setErro(err.message);
    }
  }

  async function alternarArquivada() {
    setErro('');
    try {
      await arquivar(`/turmas/${encodeURIComponent(id)}`, !dados.arquivada);
      recarregar();
    } catch (err) {
      setErro(err.message);
    }
  }

  async function apagar() {
    if (!confirm(`Apagar a turma ${id}? O arquivo turmas/${id}.csv será removido.`)) return;
    try {
      await api('DELETE', `/turmas/${encodeURIComponent(id)}`);
      ir('/turmas');
    } catch (err) {
      setErro(err.message);
    }
  }

  return html`
    <div class="tela">
    <${Titulo} titulo=${id} voltar=${{ href: '#/turmas', texto: 'Turmas' }}>
      ${pode(dados, 'editar') && html`<button class="botao" onClick=${() => setTrocando(!trocando)}>Substituir lista</button>`}
      ${dados && !dados.arquivada && pode(dados, 'arquivar') && html`<button class="botao" onClick=${alternarArquivada}>Arquivar</button>`}
      ${pode(dados, 'apagar') && html`<button class="botao perigo" onClick=${apagar}>Apagar</button>`}
    <//>
    <${Estado} ...${estado} recarregar=${recarregar} />
    ${dados?.arquivada && pode(dados, 'arquivar') && html`<${AvisoArquivada} texto="Turma arquivada: fica fora da lista e da escolha de turma nas avaliações novas." aoDesarquivar=${alternarArquivada} />`}
    ${erro && html`<p class="erro" role="alert">${erro}</p>`}
    ${trocando && html`
      <form class="formulario" onSubmit=${substituir}>
        <${CampoLista} csv=${csv} setCsv=${setCsv} />
        <p class="suave">Correções já feitas ficam ligadas à matrícula: alunos que continuarem na lista mantêm as notas.</p>
        <div class="acoes">
          <button class="botao primario" disabled=${!csv.trim()}>Substituir</button>
          <button type="button" class="botao" onClick=${() => setTrocando(false)}>Cancelar</button>
        </div>
      </form>
    `}
    ${dados && html`
      <p class="suave">${plural(dados.alunos.length, 'aluno', 'alunos')}</p>
      <table class="tabela">
        <thead><tr><th>Matrícula</th><th>Nome</th></tr></thead>
        <tbody>
          ${dados.alunos.map((a) => html`<tr><td class="numerico">${a.id}</td><td>${a.nome}</td></tr>`)}
        </tbody>
      </table>
    `}
    ${pode(dados, 'compartilhar') && html`<${Compartilhar} caminho=${`/turmas/${encodeURIComponent(id)}`} pasta="turmas" />`}
    </div>
  `;
}
