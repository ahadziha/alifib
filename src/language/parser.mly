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

let spans_of_nodes nodes =
  nodes |> List.filter_map (fun node -> node.span)

let spans_of_option_node = function
  | Some node -> spans_of_nodes [node]
  | None -> []

let nonempty_of head tail = { Nonempty.head = head; tail }

let nonempty_singleton x = Nonempty.singleton x

let append_nonempty = Nonempty.append

let spans_of_nonempty nonempty =
  spans_of_nodes (Nonempty.to_list nonempty)

let diagram_of_components components =
  make_node (spans_of_nonempty components) (Diagram_concat components)

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

%}

%token <Token.t> AT
%token <Token.t> KW_TYPE
%token <Token.t> KW_INCLUDE
%token <Token.t> KW_ATTACH
%token <Token.t> KW_ALONG
%token <Token.t> KW_ASSERT
%token <Token.t> KW_LET
%token <Token.t> KW_AS
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
%token <Token.t> EQUAL
%token <Token.t> HOLE
%token <Token.t> IDENT
%token <Token.t> NAT
%token <Token.t * Token.t> DOT_SELECTOR
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
      make_node (spans_of_nodes [namer]) (Diagram_namer_type namer.node)
    }
  | namer=morphism_namer {
      make_node (spans_of_nodes [namer]) (Morphism_namer_type namer.node)
    }
  | stmt=include_statement {
      let record, spans = stmt in
      make_node spans (Include_module record)
    }

cpx_instr:
  | generator=generator {
      make_node (spans_of_nodes [generator]) (Generator generator.node)
    }
  | namer=diagram_namer {
      make_node (spans_of_nodes [namer]) (Diagram_namer namer.node)
    }
  | namer=morphism_namer {
      make_node (spans_of_nodes [namer]) (Morphism_namer namer.node)
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
      make_node (spans_of_nodes [namer]) (Diagram_namer_local namer.node)
    }
  | namer=morphism_namer {
      make_node (spans_of_nodes [namer]) (Morphism_namer_local namer.node)
    }
  | stmt=assert_statement {
      let record, spans = stmt in
      make_node spans (Assert record)
    }

generator:
  | name_tok=IDENT {
      let name = identifier_of_token name_tok in
      make_node (spans_of_nodes [name]) { name; boundaries = None }
    }
  | name_tok=IDENT colon=COLON bounds=boundaries {
      let name = identifier_of_token name_tok in
      let bound_record, span_opt = bounds in
      let spans =
        let spans = spans_of_nodes [name] in
        let spans = spans @ [span_of_token colon] in
        match span_opt with
        | Some span ->
            spans @ [span]
        | None ->
            spans
      in
      make_node spans { name; boundaries = Some bound_record }
    }

boundaries:
  | left=diagram arrow=ARROW right=diagram {
      let record = { input = left; output = right } in
      let spans = span_of_token arrow :: spans_of_nodes [left; right] in
      (record, merge_spans spans)
    }

diagram_namer:
  | let_kw=KW_LET name_tok=IDENT eq=EQUAL expr=diagram {
      let name = identifier_of_token name_tok in
      let record = { diagram_name = name; constraints = None; diagram_def = expr } in
      let spans =
        let spans = span_of_token let_kw :: spans_of_nodes [name] in
        let spans = spans @ [span_of_token eq] in
        spans @ spans_of_nodes [expr]
      in
      make_node spans record
    }
  | let_kw=KW_LET name_tok=IDENT colon=COLON bounds=boundaries eq=EQUAL expr=diagram {
      let name = identifier_of_token name_tok in
      let bound_record, bound_span_opt = bounds in
      let record = { diagram_name = name; constraints = Some bound_record; diagram_def = expr } in
      let spans =
        let spans = span_of_token let_kw :: spans_of_nodes [name] in
        let spans = spans @ [span_of_token colon] in
        let spans =
          match bound_span_opt with
          | Some span ->
              spans @ [span]
          | None ->
              spans
        in
        let spans = spans @ [span_of_token eq] in
        spans @ spans_of_nodes [expr]
      in
      make_node spans record
    }

morphism_namer:
  | let_kw=KW_LET name_tok=IDENT of_shape=OF_SHAPE loc=location eq=EQUAL morph=morphism {
      let name = identifier_of_token name_tok in
      let record = { morphism_name = name; domain = loc; morphism_def = morph } in
      let spans =
        let spans = span_of_token let_kw :: spans_of_nodes [name] in
        let spans = spans @ [span_of_token of_shape] in
        let spans = spans @ spans_of_nodes [loc] in
        let spans = spans @ [span_of_token eq] in
        spans @ spans_of_nodes [morph]
      in
      make_node spans record
    }

include_statement:
  | kw=KW_INCLUDE loc=location as_part=include_alias_opt {
      let inclusion, alias_spans = as_part in
      let record = { inclusion; include_location = loc } in
      let spans =
        let spans = span_of_token kw :: spans_of_nodes [loc] in
        spans @ alias_spans
      in
      (record, spans)
    }

include_alias_opt:
  | { (None, []) }
  | kw=KW_AS name_tok=IDENT {
      let name = identifier_of_token name_tok in
      let spans = span_of_token kw :: spans_of_nodes [name] in
      (Some name, spans)
    }

attach_statement:
  | kw=KW_ATTACH name_tok=IDENT of_shape=OF_SHAPE loc=location along=attach_along_opt {
      let attachment = identifier_of_token name_tok in
      let along, along_spans = along in
      let record = { attachment; shape = loc; along } in
      let spans =
        let spans = span_of_token kw :: spans_of_nodes [attachment] in
        let spans = spans @ [span_of_token of_shape] in
        let spans = spans @ spans_of_nodes [loc] in
        spans @ along_spans
      in
      (record, spans)
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
  | loc=location { Some loc }
  | { None }

location:
  | head_tok=IDENT tail=location_tail {
      let head = identifier_of_token head_tok in
      let locations =
        List.fold_left
          (fun acc (_, ident_node) -> append_nonempty acc ident_node)
          (nonempty_singleton head)
          tail
      in
      let name_spans = spans_of_nodes (Nonempty.to_list locations) in
      let dot_spans = List.map (fun (dot_token, _) -> span_of_token dot_token) tail in
      let spans = name_spans @ dot_spans in
      make_node spans locations
    }

location_tail:
  | dot=DOT ident=IDENT tail=location_tail {
      let ident_node = identifier_of_token ident in
      (dot, ident_node) :: tail
    }
  | { [] }

diagram:
  | component=diagram_component {
      let concat = nonempty_singleton component in
      diagram_of_components concat
    }
  | compound=diagram_compound { compound }

diagram_compound:
  | concat=diagram_concat_compound {
      diagram_of_components concat
    }
  | paste=diagram_paste { paste }

diagram_concat_compound:
  | first=diagram_component second=diagram_component rest=diagram_component_list {
      nonempty_of first (second :: rest)
    }

diagram_component_list:
  | component=diagram_component rest=diagram_component_list { component :: rest }
  | { [] }

diagram_paste:
  | base=diagram_paste_term first=diagram_paste_segment rest=diagram_paste_segment_list {
      let base_diagram = diagram_of_components base in
      let segments = first :: rest in
      List.fold_left
        (fun acc (paste_token, dim_token, suffix_components) ->
          let dim = nat_of_token dim_token in
          let spans =
            let spans = spans_of_nodes [acc] in
            let spans = spans @ [span_of_token paste_token] in
            let spans = spans @ spans_of_nodes [dim] in
            spans @ spans_of_nonempty suffix_components
          in
          make_node spans
            (Diagram_paste
               {
                 paste_base = acc;
                 paste_dim = dim;
                 paste_suffix = suffix_components;
               }))
        base_diagram segments
    }

diagram_paste_term:
  | component=diagram_component { nonempty_singleton component }
  | concat=diagram_concat_compound { concat }

diagram_paste_segment:
  | paste=PASTE count=NAT term=diagram_paste_term { (paste, count, term) }

diagram_paste_segment_list:
  | segment=diagram_paste_segment rest=diagram_paste_segment_list { segment :: rest }
  | { [] }

diagram_component:
  | prefix=diagram_prefix core=diagram_simple suffix=diagram_suffix_opt {
      let morph, prefix_spans = prefix in
      let suffix_nodes = suffix in
      let spans =
        prefix_spans @ spans_of_nodes [core] @ spans_of_nodes suffix_nodes
      in
      let record =
        { prefix = Some morph; base = core; suffix = suffix_nodes }
      in
      make_node spans record
    }
  | core=diagram_simple suffix=diagram_suffix_opt {
      let suffix_nodes = suffix in
      let spans = spans_of_nodes [core] @ spans_of_nodes suffix_nodes in
      let record = { prefix = None; base = core; suffix = suffix_nodes } in
      make_node spans record
    }

diagram_prefix:
  | morph=morphism dot=DOT {
      let spans = spans_of_nodes [morph] @ [span_of_token dot] in
      (morph, spans)
    }

diagram_suffix_opt:
  | { [] }
  | suffix=diagram_suffix { suffix }

diagram_suffix:
  | selector=diagram_selector rest=diagram_selector_list { selector :: rest }

diagram_selector_list:
  | selector=diagram_selector rest=diagram_selector_list { selector :: rest }
  | { [] }

diagram_selector:
  | selector=DOT_SELECTOR {
      let dot, kw = selector in
      make_selector dot kw
    }

diagram_simple:
  | name_tok=IDENT {
      let name = identifier_of_token name_tok in
      make_node (spans_of_nodes [name]) (Diagram_name name)
    }
  | lpar=LPAREN inner=diagram_compound rpar=RPAREN {
      let spans =
        let spans = span_of_token lpar :: spans_of_nodes [inner] in
        spans @ [span_of_token rpar]
      in
      make_node spans (Diagram_parens inner)
    }
  | hole=HOLE {
      make_node [span_of_token hole] Diagram_hole
    }

morphism:
  | expr=morphism_expr {
      let value = nonempty_singleton expr in
      make_node (spans_of_nonempty value) value
    }
  | base=morphism dot=DOT expr=morphism_expr {
      let updated = append_nonempty base.node expr in
      let spans =
        let spans = spans_of_nodes [base] in
        let spans = spans @ [span_of_token dot] in
        spans @ spans_of_nodes [expr]
      in
      make_node spans updated
    }

morphism_expr:
  | builder=morphism_builder {
      make_node (spans_of_nodes [builder]) (Morphism_builder builder.node)
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

morphism_compound:
  | first=morphism_expr dot=DOT second=morphism_expr rest=morphism_compound_tail {
      let additional_exprs = List.map snd rest in
      let exprs = nonempty_of first (second :: additional_exprs) in
      let dot_spans =
        span_of_token dot :: List.map (fun (dot_token, _) -> span_of_token dot_token) rest
      in
      let spans = spans_of_nonempty exprs @ dot_spans in
      make_node spans exprs
    }

morphism_compound_tail:
  | dot=DOT expr=morphism_expr rest=morphism_compound_tail { (dot, expr) :: rest }
  | { [] }

morphism_builder:
  | simple=morphism_simple {
      make_node (spans_of_nodes [simple]) (Morphism_simple simple.node)
    }
  | lbr=LBRACKET block=morphism_block_opt rbr=RBRACKET {
      let spans =
        let spans = span_of_token lbr :: spans_of_nodes block in
        spans @ [span_of_token rbr]
      in
      make_node spans (Morphism_block { base = None; extension = block })
    }
  | base=morphism_simple lbr=LBRACKET block=morphism_block_opt rbr=RBRACKET {
      let spans =
        let spans = spans_of_nodes [base] in
        let spans = spans @ [span_of_token lbr] in
        let spans = spans @ spans_of_nodes block in
        spans @ [span_of_token rbr]
      in
      make_node spans (Morphism_block { base = Some base; extension = block })
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
  | lpar=LPAREN morph=morphism_compound rpar=RPAREN {
      let spans =
        let spans = span_of_token lpar :: spans_of_nodes [morph] in
        spans @ [span_of_token rpar]
      in
      make_node spans (Morphism_parens morph)
    }
