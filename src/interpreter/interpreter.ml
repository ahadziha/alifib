module Report = Diagnostics.Report

type session = { current_module: Id.Module.t; state: State.t }

let make_session ~module_id ~state = { current_module= module_id; state }
let session_module { current_module; _ } = current_module
let session_state { state; _ } = state
let with_state session state = { session with state }

type load_error = [ `Not_found | `Io_error of string ]

type file_loader = {
  search_paths: string list;
  read_file: string -> (string, load_error) result;
}

type status = [ `Ok | `Error ]
type result = { session: session; diagnostics: Report.t; status: status }

let empty_result session = { session; diagnostics= Report.empty; status= `Ok }

let add_diagnostic result diagnostic =
  let status =
    match (diagnostic.Diagnostics.severity, result.status) with
    | `Error, _ ->
        `Error
    | (`Warning | `Info), status ->
        status
  in
  {
    session= result.session;
    diagnostics= Report.add diagnostic result.diagnostics;
    status;
  }

let combine left right =
  let diagnostics = Report.append left.diagnostics right.diagnostics in
  let status =
    match (left.status, right.status) with
    | `Error, _ | _, `Error ->
        `Error
    | `Ok, `Ok ->
        `Ok
  in
  { session= right.session; diagnostics; status }

let has_errors { status; _ } = match status with `Error -> true | `Ok -> false

let normalize_loader ({ search_paths; read_file } as loader) =
  let normalized_paths = Path.normalize_search_paths search_paths in
  if normalized_paths = search_paths then loader
  else { search_paths= normalized_paths; read_file }

let interpreter_producer =
  { Error.Located.phase= `Interpreter; module_path= Some "interpreter" }

let unknown_span = Positions.point_span Positions.unknown_point
let span_or_unknown = function Some span -> span | None -> unknown_span

module Lang_ast = struct
  include Ast

  let span_of_node (node : _ node) = span_or_unknown node.span
end

let stub_message kind =
  Printf.sprintf "Interpreter stub not implemented for %s" kind

let stub_diagnostic kind span =
  Diagnostics.make `Error interpreter_producer span (stub_message kind)

let stub_node kind session (node : _ Lang_ast.node) =
  let span = Lang_ast.span_of_node node in
  add_diagnostic (empty_result session) (stub_diagnostic kind span)

let name_to_string (name : Lang_ast.name) = Id.Local.to_string name.value

let address_segments (address : Lang_ast.address) =
  List.map name_to_string address.value

let segments_to_relative segments =
  match segments with
  | [] ->
      None
  | segment :: rest ->
      let base = List.fold_left Filename.concat segment rest in
      Some (base ^ ".ali")

let missing_module_diagnostic span relative =
  let message = Printf.sprintf "Could not find module `%s`" relative in
  Diagnostics.make `Error interpreter_producer span message

let interpret_program ~loader:_ session program =
  stub_node "program" session program

let interpret_block ~loader:_ session block = stub_node "block" session block

let interpret_complex ~loader:_ session complex =
  stub_node "complex" session complex

let interpret_c_block_type ~loader:_ session c_block_type =
  stub_node "c_block_type" session c_block_type

let interpret_c_block ~loader:_ session c_block =
  stub_node "c_block" session c_block

let interpret_c_block_local ~loader:_ session c_block_local =
  stub_node "c_block_local" session c_block_local

let interpret_c_instr_type ~loader:_ session c_instr_type =
  stub_node "c_instr_type" session c_instr_type

let interpret_c_instr ~loader:_ session c_instr =
  stub_node "c_instr" session c_instr

let interpret_c_instr_local ~loader:_ session c_instr_local =
  stub_node "c_instr_local" session c_instr_local

let interpret_generator_type ~loader:_ session generator_type =
  stub_node "generator_type" session generator_type

let interpret_generator ~loader:_ session generator =
  stub_node "generator" session generator

let interpret_boundaries ~loader:_ session boundaries =
  stub_node "boundaries" session boundaries

let interpret_address ~loader:_ session address =
  stub_node "address" session address

let interpret_morphism ~loader:_ session morphism =
  stub_node "morphism" session morphism

let interpret_m_comp ~loader:_ session m_comp =
  stub_node "m_comp" session m_comp

let interpret_m_term ~loader:_ session m_term =
  stub_node "m_term" session m_term

let interpret_m_ext ~loader:_ session m_ext = stub_node "m_ext" session m_ext
let interpret_m_def ~loader:_ session m_def = stub_node "m_def" session m_def

let interpret_m_block ~loader:_ session m_block =
  stub_node "m_block" session m_block

let interpret_m_instr ~loader:_ session m_instr =
  stub_node "m_instr" session m_instr

let interpret_mnamer ~loader:_ session mnamer =
  stub_node "mnamer" session mnamer

let interpret_dnamer ~loader:_ session dnamer =
  stub_node "dnamer" session dnamer

let interpret_include ~loader session include_stmt =
  let loader = normalize_loader loader in
  let span = Lang_ast.span_of_node include_stmt in
  let open Lang_ast in
  let { value= include_desc; _ } = include_stmt in
  let address = include_desc.include_address in
  let segments = address_segments address in
  match segments_to_relative segments with
  | None ->
      stub_node "include" session include_stmt
  | Some relative ->
      let rec attempt = function
        | [] ->
            add_diagnostic (empty_result session)
              (missing_module_diagnostic span relative)
        | directory :: rest -> (
            let candidate = Filename.concat directory relative in
            match loader.read_file candidate with
            | Ok _contents -> (
                let canonical = Path.canonicalize candidate in
                let module_id = Id.Module.of_path canonical in
                let state = session.state in
                match State.find_module state module_id with
                | Some _ ->
                    empty_result session
                | None ->
                    stub_node "include" session include_stmt)
            | Error `Not_found ->
                attempt rest
            | Error (`Io_error reason) ->
                let message =
                  Printf.sprintf "Failed to load module `%s`: %s" relative
                    reason
                in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer span message
                in
                add_diagnostic (empty_result session) diagnostic)
      in
      attempt loader.search_paths

let interpret_attach ~loader:_ session attach =
  stub_node "attach" session attach

let interpret_assert ~loader:_ session assert_stmt =
  stub_node "assert" session assert_stmt

let interpret_diagram ~loader:_ session diagram =
  stub_node "diagram" session diagram

let interpret_d_concat ~loader:_ session d_concat =
  stub_node "d_concat" session d_concat

let interpret_d_expr ~loader:_ session d_expr =
  stub_node "d_expr" session d_expr

let interpret_d_comp ~loader:_ session d_comp =
  stub_node "d_comp" session d_comp

let interpret_d_term ~loader:_ session d_term =
  stub_node "d_term" session d_term

let interpret_bd ~loader:_ session bd = stub_node "bd" session bd

let interpret_pasting ~loader:_ session pasting =
  stub_node "pasting" session pasting

let interpret_concat ~loader:_ session concat =
  stub_node "concat" session concat

let interpret_expr ~loader:_ session expr = stub_node "expr" session expr
let interpret_name ~loader:_ session name = stub_node "name" session name
let interpret_nat ~loader:_ session nat = stub_node "nat" session nat
