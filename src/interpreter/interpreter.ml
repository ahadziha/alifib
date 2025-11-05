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

type mode = Global | Local
type namespace = { root: Id.Global.t; location: Complex.t }
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

let interpret_c_block_type ~loader:_ context c_block_type =
  stub_node "c_block_type" context c_block_type

let interpret_c_block_local context (_ : namespace) c_block_local =
  stub_node "c_block_local" context c_block_local

let interpret_complex context ~mode:_ complex =
  let result = stub_node "complex" context complex in
  ((None : namespace option), result)

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
      let empty_name = Id.Local.make "" in
      let empty_diagram =
        match Diagram.cell empty_type_tag Diagram.Zero with
        | Ok diagram ->
            diagram
        | Error err ->
            let message =
              Format.asprintf "Failed to create zero cell: %a" Error.pp err
            in
            invalid_arg message
      in
      let module_complex =
        Complex.empty
        |> Complex.add_generator ~name:empty_name ~dim:0 ~tag:empty_type_tag
      in
      let module_complex =
        Complex.add_diagram module_complex ~name:empty_name empty_diagram
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
            let status = result.status in
            if result.status = `Error then
              { context= result.context; diagnostics; status }
            else interpret_blocks result.context diagnostics status rest
      in
      interpret_blocks context Report.empty `Ok blocks

and interpret_block ~loader context block =
  let open Lang_ast in
  match block.value with
  | Block_type { block_type_body= None } ->
      empty_result context
  | Block_type { block_type_body= Some c_block_type } ->
      interpret_c_block_type ~loader context c_block_type
  | Block_complex { block_complex_body; block_local_body } -> (
      let namespace_opt, complex_result =
        interpret_complex context ~mode:Local block_complex_body
      in
      let context = complex_result.context in
      let diagnostics = complex_result.diagnostics in
      let status = complex_result.status in
      match (namespace_opt, block_local_body) with
      | Some namespace, Some local_block when status <> `Error ->
          let local_result =
            interpret_c_block_local context namespace local_block
          in
          {
            context= local_result.context;
            diagnostics= Report.append diagnostics local_result.diagnostics;
            status= local_result.status;
          }
      | _ ->
          { context; diagnostics; status })

let interpret_c_block context ~location:_ c_block =
  stub_node "c_block" context c_block

let interpret_c_instr_type ~loader:_ context c_instr_type =
  stub_node "c_instr_type" context c_instr_type

let interpret_c_instr context ~location:_ c_instr =
  stub_node "c_instr" context c_instr

let interpret_c_instr_local context c_instr_local =
  stub_node "c_instr_local" context c_instr_local

let interpret_generator_type context generator_type =
  stub_node "generator_type" context generator_type

let interpret_generator context ~location:_ generator =
  stub_node "generator" context generator

let interpret_boundaries context ~location:_ boundaries =
  stub_node "boundaries" context boundaries

let interpret_name context (name : Lang_ast.name) = (name.value, context)
let interpret_nat context (nat : Lang_ast.nat) = (nat.value, context)

let interpret_address context address =
  let open Lang_ast in
  let rec gather acc ctx = function
    | [] ->
        (List.rev acc, ctx)
    | name :: rest ->
        let value, ctx' = interpret_name ctx name in
        gather (value :: acc) ctx' rest
  in
  gather [] context address.value

let interpret_morphism context ~location:_ morphism =
  stub_node "morphism" context morphism

let interpret_m_comp context ~location:_ m_comp =
  stub_node "m_comp" context m_comp

let interpret_m_term context ~location:_ m_term =
  stub_node "m_term" context m_term

let interpret_m_ext context ~location:_ m_ext = stub_node "m_ext" context m_ext
let interpret_m_def context ~location:_ m_def = stub_node "m_def" context m_def

let interpret_m_block context ~location:_ m_block =
  stub_node "m_block" context m_block

let interpret_m_instr context ~location:_ m_instr =
  stub_node "m_instr" context m_instr

let interpret_mnamer context ~location:_ mnamer =
  stub_node "mnamer" context mnamer

let interpret_dnamer context ~location:_ dnamer =
  stub_node "dnamer" context dnamer

let interpret_include context include_stmt =
  stub_node "include" context include_stmt

let interpret_attach context ~location:_ attach =
  stub_node "attach" context attach

let interpret_assert context ~location:_ assert_stmt =
  stub_node "assert" context assert_stmt

let interpret_diagram context ~location:_ diagram =
  stub_node "diagram" context diagram

let interpret_d_concat context ~location:_ d_concat =
  stub_node "d_concat" context d_concat

let interpret_d_expr context ~location:_ d_expr =
  stub_node "d_expr" context d_expr

let interpret_d_comp context ~location:_ d_comp =
  stub_node "d_comp" context d_comp

let interpret_d_term context ~location:_ d_term =
  stub_node "d_term" context d_term

let interpret_bd context (bd : Lang_ast.bd) = (bd.value, context)

let interpret_pasting context ~location:_ pasting =
  stub_node "pasting" context pasting

let interpret_concat context ~location:_ concat =
  stub_node "concat" context concat

let interpret_expr context ~location:_ expr = stub_node "expr" context expr
