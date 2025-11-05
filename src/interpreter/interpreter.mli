type context = { current_module: Id.Module.t; state: State.t }

val make_context : module_id:Id.Module.t -> state:State.t -> context
val context_module : context -> Id.Module.t
val context_state : context -> State.t
val with_state : context -> State.t -> context

type load_error = [ `Not_found | `Io_error of string ]

type file_loader = {
  search_paths: string list;
  read_file: string -> (string, load_error) result;
}

type mode = Global | Local
type namespace = { root: Id.Global.t; location: Complex.t }
type status = [ `Ok | `Error ]

type result = {
  context: context;
  diagnostics: Diagnostics.report;
  status: status;
}

val empty_result : context -> result
val add_diagnostic : result -> Diagnostics.diagnostic -> result
val combine : result -> result -> result
val has_errors : result -> bool
val interpret_program : loader:file_loader -> context -> Ast.program -> result
val interpret_block : loader:file_loader -> context -> Ast.block -> result

val interpret_complex :
  context -> mode:mode -> Ast.complex -> namespace option * result

val interpret_c_block_type :
  loader:file_loader -> context -> Ast.c_block_type -> result

val interpret_c_block :
  context ->
  mode:mode ->
  location:Complex.t ->
  Ast.c_block ->
  Complex.t option * result

val interpret_c_block_local :
  context -> namespace -> Ast.c_block_local -> result

val interpret_c_instr_type :
  loader:file_loader -> context -> Ast.c_instr_type -> result

val interpret_c_instr : context -> location:Complex.t -> Ast.c_instr -> result
val interpret_c_instr_local : context -> Ast.c_instr_local -> result
val interpret_generator_type : context -> Ast.generator_type -> result

val interpret_generator :
  context -> location:Complex.t -> Ast.generator -> result

val interpret_boundaries :
  context -> location:Complex.t -> Ast.boundaries -> result

val interpret_address : context -> Ast.address -> Id.Global.t option * result
val interpret_morphism : context -> location:Complex.t -> Ast.morphism -> result
val interpret_m_comp : context -> location:Complex.t -> Ast.m_comp -> result
val interpret_m_term : context -> location:Complex.t -> Ast.m_term -> result
val interpret_m_ext : context -> location:Complex.t -> Ast.m_ext -> result
val interpret_m_def : context -> location:Complex.t -> Ast.m_def -> result
val interpret_m_block : context -> location:Complex.t -> Ast.m_block -> result
val interpret_m_instr : context -> location:Complex.t -> Ast.m_instr -> result
val interpret_mnamer : context -> location:Complex.t -> Ast.mnamer -> result
val interpret_dnamer : context -> location:Complex.t -> Ast.dnamer -> result
val interpret_include : context -> Ast.include_statement -> result

val interpret_attach :
  context -> location:Complex.t -> Ast.attach_statement -> result

val interpret_assert :
  context -> location:Complex.t -> Ast.assert_statement -> result

val interpret_diagram : context -> location:Complex.t -> Ast.diagram -> result
val interpret_d_concat : context -> location:Complex.t -> Ast.d_concat -> result
val interpret_d_expr : context -> location:Complex.t -> Ast.d_expr -> result
val interpret_d_comp : context -> location:Complex.t -> Ast.d_comp -> result
val interpret_d_term : context -> location:Complex.t -> Ast.d_term -> result
val interpret_bd : context -> Ast.bd -> Diagram.sign * context
val interpret_pasting : context -> location:Complex.t -> Ast.pasting -> result
val interpret_concat : context -> location:Complex.t -> Ast.concat -> result
val interpret_expr : context -> location:Complex.t -> Ast.expr -> result
val interpret_name : context -> Ast.name -> Id.Local.t * context
val interpret_nat : context -> Ast.nat -> int * context
