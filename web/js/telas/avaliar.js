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
//
// As peças ficam em avaliar/: o cartão e o gesto, as opções, a lista de alunos,
// o resumo, o teclado e a navegação entre itens.
import { html, useState, useEffect, useRef, useMemo } from '../../vendor/preact-htm.js';
import { api, carregarPacote, fila, guardarCorrecoes } from '../api.js';
import { aplicarLote } from '../lote.js';
import { calcular, itens as listarItens, rotulo, tom, pontos, temIntermediarios, ehRoteiro } from '../nota.js';
import { PainelCodigo, avisosEad, avisosDoCodigo } from '../codigo.js';
import { ir, rota, num, cls, vibrar, plural } from '../util.js';
import { Estado, Folha, useMedia, useTelaInteira } from './comum.js';
import { FichaCriterio, FichaPasso, DescricaoCriterio, temDescricao } from './avaliar/ficha.js';
import { Opcoes } from './avaliar/opcoes.js';
import { Alunos, useAlunos } from './avaliar/alunos.js';
import { Resumo, Sincronia } from './avaliar/resumo.js';
import { useTeclado } from './avaliar/teclado.js';
import { Rodape, Teclas } from './avaliar/botoes.js';
import { respostaDe, primeiroSemResposta, passosAntes, contarCriterios, mesmaMarcacao } from './avaliar/navegacao.js';

// Qual folha está aberta por cima do cartão: null, 'opcoes' (por deslize, tecla ou
// botão), 'opcoes-toque' (por um toque, que espera um pouco antes de aceitar
// cliques), 'alunos' ou 'codigo'.
const ehOpcoes = (folha) => folha === 'opcoes' || folha === 'opcoes-toque';

export function Avaliar({ id, alunoId }) {
  const [pacote, setPacote] = useState(null);
  const [erro, setErro] = useState(null);
  const [correcoes, setCorrecoes] = useState({});
  const [idx, setIdx] = useState(0);
  const [veioDe, setVeioDe] = useState(-1); // item anterior, para saber se o roteiro já foi visto
  const [entrada, setEntrada] = useState('frente');
  const [folha, setFolha] = useState(null);
  const [rascunhos, setRascunhos] = useState({});
  const [ultima, setUltima] = useState(null); // última marcação, para desfazer
  const [sinc, setSinc] = useState(fila.estado());
  const largo = useMedia('(min-width: 960px)');
  // Com submissões, a tela bem larga mostra o código no lugar da lista de alunos.
  const muitoLargo = useMedia('(min-width: 1280px)');
  const controle = useRef(null);
  const focoNota = useRef(null);
  const focoComentario = useRef(null);
  const acoes = useRef({});

  async function carregar() {
    try {
      const p = await carregarPacote(id);
      // Quem só pode ver não marca nada aqui: as marcações seriam recusadas depois.
      if (p.acoes && !p.acoes.includes('corrigir')) {
        ir(rota('avaliacoes', id));
        return;
      }
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

  useTelaInteira();

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
  const turma = useAlunos({ alunos, ead, resultados, atual: alunoId });

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
    ir(rota('avaliar', id, turma.primeiroPendente().id), true);
  }, [pacote, alunoId]);

  // Ao abrir e ao trocar de aluno: vai ao primeiro critério sem marcação. Recarregar
  // os dados (a rede voltou, o servidor recusou algo) não mexe em onde o usuário está.
  const carregado = !!pacote;
  useEffect(() => {
    if (!pacote || !aluno) return;
    setIdx(primeiroSemResposta(itens, correcoes[aluno.id]));
    setVeioDe(-1);
    setEntrada('frente');
    setFolha(null);
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

  useTeclado(acoes, () => setFolha(null));

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
  const criterio = item && !passo ? item.criterio : null;
  const resposta = criterio ? respostaDe(correcao, item) : null;
  const chaveItem = item ? fila.chave(item.origem, item.criterio.id) : '';
  const chaveRascunho = `${aluno.id}\n${chaveItem}`;
  const comentario = item ? (resposta ? resposta.comentario || '' : rascunhos[chaveRascunho] || '') : '';

  /** Toda alteração passa por aqui: entra na tela já e vai para a fila. */
  function registrar(lote) {
    setCorrecoes((todas) => ({ ...todas, [aluno.id]: aplicarLote(todas[aluno.id], lote) }));
    fila.adicionar(id, aluno.id, lote);
  }
  const gravar = (it, nova) => registrar({ respostas: { [fila.chave(it.origem, it.criterio.id)]: nova } });

  /** Vai ao item `i` (o resumo é `itens.length`). Andar fecha as opções; a lista e
      o código continuam abertos, para quem marca pelo teclado olhando para eles. */
  function irPara(i, sentido = i < idx ? 'tras' : 'frente') {
    setEntrada(sentido);
    setFolha((f) => (ehOpcoes(f) ? null : f));
    setVeioDe(idx);
    setIdx(Math.max(0, Math.min(itens.length, i)));
  }

  function marcar(marcacao, dir) {
    if (!criterio) return;
    const nova = comentario.trim() ? { ...marcacao, comentario: comentario.trim() } : marcacao;
    controle.current?.voar(dir, { texto: rotulo(criterio, nova), tom: tom(criterio, nova) });
    gravar(item, nova);
    vibrar();
    setUltima({ aluno: aluno.id, idx, anterior: resposta, texto: rotulo(criterio, nova) });
    // Segue para o próximo critério ainda sem marcação (ou para o roteiro dele); se
    // não há mais, para o resumo. Assim, revisar um critério no meio não obriga a
    // passar de novo pelos já marcados.
    irPara(primeiroSemResposta(itens, correcao, idx + 1), 'frente');
  }

  function avancar(dir) {
    if (!passo) return;
    controle.current?.voar(dir);
    irPara(idx + 1, 'frente');
  }

  function desfazer() {
    if (!ultima || ultima.aluno !== aluno.id) return;
    gravar(itens[ultima.idx], ultima.anterior);
    irPara(ultima.idx, 'tras');
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
      setRascunhos({ ...rascunhos, [chaveRascunho]: texto });
    }
  }

  function abrirOpcoes(origem) {
    if (!criterio) return;
    if (largo) {
      if (origem !== 'toque') (focoNota.current || document.querySelector('.av-painel .opcao'))?.focus();
    } else {
      setFolha(origem === 'toque' ? 'opcoes-toque' : 'opcoes');
    }
  }

  function trocarAluno(novo) {
    // Trocar de aluno não empilha histórico: "voltar" sai da avaliação, não desfila alunos.
    if (novo && novo !== aluno.id) ir(rota('avaliar', id, novo), true);
    setFolha((f) => (f === 'alunos' ? null : f));
  }

  const vizinho = (d) => trocarAluno(turma.vizinho(d).id);
  const proximo = turma.proximoPendente;
  const completar = () => irPara(primeiroSemResposta(itens, correcao));
  function seguir() {
    if (!resultado.concluida) completar();
    else if (proximo) trocarAluno(proximo.id);
    else ir(rota('avaliacoes', id));
  }

  const podeDesfazer = ultima && ultima.aluno === aluno.id ? { texto: ultima.texto, acao: desfazer } : null;
  // Sugestão das checagens automáticas: aparece até o critério ter exatamente essa marcação.
  const sug = criterio && ead?.relatorios[aluno.id]?.sugestoes.find((x) => x.rubrica === item.origem && x.criterio === criterio.id);
  const sugestao = sug && pontos(criterio, sug.marcacao) !== null && !mesmaMarcacao(resposta, sug.marcacao)
    ? {
      texto: rotulo(criterio, sug.marcacao),
      motivo: sug.motivo,
      tom: tom(criterio, sug.marcacao),
      tecla: largo,
      aceitar: () => {
        const t = tom(criterio, sug.marcacao);
        marcar(sug.marcacao, t === 'sim' ? 'direita' : t === 'nao' ? 'esquerda' : 'cima');
      },
    }
    : null;

  // O que de fato aparece por cima: no largo, as opções ficam na lateral e a lista, fixa.
  const folhaOpcoes = ehOpcoes(folha) && criterio && !largo;
  const folhaAlunos = folha === 'alunos' && !listaFixa;
  const folhaCodigo = folha === 'codigo' && !!ead;
  acoes.current = {
    marcar, avancar, irPara, abrirOpcoes, desfazer, seguir, vizinho, item, idx, noResumo, sugestao,
    comentar: () => {
      if (!largo) setFolha('opcoes');
      setTimeout(() => focoComentario.current?.focus(), 60);
    },
    folhaAberta: folhaOpcoes || folhaAlunos || folhaCodigo,
  };

  const totalCriterios = contarCriterios(itens);
  const posicao = contarCriterios(itens, idx + 1);
  const hrefNotas = `#${rota('avaliacoes', id)}`;
  const avisos = avisosEad(ead, aluno.id, alunos);

  const opcoes = criterio && html`
    <${Opcoes} key=${`${aluno.id}:${chaveItem}`} item=${item} resposta=${resposta} comentario=${comentario} comTeclas=${largo}
      focoNota=${focoNota} focoComentario=${focoComentario}
      aoMarcar=${(m) => marcar(m, 'cima')} aoComentar=${comentar} aoLimpar=${() => resposta && gravar(item, null)} aoPular=${() => irPara(idx + 1)} />
  `;
  const listaAlunos = html`<${Alunos} alunos=${alunos} resultados=${resultados} atual=${aluno.id} aoEscolher=${trocarAluno} filtro=${turma.filtro} />`;
  const ficha = passo
    ? html`<${FichaPasso} key=${`${aluno.id}:${idx}`} item=${item} entrada=${entrada} controle=${controle} desfazer=${podeDesfazer}
        posicao=${posicao < totalCriterios ? `antes do critério ${posicao + 1}` : ''}
        aoSeguir=${avancar} aoBaixo=${() => irPara(idx - 1)} />`
    : html`<${FichaCriterio} key=${`${aluno.id}:${idx}`} item=${item} resposta=${resposta} posicao=${`${posicao} de ${totalCriterios}`}
        mostrarRubrica=${avaliacao.rubricas.length > 1} entrada=${entrada} comentario=${comentario} controle=${controle} desfazer=${podeDesfazer}
        passos=${veioDe !== idx - 1 ? passosAntes(itens, idx) : []} sugestao=${sugestao}
        aoMarcar=${marcar} aoCima=${abrirOpcoes} aoBaixo=${() => irPara(idx - 1)} />`;

  return html`
    <div class=${cls('av', largo && 'largo', comCodigo && 'com-codigo')}>
      <header class="av-topo">
        <a class="av-sair" href=${hrefNotas} aria-label="Voltar às notas">‹</a>
        <button type="button" class="av-vizinho" aria-label="Aluno anterior" onClick=${() => vizinho(-1)}>◂</button>
        <button type="button" class="av-aluno" onClick=${() => !listaFixa && setFolha('alunos')} aria-haspopup=${listaFixa ? null : 'dialog'}>
          <span class="av-aluno-nome">${aluno.nome}</span>
          <span class="av-aluno-meta">${turma.posicao + 1} de ${turma.navegaveis.length}${listaFixa ? '' : ' ▾'}</span>
        </button>
        <button type="button" class="av-vizinho" aria-label="Próximo aluno" onClick=${() => vizinho(1)}>▸</button>
        ${ead && !comCodigo
          ? html`<button type="button" class="av-codigo" onClick=${() => setFolha('codigo')} aria-haspopup="dialog">
              <span aria-hidden="true">${'</>'}</span><span>Código</span>
            </button>`
          : html`<span></span>`}
        <div class="av-nota" title="Nota até aqui">
          <strong>${num(resultado.nota)}</strong>
          <${Sincronia} estado=${sinc} />
        </div>
        <nav class="trilha" aria-label="Critérios">
          ${itens.map((it, i) => {
            const r = respostaDe(correcao, it);
            return html`
              <button type="button" class="trecho" data-tom=${tom(it.criterio, r)} data-tipo=${ehRoteiro(it.criterio) ? 'passo' : null}
                aria-current=${i === idx ? 'step' : null} aria-label=${`${it.criterio.nome}${r ? `: ${rotulo(it.criterio, r)}` : ''}`}
                onClick=${() => irPara(i)}></button>
            `;
          })}
          <button type="button" class="trecho fim" aria-current=${noResumo ? 'step' : null} aria-label="Resumo do aluno" onClick=${() => irPara(itens.length)}></button>
        </nav>
      </header>

      ${listaFixa && html`<aside class="av-lateral">${listaAlunos}</aside>`}
      ${comCodigo && html`
        <aside class="av-codigo-lateral" aria-label="Código do aluno">
          <${PainelCodigo} avaliacao=${id} aluno=${aluno.id} ead=${ead} aoMudar=${carregar} />
        </aside>
      `}

      <main class="av-palco">
        ${pacote.offline && html`<p class="av-aviso">Sem conexão: usando a cópia guardada neste aparelho.</p>`}
        ${avisos.length > 0 && html`
          <ul class="av-avisos" aria-label="Avisos sobre a submissão">
            ${avisos.map((a) => html`<li data-tipo=${a.tipo} title=${a.dica}>${a.texto}</li>`)}
          </ul>
        `}
        ${noResumo
          ? html`<${Resumo} key=${`${aluno.id}:resumo`} aluno=${aluno} avaliacao=${avaliacao} itens=${itens} correcao=${correcao} resultado=${resultado}
              desfazer=${podeDesfazer} aoIrPara=${irPara} aoComentarGeral=${(texto) => registrar({ comentario_geral: texto })} />`
          : html`
            <div class="pilha">
              ${idx < itens.length - 1 && html`<div class="ficha-debaixo" aria-hidden="true"></div>`}
              ${ficha}
            </div>
          `}
      </main>

      ${largo && html`
        <aside class="av-painel">
          ${passo
            ? html`<h2 class="av-painel-titulo">Passo de roteiro</h2><p class="suave">Não vale nota. <kbd>→</kbd> ou <kbd>Enter</kbd> para seguir.</p>`
            : criterio ? html`<h2 class="av-painel-titulo">${criterio.nome}</h2>${opcoes}` : html`<p class="suave">Escolha um critério do resumo para revisar.</p>`}
          <${Teclas} comSugestao=${!!ead?.pipeline} />
        </aside>
      `}

      <${Rodape} modo=${noResumo ? 'resumo' : passo ? 'roteiro' : 'criterio'} criterio=${criterio} largo=${largo}
        rotuloOpcoes=${criterio && (!temIntermediarios(criterio) ? 'Comentar' : criterio.escala.tipo === 'niveis' ? 'Níveis' : 'Nota')}
        aoMarcar=${marcar} aoOpcoes=${abrirOpcoes} aoSeguirPasso=${avancar}
        resumo=${{ concluida: resultado.concluida, proximo, hrefNotas, aoCompletar: completar, aoProximo: () => trocarAluno(proximo.id) }} />

      ${folhaOpcoes && html`
        <${Folha} titulo=${criterio.nome} espera=${folha === 'opcoes-toque' ? 350 : 0} aoFechar=${() => setFolha(null)}
          subtitulo=${temDescricao(criterio) && html`<${DescricaoCriterio} criterio=${criterio} />`}>${opcoes}<//>
      `}
      ${folhaCodigo && html`
        <${Folha} titulo=${aluno.nome} alta aoFechar=${() => setFolha(null)}>
          <${PainelCodigo} avaliacao=${id} aluno=${aluno.id} ead=${ead} avisos=${avisosDoCodigo(ead, aluno.id, alunos)} aoMudar=${carregar} />
        <//>
      `}
      ${folhaAlunos && html`
        <${Folha} titulo=${avaliacao.titulo} subtitulo=${`${plural(alunos.filter((a) => resultados[a.id].concluida).length, 'avaliado', 'avaliados')} de ${alunos.length}`} alta espera=${350} aoFechar=${() => setFolha(null)}>
          ${listaAlunos}
        <//>
      `}
    </div>
  `;
}
