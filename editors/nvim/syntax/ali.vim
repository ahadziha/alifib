" Vim syntax file for alifib (.ali)
" Mirrors editors/vscode/syntaxes/ali.tmLanguage.json
"
" Ordering matters: Vim breaks ties between syntax items starting at the
" same column by giving priority to whichever was :syntax'd last. Regions
" (comments, strings) share opening delimiters with the punctuation class
" below, so they must be declared after it to win.

if exists("b:current_syntax")
  finish
endif

syn keyword aliKeywordControl attach along include assert for bar index
syn keyword aliKeywordOther   let as total map run in out on

" @Name / @Name.Path block-header annotations; @Type is its own keyword.
syn match   aliAnnotation     /@[a-zA-Z0-9_]\+\%(\.[a-zA-Z0-9_]\+\)*/
syn match   aliAnnotationType /@Type\>/

" identifier <<= ...  — the bound name is a type name.
syn match   aliTypeName /\<[a-zA-Z0-9_]\+\>\ze\s*<<=/

syn match   aliOperator /<<=\|::\|=>\|->\|=/
syn match   aliPaste    /#[0-9]\+\|#/
syn match   aliHole     /?/

" < and > are never bare punctuation in practice — they only occur inside
" <<=, ->, =>, or <name> interpolation, all matched above. A lone ":" is
" punctuation (type annotations); "::" (scope) is claimed by aliOperator, so
" a colon adjacent to another colon is excluded here.
syn match   aliPunctuation    /[.,;(){}\[\]]\|\(:\)\@<!:\(:\)\@!/
syn match   aliInterpolation  /<[A-Za-z_][A-Za-z0-9_]*>/

syn region  aliComment start="(\*" end="\*)" contains=aliComment

syn region  aliString start=+"+ skip=+\\.+ end=+"+ contains=aliStringEscape
syn region  aliString start=+'+ skip=+\\.+ end=+'+ contains=aliStringEscape
syn match   aliStringEscape +\\.+ contained

hi def link aliComment        Comment
hi def link aliString         String
hi def link aliStringEscape   SpecialChar
hi def link aliKeywordControl Conditional
hi def link aliKeywordOther   Keyword
hi def link aliAnnotation     Type
hi def link aliAnnotationType Keyword
hi def link aliTypeName       Type
hi def link aliOperator       Operator
hi def link aliPaste          Special
hi def link aliHole           Special
hi def link aliPunctuation    Delimiter
hi def link aliInterpolation  Identifier

let b:current_syntax = "ali"
