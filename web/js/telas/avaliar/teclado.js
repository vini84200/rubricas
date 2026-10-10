// Atalhos de teclado da tela de avaliar, por modo: no resumo, num passo de roteiro
// ou num critério. Cada ação devolve `false` quando a tecla não vale ali.
import { useEffect } from '../../../vendor/preact-htm.js';
import { ehRoteiro, marcacaoRapida, opcoesDe } from '../../nota.js';

const TROCAR_ALUNO = {
  '[': (a) => a.vizinho(-1),
  PageUp: (a) => a.vizinho(-1),
  ']': (a) => a.vizinho(1),
  PageDown: (a) => a.vizinho(1),
};

const voltar = (a) => a.irPara(a.idx - 1);
const seguirPasso = (a) => a.avancar('direita');
const marcarRapido = (positivo, dir) => (a) => a.marcar(marcacaoRapida(a.item.criterio, positivo), dir);

const MODOS = {
  resumo: {
    z: (a) => a.desfazer(), Enter: (a) => a.seguir(), ArrowDown: voltar, Backspace: voltar,
    g: (a) => (a.sugestaoEfeito ? a.sugestaoEfeito() : false),
  },
  roteiro: {
    ArrowRight: seguirPasso, d: seguirPasso, Enter: seguirPasso, ' ': seguirPasso, s: seguirPasso,
    ArrowLeft: (a) => a.avancar('esquerda'), a: (a) => a.avancar('esquerda'),
    ArrowDown: voltar, Backspace: voltar,
    z: (a) => a.desfazer(),
  },
  criterio: {
    ArrowRight: marcarRapido(true, 'direita'), d: marcarRapido(true, 'direita'),
    ArrowLeft: marcarRapido(false, 'esquerda'), a: marcarRapido(false, 'esquerda'),
    ArrowUp: (a) => a.abrirOpcoes('tecla'),
    ArrowDown: voltar, Backspace: voltar,
    s: (a) => a.irPara(a.idx + 1),
    z: (a) => a.desfazer(),
    g: (a) => (a.sugestao ? a.sugestao.aceitar() : false),
    c: (a) => a.comentar(),
  },
};

/** 1–9 escolhem a opção com aquela tecla (a mesma que aparece ao lado dela). */
function escolher(a, n) {
  const o = opcoesDe(a.item.criterio).find((x) => x.tecla === n);
  if (!o) return false;
  const dir = o.marcacao.tipo === 'binaria' ? (o.marcacao.atende ? 'direita' : 'esquerda') : 'cima';
  return a.marcar(o.marcacao, dir);
}

function modo(a) {
  if (a.noResumo) return 'resumo';
  return ehRoteiro(a.item.criterio) ? 'roteiro' : 'criterio';
}

/**
 * Liga o teclado uma vez; `acoes.current` traz sempre a versão atual das ações e
 * do estado (`item`, `idx`, `noResumo`, `folhaAberta`…). `aoEscape` fecha o que
 * estiver aberto.
 */
export function useTeclado(acoes, aoEscape) {
  useEffect(() => {
    function tecla(e) {
      const a = acoes.current;
      if (!a.marcar || e.repeat) return;
      const campoAtivo = e.target.closest?.('input, textarea, select');
      if (e.key === 'Escape') {
        if (campoAtivo) campoAtivo.blur();
        aoEscape();
        return;
      }
      if (campoAtivo || e.ctrlKey || e.metaKey || e.altKey) return;
      const digito = /^[1-9]$/.test(e.key);
      if (a.folhaAberta && !digito) return;
      const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      const m = modo(a);
      const acao = TROCAR_ALUNO[k] || MODOS[m][k] || (digito && m === 'criterio' && ((x) => escolher(x, Number(k))));
      if (!acao || acao(a) === false) return;
      e.preventDefault();
    }
    window.addEventListener('keydown', tecla);
    return () => window.removeEventListener('keydown', tecla);
  }, []);
}
