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
      greedy: true
    },
    'declaration-keyword': /\b(doc|fact|rule)\b/,
    'conditional-keyword': /\b(unless|then|veto|and|or|not|in)\b/,
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
    'reference': /\b[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*\??/
  };
})(typeof Prism !== 'undefined' ? Prism : {});
