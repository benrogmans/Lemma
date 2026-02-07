/**
 * Monaco Editor integration for Lemma syntax highlighting.
 * Used by the WASM playground (index.html).
 */
export function registerLemmaLanguage(monaco) {
  monaco.languages.register({ id: 'lemma' });

  monaco.languages.setMonarchTokensProvider('lemma', {
    tokenizer: {
      root: [
        [/"""[^"]*"""/, 'comment'],
        [/\b(doc|fact|rule|type|unless|then|and|or|not|veto|in|is|from|with)\b/, 'keyword'],
        [/\b(boolean|scale|number|percent|ratio|text|date|time|duration)\b/, 'type'],
        [/\b(minimum|maximum|minimal|decimals|precision|unit|units|options|length|default|help)\b/, 'keyword'],
        [/\b(sqrt|sin|cos|tan|asin|acos|atan|log|exp|abs|floor|ceil|round)\b/, 'function'],
        [/\b(true|false|yes|no|accept|reject)\b/, 'constant'],
        [/\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?\b/, 'number'],
        [/\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?%\b/, 'number'],
        [/"[^"]*"/, 'string'],
        [/\/[^\/]*\//, 'string.regexp'],
        [/\b\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?)?\b/, 'constant'],
        [/\b\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?\b/, 'constant'],
        [/\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?\s+(kilograms?|grams?|pounds?|ounces?|tons?|mass|weight|meters?|kilometers?|miles?|feet?|inches?|yards?|centimeters?|millimeters?|length|distance|liters?|gallons?|milliliters?|volume|hours?|minutes?|seconds?|days?|weeks?|months?|years?|duration|time|degrees?|celsius|fahrenheit|kelvin|temperature|watts?|kilowatts?|power|joules?|kilojoules?|energy|newtons?|force|pascals?|pressure|hertz?|frequency|bytes?|kilobytes?|megabytes?|gigabytes?|terabytes?|data_size|data)\b/, 'number'],
        [/\[[^\]]+\]/, 'type'],
        [/\b[a-zA-Z_][a-zA-Z0-9_.-]*(\/[a-zA-Z_][a-zA-Z0-9_.-]*)+\b/, 'string'],
        [/->/, 'operator'],
        [/==|!=|>=|<=|>|<|is not/, 'operator'],
        [/[+\-*/%^=]/, 'operator'],
        [/\b\w+(\.[\w]+)*\?/, 'variable'],
        [/\b\w+(\.[\w]+)+/, 'variable.name'],
        [/\b[a-zA-Z_][a-zA-Z0-9_]*\b/, 'identifier'],
        [/\s+/, 'white']
      ]
    }
  });

  monaco.languages.setLanguageConfiguration('lemma', {
    comments: { blockComment: ['"""', '"""'] },
    brackets: [['(', ')'], ['[', ']']],
    autoClosingPairs: [
      { open: '(', close: ')' },
      { open: '[', close: ']' },
      { open: '"', close: '"' },
      { open: '/', close: '/' }
    ],
    surroundingPairs: [
      { open: '(', close: ')' },
      { open: '[', close: ']' },
      { open: '"', close: '"' },
      { open: '/', close: '/' }
    ],
    indentationRules: {
      increaseIndentPattern: /^\s*(rule|fact|type|unless).*$/,
      decreaseIndentPattern: /^\s*(then|unless).*$/
    }
  });
}
