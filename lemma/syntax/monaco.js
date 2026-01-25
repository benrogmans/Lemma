/**
 * Monaco Editor integration for Lemma syntax highlighting
 * 
 * Usage:
 *   import { registerLemmaLanguage } from './monaco.js';
 *   registerLemmaLanguage(monaco);
 */

export function registerLemmaLanguage(monaco) {
  monaco.languages.register({ id: 'lemma' });

  monaco.languages.setMonarchTokensProvider('lemma', {
    tokenizer: {
      root: [
        // Comments
        [/"""[^"]*"""/, 'comment'],
        
        // Keywords
        [/\b(doc|fact|rule|type|unless|then|and|or|not|veto|in|is|from|with)\b/, 'keyword'],
        
        // Built-in types
        [/\b(boolean|scale|number|percent|ratio|text|date|time|duration)\b/, 'type'],
        
        // Type override/constraint commands
        [/\b(minimum|maximum|minimal|decimals|precision|unit|units|options|length|default|help)\b/, 'keyword'],
        
        // Functions
        [/\b(sqrt|sin|cos|tan|asin|acos|atan|log|exp|abs|floor|ceil|round)\b/, 'function'],
        
        // Booleans
        [/\b(true|false|yes|no|accept|reject)\b/, 'constant'],
        
        // Numbers with separators
        [/\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?\b/, 'number'],
        
        // Percentages
        [/\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?%\b/, 'number'],
        
        // Strings
        [/"[^"]*"/, 'string'],
        
        // Regex
        [/\/[^\/]*\//, 'string.regexp'],
        
        // Dates
        [/\b\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?)?\b/, 'constant'],
        
        // Times
        [/\b\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?\b/, 'constant'],
        
        // Units
        [/\b\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?\s+(kilograms?|grams?|pounds?|ounces?|tons?|mass|weight|meters?|kilometers?|miles?|feet?|inches?|yards?|centimeters?|millimeters?|length|distance|liters?|gallons?|milliliters?|volume|hours?|minutes?|seconds?|days?|weeks?|months?|years?|duration|time|degrees?|celsius|fahrenheit|kelvin|temperature|watts?|kilowatts?|power|joules?|kilojoules?|energy|newtons?|force|pascals?|pressure|hertz?|frequency|bytes?|kilobytes?|megabytes?|gigabytes?|terabytes?|data_size|data)\b/, 'number'],
        
        // Type annotations
        [/\[[^\]]+\]/, 'type'],

        // Doc/module paths (e.g. lemma/std)
        [/\b[a-zA-Z_][a-zA-Z0-9_.-]*(\/[a-zA-Z_][a-zA-Z0-9_.-]*)+\b/, 'string'],
        
        // Arrow operator (type override chain)
        [/->/, 'operator'],
        
        // Comparison operators
        [/==|!=|>=|<=|>|<|is not/, 'operator'],
        
        // Arithmetic operators
        [/[+\-*/%^=]/, 'operator'],
        
        // Rule references (with ?)
        [/\b\w+(\.[\w]+)*\?/, 'variable'],
        
        // Fact references (with dots)
        [/\b\w+(\.[\w]+)+/, 'variable.name'],
        
        // Identifiers
        [/\b[a-zA-Z_][a-zA-Z0-9_]*\b/, 'identifier'],
        
        // Whitespace
        [/\s+/, 'white']
      ]
    }
  });

  monaco.languages.setLanguageConfiguration('lemma', {
    comments: {
      blockComment: ['"""', '"""']
    },
    brackets: [
      ['(', ')'],
      ['[', ']']
    ],
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

