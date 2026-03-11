from setuptools import setup

setup(
    name='alifib-pygments',
    version='0.1',
    py_modules=['alifib_lexer'],
    entry_points={
        'pygments.lexers': [
            'alifib=alifib_lexer:AlifibLexer',
        ],
    },
)
