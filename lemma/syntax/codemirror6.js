/**
 * CodeMirror 6 integration for Lemma syntax highlighting
 * 
 * Usage:
 *   import { lemmaLanguage } from './codemirror6.js';
 *   import { EditorView } from '@codemirror/view';
 *   import { EditorState } from '@codemirror/state';
 *   
 *   new EditorView({
 *     state: EditorState.create({
 *       doc: code,
 *       extensions: [lemmaLanguage]
 *     }),
 *     parent: document.body
 *   });
 */

import { StreamLanguage } from '@codemirror/language';
import { LanguageSupport } from '@codemirror/language';

const lemmaLanguage = StreamLanguage.define({
  name: 'lemma',
  token: (stream) => {
    // Comments
    if (stream.match(/"""/, false)) {
      stream.match(/"""/);
      let depth = 1;
      while (!stream.eol() && depth > 0) {
        if (stream.match(/"""/, false)) {
          stream.match(/"""/);
          depth--;
        } else {
          stream.next();
        }
      }
      return 'comment';
    }

    // Skip whitespace
    if (stream.eatSpace()) return null;

    // Keywords
    if (stream.match(/^(doc|fact|rule|type|unless|then|and|or|not|veto|in|is|from|with)\b/)) {
      return 'keyword';
    }

    // Built-in types
    if (stream.match(/^(boolean|scale|number|percent|ratio|text|date|time|duration)\b/)) {
      return 'typeName';
    }

    // Type override/constraint commands
    if (stream.match(/^(minimum|maximum|minimal|decimals|precision|unit|units|options|length|default|help)\b/)) {
      return 'keyword';
    }

    // Functions
    if (stream.match(/^(sqrt|sin|cos|tan|asin|acos|atan|log|exp|abs|floor|ceil|round)\b/)) {
      return 'function';
    }

    // Booleans
    if (stream.match(/^(true|false|yes|no|accept|reject)\b/)) {
      return 'atom';
    }

    // Strings
    if (stream.match(/^"/)) {
      let escaped = false;
      while (!stream.eol()) {
        if (!escaped && stream.next() === '"') {
          break;
        }
        escaped = !escaped && stream.peek() === '\\';
      }
      return 'string';
    }

    // Regex
    if (stream.match(/^\//)) {
      let escaped = false;
      while (!stream.eol()) {
        if (!escaped && stream.next() === '/') {
          break;
        }
        escaped = !escaped && stream.peek() === '\\';
      }
      return 'string.special';
    }

    // Numbers with separators
    if (stream.match(/^\d+([_,]\d+)*(\.\d+)?([eE][+-]?\d+)?/)) {
      if (stream.match(/^%/)) {
        return 'number';
      }
      if (stream.match(/^\s+(kilograms?|grams?|pounds?|ounces?|tons?|mass|weight|meters?|kilometers?|miles?|feet?|inches?|yards?|centimeters?|millimeters?|length|distance|liters?|gallons?|milliliters?|volume|hours?|minutes?|seconds?|days?|weeks?|months?|years?|duration|time|degrees?|celsius|fahrenheit|kelvin|temperature|watts?|kilowatts?|power|joules?|kilojoules?|energy|newtons?|force|pascals?|pressure|hertz?|frequency|bytes?|kilobytes?|megabytes?|gigabytes?|terabytes?|data_size|data)\b/)) {
        return 'number';
      }
      return 'number';
    }

    // Dates
    if (stream.match(/^\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?)?/)) {
      return 'number';
    }

    // Times
    if (stream.match(/^\d{2}:\d{2}(:\d{2})?([+-]\d{2}:\d{2}|Z)?/)) {
      return 'number';
    }

    // Type annotations
    if (stream.match(/^\[/)) {
      stream.match(/^[^\]]+\]/);
      return 'typeName';
    }

    // Arrow operator (type override chain)
    if (stream.match(/^->/)) {
      return 'operator';
    }

    // Comparison operators
    if (stream.match(/^(==|!=|>=|<=|>|<|is not)/)) {
      return 'operator';
    }

    // Arithmetic operators
    if (stream.match(/^[+\-*/%^=]/)) {
      return 'operator';
    }

    // Doc/module paths (e.g. lemma/std)
    if (stream.match(/^[a-zA-Z_][a-zA-Z0-9_.-]*(?:\/[a-zA-Z_][a-zA-Z0-9_.-]*)+/)) {
      return 'string.special';
    }

    // Rule references (with ?)
    if (stream.match(/^[a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)*\?/)) {
      return 'variableName';
    }

    // Fact references (with dots)
    if (stream.match(/^[a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z_][a-zA-Z0-9_]*)+/)) {
      return 'variableName.special';
    }

    // Identifiers
    if (stream.match(/^[a-zA-Z_][a-zA-Z0-9_]*/)) {
      return 'variable';
    }

    // Punctuation
    if (stream.match(/^[()[\]{}]/)) {
      return 'punctuation';
    }

    stream.next();
    return null;
  }
});

export const lemma = new LanguageSupport(lemmaLanguage);

