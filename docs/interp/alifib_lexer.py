"""Pygments lexer for the alifib language."""

from pygments.lexer import RegexLexer, words
from pygments.token import (
    Comment, Keyword, Name, Operator, Punctuation, Whitespace,
)


class AlifibLexer(RegexLexer):
    name = 'Alifib'
    aliases = ['alifib', 'ali']
    filenames = ['*.ali']

    tokens = {
        'root': [
            # Comments (* ... *), nestable
            (r'\(\*', Comment.Multiline, 'comment'),

            # Block markers: @Type or @Identifier
            (r'@', Keyword.Namespace, 'block_head'),

            # Keywords
            (words((
                'let', 'attach', 'along', 'include', 'as',
                'assert', 'total', 'map',
            ), suffix=r'\b'), Keyword),

            # Boundary keywords
            (r'\b(in|out)\b', Keyword.Pseudo),

            # Operators
            (r'<<=', Operator),
            (r'->', Operator),
            (r'=>', Operator),
            (r'::', Operator),
            (r'#\d+', Operator),

            # Hole
            (r'\?', Name.Builtin),

            # Punctuation
            (r'[=:.,]', Punctuation),
            (r'[{}\[\]()]', Punctuation),

            # Identifiers: capitalised names as Name.Class
            (r'[A-Z][A-Za-z0-9_]*', Name.Class),
            (r'[a-z_][A-Za-z0-9_]*', Name),

            # Whitespace
            (r'\s+', Whitespace),
        ],

        'block_head': [
            (r'Type\b', Keyword.Namespace, '#pop'),
            (r'[A-Z][A-Za-z0-9_.]*', Name.Class, '#pop'),
            (r'[a-z_][A-Za-z0-9_.]*', Name, '#pop'),
            (r'', Whitespace, '#pop'),  # fallback
        ],

        'comment': [
            (r'[^*()+]', Comment.Multiline),
            (r'\(\*', Comment.Multiline, '#push'),
            (r'\*\)', Comment.Multiline, '#pop'),
            (r'[*()+]', Comment.Multiline),
        ],
    }
