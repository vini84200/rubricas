// Acesso ao servidor e fila de correções por enviar.
import { CHAVES, guardar, recuperar, apagar } from './armazem.js';
import { MAPAS, aplicarLote, chaveItem, descontar, separar, tamanho } from './lote.js';
import { rota } from './util.js';

export class ErroApi extends Error {
  constructor(status, mensagem) {
    super(mensagem);
    this.status = status;
  }
  /** Falha de rede: o pedido nem chegou ao servidor. */
  get semRede() {
    return this.status === 0;
  }
}

/** Um `Blob` (arquivo escolhido) vai como está; o resto, como JSON. */
export async function api(metodo, caminho, corpo) {
  const bruto = corpo instanceof Blob;
  let resposta;
  try {
    resposta = await fetch(`api${caminho}`, {
      method: metodo,
      headers: corpo === undefined || bruto ? {} : { 'content-type': 'application/json' },
      body: corpo === undefined ? undefined : bruto ? corpo : JSON.stringify(corpo),
      credentials: 'same-origin',
    });
  } catch {
    throw new ErroApi(0, 'Sem conexão com o servidor');
  }
  let dados = null;
  try {
    dados = await resposta.json();
  } catch {
    /* resposta sem corpo JSON */
  }
  if (!resposta.ok) {
    if (resposta.status === 401 && caminho !== '/entrar') {
      window.dispatchEvent(new Event('sessao-expirada'));
    }
    throw new ErroApi(resposta.status, dados?.erro || `O servidor respondeu ${resposta.status}`);
  }
  return dados;
}

// ---------------------------------------------------------------------------
// Fila de correções
//
// Cada marcação entra aqui antes de ir ao servidor. A fila fica no localStorage,
// separada por usuário, então fechar a aba ou perder o sinal no meio da aula não
// perde nada: o envio recomeça quando a conexão (ou a sessão) volta.
//
// Formato: { "<avaliacao>\n<aluno>": lote } — o lote está descrito em lote.js.
// ---------------------------------------------------------------------------

let dono = null;
let pendentes = {};
let enviando = false;
let temporizador = null;
let espera = 1000;
let ultimoErro = '';
let falhando = false;
let recusas = 0;
const ouvintes = new Set();
const aoConfirmar = new Set();

const chaveFila = () => CHAVES.fila(dono);

function contar() {
  return Object.values(pendentes).reduce((n, p) => n + tamanho(p), 0);
}

function avisar() {
  const estado = fila.estado();
  ouvintes.forEach((f) => f(estado));
}

function agendar(ms) {
  clearTimeout(temporizador);
  temporizador = setTimeout(enviar, ms);
}

async function enviar() {
  if (enviando || !dono) return;
  const chaves = Object.keys(pendentes);
  if (!chaves.length) return;
  enviando = true;
  avisar();
  const deQuem = dono;
  let tentarDeNovo = false;
  for (const k of chaves) {
    const lote = pendentes[k];
    if (!lote) continue;
    const [avaliacao, aluno] = separar(k);
    const corpo = {
      respostas: Object.entries(lote.respostas || {}).map(([c, resposta]) => {
        const [rubrica, criterio] = separar(c);
        return { rubrica, criterio, resposta };
      }),
    };
    if (lote.comentario_geral !== undefined) corpo.comentario_geral = lote.comentario_geral;
    corpo.efeitos = Object.entries(lote.efeitos || {}).map(([efeito, aplicado]) => ({ efeito, aplicado }));
    corpo.dispensadas = Object.entries(lote.dispensadas || {}).map(([efeito, dispensa]) => ({ efeito, dispensa }));
    let resultado = null;
    let erro = null;
    try {
      resultado = await api('PATCH', rota('avaliacoes', avaliacao, 'correcoes', aluno), corpo);
    } catch (e) {
      erro = e;
    }
    if (dono !== deQuem) break; // trocou de usuário durante o envio
    if (erro && erro.status !== 403 && erro.status !== 404 && erro.status !== 422) {
      // Sem rede, sessão vencida ou servidor com problema: o lote fica na fila.
      falhando = true;
      if (!erro.semRede && erro.status !== 401) ultimoErro = erro.message;
      tentarDeNovo = erro.status !== 401; // depois do login a fila é reenviada
      break;
    }
    // O servidor respondeu de vez. Sai da fila só o que foi enviado; marcações
    // feitas durante o envio continuam nela.
    const resto = descontar(pendentes[k], lote);
    if (resto) pendentes[k] = resto;
    else delete pendentes[k];
    falhando = false;
    espera = 1000;
    if (erro) {
      // 404: o aluno saiu da turma ou a avaliação foi apagada; 403: tiraram o seu acesso.
      ultimoErro = `As marcações de ${aluno} em “${avaliacao}” não foram salvas: ${erro.message}`;
      recusas += 1;
    } else {
      const n = resultado.recusadas?.length || 0;
      if (n) {
        ultimoErro = `${n === 1 ? 'Uma marcação' : `${n} marcações`} de ${aluno} em “${avaliacao}” ${n === 1 ? 'não foi salva' : 'não foram salvas'}: ${resultado.recusadas[0].motivo}`;
        recusas += n;
      }
      aoConfirmar.forEach((f) => f({ avaliacao, aluno, correcao: resultado.correcao, pendente: pendentes[k] || null }));
    }
  }
  if (dono === deQuem) guardar(chaveFila(), pendentes);
  enviando = false;
  avisar();
  if (tentarDeNovo) {
    agendar(espera);
    espera = Math.min(espera * 2, 30000);
  } else if (!falhando && Object.keys(pendentes).length) {
    agendar(50);
  }
}

export const fila = {
  /** Define de quem é a fila. Sem usuário, ela fica parada. */
  usarUsuario(nome) {
    if (nome === dono) return;
    clearTimeout(temporizador);
    dono = nome || null;
    pendentes = dono ? recuperar(chaveFila(), {}) || {} : {};
    ultimoErro = '';
    falhando = false;
    avisar();
    if (contar()) agendar(300);
  },

  /** Registra alterações de um aluno. `respostas` usa a chave "<origem>\n<criterio>";
      `efeitos` e `dispensadas`, o id do tipo de efeito. */
  adicionar(avaliacao, aluno, alteracoes) {
    if (!dono) return;
    const k = chaveItem(avaliacao, aluno);
    const lote = structuredClone(pendentes[k] || { respostas: {} });
    Object.assign(lote.respostas, alteracoes.respostas || {});
    if (alteracoes.comentario_geral !== undefined) lote.comentario_geral = alteracoes.comentario_geral;
    for (const campo of MAPAS) {
      if (alteracoes[campo]) lote[campo] = { ...lote[campo], ...alteracoes[campo] };
    }
    pendentes = { ...pendentes, [k]: lote };
    guardar(chaveFila(), pendentes);
    avisar();
    agendar(350);
  },

  /** Alterações ainda não confirmadas de uma avaliação: { aluno: lote }. */
  de(avaliacao) {
    const saida = {};
    for (const [k, lote] of Object.entries(pendentes)) {
      const [av, aluno] = separar(k);
      if (av === avaliacao) saida[aluno] = lote;
    }
    return saida;
  },

  /** `recusas` só cresce: quem observa recarrega os dados quando ele muda. */
  estado() {
    return { pendentes: contar(), enviando, falhando, recusas, online: navigator.onLine !== false, erro: ultimoErro };
  },

  ouvir(f) {
    ouvintes.add(f);
    return () => ouvintes.delete(f);
  },

  /** Chamado a cada envio aceito, com a correção como ficou no servidor. */
  aoConfirmar(f) {
    aoConfirmar.add(f);
    return () => aoConfirmar.delete(f);
  },

  dispensarErro() {
    ultimoErro = '';
    avisar();
  },

  enviarAgora() {
    espera = 1000;
    agendar(0);
  },

  chave: chaveItem,
};

window.addEventListener('online', () => fila.enviarAgora());
window.addEventListener('offline', avisar);
window.addEventListener('online', avisar);
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible' && contar()) fila.enviarAgora();
});

/** Carrega o pacote de uma avaliação; sem rede, usa a última cópia guardada. */
export async function carregarPacote(id) {
  const chavePacote = CHAVES.pacote(id);
  let pacote;
  let offline = false;
  try {
    pacote = await api('GET', rota('avaliacoes', id));
    guardar(chavePacote, pacote);
  } catch (e) {
    // Sem acesso (revogado) ou apagada: a cópia deste aparelho também some.
    if (e.status === 403 || e.status === 404) apagar(chavePacote);
    if (!e.semRede) throw e;
    pacote = recuperar(chavePacote);
    if (!pacote) throw e;
    offline = true;
  }
  const correcoes = { ...pacote.correcoes };
  for (const [aluno, lote] of Object.entries(fila.de(id))) {
    correcoes[aluno] = aplicarLote(correcoes[aluno], lote);
  }
  return { ...pacote, correcoes, offline };
}

/** Atualiza a cópia offline com o estado local, para um recarregamento sem rede. */
export function guardarCorrecoes(id, correcoes) {
  const chavePacote = CHAVES.pacote(id);
  const pacote = recuperar(chavePacote);
  if (pacote) guardar(chavePacote, { ...pacote, correcoes });
}
