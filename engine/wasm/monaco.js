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
