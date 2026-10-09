// Tela de avaliação: um aluno, um critério por cartão.
//
// Gestos no cartão:        Teclado:
//   → direita  = máximo      →  ou D
//   ← esquerda = mínimo      ←  ou A
//   ↑ cima     = opções      ↑  (níveis intermediários, nota, comentário)
//   ↓ baixo    = voltar      ↓  ou Backspace
//                            1–9 escolhe o nível, S pula, C comenta, Z desfaz,
//                            Enter próximo aluno, [ e ] trocam de aluno,
//                            G aceita a sugestão das checagens automáticas
//
// Passos de roteiro ganham um cartão próprio, sem marcação: deslizar para os lados,
// →, ←, Enter ou espaço seguem para o próximo item.
import { html, useState, useEffect, useRef, useMemo } from '../../vendor/preact-htm.js';
import { api, carregarPacote, fila, guardarCorrecoes, aplicarLote } from '../api.js';
import {
  calcular, itens as listarItens, marcacaoRapida, niveisOrdenados, rotulo, tom, pontos, maximo,
  temIntermediarios, passoDe, DESEMPENHOS, ehRoteiro, pesoDe,
} from '../nota.js';
import { Markdown } from '../markdown.js';
import { PainelCodigo, avisosEad } from '../codigo.js';
import { ir, rota, num, campo, lerNum, cls, vibrar, semAcento, plural } from '../util.js';
import { Estado } from './comum.js';

const LIMIAR_Y = 64;
const poucoMovimento = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

function useMedia(consulta) {
  const [vale, setVale] = useState(() => window.matchMedia(consulta).matches);
  useEffect(() => {
    const m = window.matchMedia(consulta);
    const f = () => setVale(m.matches);
    m.addEventListener('change', f);
    return () => m.removeEventListener('change', f);
  }, [consulta]);
  return vale;
}

function respostaDe(correcao, item) {
  return correcao?.respostas?.[item.origem]?.[item.criterio.id] || null;
}

function semResposta(itens, correcao, i) {
  return pontos(itens[i].criterio, respostaDe(correcao, itens[i])) === null;
}

/** Onde parar: critério sem marcação, ou passo de roteiro que leva a um. Passos
    de critérios já marcados ficam para trás. */
function primeiroSemResposta(itens, correcao, de = 0) {
  for (let i = de; i < itens.length; i += 1) {
    let alvo = i;
    while (alvo < itens.length && ehRoteiro(itens[alvo].criterio)) alvo += 1;
    if (alvo < itens.length && semResposta(itens, correcao, alvo)) return i;
  }
  return itens.length;
}

/** Passos de roteiro logo antes do item `i`. */
function passosAntes(itens, i) {
  let j = i;
  while (j > 0 && ehRoteiro(itens[j - 1].criterio)) j -= 1;
  return itens.slice(j, i).map((it) => it.criterio);
}

function mesmaMarcacao(a, b) {
  return !!a && !!b && ['tipo', 'nivel', 'atende', 'nota', 'desempenho'].every((k) => a[k] === b[k]);
}

/** Rótulo e pontos do que o deslize lateral marca. */
function rapido(criterio, positivo) {
  const m = marcacaoRapida(criterio, positivo);
  return { texto: rotulo(criterio, m), pontos: pontos(criterio, m), marcacao: m };
}

// ---------------------------------------------------------------------------
// Cartão do critério
// ---------------------------------------------------------------------------

const SEGUIR = { texto: 'Seguir' };

function Ficha({ item, resposta, posicao, mostrarRubrica, entrada, comentario, desfazer, passos, sugestao, aoPositivo, aoNegativo, aoCima, aoBaixo, controle }) {
  const ref = useRef(null);
  const gesto = useRef(null);
  const toque = useRef(false);
  const c = item.criterio;
  const passo = ehRoteiro(c);
  const pos = passo ? SEGUIR : rapido(c, true);
  const neg = passo ? SEGUIR : rapido(c, false);
  const intermediarios = !passo && temIntermediarios(c);

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
  controle.current = { voar };

  function aoDescer(e) {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    if (e.target.closest('button, a, textarea, input')) return;
    toque.current = false;
    gesto.current = { id: e.pointerId, x: e.clientX, y: e.clientY, t: e.timeStamp, eixo: null, dx: 0, dy: 0, rastro: [] };
    try {
      ref.current.setPointerCapture(e.pointerId);
    } catch {
      /* ponteiro já solto */
    }
  }

  function aoMover(e) {
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

  function aoSoltar(e) {
    const g = gesto.current;
    if (!g || e.pointerId !== g.id) return;
    gesto.current = null;
    const el = ref.current;
    if (!g.eixo) {
      // Sem arrasto é um toque: quem abre as opções é o clique que vem a seguir,
      // para que ele não caia no fundo da folha recém-aberta e a feche.
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
        (g.dx > 0 ? aoPositivo : aoNegativo)();
        return;
      }
    } else if ((Math.abs(g.dy) >= LIMIAR_Y || impulso(g.dy, antes.dy)) && !(passo && g.dy < 0)) {
      repousar();
      (g.dy < 0 ? aoCima : aoBaixo)();
      return;
    }
    repousar();
  }

  function aoCancelar() {
    gesto.current = null;
    repousar();
  }

  function aoClicar(e) {
    // Tocar segue no passo de roteiro; no critério, abre as opções.
    if (toque.current && !e.target.closest('button, a, textarea, input')) (passo ? aoPositivo : () => aoCima('toque'))();
    toque.current = false;
  }

  const marcado = resposta ? tom(c, resposta) : '';
  if (passo) {
    return html`
      <article class=${cls('ficha', 'passo', `entra-${entrada}`)} ref=${ref} aria-roledescription="passo de roteiro"
        onPointerDown=${aoDescer} onPointerMove=${aoMover} onPointerUp=${aoSoltar} onPointerCancel=${aoCancelar} onClick=${aoClicar}>
        <span class="carimbo sim" aria-hidden="true">Seguir</span>
        <span class="carimbo nao" aria-hidden="true">Seguir</span>
        <span class="carimbo livre" aria-hidden="true"></span>
        <p class="ficha-onde">
          ${desfazer
            ? html`<button type="button" class="ficha-desfazer" onClick=${desfazer.acao}>Desfazer “${desfazer.texto}”</button>`
            : html`<span class="ficha-rotulo-passo">Roteiro</span>`}
          <span>${posicao}</span>
        </p>
        <h2 class="ficha-nome">${c.nome}</h2>
        ${c.descricao && html`<${Markdown} classe="ficha-texto" texto=${c.descricao} />`}
        <div class="ficha-dicas" aria-hidden="true">
          <span class="dica cima">toque ou deslize para seguir</span>
        </div>
      </article>
    `;
  }
  return html`
    <article class=${cls('ficha', `entra-${entrada}`)} ref=${ref} aria-roledescription="cartão de critério"
      onPointerDown=${aoDescer} onPointerMove=${aoMover} onPointerUp=${aoSoltar} onPointerCancel=${aoCancelar} onClick=${aoClicar}>
      <span class="carimbo sim" aria-hidden="true">${pos.texto}</span>
      <span class="carimbo nao" aria-hidden="true">${neg.texto}</span>
      <span class="carimbo livre" aria-hidden="true"></span>
      <p class="ficha-onde">
        ${desfazer
          ? html`<button type="button" class="ficha-desfazer" onClick=${desfazer.acao}>Desfazer “${desfazer.texto}”</button>`
          : mostrarRubrica && html`<span class="ficha-rubrica">${item.rubrica}</span>`}
        <span>${posicao}</span>
      </p>
      ${passos?.length > 0 && html`
        <ul class="ficha-passos" aria-label="Passos de roteiro antes deste critério">
          ${passos.map((p) => html`<li>${p.nome}</li>`)}
        </ul>
      `}
      <h2 class="ficha-nome">${c.nome}</h2>
      ${sugestao && html`
        <button type="button" class="ficha-sugestao" data-tom=${sugestao.tom} onClick=${sugestao.aceitar}>
          <span>Sugestão: <strong>${sugestao.texto}</strong> <span class="ficha-sugestao-motivo">(${sugestao.motivo})</span></span>
          <span class="ficha-sugestao-acao">${sugestao.tecla ? html`<kbd>G</kbd>` : 'Aceitar'}</span>
        </button>
      `}
      <div class="ficha-texto">
        ${c.descricao && html`<${Markdown} texto=${c.descricao} />`}
        ${c.escala.tipo === 'ponto_unico' && html`<p><strong>Esperado:</strong> ${c.escala.esperado}</p>`}
      </div>
      ${resposta && html`
        <p class="ficha-marcado" data-tom=${marcado}>
          Marcado: <strong>${rotulo(c, resposta)}</strong>
          ${resposta.tipo !== 'nota' && html` (${num(pontos(c, resposta) ?? 0)} pt)`}
        </p>
      `}
      ${comentario && html`<p class="ficha-comentario">“${comentario}”</p>`}
      <div class="ficha-dicas" aria-hidden="true">
        <span class="dica nao">‹ ${neg.texto}</span>
        <span class="dica cima">${intermediarios ? (c.escala.tipo === 'niveis' ? `${c.escala.niveis.length} níveis` : 'nota') : 'comentar'}<i>︿</i></span>
        <span class="dica sim">${pos.texto} ›</span>
      </div>
      <span class="solte" aria-hidden="true">${intermediarios ? 'Solte para ver as opções' : 'Solte para comentar'}</span>
    </article>
  `;
}

function limiarX(el) {
  return Math.min(110, el.offsetWidth * 0.28);
}

// ---------------------------------------------------------------------------
// Opções do critério: níveis, nota, comentário
// ---------------------------------------------------------------------------

function CampoNota({ max, passo, inicial, rotuloBotao, aoAplicar, foco }) {
  const [texto, setTexto] = useState(inicial ?? '');
  const valor = lerNum(texto);
  const valido = Number.isFinite(valor) && valor >= 0 && valor <= max + 1e-9;
  const arredondar = (v) => Math.round(v / passo) * passo;
  const atalhos = [...new Set([0, 0.25, 0.5, 0.75, 1].map((f) => Math.round(arredondar(max * f) * 1000) / 1000))];
  const aplicar = () => valido && aoAplicar(valor);
  return html`
    <div class="nota">
      <div class="nota-atalhos">
        ${atalhos.map((v) => html`<button type="button" class="pastilha" onClick=${() => aoAplicar(v)}>${num(v)}</button>`)}
      </div>
      <div class="nota-linha">
        <input class="nota-valor" inputmode="decimal" aria-label="Nota" placeholder=${`0 a ${num(max)}`} ref=${foco}
          value=${texto} onInput=${(e) => setTexto(e.target.value)}
          onKeyDown=${(e) => { if (e.key === 'Enter') { e.preventDefault(); aplicar(); } }} />
        <span class="suave">de ${num(max)}</span>
        <button type="button" class="botao primario" disabled=${!valido} onClick=${aplicar}>${rotuloBotao}</button>
      </div>
      <input class="nota-barra" type="range" min="0" max=${max} step=${passo} aria-label="Ajustar nota"
        value=${Number.isFinite(valor) ? valor : 0} onInput=${(e) => setTexto(campo(Number(e.target.value)))} />
    </div>
  `;
}

function Opcoes({ item, resposta, comentario, aoMarcar, aoComentar, aoLimpar, aoPular, comTeclas, focoNota, focoComentario }) {
  const c = item.criterio;
  const e = c.escala;
  const [abaixo, setAbaixo] = useState(resposta?.tipo === 'ponto_unico' && resposta.desempenho === 'abaixo');
  let corpo;
  if (e.tipo === 'niveis') {
    const ordem = niveisOrdenados(c);
    corpo = html`
      <ol class="opcoes-niveis">
        ${ordem.map((n, i) => {
          const m = { tipo: 'nivel', nivel: n.id };
          return html`
            <li>
              <button type="button" class=${cls('opcao', resposta?.nivel === n.id && 'marcada')} data-tom=${tom(c, m)} onClick=${() => aoMarcar(m)}>
                ${comTeclas && html`<kbd>${ordem.length - i}</kbd>`}
                <span class="opcao-rotulo">${n.rotulo}</span>
                <span class="opcao-pontos">${num(n.pontos)} pt</span>
                ${n.descritor && html`<span class="opcao-descritor">${n.descritor}</span>`}
              </button>
            </li>
          `;
        })}
      </ol>
    `;
  } else if (e.tipo === 'binaria') {
    corpo = html`
      <ol class="opcoes-niveis">
        ${[true, false].map((atende) => {
          const m = { tipo: 'binaria', atende };
          return html`
            <li>
              <button type="button" class=${cls('opcao', resposta?.atende === atende && 'marcada')} data-tom=${atende ? 'sim' : 'nao'} onClick=${() => aoMarcar(m)}>
                ${comTeclas && html`<kbd>${atende ? 2 : 1}</kbd>`}
                <span class="opcao-rotulo">${atende ? 'Atende' : 'Não atende'}</span>
                <span class="opcao-pontos">${num(atende ? e.pontos : 0)} pt</span>
              </button>
            </li>
          `;
        })}
      </ol>
    `;
  } else if (e.tipo === 'livre') {
    corpo = html`<${CampoNota} max=${e.max} passo=${passoDe(e)} foco=${focoNota} rotuloBotao="Aplicar"
      inicial=${resposta?.tipo === 'nota' ? campo(resposta.nota) : ''} aoAplicar=${(nota) => aoMarcar({ tipo: 'nota', nota })} />`;
  } else {
    corpo = html`
      <div class="tres">
        ${Object.entries(DESEMPENHOS).map(([d, nome]) => html`
          <button type="button" data-tom=${d === 'abaixo' ? 'nao' : 'sim'}
            class=${cls('opcao', 'curta', (resposta?.desempenho === d || (d === 'abaixo' && abaixo)) && 'marcada')}
            onClick=${() => (d === 'abaixo' ? setAbaixo(true) : aoMarcar({ tipo: 'ponto_unico', desempenho: d, nota: e.max }))}>
            <span class="opcao-rotulo">${nome}</span>
            ${d !== 'abaixo' && html`<span class="opcao-pontos">${num(e.max)} pt</span>`}
          </button>
        `)}
      </div>
      ${abaixo && html`
        <p class="suave">Quanto vale o que foi entregue?</p>
        <${CampoNota} max=${e.max} passo=${passoDe(e)} foco=${focoNota} rotuloBotao="Aplicar"
          inicial=${resposta?.desempenho === 'abaixo' ? campo(resposta.nota) : ''}
          aoAplicar=${(nota) => aoMarcar({ tipo: 'ponto_unico', desempenho: 'abaixo', nota })} />
      `}
    `;
  }
  return html`
    <div class="opcoes">
      ${corpo}
      <label class="campo comentario">
        <span>Comentário para o aluno</span>
        <textarea rows="2" ref=${focoComentario} value=${comentario} placeholder="Opcional" onInput=${(ev) => aoComentar(ev.target.value)}></textarea>
      </label>
      <div class="opcoes-rodape">
        <button type="button" class="botao-texto" onClick=${aoPular}>Pular sem marcar</button>
        ${resposta && html`<button type="button" class="botao-texto" onClick=${aoLimpar}>Limpar marcação</button>`}
      </div>
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Folha que sobe de baixo (opções e lista de alunos no celular)
// ---------------------------------------------------------------------------

function Folha({ titulo, subtitulo, aoFechar, alta, espera = 0, children }) {
  const pega = useRef(null);
  const caixa = useRef(null);
  const aberta = useRef(performance.now());
  // Quando a folha abre por um toque, um toque duplo não pode fechá-la nem acertar,
  // sem querer, uma opção que acabou de subir sob o dedo: por `espera` ms ela
  // ignora cliques. Aberta por deslize ou botão, responde na hora.
  const recente = () => performance.now() - aberta.current < espera;
  const fecharPeloFundo = () => {
    if (!recente()) aoFechar();
  };
  const segurarClique = (e) => {
    if (recente()) {
      e.stopPropagation();
      e.preventDefault();
    }
  };

  // Enquanto aberta, a folha segura o foco; ao fechar, devolve a quem o tinha.
  useEffect(() => {
    const antes = document.activeElement;
    caixa.current?.focus();
    return () => antes?.focus?.();
  }, []);

  function aoTeclar(e) {
    if (e.key !== 'Tab') return;
    const focaveis = [...caixa.current.querySelectorAll('button, input, textarea, select, a[href]')].filter((el) => !el.disabled);
    if (!focaveis.length) return;
    const [primeiro, ultimo] = [focaveis[0], focaveis[focaveis.length - 1]];
    const fora = !caixa.current.contains(document.activeElement) || document.activeElement === caixa.current;
    if (e.shiftKey && (fora || document.activeElement === primeiro)) {
      e.preventDefault();
      ultimo.focus();
    } else if (!e.shiftKey && (fora || document.activeElement === ultimo)) {
      e.preventDefault();
      primeiro.focus();
    }
  }

  function aoDescer(e) {
    pega.current = e.clientY;
  }
  function aoSoltar(e) {
    if (pega.current !== null && e.clientY - pega.current > 40) aoFechar();
    pega.current = null;
  }
  return html`
    <div class="folha-fundo" onClick=${fecharPeloFundo}></div>
    <section class=${cls('folha', alta && 'alta')} role="dialog" aria-modal="true" aria-label=${titulo} tabindex="-1" ref=${caixa} onKeyDown=${aoTeclar} onClickCapture=${segurarClique}>
      <header class="folha-topo" onPointerDown=${aoDescer} onPointerUp=${aoSoltar}>
        <span class="folha-pega" aria-hidden="true"></span>
        <h2>${titulo}</h2>
        <button type="button" class="icone" aria-label="Fechar" onClick=${aoFechar}>×</button>
      </header>
      <div class="folha-corpo">
        ${subtitulo && html`<div class="folha-intro">${subtitulo}</div>`}
        ${children}
      </div>
    </section>
  `;
}

// ---------------------------------------------------------------------------
// Lista de alunos
// ---------------------------------------------------------------------------

function Alunos({ alunos, resultados, atual, aoEscolher, focar }) {
  const [busca, setBusca] = useState('');
  const [soFaltam, setSoFaltam] = useState(false);
  const lista = useRef(null);
  useEffect(() => {
    lista.current?.querySelector('.aluno.atual')?.scrollIntoView({ block: 'center' });
  }, [atual]);
  const termo = semAcento(busca.trim());
  const visiveis = alunos.filter((a) => {
    if (soFaltam && resultados[a.id].concluida && a.id !== atual) return false;
    return !termo || semAcento(a.nome).includes(termo) || semAcento(a.id).includes(termo);
  });
  const feitos = alunos.filter((a) => resultados[a.id].concluida).length;
  return html`
    <div class="alunos">
      <div class="alunos-topo">
      <input class="alunos-busca" type="search" placeholder="Buscar aluno" aria-label="Buscar aluno" autofocus=${focar}
        value=${busca} onInput=${(e) => setBusca(e.target.value)}
        onKeyDown=${(e) => { if (e.key === 'Enter' && visiveis.length) aoEscolher(visiveis[0].id); }} />
      <label class="alunos-filtro">
        <input type="checkbox" checked=${soFaltam} onChange=${(e) => setSoFaltam(e.target.checked)} />
        Só quem falta (${alunos.length - feitos})
      </label>
      </div>
      <ul class="alunos-lista" ref=${lista}>
        ${visiveis.map((a) => {
          const r = resultados[a.id];
          const estado = r.concluida ? 'feito' : r.respondidos ? 'meio' : 'nada';
          return html`
            <li>
              <button type="button" class=${cls('aluno', a.id === atual && 'atual')} data-estado=${estado}
                aria-current=${a.id === atual ? 'true' : null} onClick=${() => aoEscolher(a.id)}>
                <span class="aluno-marca" aria-hidden="true">${r.concluida ? '✓' : ''}</span>
                <span class="aluno-nome">${a.nome}</span>
                <span class="aluno-nota">${r.concluida ? num(r.nota) : r.respondidos ? `${r.respondidos}/${r.total}` : ''}</span>
              </button>
            </li>
          `;
        })}
        ${!visiveis.length && html`<li class="suave alunos-vazio">Ninguém com esse nome.</li>`}
      </ul>
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Resumo do aluno, depois do último critério
// ---------------------------------------------------------------------------

function Resumo({ aluno, avaliacao, itens, correcao, resultado, desfazer, aoIrPara, aoComentarGeral }) {
  const faltam = resultado.total - resultado.respondidos;
  const varias = avaliacao.rubricas.length > 1;
  let i = -1;
  return html`
    <article class="ficha resumo entra-frente">
      <p class="ficha-onde">
        ${desfazer && html`<button type="button" class="ficha-desfazer" onClick=${desfazer.acao}>Desfazer “${desfazer.texto}”</button>`}
        <span>${aluno.nome}</span>
      </p>
      <p class="resumo-nota" data-parcial=${!resultado.concluida}>
        <strong>${num(resultado.nota)}</strong><span> de ${num(avaliacao.nota_maxima)}</span>
      </p>
      ${faltam > 0 && html`<p class="resumo-faltam">${faltam === 1 ? 'Falta 1 critério' : `Faltam ${faltam} critérios`}: a nota conta zero neles.</p>`}
      ${avaliacao.rubricas.map((ra, n) => html`
        ${varias && html`
          <p class="resumo-rubrica">
            <span>${ra.rubrica.titulo}</span>
            <span>${num(resultado.rubricas[n].pontos)} / ${num(resultado.rubricas[n].maximo)}</span>
          </p>
        `}
        <ul class="resumo-lista">
          ${ra.rubrica.criterios.map((criterio) => {
            i += 1;
            if (ehRoteiro(criterio)) return null;
            const pos = i;
            const it = itens[pos];
            const r = respostaDe(correcao, it);
            const p = pontos(criterio, r);
            return html`
              <li>
                <button type="button" class="resumo-item" data-tom=${tom(criterio, r)} onClick=${() => aoIrPara(pos)}>
                  <span class="resumo-ponto" aria-hidden="true"></span>
                  <span class="resumo-criterio">${criterio.nome}</span>
                  <span class="resumo-pontos">${p === null ? '' : `${num(p * pesoDe(criterio))} / ${num(maximo(criterio) * pesoDe(criterio))}`}</span>
                  <span class="resumo-marca">${p === null ? 'sem marcação' : rotulo(criterio, r)}</span>
                  ${r?.comentario && html`<span class="resumo-comentario">“${r.comentario}”</span>`}
                </button>
              </li>
            `;
          })}
        </ul>
      `)}
      <label class="campo comentario">
        <span>Comentário geral</span>
        <textarea rows="2" placeholder="Opcional" value=${correcao?.comentario_geral || ''} onInput=${(e) => aoComentarGeral(e.target.value)}></textarea>
      </label>
    </article>
  `;
}

function Sincronia({ estado }) {
  let texto = 'Salvo';
  let tipo = 'ok';
  if (estado.erro) {
    texto = 'Veja o aviso';
    tipo = 'erro';
  } else if (estado.pendentes && (estado.falhando || !estado.online)) {
    texto = `Sem conexão, ${estado.pendentes} por enviar`;
    tipo = 'espera';
  } else if (estado.pendentes) {
    texto = 'Salvando…';
    tipo = 'indo';
  }
  return html`<span class="sincronia" data-tipo=${tipo} role="status">${texto}</span>`;
}

// ---------------------------------------------------------------------------
// A tela
// ---------------------------------------------------------------------------

export function Avaliar({ id, alunoId }) {
  const [pacote, setPacote] = useState(null);
  const [erro, setErro] = useState(null);
  const [correcoes, setCorrecoes] = useState({});
  const [idx, setIdx] = useState(0);
  const [veioDe, setVeioDe] = useState(-1); // item anterior, para saber se o roteiro já foi visto
  const [entrada, setEntrada] = useState('frente');
  const [bandeja, setBandeja] = useState(false);
  const [listaAberta, setListaAberta] = useState(false);
  const [rascunhos, setRascunhos] = useState({});
  const [ultima, setUltima] = useState(null); // última marcação, para desfazer
  const [sinc, setSinc] = useState(fila.estado());
  const largo = useMedia('(min-width: 960px)');
  // Com submissões, a tela bem larga mostra o código no lugar da lista de alunos.
  const muitoLargo = useMedia('(min-width: 1280px)');
  const [codigoAberto, setCodigoAberto] = useState(false);
  const controle = useRef(null);
  const focoNota = useRef(null);
  const focoComentario = useRef(null);
  const acoes = useRef({});

  async function carregar() {
    try {
      const p = await carregarPacote(id);
      setCorrecoes(p.correcoes);
      setPacote(p);
      setErro(null);
    } catch (e) {
      setErro(e);
    }
  }
  useEffect(() => {
    carregar();
    const parar = fila.ouvir(setSinc);
    // A cada envio aceito, a tela passa a mostrar a correção como ficou no servidor
    // (com o que outro avaliador marcou) mais o que ainda está na fila daqui.
    const pararConfirmar = fila.aoConfirmar((c) => {
      if (c.avaliacao === id) setCorrecoes((todas) => ({ ...todas, [c.aluno]: aplicarLote(c.correcao, c.pendente) }));
    });
    return () => {
      parar();
      pararConfirmar();
    };
  }, [id]);

  // Recarrega do servidor quando ele recusa marcações (a rubrica mudou) e quando
  // a conexão volta depois de abrir a tela com a cópia guardada.
  // Só com a fila parada: uma leitura feita com envio em curso poderia chegar depois
  // dele e mostrar o estado de antes.
  const offline = !!pacote?.offline;
  const [recarregar, setRecarregar] = useState(false);
  useEffect(() => {
    if (!sinc.recusas) return;
    setUltima(null);
    setRecarregar(true);
  }, [sinc.recusas]);
  useEffect(() => {
    if (!(offline || recarregar) || !sinc.online || sinc.enviando || sinc.pendentes || sinc.falhando) return;
    setRecarregar(false);
    carregar();
  }, [offline, recarregar, sinc.online, sinc.enviando, sinc.pendentes, sinc.falhando]);

  // O aviso de desfazer some sozinho.
  useEffect(() => {
    if (!ultima) return undefined;
    const t = setTimeout(() => setUltima(null), 6000);
    return () => clearTimeout(t);
  }, [ultima]);

  // Fora desta tela a página rola; aqui o cartão é que se move.
  useEffect(() => {
    document.documentElement.classList.add('avaliando');
    return () => document.documentElement.classList.remove('avaliando');
  }, []);

  const avaliacao = pacote?.avaliacao;
  const alunos = pacote?.turma.alunos || [];
  const itens = useMemo(() => (avaliacao ? listarItens(avaliacao) : []), [avaliacao]);
  const resultados = useMemo(() => {
    const r = {};
    if (avaliacao) for (const a of alunos) r[a.id] = calcular(avaliacao, correcoes[a.id]);
    return r;
  }, [avaliacao, correcoes]);
  const aluno = alunos.find((a) => a.id === alunoId) || null;
  const ead = pacote?.ead || null;
  const comCodigo = muitoLargo && !!ead;
  // A lista de alunos fica fixa na lateral só quando o código não ocupa esse lugar.
  const listaFixa = largo && !comCodigo;

  // A tela "Acompanhar", aberta em outro aparelho, segue o aluno aberto aqui.
  useEffect(() => {
    if (!aluno) return undefined;
    const avisar = () => api('PUT', '/foco', { avaliacao: id, aluno: aluno.id }).catch(() => {});
    avisar();
    const aoVoltar = () => document.visibilityState === 'visible' && avisar();
    document.addEventListener('visibilitychange', aoVoltar);
    return () => document.removeEventListener('visibilitychange', aoVoltar);
  }, [id, aluno?.id]);

  // Sem aluno no endereço: começa por quem ainda falta.
  useEffect(() => {
    if (!pacote || aluno || !alunos.length) return;
    const alvo = alunos.find((a) => !resultados[a.id].concluida) || alunos[0];
    ir(rota('avaliar', id, alvo.id), true);
  }, [pacote, alunoId]);

  // Ao abrir e ao trocar de aluno: vai ao primeiro critério sem marcação. Recarregar
  // os dados (a rede voltou, o servidor recusou algo) não mexe em onde o usuário está.
  const carregado = !!pacote;
  useEffect(() => {
    if (!pacote || !aluno) return;
    setIdx(primeiroSemResposta(itens, correcoes[aluno.id]));
    setVeioDe(-1);
    setEntrada('frente');
    setBandeja(false);
    setListaAberta(false);
    setCodigoAberto(false);
    setUltima(null);
  }, [carregado, alunoId]);

  // Mantém a cópia offline em dia com o que está na tela — logo depois de cada
  // mudança e, sem falta, quando a página é fechada, recarregada ou vai para o fundo.
  const ultimasCorrecoes = useRef(correcoes);
  ultimasCorrecoes.current = correcoes;
  useEffect(() => {
    if (!pacote) return undefined;
    const t = setTimeout(() => guardarCorrecoes(id, correcoes), 250);
    return () => clearTimeout(t);
  }, [correcoes]);
  useEffect(() => {
    if (!carregado) return undefined;
    const agora = () => guardarCorrecoes(id, ultimasCorrecoes.current);
    const aoEsconder = () => document.visibilityState === 'hidden' && agora();
    window.addEventListener('pagehide', agora);
    document.addEventListener('visibilitychange', aoEsconder);
    return () => {
      agora();
      window.removeEventListener('pagehide', agora);
      document.removeEventListener('visibilitychange', aoEsconder);
    };
  }, [carregado, id]);

  // O ouvinte de teclado é registrado uma vez e sempre chama a versão atual das ações.
  useEffect(() => {
    function tecla(e) {
      const a = acoes.current;
      if (!a.marcar || e.repeat) return;
      const campoAtivo = e.target.closest?.('input, textarea, select');
      if (e.key === 'Escape') {
        if (campoAtivo) campoAtivo.blur();
        setBandeja(false);
        setListaAberta(false);
        setCodigoAberto(false);
        return;
      }
      if (campoAtivo || e.ctrlKey || e.metaKey || e.altKey) return;
      if (a.folhaAberta && !/^[1-9]$/.test(e.key)) return;
      const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      if (k === 'z' && a.noResumo) a.desfazer();
      else if (k === '[' || k === 'PageUp') a.vizinho(-1);
      else if (k === ']' || k === 'PageDown') a.vizinho(1);
      else if (a.noResumo) {
        if (k === 'Enter') a.seguir();
        else if (k === 'ArrowDown' || k === 'Backspace') a.irPara(a.idx - 1);
        else return;
      } else if (ehRoteiro(a.item.criterio)) {
        if (k === 'ArrowRight' || k === 'd' || k === 'Enter' || k === ' ' || k === 's') a.avancar('direita');
        else if (k === 'ArrowLeft' || k === 'a') a.avancar('esquerda');
        else if (k === 'ArrowDown' || k === 'Backspace') a.irPara(a.idx - 1);
        else if (k === 'z') a.desfazer();
        else return;
      } else if (k === 'ArrowRight' || k === 'd') a.marcar(marcacaoRapida(a.item.criterio, true), 'direita');
      else if (k === 'ArrowLeft' || k === 'a') a.marcar(marcacaoRapida(a.item.criterio, false), 'esquerda');
      else if (k === 'ArrowUp') a.abrirOpcoes('tecla');
      else if (k === 'ArrowDown' || k === 'Backspace') a.irPara(a.idx - 1);
      else if (k === 's') a.irPara(a.idx + 1);
      else if (k === 'z') a.desfazer();
      else if (k === 'g' && a.sugestao) a.sugestao.aceitar();
      else if (k === 'c') {
        if (!a.largo) setBandeja(true);
        setTimeout(() => focoComentario.current?.focus(), 60);
      } else if (/^[1-9]$/.test(k)) {
        const esc = a.item.criterio.escala;
        const n = Number(k);
        if (esc.tipo === 'niveis') {
          const ordem = niveisOrdenados(a.item.criterio);
          const nivel = ordem[ordem.length - n];
          if (!nivel) return;
          a.marcar({ tipo: 'nivel', nivel: nivel.id }, 'cima');
        } else if (esc.tipo === 'binaria' && n <= 2) {
          a.marcar({ tipo: 'binaria', atende: n === 2 }, n === 2 ? 'direita' : 'esquerda');
        } else return;
      } else return;
      e.preventDefault();
    }
    window.addEventListener('keydown', tecla);
    return () => window.removeEventListener('keydown', tecla);
  }, []);

  if (erro) {
    return html`
      <div class="av av-so-aviso">
        <${Estado} erro=${erro} recarregar=${carregar} />
        <a class="botao" href="#/avaliacoes">Voltar às avaliações</a>
      </div>
    `;
  }
  if (!pacote) return html`<div class="av av-so-aviso"><p aria-busy="true">Carregando…</p></div>`;
  if (!alunos.length) {
    return html`
      <div class="av av-so-aviso">
        <p>A turma ${pacote.turma.id} não tem alunos.</p>
        <a class="botao" href=${`#${rota('turmas', pacote.turma.id)}`}>Abrir a turma</a>
      </div>
    `;
  }
  if (!aluno) return html`<div class="av av-so-aviso"><p aria-busy="true">Carregando…</p></div>`;

  const correcao = correcoes[aluno.id] || null;
  const resultado = resultados[aluno.id];
  const noResumo = idx >= itens.length;
  const item = noResumo ? null : itens[idx];
  const passo = !!item && ehRoteiro(item.criterio);
  const resposta = item && !passo ? respostaDe(correcao, item) : null;
  const chaveItem = item ? fila.chave(item.origem, item.criterio.id) : '';
  const comentario = item ? (resposta ? resposta.comentario || '' : rascunhos[`${aluno.id}\n${chaveItem}`] || '') : '';
  const posAluno = alunos.findIndex((a) => a.id === aluno.id);

  function gravar(it, nova) {
    setCorrecoes((todas) => {
      const c = structuredClone(todas[aluno.id] || { respostas: {} });
      c.respostas ||= {};
      if (nova) (c.respostas[it.origem] ||= {})[it.criterio.id] = nova;
      else if (c.respostas[it.origem]) delete c.respostas[it.origem][it.criterio.id];
      return { ...todas, [aluno.id]: c };
    });
    fila.adicionar(id, aluno.id, { respostas: { [fila.chave(it.origem, it.criterio.id)]: nova } });
  }

  function marcar(marcacao, dir) {
    if (!item) return;
    const nova = comentario.trim() ? { ...marcacao, comentario: comentario.trim() } : marcacao;
    controle.current?.voar(dir, { texto: rotulo(item.criterio, nova), tom: tom(item.criterio, nova) });
    gravar(item, nova);
    vibrar();
    setUltima({ aluno: aluno.id, idx, anterior: resposta, texto: rotulo(item.criterio, nova) });
    // Segue para o próximo critério ainda sem marcação (ou para o roteiro dele); se
    // não há mais, para o resumo. Assim, revisar um critério no meio não obriga a
    // passar de novo pelos já marcados.
    setEntrada('frente');
    setBandeja(false);
    setVeioDe(idx);
    setIdx(primeiroSemResposta(itens, correcao, idx + 1));
  }

  function avancar(dir) {
    if (!passo) return;
    controle.current?.voar(dir);
    setEntrada('frente');
    setVeioDe(idx);
    setIdx(idx + 1);
  }

  function desfazer() {
    if (!ultima || ultima.aluno !== aluno.id) return;
    gravar(itens[ultima.idx], ultima.anterior);
    setEntrada('tras');
    setBandeja(false);
    setVeioDe(idx);
    setIdx(ultima.idx);
    setUltima(null);
  }

  function comentar(texto) {
    if (!item) return;
    if (resposta) {
      const nova = { ...resposta };
      if (texto.trim()) nova.comentario = texto;
      else delete nova.comentario;
      gravar(item, nova);
    } else {
      setRascunhos({ ...rascunhos, [`${aluno.id}\n${chaveItem}`]: texto });
    }
  }

  function limpar() {
    if (item && resposta) gravar(item, null);
  }

  function comentarGeral(texto) {
    setCorrecoes((todas) => ({ ...todas, [aluno.id]: { ...(todas[aluno.id] || { respostas: {} }), comentario_geral: texto } }));
    fila.adicionar(id, aluno.id, { comentario_geral: texto });
  }

  function irPara(i) {
    setEntrada(i < idx ? 'tras' : 'frente');
    setBandeja(false);
    setVeioDe(idx);
    setIdx(Math.max(0, Math.min(itens.length, i)));
  }

  function abrirOpcoes(origem) {
    if (noResumo || passo) return;
    if (largo) {
      if (origem !== 'toque') (focoNota.current || document.querySelector('.av-painel .opcao'))?.focus();
    } else {
      setBandeja(origem === 'toque' ? 'toque' : true);
    }
  }

  function trocarAluno(novo) {
    // Trocar de aluno não empilha histórico: "voltar" sai da avaliação, não desfila alunos.
    if (novo && novo !== aluno.id) ir(rota('avaliar', id, novo), true);
    setListaAberta(false);
  }

  function pendenteDepois() {
    for (let k = 1; k < alunos.length; k += 1) {
      const a = alunos[(posAluno + k) % alunos.length];
      if (!resultados[a.id].concluida) return a;
    }
    return null;
  }
  const proximo = pendenteDepois();
  const todosProntos = alunos.every((a) => resultados[a.id].concluida);

  function vizinho(delta) {
    trocarAluno(alunos[(posAluno + delta + alunos.length) % alunos.length].id);
  }

  function seguir() {
    if (!resultado.concluida) irPara(primeiroSemResposta(itens, correcao));
    else if (proximo) trocarAluno(proximo.id);
    else ir(rota('avaliacoes', id));
  }

  const podeDesfazer = ultima && ultima.aluno === aluno.id ? { texto: ultima.texto, acao: desfazer } : null;
  // Sugestão das checagens automáticas: aparece até o critério ter exatamente essa marcação.
  const sug = item && !passo && ead?.relatorios[aluno.id]?.sugestoes.find((x) => x.rubrica === item.origem && x.criterio === item.criterio.id);
  const sugestao = sug && pontos(item.criterio, sug.marcacao) !== null && !mesmaMarcacao(resposta, sug.marcacao)
    ? {
      texto: rotulo(item.criterio, sug.marcacao),
      motivo: sug.motivo,
      tom: tom(item.criterio, sug.marcacao),
      tecla: largo,
      aceitar: () => {
        const t = tom(item.criterio, sug.marcacao);
        marcar(sug.marcacao, t === 'sim' ? 'direita' : t === 'nao' ? 'esquerda' : 'cima');
      },
    }
    : null;
  const avisos = avisosEad(ead, aluno.id, alunos);
  acoes.current = { marcar, avancar, irPara, abrirOpcoes, vizinho, seguir, desfazer, item, idx, noResumo, largo, sugestao, folhaAberta: ((bandeja || listaAberta) && !largo) || (listaAberta && comCodigo) || codigoAberto };
  const pos = item && !passo ? rapido(item.criterio, true) : null;
  const neg = item && !passo ? rapido(item.criterio, false) : null;
  // Critérios contam "3 de 8"; passos de roteiro não entram na conta.
  const totalCriterios = itens.filter((it) => !ehRoteiro(it.criterio)).length;
  const posicao = itens.slice(0, idx + 1).filter((it) => !ehRoteiro(it.criterio)).length;
  // Chegou ao critério sem passar pelo roteiro dele: os passos aparecem resumidos no cartão.
  const passos = item && !passo && veioDe !== idx - 1 ? passosAntes(itens, idx) : [];
  const variasRubricas = avaliacao.rubricas.length > 1;
  const hrefNotas = `#${rota('avaliacoes', id)}`;
  const rotuloOpcoes = !item || passo ? '' : !temIntermediarios(item.criterio) ? 'Comentar' : item.criterio.escala.tipo === 'niveis' ? 'Níveis' : 'Nota';

  const opcoes = item && !passo && html`
    <${Opcoes} key=${`${aluno.id}:${chaveItem}`} item=${item} resposta=${resposta} comentario=${comentario} comTeclas=${largo}
      focoNota=${focoNota} focoComentario=${focoComentario}
      aoMarcar=${(m) => marcar(m, 'cima')} aoComentar=${comentar} aoLimpar=${limpar} aoPular=${() => irPara(idx + 1)} />
  `;
  const listaAlunos = (focar) => html`<${Alunos} alunos=${alunos} resultados=${resultados} atual=${aluno.id} aoEscolher=${trocarAluno} focar=${focar} />`;

  return html`
    <div class=${cls('av', largo && 'largo', comCodigo && 'com-codigo')}>
      <header class="av-topo">
        <a class="av-sair" href=${hrefNotas} aria-label="Voltar às notas">‹</a>
        <button type="button" class="av-vizinho" aria-label="Aluno anterior" onClick=${() => vizinho(-1)}>◂</button>
        <button type="button" class="av-aluno" onClick=${() => !listaFixa && setListaAberta(true)} aria-haspopup=${listaFixa ? null : 'dialog'}>
          <span class="av-aluno-nome">${aluno.nome}</span>
          <span class="av-aluno-meta">${posAluno + 1} de ${alunos.length}${listaFixa ? '' : ' ▾'}</span>
        </button>
        <button type="button" class="av-vizinho" aria-label="Próximo aluno" onClick=${() => vizinho(1)}>▸</button>
        ${ead && !comCodigo
          ? html`<button type="button" class="av-codigo" onClick=${() => setCodigoAberto(true)} aria-haspopup="dialog">
              <span aria-hidden="true">${'</>'}</span><span>Código</span>
            </button>`
          : html`<span></span>`}
        <div class="av-nota" title="Nota até aqui">
          <strong>${num(resultado.nota)}</strong>
          <${Sincronia} estado=${sinc} />
        </div>
        <nav class="trilha" aria-label="Critérios">
          ${itens.map((it, i) => html`
            <button type="button" class="trecho" data-tom=${tom(it.criterio, respostaDe(correcao, it))} data-tipo=${ehRoteiro(it.criterio) ? 'passo' : null}
              aria-current=${i === idx ? 'step' : null} aria-label=${`${it.criterio.nome}${respostaDe(correcao, it) ? `: ${rotulo(it.criterio, respostaDe(correcao, it))}` : ''}`}
              onClick=${() => irPara(i)}></button>
          `)}
          <button type="button" class="trecho fim" aria-current=${noResumo ? 'step' : null} aria-label="Resumo do aluno" onClick=${() => irPara(itens.length)}></button>
        </nav>
      </header>

      ${listaFixa && html`<aside class="av-lateral">${listaAlunos(false)}</aside>`}
      ${comCodigo && html`
        <aside class="av-codigo-lateral" aria-label="Código do aluno">
          <${PainelCodigo} avaliacao=${id} aluno=${aluno.id} ead=${ead} alunos=${alunos} />
        </aside>
      `}

      <main class="av-palco">
        ${pacote.offline && html`<p class="av-aviso">Sem conexão: usando a cópia guardada neste aparelho.</p>`}
        ${avisos.length > 0 && html`
          <ul class="av-avisos" aria-label="Avisos sobre a submissão">
            ${avisos.map((a) => html`<li data-tipo=${a.tipo}>${a.texto}</li>`)}
          </ul>
        `}
        ${noResumo
          ? html`<${Resumo} key=${`${aluno.id}:resumo`} aluno=${aluno} avaliacao=${avaliacao} itens=${itens} correcao=${correcao} resultado=${resultado}
              desfazer=${podeDesfazer} aoIrPara=${irPara} aoComentarGeral=${comentarGeral} />`
          : html`
            <div class="pilha">
              ${idx < itens.length - 1 && html`<div class="ficha-debaixo" aria-hidden="true"></div>`}
              <${Ficha} key=${`${aluno.id}:${idx}`} item=${item} resposta=${resposta}
                posicao=${passo ? (posicao < totalCriterios ? `antes do critério ${posicao + 1}` : '') : `${posicao} de ${totalCriterios}`}
                mostrarRubrica=${variasRubricas} entrada=${entrada} comentario=${comentario} controle=${controle} desfazer=${podeDesfazer} passos=${passos} sugestao=${sugestao}
                aoPositivo=${() => (passo ? avancar('direita') : marcar(pos.marcacao, 'direita'))}
                aoNegativo=${() => (passo ? avancar('esquerda') : marcar(neg.marcacao, 'esquerda'))}
                aoCima=${abrirOpcoes} aoBaixo=${() => irPara(idx - 1)} />
            </div>
          `}
      </main>

      ${largo && html`
        <aside class="av-painel">
          ${passo
            ? html`<h2 class="av-painel-titulo">Passo de roteiro</h2><p class="suave">Não vale nota. <kbd>→</kbd> ou <kbd>Enter</kbd> para seguir.</p>`
            : item ? html`<h2 class="av-painel-titulo">${item.criterio.nome}</h2>${opcoes}` : html`<p class="suave">Escolha um critério do resumo para revisar.</p>`}
          <dl class="teclas">
            <dt><kbd>←</kbd> <kbd>→</kbd></dt><dd>mínimo e máximo</dd>
            <dt><kbd>1</kbd>–<kbd>9</kbd></dt><dd>escolher o nível</dd>
            <dt><kbd>↑</kbd></dt><dd>ir às opções</dd>
            <dt><kbd>↓</kbd></dt><dd>voltar um critério</dd>
            <dt><kbd>S</kbd></dt><dd>pular sem marcar</dd>
            <dt><kbd>C</kbd></dt><dd>comentar</dd>
            <dt><kbd>Z</kbd></dt><dd>desfazer a última marcação</dd>
            ${ead?.pipeline && html`<dt><kbd>G</kbd></dt><dd>aceitar a sugestão</dd>`}
            <dt><kbd>Enter</kbd></dt><dd>próximo aluno, no resumo</dd>
            <dt><kbd>[</kbd> <kbd>]</kbd></dt><dd>trocar de aluno</dd>
          </dl>
        </aside>
      `}

      ${passo && html`
        <footer class="av-botoes um">
          <button type="button" class="av-botao seguir" onClick=${() => avancar('direita')}>
            <span class="av-botao-texto">Seguir ›</span>
          </button>
        </footer>
      `}

      ${!noResumo && !passo && html`
        <footer class="av-botoes">
          <button type="button" class="av-botao nao" onClick=${() => marcar(neg.marcacao, 'esquerda')}>
            <span class="av-botao-sinal" aria-hidden="true">✗</span>
            <span class="av-botao-texto">${neg.texto}</span>
          </button>
          <button type="button" class="av-botao meio" onClick=${() => abrirOpcoes('botao')} aria-haspopup=${largo ? null : 'dialog'}>
            <span class="av-botao-sinal" aria-hidden="true">︿</span>
            <span class="av-botao-texto">${rotuloOpcoes}</span>
          </button>
          <button type="button" class="av-botao sim" onClick=${() => marcar(pos.marcacao, 'direita')}>
            <span class="av-botao-sinal" aria-hidden="true">✓</span>
            <span class="av-botao-texto">${pos.texto}</span>
          </button>
        </footer>
      `}

      ${noResumo && html`
        <footer class="av-botoes um">
          ${!resultado.concluida
            ? html`<button type="button" class="av-botao seguir" onClick=${() => irPara(primeiroSemResposta(itens, correcao))}>Completar o que falta</button>`
            : proximo
              ? html`<button type="button" class="av-botao seguir" onClick=${() => trocarAluno(proximo.id)}>
                  <span class="av-botao-texto">Próximo aluno: ${proximo.nome}</span>
                </button>`
              : html`<a class="av-botao seguir" href=${hrefNotas}>Turma toda avaliada. Ver notas</a>`}
        </footer>
      `}

      ${bandeja && item && !largo && html`
        <${Folha} titulo=${item.criterio.nome} espera=${bandeja === 'toque' ? 350 : 0} aoFechar=${() => setBandeja(false)}
          subtitulo=${(item.criterio.descricao || item.criterio.escala.tipo === 'ponto_unico') && html`
            ${item.criterio.descricao && html`<${Markdown} texto=${item.criterio.descricao} />`}
            ${item.criterio.escala.tipo === 'ponto_unico' && html`<p><strong>Esperado:</strong> ${item.criterio.escala.esperado}</p>`}
          `}>${opcoes}<//>
      `}
      ${codigoAberto && ead && html`
        <${Folha} titulo=${aluno.nome} alta aoFechar=${() => setCodigoAberto(false)}>
          <${PainelCodigo} avaliacao=${id} aluno=${aluno.id} ead=${ead} alunos=${alunos} />
        <//>
      `}
      ${listaAberta && !listaFixa && html`
        <${Folha} titulo=${avaliacao.titulo} subtitulo=${`${plural(alunos.filter((a) => resultados[a.id].concluida).length, 'avaliado', 'avaliados')} de ${alunos.length}`} alta espera=${350} aoFechar=${() => setListaAberta(false)}>
          ${listaAlunos(false)}
        <//>
      `}
    </div>
  `;
}
