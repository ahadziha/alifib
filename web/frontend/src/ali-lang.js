import { StreamLanguage } from '@codemirror/language';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { Tag } from '@lezer/highlight';
import { tags } from '@lezer/highlight';

const KEYWORDS_CONTROL = new Set(['attach', 'along', 'include', 'assert', 'for', 'bar', 'index']);
const KEYWORDS_OTHER   = new Set(['let', 'as', 'total', 'map', 'run', 'in', 'out', 'on']);

export const aliTags = {
  decoType: Tag.define(),
  decoId: Tag.define(),
  typeHead: Tag.define(),
  arrow: Tag.define(),
  paste: Tag.define(),
  hole: Tag.define(),
  interpolation: Tag.define(),
};

const aliMode = {
  startState() {
    return { commentDepth: 0 };
  },

  token(stream, state) {
    if (state.commentDepth > 0) {
      while (!stream.eol()) {
        if (stream.match('(*')) { state.commentDepth++; }
        else if (stream.match('*)')) {
          state.commentDepth--;
          if (state.commentDepth === 0) return 'blockComment';
        }
        else { stream.next(); }
      }
      return 'blockComment';
    }

    if (stream.match('(*')) {
      state.commentDepth = 1;
      while (!stream.eol()) {
        if (stream.match('(*')) { state.commentDepth++; }
        else if (stream.match('*)')) {
          state.commentDepth--;
          if (state.commentDepth === 0) return 'blockComment';
        }
        else { stream.next(); }
      }
      return 'blockComment';
    }

    const quote = stream.peek();
    if (quote === '"' || quote === "'") {
      stream.next();
      while (!stream.eol()) {
        const c = stream.next();
        if (c === '\\') { stream.next(); }
        else if (c === quote) { break; }
      }
      return 'string';
    }

    if (stream.peek() === '@') {
      stream.next();
      stream.match(/^[A-Za-z0-9_][A-Za-z0-9_.]*/);
      return stream.current() === '@Type' ? 'decoType' : 'decoId';
    }

    if (stream.match(/^<[A-Za-z_][A-Za-z0-9_]*>/)) return 'interpolation';

    // Identifiers may start with a digit (e.g. `2Morphism`, `0Simplex`); only an
    // all-digit token is a number. Matches the lexer, which lexes `[A-Za-z0-9_]+`
    // as one identifier unless every character is a digit.
    if (stream.match(/^[A-Za-z0-9_]+/)) {
      const word = stream.current();
      if (/^[0-9]+$/.test(word)) return null;
      if (KEYWORDS_CONTROL.has(word) || KEYWORDS_OTHER.has(word)) return 'keyword';
      const rest = stream.string.slice(stream.pos);
      if (/^[ \t]*<<=/.test(rest)) return 'typeHead';
      return null;
    }

    if (stream.match(/^#[0-9]+/)) return 'paste';

    if (stream.match('<<=')) return 'arrow';
    if (stream.match('->')) return 'arrow';
    if (stream.match('=>')) return 'arrow';
    if (stream.match('::')) return 'arrow';

    const ch = stream.next();
    if (ch === '?') return 'hole';
    if (ch === '#' || ch === '=') return 'arrow';
    if (ch === '.' || ch === ',' || ch === ':' || ch === ';') return 'punctuation';
    if (ch === '(' || ch === ')' || ch === '[' || ch === ']' || ch === '{' || ch === '}' || ch === '<' || ch === '>') return 'punctuation';

    return null;
  },

  tokenTable: {
    decoType: aliTags.decoType,
    decoId: aliTags.decoId,
    typeHead: aliTags.typeHead,
    arrow: aliTags.arrow,
    paste: aliTags.paste,
    hole: aliTags.hole,
    interpolation: aliTags.interpolation,
  },
};

export const aliLanguage = StreamLanguage.define(aliMode);

export const aliDarkHighlight = HighlightStyle.define([
  { tag: tags.blockComment,  color: '#6b8a6b', fontStyle: 'italic' },
  { tag: tags.string,        color: '#ce9178' },
  { tag: tags.keyword,       color: '#c586c0', fontWeight: '600' },
  { tag: aliTags.decoType,   color: '#7c6af2', fontWeight: '600' },
  { tag: aliTags.decoId,     color: '#5fa8d3' },
  { tag: aliTags.arrow,      color: '#fbbf24' },
  { tag: aliTags.paste,      color: '#71717a' },
  { tag: tags.punctuation,   color: '#71717a' },
  { tag: aliTags.hole,          color: '#f87171', fontWeight: '600' },
  { tag: aliTags.typeHead,      color: '#5fa8d3', fontWeight: '600' },
  { tag: aliTags.interpolation, color: '#6aaa9a' },
]);

export const aliLightHighlight = HighlightStyle.define([
  { tag: tags.blockComment,  color: '#6a8a58', fontStyle: 'italic' },
  { tag: tags.string,        color: '#a85d2a' },
  { tag: tags.keyword,       color: '#a03870', fontWeight: '600' },
  { tag: aliTags.decoType,   color: '#1a7a7a', fontWeight: '600' },
  { tag: aliTags.decoId,     color: '#1a6060' },
  { tag: aliTags.arrow,      color: '#b87000' },
  { tag: aliTags.paste,      color: '#9a9488' },
  { tag: tags.punctuation,   color: '#9a9488' },
  { tag: aliTags.hole,          color: '#c83030', fontWeight: '600' },
  { tag: aliTags.typeHead,      color: '#1a6060', fontWeight: '600' },
  { tag: aliTags.interpolation, color: '#3a7a6a' },
]);

export function aliExtensions(dark = true) {
  const hl = dark ? aliDarkHighlight : aliLightHighlight;
  return [aliLanguage, syntaxHighlighting(hl)];
}
