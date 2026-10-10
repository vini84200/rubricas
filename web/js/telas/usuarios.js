// Usuários (para quem os gere) e a própria conta (trocar a senha).
import { html, useState } from '../../vendor/preact-htm.js';
import { api } from '../api.js';
import { sessao, descreverPapel } from '../sessao.js';
import { rota } from '../util.js';
import { useDados, useAcao, Estado, Titulo } from './comum.js';

function NovoUsuario({ aoCriar }) {
  const papeis = sessao.get().papeis_globais;
  const [nome, setNome] = useState('');
  const [senha, setSenha] = useState('');
  const [papel, setPapel] = useState(papeis.includes('monitor') ? 'monitor' : papeis[0]);
  const { rodar, ocupado, erro } = useAcao();

  function enviar(e) {
    e.preventDefault();
    rodar(async () => {
      await api('POST', '/usuarios', { nome: nome.trim(), senha, papel });
      setNome('');
      setSenha('');
      aoCriar();
    });
  }

  return html`
    <form class="formulario" onSubmit=${enviar}>
      <h2>Novo usuário</h2>
      <div class="par">
        <label class="campo">
          <span>Nome</span>
          <input autocomplete="off" autocapitalize="none" autocorrect="off" required pattern="[A-Za-z0-9._\\-]{1,40}"
            title="Letras sem acento, números, ponto, hífen ou sublinhado" value=${nome} onInput=${(e) => setNome(e.target.value)} />
        </label>
        <label class="campo">
          <span>Senha inicial</span>
          <input type="password" autocomplete="new-password" required minlength="8" value=${senha} onInput=${(e) => setSenha(e.target.value)} />
        </label>
      </div>
      <label class="campo">
        <span>Papel</span>
        <select value=${papel} onChange=${(e) => setPapel(e.target.value)}>
          ${papeis.map((p) => html`<option value=${p}>${descreverPapel(p)}</option>`)}
        </select>
      </label>
      ${erro && html`<p class="erro" role="alert">${erro}</p>`}
      <div class="acoes"><button class="botao primario" disabled=${ocupado}>Criar usuário</button></div>
    </form>
  `;
}

function LinhaUsuario({ u, aoMudar }) {
  const { rodar, erro } = useAcao();
  const eu = sessao.get().usuario === u.nome;

  const fazer = (metodo, corpo) => rodar(async () => {
    await api(metodo, rota('usuarios', u.nome), corpo);
    aoMudar();
  });

  function novaSenha() {
    const senha = prompt(`Nova senha para ${u.nome} (pelo menos 8 caracteres). As sessões abertas dele terminam.`);
    if (senha) fazer('PATCH', { senha });
  }

  function remover() {
    if (confirm(`Remover ${u.nome}? Os acessos dele também somem. As correções que ele fez continuam.`)) fazer('DELETE');
  }

  return html`
    <li class="usuario">
      <span class="usuario-nome">${u.nome}${eu ? ' (você)' : ''}${u.tem_token ? html` <small class="suave">com token de API</small>` : ''}</span>
      <select aria-label=${`Papel de ${u.nome}`} onChange=${(e) => fazer('PATCH', { papel: e.target.value })}>
        ${sessao.get().papeis_globais.map((p) => html`<option value=${p} selected=${p === u.papel}>${descreverPapel(p)}</option>`)}
      </select>
      <span class="acoes">
        <button type="button" class="botao pequeno" onClick=${novaSenha}>Nova senha</button>
        ${!eu && html`<button type="button" class="botao pequeno perigo" onClick=${remover}>Remover</button>`}
      </span>
      ${erro && html`<p class="erro" role="alert">${erro}</p>`}
    </li>
  `;
}

export function Usuarios() {
  const { dados, recarregar, ...estado } = useDados('/usuarios');
  return html`
    <div class="tela">
      <${Titulo} titulo="Usuários" />
      <p class="suave">
        O papel diz o que cada um pode fazer em geral. Em cada avaliação, rubrica ou turma, quem é dono pode
        compartilhar com outras pessoas (em “Compartilhar”, no fim da página dela).
      </p>
      <${Estado} ...${estado} recarregar=${recarregar} />
      ${dados && html`<ul class="lista usuarios">${dados.map((u) => html`<${LinhaUsuario} key=${u.nome} u=${u} aoMudar=${recarregar} />`)}</ul>`}
      <${NovoUsuario} aoCriar=${recarregar} />
    </div>
  `;
}

export function Conta() {
  const s = sessao.get();
  const [atual, setAtual] = useState('');
  const [nova, setNova] = useState('');
  const [repetida, setRepetida] = useState('');
  const { rodar, ocupado, erro, setErro } = useAcao();

  function enviar(e) {
    e.preventDefault();
    if (nova !== repetida) {
      setErro('As senhas novas não conferem.');
      return;
    }
    rodar(async () => {
      await api('PUT', '/senha', { atual, nova });
      // Senha nova encerra todas as sessões, inclusive esta.
      window.dispatchEvent(new Event('sessao-expirada'));
    });
  }

  return html`
    <div class="tela">
      <${Titulo} titulo="Sua conta" />
      <p>Você entrou como <strong>${s.usuario}</strong>. ${descreverPapel(s.papel)}.</p>
      <form class="formulario" onSubmit=${enviar}>
        <h2>Trocar a senha</h2>
        <label class="campo">
          <span>Senha atual</span>
          <input type="password" autocomplete="current-password" required value=${atual} onInput=${(e) => setAtual(e.target.value)} />
        </label>
        <div class="par">
          <label class="campo">
            <span>Senha nova</span>
            <input type="password" autocomplete="new-password" required minlength="8" value=${nova} onInput=${(e) => setNova(e.target.value)} />
          </label>
          <label class="campo">
            <span>Repita a senha nova</span>
            <input type="password" autocomplete="new-password" required minlength="8" value=${repetida} onInput=${(e) => setRepetida(e.target.value)} />
          </label>
        </div>
        <p class="suave">Depois de trocar, você entra de novo em todos os aparelhos.</p>
        ${erro && html`<p class="erro" role="alert">${erro}</p>`}
        <div class="acoes"><button class="botao primario" disabled=${ocupado}>Trocar a senha</button></div>
      </form>
    </div>
  `;
}
