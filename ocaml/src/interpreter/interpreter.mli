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
type morphism_component = { morphism: Morphism.t; source: Complex.t }
type term = M_term of morphism_component | D_term of Diagram.t
type component = Term of term | Hole | Bd of Diagram.sign

type term_pair =
  | M_term_pair of { fst: Morphism.t; snd: Morphism.t; source: Complex.t }
  | D_term_pair of { fst: Diagram.t; snd: Diagram.t }

type result = {
  context: context;
  diagnostics: Diagnostics.report;
  status: status;
}

val empty_result : context -> result
val add_diagnostic : result -> Diagnostics.diagnostic -> result
val combine : previous:result -> next:result -> result
val has_errors : result -> bool
val interpret_program : loader:file_loader -> context -> Ast.program -> result
val interpret_block : loader:file_loader -> context -> Ast.block -> result

val interpret_complex :
  context -> mode:mode -> Ast.complex -> namespace option * result

val interpret_c_block_type :
  loader:file_loader -> context -> Ast.c_block_type -> Complex.t option * result

val interpret_c_block :
  context ->
  mode:mode ->
  location:Complex.t ->
  Ast.c_block ->
  Complex.t option * result

val interpret_c_block_local :
  context -> namespace -> Ast.c_block_local -> Complex.t option * result

val interpret_c_instr_type :
  loader:file_loader -> context -> Ast.c_instr_type -> Complex.t option * result

val interpret_c_instr :
  context ->
  mode:mode ->
  location:Complex.t ->
  Ast.c_instr ->
  Complex.t option * result

val interpret_c_instr_local :
  context -> namespace -> Ast.c_instr_local -> Complex.t option * result

val interpret_generator_type :
  context ->
  Ast.generator_type ->
  (Id.Local.t * Diagram.cell_data * Complex.t) option * result

val interpret_generator :
  context ->
  location:Complex.t ->
  Ast.generator ->
  (Id.Local.t * Diagram.cell_data) option * result

val interpret_boundaries :
  context ->
  location:Complex.t ->
  Ast.boundaries ->
  Diagram.cell_data option * result

val interpret_address : context -> Ast.address -> Id.Global.t option * result

val interpret_morphism :
  context ->
  location:Complex.t ->
  Ast.morphism ->
  morphism_component option * result

val interpret_m_comp :
  context ->
  location:Complex.t ->
  Ast.m_comp ->
  morphism_component option * result

val interpret_m_term :
  context ->
  location:Complex.t ->
  Ast.m_term ->
  morphism_component option * result

val interpret_m_ext :
  context ->
  location:Complex.t ->
  source:Complex.t ->
  Ast.m_ext ->
  Morphism.t option * result

val interpret_m_def :
  context ->
  location:Complex.t ->
  source:Complex.t ->
  Ast.m_def ->
  Morphism.t option * result

val interpret_m_block :
  context ->
  location:Complex.t ->
  source:Complex.t ->
  morphism:Morphism.t ->
  Ast.m_block ->
  Morphism.t option * result

val interpret_m_instr :
  context ->
  location:Complex.t ->
  source:Complex.t ->
  morphism:Morphism.t ->
  Ast.m_instr ->
  Morphism.t option * result

val interpret_mnamer :
  context ->
  location:Complex.t ->
  Ast.mnamer ->
  (Id.Local.t * Morphism.t * Complex.morphism_domain) option * result

val interpret_dnamer :
  context ->
  location:Complex.t ->
  Ast.dnamer ->
  (Id.Local.t * Diagram.t) option * result

val interpret_include :
  context -> Ast.include_statement -> (Id.Global.t * Id.Local.t) option * result

val interpret_include_module : Ast.include_module -> Id.Local.t * Id.Local.t

val interpret_attach :
  context ->
  location:Complex.t ->
  Ast.attach_statement ->
  (Id.Local.t * Morphism.t * Complex.morphism_domain) option * result

val interpret_assert :
  context ->
  location:Complex.t ->
  Ast.assert_statement ->
  term_pair option * result

val interpret_diagram :
  context -> location:Complex.t -> Ast.diagram -> Diagram.t option * result

val interpret_d_concat :
  context -> location:Complex.t -> Ast.d_concat -> Diagram.t option * result

val interpret_d_expr :
  context -> location:Complex.t -> Ast.d_expr -> term option * result

val interpret_d_comp :
  context -> location:Complex.t -> Ast.d_comp -> component option * result

val interpret_d_term :
  context -> location:Complex.t -> Ast.d_term -> Diagram.t option * result

val interpret_bd : Ast.bd -> Diagram.sign

val interpret_pasting :
  context -> location:Complex.t -> Ast.pasting -> term option * result

val interpret_concat :
  context -> location:Complex.t -> Ast.concat -> term option * result

val interpret_name : Ast.name -> Id.Local.t
val interpret_nat : Ast.nat -> int
