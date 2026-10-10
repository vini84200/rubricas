// Editor de código: CodeMirror 6 com os erros do Ruff (WASM) sublinhados.
// Os dois ficam em vendor/sob-demanda e só são baixados quando alguém abre o
// editor: quem só avalia no celular não paga os 11 MB do Ruff.

let codemirror = null;
let ruff = null;

export function carregarEditor() {
  codemirror ||= import('../vendor/sob-demanda/codemirror.js');
  return codemirror;
}

function carregarRuff() {
  ruff ||= (async () => {
    const m = await import('../vendor/sob-demanda/ruff_wasm.js');
    await m.default({ module_or_path: new URL('../vendor/sob-demanda/ruff_wasm_bg.wasm', import.meta.url) });
    // E9: erros de sintaxe e de compilação; F: nomes não definidos, imports sem uso…
    return new m.Workspace({ lint: { select: ['E9', 'F'] } }, m.PositionEncoding.Utf16);
  })();
  // Falhou (sem rede, navegador sem WebAssembly)? Tenta de novo na próxima vez.
  ruff.catch(() => {
    ruff = null;
  });
  return ruff;
}

/** Erro de verdade: sintaxe e nomes que não existem. O resto (import sem uso…) é aviso. */
function gravidade(codigo) {
  return codigo === 'invalid-syntax' || /^(E9|F8[0-2])/.test(codigo || '') ? 'error' : 'warning';
}

/** Linha e coluna do Ruff (contando de 1) → posição no documento. */
function posicao(doc, { row, column }) {
  if (row > doc.lines) return doc.length;
  const linha = doc.line(Math.max(1, row));
  return Math.min(linha.to, linha.from + Math.max(0, column - 1));
}

// As cores vêm de estilo.css: os tokens --codigo-* e, no destaque, as mesmas
// classes `.token` do Prism que colorem o código fora do editor.
const fracao = (cor, pc) => `color-mix(in srgb, var(${cor}) ${pc}%, transparent)`;

function tema(cm) {
  const { EditorView, HighlightStyle, tags: t } = cm;
  const visual = EditorView.theme(
    {
      '&': { height: '100%', backgroundColor: 'var(--codigo-fundo)', color: 'var(--codigo-texto)', fontSize: 'var(--editor-fonte, 14px)' },
      '.cm-scroller': { fontFamily: "ui-monospace, 'SFMono-Regular', Menlo, Consolas, monospace", lineHeight: '1.5' },
      '.cm-content': { caretColor: '#fff', padding: '10px 0' },
      '.cm-cursor, .cm-dropCursor': { borderLeftColor: '#fff', borderLeftWidth: '2px' },
      '&.cm-focused': { outline: '2px solid var(--giz)', outlineOffset: '-2px' },
      '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': { backgroundColor: fracao('--giz', 30) },
      '.cm-gutters': { backgroundColor: 'var(--codigo-fundo)', color: 'var(--codigo-apagado)', border: 0, borderRight: '1px solid var(--codigo-divisa)' },
      '.cm-activeLine': { backgroundColor: fracao('--codigo-texto', 5) },
      '.cm-activeLineGutter': { backgroundColor: fracao('--codigo-texto', 8), color: 'var(--codigo-texto)' },
      '.cm-matchingBracket': { backgroundColor: fracao('--sim-giz', 25), outline: `1px solid ${fracao('--sim-giz', 60)}` },
      '.cm-searchMatch': { backgroundColor: fracao('--giz', 25) },
      '.cm-panels': { backgroundColor: 'var(--codigo-barra)', color: 'var(--codigo-texto)' },
      '.cm-tooltip': { backgroundColor: 'var(--ficha)', color: 'var(--tinta)', border: '1px solid var(--linha)', borderRadius: '8px' },
      '.cm-diagnostic': { padding: '6px 10px', fontFamily: "'Atkinson Hyperlegible', system-ui, sans-serif" },
      '.cm-diagnostic-error': { borderLeftColor: 'var(--nao)' },
      '.cm-diagnostic-warning': { borderLeftColor: 'var(--meio)' },
      '.cm-lint-marker': { width: '0.9em', height: '0.9em' },
    },
    { dark: true },
  );
  const cores = HighlightStyle.define([
    { tag: t.comment, class: 'token comment' },
    { tag: [t.string, t.special(t.string)], class: 'token string' },
    { tag: [t.keyword, t.controlKeyword, t.definitionKeyword, t.moduleKeyword, t.operatorKeyword], class: 'token keyword' },
    { tag: [t.function(t.variableName), t.function(t.definition(t.variableName))], class: 'token function' },
    { tag: [t.standard(t.variableName), t.className, t.definition(t.className)], class: 'token builtin' },
    { tag: [t.number, t.bool, t.null], class: 'token number' },
    { tag: [t.operator, t.punctuation, t.bracket], class: 'token operator' },
    { tag: t.invalid, class: 'token invalido' },
  ]);
  return [visual, cm.syntaxHighlighting(cores)];
}

/**
 * Monta o editor em `onde`. `aoMudar(texto)` a cada edição, `aoSalvar()` com
 * Ctrl+S, `aoSair()` com Esc e `aoVerificar` com `{ erros, avisos }` a cada
 * verificação do Ruff (ou `{ carregando }` / `{ indisponivel }`).
 */
export async function criarEditor(onde, { texto, python, aoMudar, aoSalvar, aoSair, aoVerificar }) {
  const cm = await carregarEditor();
  const { EditorView, EditorState, keymap } = cm;
  const verificar = cm.linter(
    async (view) => {
      let ws;
      try {
        ws = await carregarRuff();
      } catch {
        aoVerificar({ indisponivel: true });
        return [];
      }
      const doc = view.state.doc;
      let achados;
      try {
        achados = ws.check(doc.toString());
      } catch {
        return [];
      }
      const diagnosticos = achados.map((d) => {
        let from = posicao(doc, d.start_location);
        let to = posicao(doc, d.end_location);
        // Erro no fim da linha ("faltou o :") cobre só a quebra de linha, que não
        // aparece: sublinha o último caractere dela, que é onde o olho procura.
        const linha = doc.lineAt(from);
        if (from === linha.to && from > linha.from) [from, to] = [from - 1, from];
        else if (to <= from) to = Math.min(linha.to, from + 1);
        const severity = gravidade(d.code);
        const message = d.code && d.code !== 'invalid-syntax' ? `${d.message} (${d.code})` : d.message;
        return { from, to, severity, message };
      });
      aoVerificar({
        erros: diagnosticos.filter((d) => d.severity === 'error').length,
        avisos: diagnosticos.filter((d) => d.severity === 'warning').length,
      });
      return diagnosticos;
    },
    { delay: 300 },
  );
  const view = new EditorView({
    parent: onde,
    state: EditorState.create({
      doc: texto,
      extensions: [
        cm.lineNumbers(),
        cm.highlightActiveLineGutter(),
        cm.highlightSpecialChars(),
        cm.history(),
        cm.drawSelection(),
        cm.indentUnit.of('    '),
        EditorState.tabSize.of(4),
        cm.indentOnInput(),
        cm.bracketMatching(),
        cm.closeBrackets(),
        cm.highlightActiveLine(),
        cm.highlightSelectionMatches(),
        ...(python ? [cm.python(), verificar, cm.lintGutter()] : []),
        ...tema(cm),
        keymap.of([
          { key: 'Mod-s', preventDefault: true, run: () => (aoSalvar(), true) },
          { key: 'Escape', run: () => (aoSair(), true) },
          // Tab e Shift+Tab indentam e desindentam todas as linhas selecionadas.
          cm.indentWithTab,
          ...cm.closeBracketsKeymap,
          ...cm.defaultKeymap,
          ...cm.historyKeymap,
          ...cm.searchKeymap,
        ]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) aoMudar(u.state.doc.toString());
        }),
      ],
    }),
  });
  if (python) aoVerificar({ carregando: true });
  view.focus();
  return view;
}
