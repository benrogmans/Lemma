/**
 * Monaco Editor language registration for Lemma.
 * Syntax highlighting is provided by the Rust LSP via semantic tokens.
 */

export const SEMANTIC_TOKEN_TYPES = [
  'keyword',
  'type',
  'function',
  'variable',
  'number',
  'string',
  'comment',
  'operator',
  'enumMember',
  'property',
];

export function registerLemmaLanguage(monaco) {
  monaco.languages.register({ id: 'lemma' });

  monaco.languages.setLanguageConfiguration('lemma', {
    comments: { blockComment: ['"""', '"""'] },
    brackets: [['(', ')'], ['[', ']']],
    autoClosingPairs: [
      { open: '(', close: ')' },
      { open: '[', close: ']' },
      { open: '"', close: '"' },
    ],
    surroundingPairs: [
      { open: '(', close: ')' },
      { open: '[', close: ']' },
      { open: '"', close: '"' },
    ],
  });
}

/**
 * Register LSP-backed providers for semantic tokens and formatting.
 * Call after the LspClient has been initialized and didOpen sent.
 *
 * @param {object} monaco     The monaco-editor API object
 * @param {object} lspClient  An initialized LspClient instance
 * @param {string} docUri     The document URI used with the LSP
 */
export function registerLspProviders(monaco, lspClient, docUri) {
  monaco.languages.registerDocumentSemanticTokensProvider('lemma', {
    getLegend() {
      return { tokenTypes: SEMANTIC_TOKEN_TYPES, tokenModifiers: [] };
    },
    async provideDocumentSemanticTokens() {
      const result = await lspClient.semanticTokensFull(docUri);
      if (!result || !result.data) return null;
      return { data: new Uint32Array(result.data) };
    },
    releaseDocumentSemanticTokens() {},
  });

  monaco.languages.registerDocumentFormattingEditProvider('lemma', {
    async provideDocumentFormattingEdits(model) {
      const edits = await lspClient.formatting(
        docUri,
        model.getOptions().tabSize,
        model.getOptions().insertSpaces,
      );
      if (!Array.isArray(edits)) return [];
      return edits.map(function (edit) {
        return {
          range: new monaco.Range(
            edit.range.start.line + 1, edit.range.start.character + 1,
            edit.range.end.line + 1, edit.range.end.character + 1,
          ),
          text: edit.newText,
        };
      });
    },
  });
}
