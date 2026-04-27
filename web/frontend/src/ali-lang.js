import { StreamLanguage } from '@codemirror/language';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { Tag } from '@lezer/highlight';
import { tags } from '@lezer/highlight';

const KEYWORDS_CONTROL = new Set(['attach', 'along', 'include', 'assert']);
const KEYWORDS_OTHER   = new Set(['let', 'def', 'as', 'total', 'map']);
const KEYWORDS_BOUND   = new Set(['in', 'out']);

export const aliTags = {
  decoType: Tag.define(),
  decoId: Tag.define(),
  typeHead: Tag.define(),
  arrow: Tag.define(),
  hole: Tag.define(),
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

    if (stream.peek() === '@') {
      stream.next();
      stream.match(/^[A-Za-z_][A-Za-z0-9_.]*/);
      return stream.current() === '@Type' ? 'decoType' : 'decoId';
    }

    if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
      const word = stream.current();
      if (KEYWORDS_CONTROL.has(word) || KEYWORDS_OTHER.has(word)) return 'keyword';
      if (KEYWORDS_BOUND.has(word)) return 'modifier';
      const rest = stream.string.slice(stream.pos);
      if (/^[ \t]*<<=/.test(rest)) return 'typeHead';
      return null;
    }

    if (stream.match(/^[0-9]+/)) return 'number';

    if (stream.match('<<=')) return 'arrow';
    if (stream.match('->')) return 'arrow';
    if (stream.match('=>')) return 'arrow';
    if (stream.match('::')) return 'operator';

    const ch = stream.next();
    if (ch === '?') return 'hole';
    if (ch === '#' || ch === '=') return 'arrow';
    if (ch === '.' || ch === ',' || ch === ':' || ch === ';') return 'punctuation';
    if (ch === '(' || ch === ')' || ch === '[' || ch === ']' || ch === '{' || ch === '}') return 'punctuation';

    return null;
  },

  tokenTable: {
    decoType: aliTags.decoType,
    decoId: aliTags.decoId,
    typeHead: aliTags.typeHead,
    arrow: aliTags.arrow,
    hole: aliTags.hole,
  },
};

export const aliLanguage = StreamLanguage.define(aliMode);

export const aliHighlightStyle = HighlightStyle.define([
  { tag: tags.blockComment,  color: '#6b8a6b', fontStyle: 'italic' },
  { tag: tags.keyword,       color: '#c586c0', fontWeight: '600' },
  { tag: tags.modifier,      color: '#dcdcaa' },
  { tag: aliTags.decoType,   color: '#7c6af2', fontWeight: '600' },
  { tag: aliTags.decoId,     color: '#5fa8d3' },
  { tag: tags.operator,      color: '#d4d4d8' },
  { tag: aliTags.arrow,      color: '#fbbf24' },
  { tag: tags.punctuation,   color: '#71717a' },
  { tag: tags.number,        color: '#b5cea8' },
  { tag: aliTags.hole,       color: '#f87171', fontWeight: '600' },
  { tag: aliTags.typeHead,   color: '#5fa8d3', fontWeight: '600' },
]);

export function aliExtensions() {
  return [aliLanguage, syntaxHighlighting(aliHighlightStyle)];
}
