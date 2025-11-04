module Report = Diagnostics.Report

type context = { current_module: Id.Module.t; state: State.t }

let make_context ~module_id ~state = { current_module= module_id; state }
let context_module { current_module; _ } = current_module
let context_state { state; _ } = state
let with_state ctx state = { ctx with state }

type load_error = [ `Not_found | `Io_error of string ]

type file_loader = {
  search_paths: string list;
  read_file: string -> (string, load_error) result;
}

type status = [ `Ok | `Error ]
type result = { context: context; diagnostics: Report.t; status: status }

let empty_result context = { context; diagnostics= Report.empty; status= `Ok }

let add_diagnostic result diagnostic =
  let status =
    match (diagnostic.Diagnostics.severity, result.status) with
    | `Error, _ ->
        `Error
    | (`Warning | `Info), status ->
        status
  in
  {
    context= result.context;
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
  { context= right.context; diagnostics; status }

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

let stub_node kind context (node : _ Lang_ast.node) =
  let span = Lang_ast.span_of_node node in
  add_diagnostic (empty_result context) (stub_diagnostic kind span)

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

let rec interpret_program ~loader context program =
  let module_id = context.current_module in
  match State.find_module context.state module_id with
  | Some _ ->
      empty_result context
  | None ->
      let state = context.state in
      let empty_type_id = Id.Global.fresh () in
      let state =
        State.add_type state ~id:empty_type_id ~data:Diagram.Zero
          ~complex:Complex.empty
      in
      let empty_type_tag = Id.Tag.of_global empty_type_id in
      let module_complex =
        Complex.empty
        |> Complex.add_generator ~name:(Id.Local.make "") ~dim:0
             ~tag:empty_type_tag
      in
      let state = State.add_module state ~id:module_id module_complex in
      let context = with_state context state in
      let open Lang_ast in
      let blocks = program.value.program_blocks in
      let rec interpret_blocks acc_context diagnostics status = function
        | [] ->
            { context= acc_context; diagnostics; status }
        | block :: rest ->
            let result = interpret_block ~loader acc_context block in
            let diagnostics = Report.append diagnostics result.diagnostics in
            let status =
              match (status, result.status) with
              | `Error, _ | _, `Error ->
                  `Error
              | `Ok, `Ok ->
                  `Ok
            in
            if result.status = `Error then
              { context= result.context; diagnostics; status }
            else interpret_blocks result.context diagnostics status rest
      in
      interpret_blocks context Report.empty `Ok blocks

and interpret_block ~loader:_ context block = stub_node "block" context block

let interpret_complex ~loader:_ context complex =
  stub_node "complex" context complex

let interpret_c_block_type ~loader:_ context c_block_type =
  stub_node "c_block_type" context c_block_type

let interpret_c_block ~loader:_ context c_block =
  stub_node "c_block" context c_block

let interpret_c_block_local ~loader:_ context c_block_local =
  stub_node "c_block_local" context c_block_local

let interpret_c_instr_type ~loader:_ context c_instr_type =
  stub_node "c_instr_type" context c_instr_type

let interpret_c_instr ~loader:_ context c_instr =
  stub_node "c_instr" context c_instr

let interpret_c_instr_local ~loader:_ context c_instr_local =
  stub_node "c_instr_local" context c_instr_local

let interpret_generator_type ~loader:_ context generator_type =
  stub_node "generator_type" context generator_type

let interpret_generator ~loader:_ context generator =
  stub_node "generator" context generator

let interpret_boundaries ~loader:_ context boundaries =
  stub_node "boundaries" context boundaries

let interpret_address ~loader:_ context address =
  stub_node "address" context address

let interpret_morphism ~loader:_ context morphism =
  stub_node "morphism" context morphism

let interpret_m_comp ~loader:_ context m_comp =
  stub_node "m_comp" context m_comp

let interpret_m_term ~loader:_ context m_term =
  stub_node "m_term" context m_term

let interpret_m_ext ~loader:_ context m_ext = stub_node "m_ext" context m_ext
let interpret_m_def ~loader:_ context m_def = stub_node "m_def" context m_def

let interpret_m_block ~loader:_ context m_block =
  stub_node "m_block" context m_block

let interpret_m_instr ~loader:_ context m_instr =
  stub_node "m_instr" context m_instr

let interpret_mnamer ~loader:_ context mnamer =
  stub_node "mnamer" context mnamer

let interpret_dnamer ~loader:_ context dnamer =
  stub_node "dnamer" context dnamer

let interpret_include ~loader context include_stmt =
  let loader = normalize_loader loader in
  let span = Lang_ast.span_of_node include_stmt in
  let open Lang_ast in
  let { value= include_desc; _ } = include_stmt in
  let address = include_desc.include_address in
  let segments = address_segments address in
  match segments_to_relative segments with
  | None ->
      stub_node "include" context include_stmt
  | Some relative ->
      let rec attempt = function
        | [] ->
            add_diagnostic (empty_result context)
              (missing_module_diagnostic span relative)
        | directory :: rest -> (
            let candidate = Filename.concat directory relative in
            match loader.read_file candidate with
            | Ok _contents -> (
                let canonical = Path.canonicalize candidate in
                let module_id = Id.Module.of_path canonical in
                let state = context.state in
                match State.find_module state module_id with
                | Some _ ->
                    empty_result context
                | None ->
                    stub_node "include" context include_stmt)
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
                add_diagnostic (empty_result context) diagnostic)
      in
      attempt loader.search_paths

let interpret_attach ~loader:_ context attach =
  stub_node "attach" context attach

let interpret_assert ~loader:_ context assert_stmt =
  stub_node "assert" context assert_stmt

let interpret_diagram ~loader:_ context diagram =
  stub_node "diagram" context diagram

let interpret_d_concat ~loader:_ context d_concat =
  stub_node "d_concat" context d_concat

let interpret_d_expr ~loader:_ context d_expr =
  stub_node "d_expr" context d_expr

let interpret_d_comp ~loader:_ context d_comp =
  stub_node "d_comp" context d_comp

let interpret_d_term ~loader:_ context d_term =
  stub_node "d_term" context d_term

let interpret_bd ~loader:_ context bd = stub_node "bd" context bd

let interpret_pasting ~loader:_ context pasting =
  stub_node "pasting" context pasting

let interpret_concat ~loader:_ context concat =
  stub_node "concat" context concat

let interpret_expr ~loader:_ context expr = stub_node "expr" context expr
let interpret_name ~loader:_ context name = stub_node "name" context name
let interpret_nat ~loader:_ context nat = stub_node "nat" context nat
