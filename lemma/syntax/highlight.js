/**
 * Highlight.js language definition for Lemma
 * 
 * Usage:
 *   <script src="highlight.min.js"></script>
 *   <script src="lemma-highlight.js"></script>
 *   <pre><code class="language-lemma">...</code></pre>
 */

(function(hljs) {
  hljs.registerLanguage('lemma', function(hljs) {
    return {
      name: 'Lemma',
      aliases: ['lemma'],
      case_insensitive: false,
      contains: [
        {
          className: 'comment',
          begin: /"""/,
          end: /"""/
        },
        {
          className: 'type',
          begin: /\[/,
          end: /\]/
        },
        {
          className: 'keyword',
          begin: /\b(doc|fact|rule)\b/
        },
        {
          className: 'built_in',
          begin: /\b(unless|then|veto|and|or|not|in)\b/
        },
        {
          className: 'symbol',
          begin: /==|!=|>=|<=|>|<|is not|\bis\b/
        },
        {
          className: 'symbol',
          begin: /[+\-*/%^=]/
        },
        {
          className: 'symbol',
          begin: /\b(sqrt|sin|cos|tan|asin|acos|atan|log|exp|abs|floor|ceil|round)\b/
        },
        {
          className: 'literal',
          begin: /\b(true|false|yes|no|accept|reject)\b/
        },
        {
          className: 'number',
          begin: /\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?%/
        },
        {
          className: 'number',
          begin: /\b\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?)?/
        },
        {
          className: 'number',
          begin: /\b\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?/
        },
        {
          className: 'number',
          begin: /\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?[ \t]+(?!(?:unless|then|veto|and|or|not|in|doc|fact|rule)\b)[a-zA-Z_][a-zA-Z0-9_]*/
        },
        {
          className: 'number',
          begin: /\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?/
        },
        {
          className: 'string',
          begin: /"/,
          end: /"/,
          contains: [hljs.BACKSLASH_ESCAPE]
        },
        {
          className: 'string',
          begin: /\//,
          end: /\//,
          contains: [hljs.BACKSLASH_ESCAPE]
        },
        {
          className: 'variable',
          begin: /\b[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*\?/
        },
        {
          className: 'variable',
          begin: /\?/
        },
        {
          className: 'variable',
          begin: /\b[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*/,
          relevance: 0
        }
      ]
    };
  });
})(typeof hljs !== 'undefined' ? hljs : {});
