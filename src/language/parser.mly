%{
open Ast

module Pos = Positions
module Tk = Token
module Local = Id.Local

let mk ?span value = Ast.node ?span value

let token_span token = Some (Tk.span token)

let merge_span a b =
  match (a, b) with
  | Some a, Some b -> Some (Pos.merge a b)
  | Some a, None -> Some a
  | None, Some b -> Some b
  | None, None -> None

let merge_spans spans = List.fold_left merge_span None spans

let node_span node = node.span

let option_span = function
  | Some node -> node.span
  | None -> None

let list_span nodes = merge_spans (List.map node_span nodes)

let separated_nodes head tail =
  head :: List.map snd tail

let separated_span head tail =
  let tail_spans =
    List.concat
      (List.map
         (fun (token, node) -> [ token_span token; node_span node ])
         tail)
  in
  merge_spans (node_span head :: tail_spans)

let make_name token =
  let span = token_span token in
  match Tk.kind token with
  | Tk.Identifier ident ->
      mk ?span (Local.make ident)
  | Tk.Keyword `Type ->
      mk ?span (Local.make "Type")
  | _ ->
      failwith "expected identifier"

let make_nat token =
  let span = token_span token in
  match Tk.kind token with
  | Tk.Nat digits ->
      let value = int_of_string digits in
      mk ?span value
  | _ ->
      failwith "expected natural number"

let make_bd token =
  let span = token_span token in
  match Tk.kind token with
  | Tk.Keyword `In ->
      mk ?span In
  | Tk.Keyword `Out ->
      mk ?span Out
  | _ ->
      failwith "expected boundary keyword"

let make_hole token =
  let span = token_span token in
  mk ?span D_comp_hole
%}

%token <Token.t> AT
%token <Token.t> TYPE
%token <Token.t> INCLUDE
%token <Token.t> ATTACH
%token <Token.t> ALONG
%token <Token.t> MAP
%token <Token.t> ASSERT
%token <Token.t> IN
%token <Token.t> OUT
%token <Token.t> LET
%token <Token.t> AS
%token <Token.t> LBRACE
%token <Token.t> RBRACE
%token <Token.t> LBRACKET
%token <Token.t> RBRACKET
%token <Token.t> LPAREN
%token <Token.t> RPAREN
%token <Token.t> DOT
%token <Token.t> COMMA
%token <Token.t> PASTE
%token <Token.t> COLON
%token <Token.t> OF_SHAPE
%token <Token.t> MAPS_TO
%token <Token.t> ARROW
%token <Token.t> HAS_VALUE
%token <Token.t> EQUAL
%token <Token.t> HOLE
%token <Token.t> IDENT
%token <Token.t> NAT
%token EOF

%start <Ast.program> program

%%

program:
  | blocks=block_list EOF {
      let span = list_span blocks in
      mk ?span { program_blocks = blocks }
    }

block_list:
  | { [] }
  | block=block blocks=block_list { block :: blocks }

block:
  | at=AT type_kw=TYPE body_opt=c_block_type_opt {
      let span =
        merge_spans
          [ token_span at
          ; token_span type_kw
          ; option_span body_opt
          ]
      in
      mk ?span
        (Block_type { block_type_body = body_opt })
    }
  | at=AT complex=complex locals_opt=c_block_local_opt {
      let span =
        merge_spans
          [ token_span at
          ; node_span complex
          ; option_span locals_opt
          ]
      in
      mk ?span
        (Block_complex
           { block_complex_body = complex
           ; block_local_body = locals_opt })
    }

c_block_type_opt:
  | { None }
  | body=c_block_type { Some body }

c_block_type:
  | item=c_instr_type tail=c_block_type_tail {
      let items = separated_nodes item tail in
      let span = separated_span item tail in
      mk ?span items
    }

c_block_type_tail:
  | comma=COMMA item=c_instr_type tail=c_block_type_tail {
      (comma, item) :: tail
    }
  | { [] }

c_block_opt:
  | { None }
  | body=c_block { Some body }

c_block:
  | item=c_instr tail=c_block_tail {
      let items = separated_nodes item tail in
      let span = separated_span item tail in
      mk ?span items
    }

c_block_tail:
  | comma=COMMA item=c_instr tail=c_block_tail { (comma, item) :: tail }
  | { [] }

c_block_local_opt:
  | { None }
  | body=c_block_local { Some body }

c_block_local:
  | item=c_instr_local tail=c_block_local_tail {
      let items = separated_nodes item tail in
      let span = separated_span item tail in
      mk ?span items
    }

c_block_local_tail:
  | comma=COMMA item=c_instr_local tail=c_block_local_tail {
      (comma, item) :: tail
    }
  | { [] }

c_instr_type:
  | generator_type=generator_type {
      let span = generator_type.span in
      mk ?span (C_instr_type_generator generator_type)
    }
  | dnamer=dnamer {
      let span = dnamer.span in
      mk ?span (C_instr_type_dnamer dnamer)
    }
  | mnamer=mnamer {
      let span = mnamer.span in
      mk ?span (C_instr_type_mnamer mnamer)
    }
  | include_stmt=include_statement {
      let span = include_stmt.span in
      mk ?span (C_instr_type_include include_stmt)
    }

c_instr:
  | generator=generator {
      let span = generator.span in
      mk ?span (C_instr_generator generator)
    }
  | dnamer=dnamer {
      let span = dnamer.span in
      mk ?span (C_instr_dnamer dnamer)
    }
  | mnamer=mnamer {
      let span = mnamer.span in
      mk ?span (C_instr_mnamer mnamer)
    }
  | include_stmt=include_statement {
      let span = include_stmt.span in
      mk ?span (C_instr_include include_stmt)
    }
  | attach_stmt=attach_statement {
      let span = attach_stmt.span in
      mk ?span (C_instr_attach attach_stmt)
    }

c_instr_local:
  | dnamer=dnamer {
      let span = dnamer.span in
      mk ?span (C_instr_local_dnamer dnamer)
    }
  | mnamer=mnamer {
      let span = mnamer.span in
      mk ?span (C_instr_local_mnamer mnamer)
    }
  | assertion=assert_statement {
      let span = assertion.span in
      mk ?span (C_instr_local_assert assertion)
    }

generator_type:
  | generator=generator has_value=HAS_VALUE definition=complex {
      let span =
        merge_spans
          [ node_span generator
          ; token_span has_value
          ; node_span definition
          ]
      in
      mk ?span
        { generator_type_generator = generator
        ; generator_type_definition = definition
        }
    }

generator:
  | name=name boundaries_opt=generator_boundaries_opt {
      let boundaries, spans =
        match boundaries_opt with
        | None -> (None, [])
        | Some (colon, boundaries) ->
            ( Some boundaries
            , [ token_span colon
              ; node_span boundaries
              ] )
      in
      let span =
        merge_spans (node_span name :: spans)
      in
      mk ?span
        { generator_name = name
        ; generator_boundaries = boundaries
        }
    }

generator_boundaries_opt:
  | { None }
  | colon=COLON boundaries=boundaries { Some (colon, boundaries) }

complex:
  | address_opt=address_opt lbrace=LBRACE block_opt=c_block_opt rbrace=RBRACE {
      let span =
        merge_spans
          [ option_span address_opt
          ; token_span lbrace
          ; option_span block_opt
          ; token_span rbrace
          ]
      in
      mk ?span
        { complex_address = address_opt
        ; complex_block = block_opt
        }
    }
  | address=address {
      let span = node_span address in
      mk ?span
        { complex_address = Some address
        ; complex_block = None
        }
    }

address_opt:
  | { None }
  | address=address { Some address }

address:
  | first=name tail=address_tail {
      let segments = first :: List.map snd tail in
      let span =
        merge_spans
          (node_span first
          :: List.concat
               (List.map
                  (fun (dot, segment) ->
                    [ token_span dot
                    ; node_span segment ])
                  tail))
      in
      mk ?span segments
    }

address_tail:
  | dot=DOT segment=name tail=address_tail { (dot, segment) :: tail }
  | { [] }

morphism_opt:
  | { None }
  | morphism=morphism { Some morphism }

morphism:
  | comp=m_comp {
      let span = node_span comp in
      mk ?span (Morphism_single comp)
    }
  | left=morphism dot=DOT right=m_comp {
      let span =
        merge_spans
          [ node_span left
          ; token_span dot
          ; node_span right
          ]
      in
      mk ?span
        (Morphism_concat
           { morphism_left = left
           ; morphism_right = right
           })
    }

m_comp:
  | term=m_term {
      let span = term.span in
      mk ?span (M_comp_term term)
    }
  | name=name {
      let span = name.span in
      mk ?span (M_comp_name name)
    }

m_term:
  | lp=LPAREN map_kw=MAP ext=m_ext of_shape=OF_SHAPE target=complex rp=RPAREN {
      let span =
        merge_spans
          [ token_span lp
          ; token_span map_kw
          ; node_span ext
          ; token_span of_shape
          ; node_span target
          ; token_span rp
          ]
      in
      mk ?span
        { m_term_ext = ext
        ; m_term_target = target
        }
    }

m_ext:
  | prefix_opt=morphism_opt lbracket=LBRACKET block_opt=m_block_opt rbracket=RBRACKET {
      let span =
        merge_spans
          [ option_span prefix_opt
          ; token_span lbracket
          ; option_span block_opt
          ; token_span rbracket
          ]
      in
      mk ?span
        { m_ext_prefix = prefix_opt
        ; m_ext_block = block_opt
        }
    }

m_def:
  | morphism=morphism {
      let span = morphism.span in
      mk ?span (M_def_morphism morphism)
    }
  | ext=m_ext {
      let span = ext.span in
      mk ?span (M_def_ext ext)
    }

m_block_opt:
  | { None }
  | block=m_block { Some block }

m_block:
  | item=m_instr tail=m_block_tail {
      let items = separated_nodes item tail in
      let span = separated_span item tail in
      mk ?span items
    }

m_block_tail:
  | comma=COMMA item=m_instr tail=m_block_tail { (comma, item) :: tail }
  | { [] }

m_instr:
  | source=pasting maps=MAPS_TO target=pasting {
      let span =
        merge_spans
          [ node_span source
          ; token_span maps
          ; node_span target
          ]
      in
      mk ?span
        { m_instr_source = source
        ; m_instr_target = target
        }
    }

mnamer:
  | let_kw=LET name=name of_shape=OF_SHAPE address=address equal=EQUAL definition=m_def {
      let span =
        merge_spans
          [ token_span let_kw
          ; node_span name
          ; token_span of_shape
          ; node_span address
          ; token_span equal
          ; node_span definition
          ]
      in
      mk ?span
        { mnamer_name = name
        ; mnamer_address = address
        ; mnamer_definition = definition
        }
    }

dnamer:
  | let_kw=LET name=name boundaries_opt=dnamer_boundaries_opt equal=EQUAL body=diagram {
      let boundaries, spans =
        match boundaries_opt with
        | None -> (None, [])
        | Some (colon, boundaries) ->
            ( Some boundaries
            , [ token_span colon
              ; node_span boundaries
              ] )
      in
      let span =
        merge_spans
          ( [ token_span let_kw
            ; node_span name
            ]
          @ spans
          @ [ token_span equal
            ; node_span body
            ] )
      in
      mk ?span
        { dnamer_name = name
        ; dnamer_boundaries = boundaries
        ; dnamer_body = body
        }
    }

dnamer_boundaries_opt:
  | { None }
  | colon=COLON boundaries=boundaries { Some (colon, boundaries) }

diagram:
  | concat=d_concat {
      let span = concat.span in
      mk ?span (Diagram_single concat)
    }
  | left=diagram paste=PASTE nat=nat right=d_concat {
      let span =
        merge_spans
          [ node_span left
          ; token_span paste
          ; node_span nat
          ; node_span right
          ]
      in
      mk ?span
        (Diagram_paste
           { diagram_left = left
           ; diagram_nat = nat
           ; diagram_right = right
           })
    }

d_concat:
  | expr=d_expr {
      let span = expr.span in
      mk ?span (D_concat_single expr)
    }
  | left=d_concat right=d_expr {
      let span =
        merge_spans
          [ node_span left
          ; node_span right
          ]
      in
      mk ?span
        (D_concat_concat
           { d_concat_left = left
           ; d_concat_right = right
           })
    }

d_expr:
  | comp=d_comp {
      let span = comp.span in
      mk ?span (D_expr_single comp)
    }
  | left=d_expr dot=DOT right=d_comp {
      let span =
        merge_spans
          [ node_span left
          ; token_span dot
          ; node_span right
          ]
      in
      mk ?span
        (D_expr_dot
           { d_expr_left = left
           ; d_expr_right = right
           })
    }

d_comp:
  | term=m_term {
      let span = term.span in
      mk ?span (D_comp_mterm term)
    }
  | term=d_term {
      let span = term.span in
      mk ?span (D_comp_dterm term)
    }
  | name=name {
      let span = name.span in
      mk ?span (D_comp_name name)
    }
  | bd=bd {
      let span = bd.span in
      mk ?span (D_comp_bd bd)
    }
  | hole=HOLE { make_hole hole }

d_term:
  | lp=LPAREN diagram=diagram paste=PASTE nat=nat concat=d_concat rp=RPAREN {
      let span =
        merge_spans
          [ token_span lp
          ; node_span diagram
          ; token_span paste
          ; node_span nat
          ; node_span concat
          ; token_span rp
          ]
      in
      mk ?span
        (D_term_indexed
           { d_term_diagram = diagram
           ; d_term_nat = nat
           ; d_term_tail = concat
           })
    }
  | lp=LPAREN concat=d_concat expr=d_expr rp=RPAREN {
      let span =
        merge_spans
          [ token_span lp
          ; node_span concat
          ; node_span expr
          ; token_span rp
          ]
      in
      mk ?span
        (D_term_pair
           { d_term_concat = concat
           ; d_term_expr = expr
           })
    }

bd:
  | token=IN { make_bd token }
  | token=OUT { make_bd token }

pasting:
  | concat=concat {
      let span = concat.span in
      mk ?span (Pasting_single concat)
    }
  | left=pasting paste=PASTE nat=nat right=concat {
      let span =
        merge_spans
          [ node_span left
          ; token_span paste
          ; node_span nat
          ; node_span right
          ]
      in
      mk ?span
        (Pasting_paste
           { pasting_left = left
           ; pasting_nat = nat
           ; pasting_right = right
           })
    }

concat:
  | expr=expr {
      let span = expr.span in
      mk ?span (Concat_single expr)
    }
  | left=concat right=expr {
      let span =
        merge_spans
          [ node_span left
          ; node_span right
          ]
      in
      mk ?span
        (Concat_concat
           { concat_left = left
           ; concat_right = right
           })
    }

expr:
  | comp=d_comp {
      let span = comp.span in
      mk ?span (Expr_single comp)
    }
  | left=expr dot=DOT right=d_comp {
      let span =
        merge_spans
          [ node_span left
          ; token_span dot
          ; node_span right
          ]
      in
      mk ?span
        (Expr_dot
           { expr_left = left
           ; expr_right = right
           })
    }

boundaries:
  | source=diagram arrow=ARROW target=diagram {
      let span =
        merge_spans
          [ node_span source
          ; token_span arrow
          ; node_span target
          ]
      in
      mk ?span
        { boundaries_source = source
        ; boundaries_target = target
        }
    }

include_statement:
  | include_kw=INCLUDE address=address alias_opt=include_alias_opt {
      let alias, spans =
        match alias_opt with
        | None -> (None, [])
        | Some (as_kw, alias) ->
            ( Some alias
            , [ token_span as_kw
              ; node_span alias
              ] )
      in
      let span =
        merge_spans
          ( [ token_span include_kw
            ; node_span address
            ]
          @ spans )
      in
      mk ?span
        { include_address = address
        ; include_alias = alias
        }
    }

include_alias_opt:
  | { None }
  | as_kw=AS alias=name { Some (as_kw, alias) }

attach_statement:
  | attach_kw=ATTACH name=name of_shape=OF_SHAPE address=address along_opt=attach_along_opt {
      let along, spans =
        match along_opt with
        | None -> (None, [])
        | Some (along_kw, definition) ->
            ( Some definition
            , [ token_span along_kw
              ; node_span definition
              ] )
      in
      let span =
        merge_spans
          ( [ token_span attach_kw
            ; node_span name
            ; token_span of_shape
            ; node_span address
            ]
          @ spans )
      in
      mk ?span
        { attach_name = name
        ; attach_address = address
        ; attach_along = along
        }
    }

attach_along_opt:
  | { None }
  | along_kw=ALONG definition=m_def { Some (along_kw, definition) }

assert_statement:
  | assert_kw=ASSERT left=pasting equal=EQUAL right=pasting {
      let span =
        merge_spans
          [ token_span assert_kw
          ; node_span left
          ; token_span equal
          ; node_span right
          ]
      in
      mk ?span
        { assert_left = left
        ; assert_right = right
        }
    }

name:
  | token=IDENT { make_name token }

nat:
  | token=NAT { make_nat token }
