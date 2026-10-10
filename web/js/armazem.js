// O que fica guardado neste aparelho (localStorage). Toda chave tem nome aqui, e
// todo acesso tolera armazenamento cheio ou bloqueado: aí a aplicação segue só em memória.

export const CHAVES = {
  usuario: 'rubricas.usuario',
  sessao: 'rubricas.sessao',
  sairPendente: 'rubricas.sair',
  pacote: (avaliacao) => `rubricas.pacote.${avaliacao}`,
  fila: (dono) => `rubricas.fila.v1:${dono}`,
};

function tentar(f, padrao) {
  try {
    return f();
  } catch {
    return padrao;
  }
}

/** Texto cru, ou null. */
export function ler(nome) {
  return tentar(() => localStorage.getItem(nome), null);
}

export function escrever(nome, texto) {
  tentar(() => localStorage.setItem(nome, texto));
}

export function apagar(nome) {
  tentar(() => localStorage.removeItem(nome));
}

export function guardar(nome, valor) {
  escrever(nome, JSON.stringify(valor));
}

export function recuperar(nome, padrao = null) {
  const bruto = ler(nome);
  return bruto === null ? padrao : tentar(() => JSON.parse(bruto), padrao);
}

/** Ao sair: some o que é de quem estava dentro. A fila fica, separada por usuário. */
export function limparSessao() {
  const prefixoPacote = CHAVES.pacote('');
  for (const k of tentar(() => Object.keys(localStorage), [])) {
    if (k.startsWith(prefixoPacote) || k === CHAVES.usuario || k === CHAVES.sessao) apagar(k);
  }
}
