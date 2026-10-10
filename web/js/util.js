// Pequenos utilitários compartilhados pelas telas.

const fmt = new Intl.NumberFormat('pt-BR', { maximumFractionDigits: 2, useGrouping: false });

/** Duas casas, meio para cima, com folga para empates que o ponto flutuante deixa
    um fio abaixo. Espelha `arredondar` em src/api.rs — mude os dois juntos. */
export function arredondar(v) {
  return (Math.sign(v) * Math.floor(Math.abs(v) * 100 + 0.5 + 1e-7)) / 100 || 0;
}

/** Para mostrar: 7.5 → "7,5"; 10 → "10"; 1.025 → "1,03". */
export function num(v) {
  return fmt.format(Number.isFinite(v) ? arredondar(v) : 0);
}

/** Para campos de edição: o número inteiro, sem arredondar (0.125 → "0,125"). */
export function campo(v) {
  return Number.isFinite(v) ? String(v).replace('.', ',') : '';
}

/** Lê "7,5" ou "7.5"; devolve NaN se não for um número decimal simples. */
export function lerNum(texto) {
  if (typeof texto === 'number') return texto;
  const t = String(texto).trim().replace(',', '.');
  return /^-?(\d+\.?\d*|\.\d+)$/.test(t) ? Number(t) : NaN;
}

/** "1 ponto", "2,5 pontos". */
export function pts(n) {
  return `${num(n)} ${n === 1 ? 'ponto' : 'pontos'}`;
}

/** Telas com edição não salva ligam esta guarda; a navegação pergunta antes de sair. */
export const guarda = { ativa: false };

export function cls(...partes) {
  return partes.filter(Boolean).join(' ');
}

let contador = 0;
/** Chave local para listas editáveis; nunca vai para o servidor. */
export function chave() {
  contador += 1;
  return `k${Date.now().toString(36)}${contador}`;
}

export function semAcento(texto) {
  return String(texto).normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
}

export function plural(n, um, varios) {
  return `${n} ${n === 1 ? um : varios}`;
}

export function vibrar(ms = 12) {
  try {
    navigator.vibrate?.(ms);
  } catch {
    /* sem vibração neste aparelho */
  }
}

export function ir(caminho, substituir = false) {
  const alvo = `#${caminho}`;
  if (substituir) location.replace(alvo);
  else location.hash = alvo;
}

/** Monta um caminho de rota escapando cada parte. */
export function rota(...partes) {
  return `/${partes.map(encodeURIComponent).join('/')}`;
}
