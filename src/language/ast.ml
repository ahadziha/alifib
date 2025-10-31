type span = Positions.span option
type 'a node = { span: span; value: 'a }
type bd_kind = In | Out

type program = program_desc node
and program_desc = { program_blocks: block list }
and block = block_desc node

and block_desc =
  | Block_type of { block_type_body: c_block_type option }
  | Block_complex of {
      block_complex_body: complex;
      block_local_body: c_block_local option;
    }

and complex = complex_desc node

and complex_desc = {
  complex_address: address option;
  complex_block: c_block option;
}

and c_block_type = c_block_type_desc node
and c_block_type_desc = c_instr_type list
and c_block = c_block_desc node
and c_block_desc = c_instr list
and c_block_local = c_block_local_desc node
and c_block_local_desc = c_instr_local list
and c_instr_type = c_instr_type_desc node

and c_instr_type_desc =
  | C_instr_type_generator of generator_type
  | C_instr_type_dnamer of dnamer
  | C_instr_type_mnamer of mnamer
  | C_instr_type_include of include_statement

and c_instr = c_instr_desc node

and c_instr_desc =
  | C_instr_generator of generator
  | C_instr_dnamer of dnamer
  | C_instr_mnamer of mnamer
  | C_instr_include of include_statement
  | C_instr_attach of attach_statement

and c_instr_local = c_instr_local_desc node

and c_instr_local_desc =
  | C_instr_local_dnamer of dnamer
  | C_instr_local_mnamer of mnamer
  | C_instr_local_assert of assert_statement

and generator_type = generator_type_desc node

and generator_type_desc = {
  generator_type_generator: generator;
  generator_type_definition: complex;
}

and generator = generator_desc node

and generator_desc = {
  generator_name: name;
  generator_boundaries: boundaries option;
}

and boundaries = boundaries_desc node
and boundaries_desc = { boundaries_source: diagram; boundaries_target: diagram }
and address = address_desc node
and address_desc = name list
and morphism = morphism_desc node

and morphism_desc =
  | Morphism_single of m_comp
  | Morphism_concat of { morphism_left: morphism; morphism_right: m_comp }

and m_comp = m_comp_desc node
and m_comp_desc = M_comp_term of m_term | M_comp_name of name
and m_term = m_term_desc node
and m_term_desc = { m_term_ext: m_ext; m_term_target: complex }
and m_ext = m_ext_desc node
and m_ext_desc = { m_ext_prefix: morphism option; m_ext_block: m_block option }
and m_def = m_def_desc node
and m_def_desc = M_def_morphism of morphism | M_def_ext of m_ext
and m_block = m_block_desc node
and m_block_desc = m_instr list
and m_instr = m_instr_desc node
and m_instr_desc = { m_instr_source: pasting; m_instr_target: pasting }
and mnamer = mnamer_desc node

and mnamer_desc = {
  mnamer_name: name;
  mnamer_address: address;
  mnamer_definition: m_def;
}

and dnamer = dnamer_desc node

and dnamer_desc = {
  dnamer_name: name;
  dnamer_boundaries: boundaries option;
  dnamer_body: diagram;
}

and include_statement = include_statement_desc node

and include_statement_desc = {
  include_address: address;
  include_alias: name option;
}

and attach_statement = attach_statement_desc node

and attach_statement_desc = {
  attach_name: name;
  attach_address: address;
  attach_along: m_def option;
}

and assert_statement = assert_statement_desc node
and assert_statement_desc = { assert_left: pasting; assert_right: pasting }
and diagram = diagram_desc node

and diagram_desc =
  | Diagram_single of d_concat
  | Diagram_paste of {
      diagram_left: diagram;
      diagram_nat: nat;
      diagram_right: d_concat;
    }

and d_concat = d_concat_desc node

and d_concat_desc =
  | D_concat_single of d_expr
  | D_concat_concat of { d_concat_left: d_concat; d_concat_right: d_expr }

and d_expr = d_expr_desc node

and d_expr_desc =
  | D_expr_single of d_comp
  | D_expr_dot of { d_expr_left: d_expr; d_expr_right: d_comp }

and d_comp = d_comp_desc node

and d_comp_desc =
  | D_comp_mterm of m_term
  | D_comp_dterm of d_term
  | D_comp_name of name
  | D_comp_bd of bd
  | D_comp_hole

and d_term = d_term_desc node

and d_term_desc =
  | D_term_indexed of {
      d_term_diagram: diagram;
      d_term_nat: nat;
      d_term_tail: d_concat;
    }
  | D_term_pair of { d_term_concat: d_concat; d_term_expr: d_expr }

and bd = bd_kind node
and pasting = pasting_desc node

and pasting_desc =
  | Pasting_single of concat
  | Pasting_paste of {
      pasting_left: pasting;
      pasting_nat: nat;
      pasting_right: concat;
    }

and concat = concat_desc node

and concat_desc =
  | Concat_single of expr
  | Concat_concat of { concat_left: concat; concat_right: expr }

and expr = expr_desc node

and expr_desc =
  | Expr_single of d_comp
  | Expr_dot of { expr_left: expr; expr_right: d_comp }

and name = Id.Local.t node
and nat = int node

let node ?span value = { span; value }
let empty = node { program_blocks= [] }
