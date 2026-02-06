/**
 * Prism.js language definition for Lemma
 * 
 * Usage:
 *   <script src="prism.min.js"></script>
 *   <script src="lemma-prism.js"></script>
 *   <pre><code class="language-lemma">...</code></pre>
 */

(function(Prism) {
  Prism.languages.lemma = {
    'comment': {
      pattern: /"""[\s\S]*?"""/,
      greedy: true
    },
    'type-annotation': {
      pattern: /\[[^\]]+\]/,
      greedy: true,
      inside: {
        'punctuation': /^\[|\]$/,
        'arrow-operator': /->/,
        'conditional-keyword': /\b(from|with)\b/,
        'builtin-type': /\b(boolean|scale|number|percent|ratio|text|date|time|duration)\b/,
        'type-command': /\b(minimum|maximum|minimal|decimals|precision|unit|units|options|length|default|help)\b/,
        'number': /\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?%?\b/,
        'string': /"[^"]*"/,
        'doc-path': /\b[a-zA-Z_][a-zA-Z0-9_.-]*(?:\/[a-zA-Z_][a-zA-Z0-9_.-]*)+\b/,
        'identifier': /\b[a-zA-Z_][a-zA-Z0-9_]*\b/
      }
    },
    'declaration-keyword': /\b(doc|fact|rule|type)\b/,
    'conditional-keyword': /\b(unless|then|veto|and|or|not|in|from|with)\b/,
    'builtin-type': /\b(boolean|scale|number|percent|ratio|text|date|time|duration)\b/,
    'type-command': /\b(minimum|maximum|minimal|decimals|precision|unit|units|options|length|default|help)\b/,
    'arrow-operator': /->/,
    'comparison-operator': /==|!=|>=|<=|>|<|is not|\bis\b/,
    'arithmetic-operator': /[+\-*/%^=]/,
    'math-function': /\b(sqrt|sin|cos|tan|asin|acos|atan|log|exp|abs|floor|ceil|round)\b/,
    'boolean': /\b(true|false|yes|no|accept|reject)\b/,
    'percentage': {
      pattern: /\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?%/
    },
    'datetime': {
      pattern: /\b\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?)?/
    },
    'time': {
      pattern: /\b\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?/
    },
    'unit-value': {
      pattern: /\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?[ \t]+(?!(?:unless|then|veto|and|or|not|in|doc|fact|rule)\b)[a-zA-Z_][a-zA-Z0-9_]*/
    },
    'number': {
      pattern: /\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?/
    },
    'quoted-string': {
      pattern: /"[^"]*"/,
      greedy: true
    },
    'regex': {
      pattern: /\/[^\/]*\//,
      greedy: true
    },
    'doc-path': /\b[a-zA-Z_][a-zA-Z0-9_.-]*(?:\/[a-zA-Z_][a-zA-Z0-9_.-]*)+\b/,
    'reference': /\b[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*\??/
  };
})(typeof Prism !== 'undefined' ? Prism : {});
