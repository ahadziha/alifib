%{
open Ast

let span_of_token token = Token.span token

let merge_spans spans =
  match spans with
  | [] -> None
  | span :: rest -> Some (List.fold_left Positions.merge span rest)

let make_node spans value =
  match merge_spans spans with
  | Some span -> Ast.node ~span value
  | None -> Ast.node value

let option_to_list = function
  | Some x -> [x]
  | None -> []

let spans_of_nodes nodes =
  nodes |> List.filter_map (fun node -> node.span)

let spans_of_option_node = function
  | Some node -> spans_of_nodes [node]
  | None -> []

let identifier_of_token token =
  match Token.kind token with
  | Token.Identifier name -> Ast.node ~span:(span_of_token token) name
  | _ -> failwith "expected identifier"

let nat_of_token token =
  match Token.kind token with
  | Token.Nat digits -> Ast.node ~span:(span_of_token token) digits
  | _ -> failwith "expected natural number"

let make_selector dot kw =
  let value =
    match Token.kind kw with
    | Token.Keyword `In -> In
    | Token.Keyword `Out -> Out
    | _ -> failwith "expected boundary selector"
  in
  make_node [span_of_token dot; span_of_token kw] value

let spans_of_diagram_concat concat =
  spans_of_nodes (concat.concat_head :: concat.concat_tail)
%}

%token <Token.t> AT
%token <Token.t> KW_TYPE
%token <Token.t> KW_INCLUDE
%token <Token.t> KW_ATTACH
%token <Token.t> KW_ALONG
%token <Token.t> KW_ASSERT
%token <Token.t> KW_IN
%token <Token.t> KW_OUT
%token <Token.t> LBRACE
%token <Token.t> RBRACE
%token <Token.t> LBRACKET
%token <Token.t> RBRACKET
%token <Token.t> LPAREN
%token <Token.t> RPAREN
%token <Token.t> COMMA
%token <Token.t> DOT
%token <Token.t> PASTE
%token <Token.t> COLON
%token <Token.t> OF_SHAPE
%token <Token.t> MAPS_TO
%token <Token.t> ARROW
%token <Token.t> HAS_VALUE
%token <Token.t> DEFINITION
%token <Token.t> EQUAL
%token <Token.t> HOLE
%token <Token.t> IDENT
%token <Token.t> NAT
%token EOF

%start <Ast.program> program

%%

program:
  | blocks=blocks EOF { { blocks } }

blocks:
  | { [] }
  | block=block rest=blocks { block :: rest }

block:
  | at=AT ty=KW_TYPE instrs=type_block_opt {
      let type_block = { type_instructions = instrs } in
      let spans =
        span_of_token at :: span_of_token ty :: spans_of_nodes instrs
      in
      make_node spans (Type_block type_block)
    }
  | at=AT builder=cpx_builder locals=cpx_block_local_opt {
      let local_block = { location = builder; instructions = locals } in
      let spans =
        let spans = span_of_token at :: spans_of_nodes [builder] in
        spans @ spans_of_nodes locals
      in
      make_node spans (Builder_block local_block)
    }

type_block_opt:
  | { [] }
  | list=type_block_nonempty { list }

type_block_nonempty:
  | instr=cpx_instr_type rest=type_block_tail { instr :: rest }

type_block_tail:
  | { [] }
  | COMMA instr=cpx_instr_type rest=type_block_tail { instr :: rest }

cpx_block_opt:
  | { [] }
  | list=cpx_block { list }

cpx_block:
  | instr=cpx_instr rest=cpx_block_tail { instr :: rest }

cpx_block_tail:
  | { [] }
  | COMMA instr=cpx_instr rest=cpx_block_tail { instr :: rest }

cpx_block_local_opt:
  | { [] }
  | list=cpx_block_local { list }

cpx_block_local:
  | instr=cpx_instr_local rest=cpx_block_local_tail { instr :: rest }

cpx_block_local_tail:
  | { [] }
  | COMMA instr=cpx_instr_local rest=cpx_block_local_tail { instr :: rest }

cpx_instr_type:
  | generator=generator has_value=HAS_VALUE builder=cpx_builder {
      let record = { generator; value = builder } in
      let spans =
        let spans = spans_of_nodes [generator] in
        span_of_token has_value :: (spans @ spans_of_nodes [builder])
      in
      make_node spans (Generator_type record)
    }
  | namer=diagram_namer {
      make_node (spans_of_nodes [namer]) (Diagram_namer_type namer)
    }
  | namer=morphism_namer {
      make_node (spans_of_nodes [namer]) (Morphism_namer_type namer)
    }
  | stmt=include_statement {
      let record, spans = stmt in
      make_node spans (Include_module record)
    }

cpx_instr:
  | generator=generator {
      make_node (spans_of_nodes [generator]) (Generator generator)
    }
  | namer=diagram_namer {
      make_node (spans_of_nodes [namer]) (Diagram_namer namer)
    }
  | namer=morphism_namer {
      make_node (spans_of_nodes [namer]) (Morphism_namer namer)
    }
  | stmt=include_statement {
      let record, spans = stmt in
      make_node spans (Include record)
    }
  | stmt=attach_statement {
      let record, spans = stmt in
      make_node spans (Attach record)
    }

cpx_instr_local:
  | namer=diagram_namer {
      make_node (spans_of_nodes [namer]) (Diagram_namer_local namer)
    }
  | namer=morphism_namer {
      make_node (spans_of_nodes [namer]) (Morphism_namer_local namer)
    }
  | stmt=assert_statement {
      let record, spans = stmt in
      make_node spans (Assert record)
    }

generator:
  | name_tok=IDENT bounds=generator_boundaries_opt {
      let name = identifier_of_token name_tok in
      let boundaries, boundary_spans = bounds in
      let spans = spans_of_nodes [name] @ boundary_spans in
      make_node spans { name; boundaries }
    }

generator_boundaries_opt:
  | { (None, []) }
  | colon=COLON bounds=boundaries {
      let record, span_opt = bounds in
      let spans = span_of_token colon :: option_to_list span_opt in
      (Some record, spans)
    }

boundaries:
  | left=diagram arrow=ARROW right=diagram {
      let record = { input = left; output = right } in
      let spans = span_of_token arrow :: spans_of_nodes [left; right] in
      (record, merge_spans spans)
    }

diagram_namer:
  | name_tok=IDENT constraints=generator_boundaries_opt defn=DEFINITION expr=diagram {
      let name = identifier_of_token name_tok in
      let constraints, constraint_spans = constraints in
      let record = { diagram_name = name; constraints; diagram_def = expr } in
      let spans =
        let spans = span_of_token defn :: spans_of_nodes [name] in
        let spans = spans @ constraint_spans in
        spans @ spans_of_nodes [expr]
      in
      make_node spans record
    }

morphism_namer:
  | name_tok=IDENT of_shape=OF_SHAPE addr=address defn=DEFINITION morph=morphism {
      let name = identifier_of_token name_tok in
      let record = { morphism_name = name; domain = addr; morphism_def = morph } in
      let spans =
        let spans = spans_of_nodes [name] in
        let spans = spans @ [span_of_token of_shape] in
        let spans = spans @ spans_of_nodes [addr] in
        let spans = spans @ [span_of_token defn] in
        spans @ spans_of_nodes [morph]
      in
      make_node spans record
    }

include_statement:
  | kw=KW_INCLUDE alias=binding_alias_opt addr=address {
      let inclusion, alias_spans = alias in
      let record = { inclusion; address = addr } in
      let spans =
        let spans = span_of_token kw :: alias_spans in
        spans @ spans_of_nodes [addr]
      in
      (record, spans)
    }

attach_statement:
  | kw=KW_ATTACH alias=binding_alias_opt addr=address along=attach_along_opt {
      let alias, alias_spans = alias in
      let along, along_spans = along in
      let record = { alias; attachment = addr; along } in
      let spans =
        let spans = span_of_token kw :: alias_spans in
        let spans = spans @ spans_of_nodes [addr] in
        spans @ along_spans
      in
      (record, spans)
    }

binding_alias_opt:
  | { (None, []) }
  | name_tok=IDENT defn=DEFINITION {
      let name = identifier_of_token name_tok in
      let spans = spans_of_nodes [name] @ [span_of_token defn] in
      (Some name, spans)
    }

attach_along_opt:
  | { (None, []) }
  | kw=KW_ALONG morph=morphism {
      let spans = span_of_token kw :: spans_of_nodes [morph] in
      (Some morph, spans)
    }

assert_statement:
  | kw=KW_ASSERT left=diagram eq=EQUAL right=diagram {
      let record = { left; right } in
      let spans =
        let spans = span_of_token kw :: spans_of_nodes [left] in
        let spans = spans @ [span_of_token eq] in
        spans @ spans_of_nodes [right]
      in
      (record, spans)
    }

cpx_builder:
  | root=cpx_builder_root_opt lbrace=LBRACE body=cpx_block_opt rbrace=RBRACE {
      let record = { root; extension = body } in
      let spans =
        let spans = spans_of_option_node root in
        let spans = span_of_token lbrace :: spans in
        let spans = spans @ spans_of_nodes body in
        spans @ [span_of_token rbrace]
      in
      make_node spans record
    }

cpx_builder_root_opt:
  | addr=address { Some addr }
  | { None }

cpx_builder_named:
  | builder=cpx_builder { builder }

diagram:
  | concat=diagram_concat {
      let spans = spans_of_diagram_concat concat in
      make_node spans (Diagram_concat concat)
    }
  | base=diagram paste=PASTE count=NAT suffix=diagram_concat {
      let dim = nat_of_token count in
      let record = { paste_base = base; paste_dim = dim; paste_suffix = suffix } in
      let spans =
        let spans = spans_of_nodes [base] in
        let spans = spans @ [span_of_token paste] in
        let spans = spans @ spans_of_nodes [dim] in
        spans @ spans_of_diagram_concat suffix
      in
      make_node spans (Diagram_paste record)
    }

diagram_concat:
  | head=address tail=diagram_concat_tail {
      { concat_head = head; concat_tail = tail }
    }

diagram_concat_tail:
  | addr=address tail=diagram_concat_tail { addr :: tail }
  | { [] }

diagram_simple:
  | name_tok=IDENT {
      let name = identifier_of_token name_tok in
      make_node (spans_of_nodes [name]) (Diagram_name name)
    }
  | lpar=LPAREN inner=diagram rpar=RPAREN {
      let spans =
        let spans = span_of_token lpar :: spans_of_nodes [inner] in
        spans @ [span_of_token rpar]
      in
      make_node spans (Diagram_parens inner)
    }
  | hole=HOLE {
      make_node [span_of_token hole] Diagram_hole
    }

diagram_selector_list:
  | selector=diagram_selector rest=diagram_selector_list { selector :: rest }
  | { [] }

diagram_selector:
  | dot=DOT kw=diagram_selector_keyword { make_selector dot kw }

diagram_selector_keyword:
  | kw=KW_IN { kw }
  | kw=KW_OUT { kw }

diagram_bd:
  | base=diagram_simple selectors=diagram_selector_list {
      let record = { base; selectors } in
      let spans =
        let spans = spans_of_nodes [base] in
        spans @ spans_of_nodes selectors
      in
      make_node spans record
    }

address:
  | prefix=morphism dot=DOT target=diagram_bd {
      let record = { prefix = Some prefix; target } in
      let spans =
        let spans = spans_of_nodes [prefix] in
        let spans = spans @ [span_of_token dot] in
        spans @ spans_of_nodes [target]
      in
      make_node spans record
    }
  | target=diagram_bd {
      let record = { prefix = None; target } in
      make_node (spans_of_nodes [target]) record
    }

morphism:
  | expr=morphism_expr {
      let record = { head = expr; tail = [] } in
      make_node (spans_of_nodes [expr]) record
    }
  | base=morphism dot=DOT expr=morphism_expr {
      let base_record = base.node in
      let record = { head = base_record.head; tail = base_record.tail @ [expr] } in
      let spans =
        let spans = spans_of_nodes [base] in
        let spans = spans @ [span_of_token dot] in
        spans @ spans_of_nodes [expr]
      in
      make_node spans record
    }

morphism_expr:
  | builder=morphism_builder {
      let record = Morphism_builder builder.node in
      make_node (spans_of_nodes [builder]) record
    }
  | lpar=LPAREN builder=morphism_builder of_shape=OF_SHAPE domain=cpx_builder rpar=RPAREN {
      let record =
        Morphism_with_domain { builder; domain }
      in
      let spans =
        let spans = span_of_token lpar :: spans_of_nodes [builder] in
        let spans = spans @ [span_of_token of_shape] in
        let spans = spans @ spans_of_nodes [domain] in
        spans @ [span_of_token rpar]
      in
      make_node spans record
    }

morphism_builder:
  | simple=morphism_simple {
      make_node (spans_of_nodes [simple]) (Morphism_simple simple)
    }
  | lbr=LBRACKET block=morphism_block_opt rbr=RBRACKET {
      let record = { base = None; extension = block } in
      let spans =
        let spans = span_of_token lbr :: spans_of_nodes block in
        spans @ [span_of_token rbr]
      in
      make_node spans (Morphism_block record)
    }
  | base=morphism_simple lbr=LBRACKET block=morphism_block_opt rbr=RBRACKET {
      let record = { base = Some base; extension = block } in
      let spans =
        let spans = spans_of_nodes [base] in
        let spans = spans @ [span_of_token lbr] in
        let spans = spans @ spans_of_nodes block in
        spans @ [span_of_token rbr]
      in
      make_node spans (Morphism_block record)
    }

morphism_block_opt:
  | { [] }
  | block=morphism_block { block }

morphism_block:
  | instr=morphism_instr rest=morphism_block_tail { instr :: rest }

morphism_block_tail:
  | { [] }
  | COMMA instr=morphism_instr rest=morphism_block_tail { instr :: rest }

morphism_instr:
  | from=diagram maps=MAPS_TO into=diagram {
      let record = From_cell (from, into) in
      let spans =
        let spans = spans_of_nodes [from] in
        let spans = spans @ [span_of_token maps] in
        spans @ spans_of_nodes [into]
      in
      make_node spans record
    }
  | from=morphism maps=MAPS_TO into=morphism {
      let record = From_morphism (from, into) in
      let spans =
        let spans = spans_of_nodes [from] in
        let spans = spans @ [span_of_token maps] in
        spans @ spans_of_nodes [into]
      in
      make_node spans record
    }

morphism_simple:
  | name_tok=IDENT {
      let name = identifier_of_token name_tok in
      make_node (spans_of_nodes [name]) (Morphism_name name)
    }
  | lpar=LPAREN morph=morphism rpar=RPAREN {
      let spans =
        let spans = span_of_token lpar :: spans_of_nodes [morph] in
        spans @ [span_of_token rpar]
      in
      make_node spans (Morphism_parens morph)
    }
