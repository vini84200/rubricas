// Entrada da aplicação: sessão, rotas e moldura.
import { html, render, useState, useEffect, useCallback } from '../vendor/preact-htm.js';
import { api, fila } from './api.js';
import { cls, guarda } from './util.js';
import { ListaAvaliacoes, FormAvaliacao, Resultados } from './telas/avaliacoes.js';
import { ListaRubricas, EditorRubrica } from './telas/rubricas.js';
import { ListaTurmas, NovaTurma, DetalheTurma } from './telas/turmas.js';
import { Avaliar } from './telas/avaliar.js';

function lerRota() {
  const partes = location.hash.replace(/^#\/?/, '').split('/').filter(Boolean).map((p) => {
    try {
      return decodeURIComponent(p);
    } catch {
      return p; // um % solto no endereço não derruba a tela
    }
  });
  return partes.length ? partes : ['avaliacoes'];
}

function Tela({ partes }) {
  const [secao, a, b] = partes;
  if (secao === 'rubricas') {
    if (!a) return html`<${ListaRubricas} />`;
    return html`<${EditorRubrica} key=${a} id=${a === 'nova' ? null : a} />`;
  }
  if (secao === 'turmas') {
    if (!a) return html`<${ListaTurmas} />`;
    if (a === 'nova') return html`<${NovaTurma} />`;
    return html`<${DetalheTurma} key=${a} id=${a} />`;
  }
  if (secao === 'avaliacoes' && a === 'nova') return html`<${FormAvaliacao} id=${null} />`;
  if (secao === 'avaliacoes' && a && b === 'editar') return html`<${FormAvaliacao} key=${a} id=${a} />`;
  if (secao === 'avaliacoes' && a) return html`<${Resultados} key=${a} id=${a} />`;
  return html`<${ListaAvaliacoes} />`;
}

const SECOES = [
  ['avaliacoes', 'Avaliações'],
  ['rubricas', 'Rubricas'],
  ['turmas', 'Turmas'],
];

function Moldura({ usuario, partes, aoSair }) {
  const atual = SECOES.some(([s]) => s === partes[0]) ? partes[0] : 'avaliacoes';
  return html`
    <div class="moldura">
      <header class="topo">
        <a class="marca" href="#/avaliacoes">Rubricas</a>
        <nav class="abas" aria-label="Seções">
          ${SECOES.map(
            ([s, nome]) => html`<a href="#/${s}" class=${cls('aba', s === atual && 'atual')} aria-current=${s === atual ? 'page' : null}>${nome}</a>`,
          )}
        </nav>
        <div class="conta">
          <span class="conta-nome">${usuario}</span>
          <button class="botao-texto" onClick=${aoSair}>Sair</button>
        </div>
      </header>
      <main class="conteudo"><${Tela} partes=${partes} /></main>
    </div>
  `;
}

function Entrar({ aoEntrar, expirou }) {
  const [usuario, setUsuario] = useState('');
  const [senha, setSenha] = useState('');
  const [erro, setErro] = useState('');
  const [ocupado, setOcupado] = useState(false);

  async function enviar(e) {
    e.preventDefault();
    setOcupado(true);
    setErro('');
    try {
      const r = await api('POST', '/entrar', { usuario, senha });
      aoEntrar(r.usuario);
    } catch (err) {
      setErro(err.message);
      setOcupado(false);
    }
  }

  return html`
    <div class="entrar">
      <form class="entrar-ficha" onSubmit=${enviar}>
        <h1>Rubricas</h1>
        <p class="suave">${expirou ? 'Sua sessão terminou. Entre de novo para continuar.' : 'Entre para montar rubricas e avaliar.'}</p>
        ${fila.estado().pendentes > 0 && html`<p class="aviso">Há marcações guardadas neste aparelho. Elas serão enviadas assim que você entrar.</p>`}
        <label class="campo">
          <span>Usuário</span>
          <input name="usuario" autocomplete="username" autocapitalize="none" autocorrect="off" required
            value=${usuario} onInput=${(e) => setUsuario(e.target.value)} />
        </label>
        <label class="campo">
          <span>Senha</span>
          <input name="senha" type="password" autocomplete="current-password" required
            value=${senha} onInput=${(e) => setSenha(e.target.value)} />
        </label>
        ${erro && html`<p class="erro" role="alert">${erro}</p>`}
        <button class="botao primario largo" disabled=${ocupado}>${ocupado ? 'Entrando…' : 'Entrar'}</button>
      </form>
    </div>
  `;
}

const SAIDA_PENDENTE = 'rubricas.sair';

function App() {
  // undefined = ainda verificando; null = sem sessão
  const [usuario, setUsuario] = useState(undefined);
  const [partes, setPartes] = useState(lerRota);
  const [semRede, setSemRede] = useState(false);
  const [expirou, setExpirou] = useState(false);
  const [erroFila, setErroFila] = useState('');

  function definir(nome) {
    fila.usarUsuario(nome);
    setUsuario(nome);
    if (nome) {
      localStorage.setItem('rubricas.usuario', nome);
      setExpirou(false);
    }
  }

  const verificar = useCallback(async () => {
    try {
      // Um "Sair" feito sem rede termina aqui, antes de qualquer outra coisa.
      if (localStorage.getItem(SAIDA_PENDENTE)) {
        await api('POST', '/sair');
        localStorage.removeItem(SAIDA_PENDENTE);
      }
      const r = await api('GET', '/sessao');
      setSemRede(false);
      definir(r.usuario);
    } catch (e) {
      if (e.semRede) {
        // Sem rede, quem já entrou neste aparelho segue com o que está guardado.
        setSemRede(true);
        definir(localStorage.getItem(SAIDA_PENDENTE) ? null : localStorage.getItem('rubricas.usuario') || null);
      } else {
        definir(null);
      }
    }
  }, []);

  useEffect(() => fila.ouvir((estado) => setErroFila(estado.erro)), []);

  useEffect(() => {
    verificar();
    const aoMudar = (e) => {
      if (guarda.ativa && !confirm('Há alterações não salvas. Sair desta tela sem salvar?')) {
        // Devolve o endereço de onde se tentou sair. Vale tanto para um link
        // quanto para o botão voltar do navegador, e não dispara outro evento.
        history.replaceState(null, '', e.oldURL);
        return;
      }
      guarda.ativa = false;
      setPartes(lerRota());
    };
    const aoExpirar = () => {
      setExpirou(true);
      setUsuario(null);
    };
    window.addEventListener('hashchange', aoMudar);
    window.addEventListener('sessao-expirada', aoExpirar);
    window.addEventListener('online', verificar);
    return () => {
      window.removeEventListener('hashchange', aoMudar);
      window.removeEventListener('sessao-expirada', aoExpirar);
      window.removeEventListener('online', verificar);
    };
  }, [verificar]);

  async function sair() {
    const n = fila.estado().pendentes;
    if (n && !confirm(`Há ${n} marcações que ainda não chegaram ao servidor. Elas ficam guardadas e serão enviadas quando você entrar de novo neste aparelho. Sair mesmo assim?`)) {
      return;
    }
    if (guarda.ativa && !confirm('Há alterações não salvas. Sair sem salvar?')) return;
    guarda.ativa = false;
    try {
      await api('POST', '/sair');
    } catch (e) {
      // Sem rede não dá para encerrar a sessão no servidor agora: fica anotado e é
      // a primeira coisa feita quando a conexão voltar.
      if (e.semRede) localStorage.setItem(SAIDA_PENDENTE, '1');
    }
    for (const k of Object.keys(localStorage)) {
      if (k.startsWith('rubricas.pacote.') || k === 'rubricas.usuario') localStorage.removeItem(k);
    }
    definir(null);
  }

  if (usuario === undefined) return html`<div class="carregando" aria-busy="true">Carregando…</div>`;
  if (usuario === null) {
    return html`
      <div class="raiz">
        ${semRede && html`<div class="faixa-aviso">Sem conexão com o servidor.</div>`}
        <${Entrar} expirou=${expirou} aoEntrar=${(u) => { localStorage.removeItem(SAIDA_PENDENTE); definir(u); fila.enviarAgora(); }} />
      </div>
    `;
  }
  // Marcações que o servidor recusou: o aviso fica até ser dispensado, em qualquer tela.
  const alerta = erroFila && html`
    <div class="alerta-fila" role="alert">
      <span>${erroFila}</span>
      <button type="button" class="botao pequeno" onClick=${() => fila.dispensarErro()}>Entendi</button>
    </div>
  `;
  if (partes[0] === 'avaliar' && partes[1]) {
    return html`
      <div class="raiz">
        <${Avaliar} key=${partes[1]} id=${partes[1]} alunoId=${partes[2] || null} />
        ${alerta}
      </div>
    `;
  }
  return html`
    <div class="raiz">
      ${alerta}
      ${semRede && html`<div class="faixa-aviso">Sem conexão com o servidor. Você pode continuar avaliações já abertas neste aparelho.</div>`}
      <${Moldura} usuario=${usuario} partes=${partes} aoSair=${sair} />
    </div>
  `;
}

render(html`<${App} />`, document.getElementById('app'));

if ('serviceWorker' in navigator) {
  // Quando uma versão nova assume, recarrega — menos no meio de uma avaliação ou edição.
  const jaControlada = !!navigator.serviceWorker.controller;
  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (jaControlada && !guarda.ativa && !location.hash.startsWith('#/avaliar')) location.reload();
  });
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('sw.js').catch(() => {});
  });
}
