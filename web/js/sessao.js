// Quem está dentro e o que pode fazer. Vem de GET /sessao; fica guardado para
// funcionar sem rede. Cada item das listas traz as próprias `acoes`.

const CHAVE = 'rubricas.sessao';
const VAZIA = { usuario: null, papel: null, acoes: [], papeis: {}, papeis_globais: [] };

let atual = ler();

function ler() {
  try {
    return { ...VAZIA, ...JSON.parse(localStorage.getItem(CHAVE) || '{}') };
  } catch {
    return { ...VAZIA };
  }
}

export const sessao = {
  get: () => atual,
  definir(s) {
    atual = { ...VAZIA, ...(s || {}) };
    try {
      if (s) localStorage.setItem(CHAVE, JSON.stringify(atual));
      else localStorage.removeItem(CHAVE);
    } catch {
      /* sem armazenamento: vale só nesta aba */
    }
  },
  /** Ação sobre o sistema: criar, gerir_usuarios, executar. */
  pode: (acao) => atual.acoes.includes(acao),
};

/** Ação sobre um item (avaliação, rubrica ou turma) que veio do servidor com `acoes`. */
export function pode(item, acao) {
  return !!item?.acoes?.includes(acao);
}

const DESCRICAO = {
  admin: 'Admin: vê e faz tudo, cuida dos usuários',
  professor: 'Professor: cria avaliações, rubricas e turmas',
  monitor: 'Monitor: só o que compartilharem com ele',
  executor: 'Executor: máquina que roda as checagens',
  leitor: 'Leitor: vê',
  avaliador: 'Avaliador: vê e corrige',
  editor: 'Editor: vê, corrige e altera',
  dono: 'Dono: tudo, inclusive compartilhar e apagar',
};

/** Rótulo de um papel; papéis de uma política própria aparecem pelo nome. */
export function descreverPapel(papel) {
  return DESCRICAO[papel] || papel;
}
