// Deslize do cartão: o elemento segue o dedo, mostra para onde vai e, passado o
// limiar (ou com impulso), chama a ação daquela direção.
import { useRef } from '../../../vendor/preact-htm.js';

const LIMIAR_Y = 64;
const poucoMovimento = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const interativo = (alvo) => alvo.closest('button, a, textarea, input');

function limiarX(el) {
  return Math.min(110, el.offsetWidth * 0.28);
}

/**
 * `aoDireita`, `aoEsquerda`, `aoCima` e `aoBaixo` respondem aos deslizes; `aoToque`,
 * a um toque sem arrasto. Sem `aoCima`, deslizar para cima volta ao lugar.
 * Devolve `ref` e `eventos` para o elemento e `voar(dir, carimbo)`, que solta uma
 * cópia dele saindo pela direção dada.
 */
export function useDeslize({ aoDireita, aoEsquerda, aoCima, aoBaixo, aoToque }) {
  const ref = useRef(null);
  const gesto = useRef(null);
  const toque = useRef(false);

  function repousar() {
    const el = ref.current;
    if (!el) return;
    el.classList.remove('arrastando');
    el.style.transform = '';
    el.style.setProperty('--forca', '0');
    delete el.dataset.dir;
    delete el.dataset.pronto;
  }

  // O cartão marcado sai voando como uma cópia solta, enquanto o próximo já está
  // na tela: não há espera entre uma marcação e a seguinte.
  function voar(dir, carimbo) {
    const el = ref.current;
    if (!el || poucoMovimento()) return;
    const atual = el.style.transform;
    el.style.transform = 'none';
    const r = el.getBoundingClientRect();
    const copia = el.cloneNode(true);
    copia.classList.remove('arrastando', 'entra-frente', 'entra-tras');
    copia.classList.add('fantasma');
    copia.dataset.dir = dir;
    copia.dataset.pronto = '';
    copia.style.cssText = `left:${r.left}px;top:${r.top}px;width:${r.width}px;height:${r.height}px;transform:${atual || 'none'};--forca:1`;
    if (carimbo) {
      const livre = copia.querySelector('.carimbo.livre');
      livre.textContent = carimbo.texto;
      livre.dataset.tom = carimbo.tom;
    }
    document.body.appendChild(copia);
    const destino = { direita: 'translate(135%, -8%) rotate(18deg)', esquerda: 'translate(-135%, -8%) rotate(-18deg)', cima: 'translateY(-70%) scale(.92)' }[dir];
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        copia.classList.add('indo');
        copia.style.transform = destino;
        copia.style.opacity = '0';
      });
    });
    setTimeout(() => copia.remove(), 320);
  }

  function onPointerDown(e) {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    if (interativo(e.target)) return;
    toque.current = false;
    gesto.current = { id: e.pointerId, x: e.clientX, y: e.clientY, t: e.timeStamp, eixo: null, dx: 0, dy: 0, rastro: [] };
    try {
      ref.current.setPointerCapture(e.pointerId);
    } catch {
      /* ponteiro já solto */
    }
  }

  function onPointerMove(e) {
    const g = gesto.current;
    if (!g || e.pointerId !== g.id) return;
    g.dx = e.clientX - g.x;
    g.dy = e.clientY - g.y;
    g.rastro.push({ t: e.timeStamp, dx: g.dx, dy: g.dy });
    if (g.rastro.length > 12) g.rastro.shift();
    const el = ref.current;
    if (!g.eixo) {
      if (Math.hypot(g.dx, g.dy) < 8) return;
      g.eixo = Math.abs(g.dx) >= Math.abs(g.dy) ? 'x' : 'y';
      el.classList.add('arrastando');
    }
    let forca;
    if (g.eixo === 'x') {
      el.style.transform = `translate(${g.dx}px, ${-Math.abs(g.dx) * 0.05}px) rotate(${g.dx / 22}deg)`;
      el.dataset.dir = g.dx > 0 ? 'direita' : 'esquerda';
      forca = Math.abs(g.dx) / limiarX(el);
    } else {
      el.style.transform = `translateY(${g.dy * 0.5}px)`;
      el.dataset.dir = g.dy < 0 ? 'cima' : 'baixo';
      forca = Math.abs(g.dy) / LIMIAR_Y;
    }
    el.style.setProperty('--forca', Math.min(1, forca).toFixed(3));
    if (forca >= 1) el.dataset.pronto = '';
    else delete el.dataset.pronto;
  }

  function onPointerUp(e) {
    const g = gesto.current;
    if (!g || e.pointerId !== g.id) return;
    gesto.current = null;
    const el = ref.current;
    if (!g.eixo) {
      // Sem arrasto é um toque: quem age é o clique que vem a seguir, para que
      // ele não caia no fundo de uma folha recém-aberta e a feche.
      repousar();
      toque.current = e.timeStamp - g.t < 500;
      return;
    }
    // Velocidade só do fim do gesto: quem pousa o dedo, pensa e então desliza
    // rápido também conta como impulso.
    const antes = g.rastro.find((p) => e.timeStamp - p.t <= 120) || g.rastro[g.rastro.length - 1];
    const dt = Math.max(16, e.timeStamp - antes.t);
    const impulso = (d, d0) => Math.abs(d) > 48 && (d - d0) * Math.sign(d) / dt > 0.4;
    if (g.eixo === 'x') {
      if (Math.abs(g.dx) >= limiarX(el) || impulso(g.dx, antes.dx)) {
        (g.dx > 0 ? aoDireita : aoEsquerda)();
        return;
      }
    } else if ((Math.abs(g.dy) >= LIMIAR_Y || impulso(g.dy, antes.dy)) && (g.dy > 0 || aoCima)) {
      repousar();
      (g.dy < 0 ? aoCima : aoBaixo)();
      return;
    }
    repousar();
  }

  function onPointerCancel() {
    gesto.current = null;
    repousar();
  }

  function onClick(e) {
    if (toque.current && !interativo(e.target)) aoToque();
    toque.current = false;
  }

  return { ref, voar, eventos: { onPointerDown, onPointerMove, onPointerUp, onPointerCancel, onClick } };
}
