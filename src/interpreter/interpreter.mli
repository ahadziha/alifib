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
  loader:file_loader ->
  context ->
  mode:mode ->
  Ast.complex ->
  namespace option * result

val interpret_c_block_type :
  loader:file_loader -> context -> Ast.c_block_type -> result

val interpret_c_block : loader:file_loader -> context -> Ast.c_block -> result

val interpret_c_block_local :
  loader:file_loader -> context -> namespace -> Ast.c_block_local -> result

val interpret_c_instr_type :
  loader:file_loader -> context -> Ast.c_instr_type -> result

val interpret_c_instr : loader:file_loader -> context -> Ast.c_instr -> result

val interpret_c_instr_local :
  loader:file_loader -> context -> Ast.c_instr_local -> result

val interpret_generator_type :
  loader:file_loader -> context -> Ast.generator_type -> result

val interpret_generator :
  loader:file_loader -> context -> Ast.generator -> result

val interpret_boundaries :
  loader:file_loader -> context -> Ast.boundaries -> result

val interpret_address : loader:file_loader -> context -> Ast.address -> result
val interpret_morphism : loader:file_loader -> context -> Ast.morphism -> result
val interpret_m_comp : loader:file_loader -> context -> Ast.m_comp -> result
val interpret_m_term : loader:file_loader -> context -> Ast.m_term -> result
val interpret_m_ext : loader:file_loader -> context -> Ast.m_ext -> result
val interpret_m_def : loader:file_loader -> context -> Ast.m_def -> result
val interpret_m_block : loader:file_loader -> context -> Ast.m_block -> result
val interpret_m_instr : loader:file_loader -> context -> Ast.m_instr -> result
val interpret_mnamer : loader:file_loader -> context -> Ast.mnamer -> result
val interpret_dnamer : loader:file_loader -> context -> Ast.dnamer -> result

val interpret_include :
  loader:file_loader -> context -> Ast.include_statement -> result

val interpret_attach :
  loader:file_loader -> context -> Ast.attach_statement -> result

val interpret_assert :
  loader:file_loader -> context -> Ast.assert_statement -> result

val interpret_diagram : loader:file_loader -> context -> Ast.diagram -> result
val interpret_d_concat : loader:file_loader -> context -> Ast.d_concat -> result
val interpret_d_expr : loader:file_loader -> context -> Ast.d_expr -> result
val interpret_d_comp : loader:file_loader -> context -> Ast.d_comp -> result
val interpret_d_term : loader:file_loader -> context -> Ast.d_term -> result
val interpret_bd : loader:file_loader -> context -> Ast.bd -> result
val interpret_pasting : loader:file_loader -> context -> Ast.pasting -> result
val interpret_concat : loader:file_loader -> context -> Ast.concat -> result
val interpret_expr : loader:file_loader -> context -> Ast.expr -> result
val interpret_name : loader:file_loader -> context -> Ast.name -> result
val interpret_nat : loader:file_loader -> context -> Ast.nat -> result
