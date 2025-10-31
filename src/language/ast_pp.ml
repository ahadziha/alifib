open Format
open Ast

let list_sep fmt () = fprintf fmt ";@ "

let pp_list printer fmt values =
  fprintf fmt "@[<hv 1>[%a]@]" (pp_print_list ~pp_sep:list_sep printer) values

let pp_option printer fmt = function
  | None ->
      fprintf fmt "None"
  | Some value ->
      fprintf fmt "Some(@[%a@])" printer value

let pp_field name printer fmt value =
  fprintf fmt "@[%s = %a@]" name printer value

let pp_name fmt name = fprintf fmt "%s" (Id.Local.to_string name.value)
let pp_nat fmt nat = fprintf fmt "%d" nat.value

let pp_bd fmt bd =
  match bd.value with In -> fprintf fmt "in" | Out -> fprintf fmt "out"

let pp_address fmt address =
  let segments = address.value in
  fprintf fmt "@[<h>%a@]"
    (pp_print_list ~pp_sep:(fun fmt () -> fprintf fmt ".") pp_name)
    segments

let rec pp_program fmt program =
  fprintf fmt "@[<v 2>(Program@,%a)@]"
    (pp_field "blocks" (pp_list block))
    program.value.program_blocks

and block fmt block =
  match block.value with
  | Block_type { block_type_body } ->
      fprintf fmt "@[<v 2>(Block_type@,%a)@]"
        (pp_field "body" (pp_option c_block_type))
        block_type_body
  | Block_complex { block_complex_body; block_local_body } ->
      fprintf fmt "@[<v 2>(Block_complex@,%a@,%a)@]"
        (pp_field "complex" complex)
        block_complex_body
        (pp_field "locals" (pp_option c_block_local))
        block_local_body

and complex fmt complex =
  let { complex_address; complex_block } = complex.value in
  fprintf fmt "@[<v 2>(Complex@,%a@,%a)@]"
    (pp_field "address" (pp_option address))
    complex_address
    (pp_field "block" (pp_option c_block))
    complex_block

and c_block_type fmt block = pp_list c_instr_type fmt block.value
and c_block fmt block = pp_list c_instr fmt block.value
and c_block_local fmt block = pp_list c_instr_local fmt block.value

and c_instr_type fmt instr =
  match instr.value with
  | C_instr_type_generator gt ->
      fprintf fmt "@[<v 2>(Generator_type@ %a)@]" generator_type gt
  | C_instr_type_dnamer dn ->
      fprintf fmt "@[<v 2>(Diagram_namer@ %a)@]" dnamer dn
  | C_instr_type_mnamer mn ->
      fprintf fmt "@[<v 2>(Morphism_namer@ %a)@]" mnamer mn
  | C_instr_type_include inc ->
      fprintf fmt "@[<v 2>(Include@ %a)@]" include_statement inc

and c_instr fmt instr =
  match instr.value with
  | C_instr_generator g ->
      fprintf fmt "@[<v 2>(Generator@ %a)@]" generator g
  | C_instr_dnamer dn ->
      fprintf fmt "@[<v 2>(Diagram_namer@ %a)@]" dnamer dn
  | C_instr_mnamer mn ->
      fprintf fmt "@[<v 2>(Morphism_namer@ %a)@]" mnamer mn
  | C_instr_include inc ->
      fprintf fmt "@[<v 2>(Include@ %a)@]" include_statement inc
  | C_instr_attach att ->
      fprintf fmt "@[<v 2>(Attach@ %a)@]" attach_statement att

and c_instr_local fmt instr =
  match instr.value with
  | C_instr_local_dnamer dn ->
      fprintf fmt "@[<v 2>(Diagram_namer@ %a)@]" dnamer dn
  | C_instr_local_mnamer mn ->
      fprintf fmt "@[<v 2>(Morphism_namer@ %a)@]" mnamer mn
  | C_instr_local_assert asrt ->
      fprintf fmt "@[<v 2>(Assert@ %a)@]" assert_statement asrt

and generator_type fmt gt =
  let { generator_type_generator; generator_type_definition } = gt.value in
  fprintf fmt "@[<v 2>(Generator_type@,%a@,%a)@]"
    (pp_field "generator" generator)
    generator_type_generator
    (pp_field "definition" complex)
    generator_type_definition

and generator fmt g =
  fprintf fmt "@[<v 2>(Generator@,%a@,%a)@]" (pp_field "name" pp_name)
    g.value.generator_name
    (pp_field "boundaries" (pp_option boundaries))
    g.value.generator_boundaries

and boundaries fmt b =
  let { boundaries_source; boundaries_target } = b.value in
  fprintf fmt "@[<v 2>(Boundaries@,%a@,%a)@]"
    (pp_field "source" diagram)
    boundaries_source
    (pp_field "target" diagram)
    boundaries_target

and address fmt address = pp_address fmt address

and morphism fmt m =
  match m.value with
  | Morphism_single comp ->
      fprintf fmt "@[<v 2>(Morphism_single@ %a)@]" m_comp comp
  | Morphism_concat { morphism_left; morphism_right } ->
      fprintf fmt "@[<v 2>(Morphism_concat@,%a@,%a)@]"
        (pp_field "left" morphism) morphism_left (pp_field "right" m_comp)
        morphism_right

and m_comp fmt comp =
  match comp.value with
  | M_comp_term term ->
      fprintf fmt "@[<v 2>(MTerm@ %a)@]" m_term term
  | M_comp_name name ->
      fprintf fmt "@[<v 2>(MName@ %a)@]" pp_name name

and m_term fmt term =
  let { m_term_ext; m_term_target } = term.value in
  fprintf fmt "@[<v 2>(MTerm@,%a@,%a)@]" (pp_field "ext" m_ext) m_term_ext
    (pp_field "target" complex)
    m_term_target

and m_ext fmt ext =
  let { m_ext_prefix; m_ext_block } = ext.value in
  fprintf fmt "@[<v 2>(MExt@,%a@,%a)@]"
    (pp_field "prefix" (pp_option morphism))
    m_ext_prefix
    (pp_field "block" (pp_option m_block))
    m_ext_block

and m_def fmt def =
  match def.value with
  | M_def_morphism morph ->
      fprintf fmt "@[<v 2>(MDef_morphism@ %a)@]" morphism morph
  | M_def_ext ext ->
      fprintf fmt "@[<v 2>(MDef_ext@ %a)@]" m_ext ext

and m_block fmt block = pp_list m_instr fmt block.value

and m_instr fmt instr =
  let { m_instr_source; m_instr_target } = instr.value in
  fprintf fmt "@[<v 2>(MInstr@,%a@,%a)@]"
    (pp_field "source" pasting)
    m_instr_source
    (pp_field "target" pasting)
    m_instr_target

and mnamer fmt mnamer =
  let { mnamer_name; mnamer_address; mnamer_definition } = mnamer.value in
  fprintf fmt "@[<v 2>(MNamer@,%a@,%a@,%a)@]" (pp_field "name" pp_name)
    mnamer_name
    (pp_field "address" address)
    mnamer_address
    (pp_field "definition" m_def)
    mnamer_definition

and dnamer fmt dnamer =
  let { dnamer_name; dnamer_boundaries; dnamer_body } = dnamer.value in
  fprintf fmt "@[<v 2>(DNamer@,%a@,%a@,%a)@]" (pp_field "name" pp_name)
    dnamer_name
    (pp_field "boundaries" (pp_option boundaries))
    dnamer_boundaries (pp_field "body" diagram) dnamer_body

and include_statement fmt include_stmt =
  let { include_address; include_alias } = include_stmt.value in
  fprintf fmt "@[<v 2>(Include@,%a@,%a)@]"
    (pp_field "address" address)
    include_address
    (pp_field "alias" (pp_option pp_name))
    include_alias

and attach_statement fmt attach_stmt =
  let { attach_name; attach_address; attach_along } = attach_stmt.value in
  fprintf fmt "@[<v 2>(Attach@,%a@,%a@,%a)@]" (pp_field "name" pp_name)
    attach_name
    (pp_field "address" address)
    attach_address
    (pp_field "along" (pp_option m_def))
    attach_along

and assert_statement fmt assertion =
  let { assert_left; assert_right } = assertion.value in
  fprintf fmt "@[<v 2>(Assert@,%a@,%a)@]" (pp_field "left" pasting) assert_left
    (pp_field "right" pasting) assert_right

and diagram fmt d =
  match d.value with
  | Diagram_single concat ->
      fprintf fmt "@[<v 2>(Diagram_single@ %a)@]" d_concat concat
  | Diagram_paste { diagram_left; diagram_nat; diagram_right } ->
      fprintf fmt "@[<v 2>(Diagram_paste@,%a@,%a@,%a)@]"
        (pp_field "left" diagram) diagram_left (pp_field "nat" pp_nat)
        diagram_nat
        (pp_field "right" d_concat)
        diagram_right

and d_concat fmt concat =
  match concat.value with
  | D_concat_single expr ->
      fprintf fmt "@[<v 2>(DConcat_single@ %a)@]" d_expr expr
  | D_concat_concat { d_concat_left; d_concat_right } ->
      fprintf fmt "@[<v 2>(DConcat_concat@,%a@,%a)@]" (pp_field "left" d_concat)
        d_concat_left (pp_field "right" d_expr) d_concat_right

and d_expr fmt expr =
  match expr.value with
  | D_expr_single comp ->
      fprintf fmt "@[<v 2>(DExpr_single@ %a)@]" d_comp comp
  | D_expr_dot { d_expr_left; d_expr_right } ->
      fprintf fmt "@[<v 2>(DExpr_dot@,%a@,%a)@]" (pp_field "left" d_expr)
        d_expr_left (pp_field "right" d_comp) d_expr_right

and d_comp fmt comp =
  match comp.value with
  | D_comp_mterm term ->
      fprintf fmt "@[<v 2>(DComp_mterm@ %a)@]" m_term term
  | D_comp_dterm term ->
      fprintf fmt "@[<v 2>(DComp_dterm@ %a)@]" d_term term
  | D_comp_name name ->
      fprintf fmt "@[<v 2>(DComp_name@ %a)@]" pp_name name
  | D_comp_bd bd ->
      fprintf fmt "@[<v 2>(DComp_bd@ %a)@]" pp_bd bd
  | D_comp_hole ->
      fprintf fmt "DComp_hole"

and d_term fmt term =
  match term.value with
  | D_term_indexed { d_term_diagram; d_term_nat; d_term_tail } ->
      fprintf fmt "@[<v 2>(DTerm_indexed@,%a@,%a@,%a)@]"
        (pp_field "diagram" diagram)
        d_term_diagram (pp_field "nat" pp_nat) d_term_nat
        (pp_field "tail" d_concat) d_term_tail
  | D_term_pair { d_term_concat; d_term_expr } ->
      fprintf fmt "@[<v 2>(DTerm_pair@,%a@,%a)@]"
        (pp_field "concat" d_concat)
        d_term_concat (pp_field "expr" d_expr) d_term_expr

and pasting fmt p =
  match p.value with
  | Pasting_single c ->
      fprintf fmt "@[<v 2>(Pasting_single@ %a)@]" concat c
  | Pasting_paste { pasting_left; pasting_nat; pasting_right } ->
      fprintf fmt "@[<v 2>(Pasting_paste@,%a@,%a@,%a)@]"
        (pp_field "left" pasting) pasting_left (pp_field "nat" pp_nat)
        pasting_nat (pp_field "right" concat) pasting_right

and concat fmt c =
  match c.value with
  | Concat_single e ->
      fprintf fmt "@[<v 2>(Concat_single@ %a)@]" expr e
  | Concat_concat { concat_left; concat_right } ->
      fprintf fmt "@[<v 2>(Concat_concat@,%a@,%a)@]" (pp_field "left" concat)
        concat_left (pp_field "right" expr) concat_right

and expr fmt e =
  let open Ast in
  match e.value with
  | Expr_single comp ->
      fprintf fmt "@[<v 2>(Expr_single@ %a)@]" d_comp comp
  | Expr_dot { expr_left; expr_right } ->
      fprintf fmt "@[<v 2>(Expr_dot@,%a@,%a)@]" (pp_field "left" expr) expr_left
        (pp_field "right" d_comp) expr_right

let program = pp_program
let to_string program = asprintf "%a" pp_program program
