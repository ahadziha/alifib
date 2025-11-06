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

let interpret_name context (name : Lang_ast.name) = (name.value, context)
let interpret_nat context (nat : Lang_ast.nat) = (nat.value, context)

let interpret_c_instr_type ~loader:_ context c_instr_type =
  (None, stub_node "c_instr_type" context c_instr_type)

let interpret_c_instr context ~mode:_ ~location:_ c_instr =
  (None, stub_node "c_instr" context c_instr)

let interpret_c_instr_local context _namespace c_instr_local =
  (None, stub_node "c_instr_local" context c_instr_local)

let rec smart_extend context morphism ~domain ~codomain ~tag ~dim ~diagram =
  let error msg = Error.make msg in
  if Morphism.is_defined_at morphism tag then
    match Morphism.image morphism tag with
    | Error err ->
        Error err
    | Ok current_image ->
        if Diagram.isomorphic current_image diagram then Ok morphism
        else Error (error "The same generator is mapped to multiple diagrams")
  else
    let cell_data =
      match tag with
      | `Global global_id -> (
          match State.find_cell context.state global_id with
          | Some data ->
              data
          | None ->
              assert false)
      | `Local name -> (
          match Complex.find_local_cell domain name with
          | Some { data; _ } ->
              data
          | None ->
              assert false)
    in
    let missing_tags =
      match cell_data with
      | Diagram.Zero ->
          []
      | Diagram.Boundary { boundary_in; boundary_out } ->
          let collect boundary sign acc =
            let top_dim = Diagram.dim boundary in
            let labels = Diagram.labels boundary in
            assert (top_dim >= 0 && top_dim < Array.length labels)
            ; let row = labels.(top_dim) in
              let len = Array.length row in
              let rec aux idx acc =
                if idx = len then acc
                else
                  let acc =
                    let candidate = row.(idx) in
                    if Morphism.is_defined_at morphism candidate then acc
                    else (candidate, sign) :: acc
                  in
                  aux (idx + 1) acc
              in
              aux 0 acc
          in
          let acc = collect boundary_in `Input [] in
          collect boundary_out `Output acc
    in
    let rec extend_missing partial pending =
      match pending with
      | [] ->
          Morphism.extend partial ~tag ~dim ~cell_data ~image:diagram
      | (focus, sign) :: rest -> (
          if Morphism.is_defined_at partial focus then
            extend_missing partial rest
          else
            let dim_minus_one = dim - 1 in
            let boundary_sources =
              match (cell_data, sign) with
              | Diagram.Boundary { boundary_in; _ }, `Input ->
                  Ok (boundary_in, Diagram.boundary `Input dim_minus_one diagram)
              | Diagram.Boundary { boundary_out; _ }, `Output ->
                  Ok
                    ( boundary_out,
                      Diagram.boundary `Output dim_minus_one diagram )
              | Diagram.Zero, _ ->
                  assert false
            in
            match boundary_sources with
            | Error _ as err ->
                err
            | Ok (boundary_diag, target_boundary) -> (
                if Diagram.is_cell boundary_diag then
                  match
                    smart_extend context partial ~domain ~codomain ~tag:focus
                      ~dim:dim_minus_one ~diagram:target_boundary
                  with
                  | Error _ as err ->
                      err
                  | Ok updated ->
                      extend_missing updated rest
                else
                  match
                    Ogposet.isomorphism_of
                      (Diagram.shape boundary_diag)
                      (Diagram.shape target_boundary)
                  with
                  | Error _ ->
                      Error
                        (error "Failed to extend map (more information needed)")
                  | Ok embedding -> (
                      let top_dim = Diagram.dim boundary_diag in
                      let boundary_labels = Diagram.labels boundary_diag in
                      let target_labels = Diagram.labels target_boundary in
                      let map = Ogposet.Embedding.map embedding in
                      assert (top_dim >= 0)
                      ; assert (top_dim < Array.length boundary_labels)
                      ; assert (top_dim < Array.length target_labels)
                      ; assert (top_dim < Array.length map)
                      ; let map_row = map.(top_dim) in
                        let boundary_row = boundary_labels.(top_dim) in
                        let target_row = target_labels.(top_dim) in
                        assert (Array.length map_row = Array.length boundary_row)
                        ; let determine_image () =
                            let image_ref = ref None in
                            let ok = ref true in
                            let len = Array.length boundary_row in
                            let idx = ref 0 in
                            while !ok && !idx < len do
                              (if Id.Tag.equal boundary_row.(!idx) focus then
                                 let mapped_idx =
                                   if !idx < Array.length map_row then
                                     map_row.(!idx)
                                   else -1
                                 in
                                 if
                                   mapped_idx < 0
                                   || mapped_idx >= Array.length target_row
                                 then ok := false
                                 else
                                   let mapped_tag = target_row.(mapped_idx) in
                                   match !image_ref with
                                   | None ->
                                       image_ref := Some mapped_tag
                                   | Some existing ->
                                       if not (Id.Tag.equal existing mapped_tag)
                                       then ok := false)
                              ; incr idx
                            done
                            ; if not !ok then
                                Error
                                  (error
                                     "The same generator is mapped to multiple \
                                      diagrams")
                              else
                                match !image_ref with
                                | None ->
                                    Error
                                      (error
                                         "Failed to extend map (more \
                                          information needed)")
                                | Some mapped ->
                                    Ok mapped
                          in
                          match determine_image () with
                          | Error _ as err ->
                              err
                          | Ok image_focus -> (
                              let generator_name =
                                match
                                  Complex.find_generator_by_tag codomain
                                    image_focus
                                with
                                | Some name ->
                                    name
                                | None ->
                                    assert false
                              in
                              let d_focus =
                                match
                                  Complex.classifier codomain generator_name
                                with
                                | Some diagram ->
                                    diagram
                                | None ->
                                    assert false
                              in
                              match
                                smart_extend context partial ~domain ~codomain
                                  ~tag:focus ~dim:dim_minus_one ~diagram:d_focus
                              with
                              | Error _ as err ->
                                  err
                              | Ok updated ->
                                  extend_missing updated rest))))
    in
    extend_missing morphism missing_tags
[@@warning "-32"]

let interpret_c_block_type ~loader context
    (c_block_type : Lang_ast.c_block_type) =
  let instrs = c_block_type.value in
  let rec loop acc_location acc_result = function
    | [] ->
        (acc_location, acc_result)
    | instr :: rest ->
        let ctx = acc_result.context in
        let location_opt, instr_result =
          interpret_c_instr_type ~loader ctx instr
        in
        let combined = combine acc_result instr_result in
        let acc_location =
          match location_opt with Some loc -> Some loc | None -> acc_location
        in
        if has_errors instr_result then (acc_location, combined)
        else loop acc_location combined rest
  in
  loop None (empty_result context) instrs

let interpret_c_block_local context namespace
    (c_block_local : Lang_ast.c_block_local) =
  let instrs = c_block_local.value in
  let rec loop current_namespace acc_location acc_result = function
    | [] ->
        (acc_location, acc_result)
    | instr :: rest ->
        let ctx = acc_result.context in
        let location_opt, instr_result =
          interpret_c_instr_local ctx current_namespace instr
        in
        let combined = combine acc_result instr_result in
        let acc_location =
          match location_opt with Some loc -> Some loc | None -> acc_location
        in
        let next_namespace =
          match location_opt with
          | Some loc ->
              { current_namespace with location= loc }
          | None ->
              current_namespace
        in
        if has_errors instr_result then (acc_location, combined)
        else loop next_namespace acc_location combined rest
  in
  loop namespace None (empty_result context) instrs

let interpret_c_block context ~mode ~location (c_block : Lang_ast.c_block) =
  let instrs = c_block.value in
  let rec loop current_location acc_location acc_result = function
    | [] ->
        (acc_location, acc_result)
    | instr :: rest ->
        let ctx = acc_result.context in
        let location_opt, instr_result =
          interpret_c_instr ctx ~mode ~location:current_location instr
        in
        let combined = combine acc_result instr_result in
        let acc_location =
          match location_opt with Some loc -> Some loc | None -> acc_location
        in
        let next_location =
          match location_opt with Some loc -> loc | None -> current_location
        in
        if has_errors instr_result then (acc_location, combined)
        else loop next_location acc_location combined rest
  in
  loop location None (empty_result context) instrs

let rec interpret_complex context ~mode complex =
  let open Lang_ast in
  let string_or_empty name =
    let raw = Id.Local.to_string name in
    if String.length raw = 0 then "<empty>" else raw
  in
  let complex_span = Lang_ast.span_of_node complex in
  let state = context.state in
  let module_id = context.current_module in
  match State.find_module state module_id with
  | None ->
      let message =
        Format.asprintf "Module %s not found" (Id.Module.to_string module_id)
      in
      let diagnostic =
        Diagnostics.make `Error interpreter_producer complex_span message
      in
      (None, add_diagnostic (empty_result context) diagnostic)
  | Some module_space -> (
      let empty_name = Id.Local.make "" in
      let root_opt, root_result =
        match complex.value.complex_address with
        | None -> (
            let base_result = empty_result context in
            match Complex.find_generator module_space empty_name with
            | None ->
                let message =
                  Printf.sprintf "Root generator %s not found"
                    (string_or_empty empty_name)
                in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer complex_span
                    message
                in
                (None, add_diagnostic base_result diagnostic)
            | Some { tag= `Global root; _ } ->
                (Some root, base_result)
            | Some { tag= `Local _; _ } ->
                assert false)
        | Some address ->
            interpret_address context address
      in
      let context = root_result.context in
      let state = context.state in
      match root_opt with
      | None ->
          (None, root_result)
      | Some root -> (
          let type_span =
            match complex.value.complex_address with
            | None ->
                complex_span
            | Some address ->
                Lang_ast.span_of_node address
          in
          match State.find_type state root with
          | None ->
              let message =
                Format.asprintf "Type %a not found in global record"
                  Id.Global.pp root
              in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer type_span message
              in
              (None, add_diagnostic root_result diagnostic)
          | Some { complex= temporary_location; _ } -> (
              match complex.value.complex_block with
              | None ->
                  let namespace = { root; location= temporary_location } in
                  (Some namespace, root_result)
              | Some block ->
                  let location_opt, block_result =
                    interpret_c_block context ~mode ~location:temporary_location
                      block
                  in
                  let combined_result = combine root_result block_result in
                  let namespace_opt =
                    match location_opt with
                    | Some location ->
                        Some { root; location }
                    | None ->
                        None
                  in
                  (namespace_opt, combined_result))))

and interpret_address context address =
  let open Lang_ast in
  let string_or_empty name =
    let raw = Id.Local.to_string name in
    if String.length raw = 0 then "<empty>" else raw
  in
  let rec gather ctx = function
    | [] ->
        ([], ctx)
    | name :: rest ->
        let value, ctx' = interpret_name ctx name in
        let tail, ctx'' = gather ctx' rest in
        ((name, value) :: tail, ctx'')
  in
  let pairs, context = gather context address.value in
  let base_result = empty_result context in
  let state = context.state in
  let module_id = context.current_module in
  let address_span = Lang_ast.span_of_node address in
  let make_error span message =
    let diagnostic =
      Diagnostics.make `Error interpreter_producer span message
    in
    (None, add_diagnostic base_result diagnostic)
  in
  match State.find_module state module_id with
  | None ->
      let message =
        Format.asprintf "Module %s not found" (Id.Module.to_string module_id)
      in
      make_error address_span message
  | Some module_space -> (
      let empty_name = Id.Local.make "" in
      let default_root span =
        match Complex.find_generator module_space empty_name with
        | None ->
            let message =
              Printf.sprintf "Root generator %s not found"
                (string_or_empty empty_name)
            in
            make_error span message
        | Some { tag= `Global root; _ } ->
            (Some root, base_result)
        | Some { tag= `Local _; _ } ->
            assert false
      in
      let rec split_last acc = function
        | [] ->
            None
        | [ x ] ->
            Some (List.rev acc, x)
        | x :: xs ->
            split_last (x :: acc) xs
      in
      match pairs with
      | [] ->
          default_root address_span
      | _ -> (
          match split_last [] pairs with
          | None ->
              default_root address_span
          | Some (prefix, (last_node, last_name)) -> (
              let rec traverse space = function
                | [] ->
                    Ok space
                | (node, name) :: rest -> (
                    match Complex.find_morphism space name with
                    | None ->
                        let message =
                          Printf.sprintf "Map %s not found"
                            (string_or_empty name)
                        in
                        Error (node, message)
                    | Some { domain; _ } -> (
                        match domain with
                        | Complex.Module next_module_id -> (
                            match State.find_module state next_module_id with
                            | Some next_space ->
                                traverse next_space rest
                            | None ->
                                let message =
                                  Format.asprintf "Module %s not found"
                                    (Id.Module.to_string next_module_id)
                                in
                                Error (node, message))
                        | Complex.Type _ ->
                            let message =
                              Printf.sprintf "Domain of %s is not a module"
                                (string_or_empty name)
                            in
                            Error (node, message)))
              in
              match traverse module_space prefix with
              | Error (node, message) ->
                  make_error (Lang_ast.span_of_node node) message
              | Ok final_space -> (
                  match Complex.find_diagram final_space last_name with
                  | None ->
                      let message =
                        Printf.sprintf "Type %s not found"
                          (string_or_empty last_name)
                      in
                      make_error (Lang_ast.span_of_node last_node) message
                  | Some diagram -> (
                      if not (Diagram.is_cell diagram) then
                        let message =
                          Printf.sprintf "%s is not a cell"
                            (string_or_empty last_name)
                        in
                        make_error (Lang_ast.span_of_node last_node) message
                      else
                        let top_dim = Diagram.dim diagram in
                        let labels = Diagram.labels diagram in
                        assert (top_dim < Array.length labels)
                        ; assert (Array.length labels.(top_dim) > 0)
                        ; let top_tag = labels.(top_dim).(0) in
                          match top_tag with
                          | `Global root ->
                              (Some root, base_result)
                          | `Local _ ->
                              assert false)))))

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
        |> Complex.add_generator ~name:empty_name ~classifier:empty_diagram
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
      let _, result = interpret_c_block_type ~loader context c_block_type in
      result
  | Block_complex { block_complex_body; block_local_body } -> (
      let namespace_opt, complex_result =
        interpret_complex context ~mode:Local block_complex_body
      in
      let context = complex_result.context in
      let diagnostics = complex_result.diagnostics in
      let status = complex_result.status in
      match (namespace_opt, block_local_body) with
      | Some namespace, Some local_block when status <> `Error ->
          let _, local_result =
            interpret_c_block_local context namespace local_block
          in
          {
            context= local_result.context;
            diagnostics= Report.append diagnostics local_result.diagnostics;
            status= local_result.status;
          }
      | _ ->
          { context; diagnostics; status })

let interpret_generator_type context generator_type =
  stub_node "generator_type" context generator_type

let interpret_generator context ~location:_ generator =
  stub_node "generator" context generator

let interpret_boundaries context ~location:_ boundaries =
  stub_node "boundaries" context boundaries

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

let interpret_include_module context include_mod =
  stub_node "include_module" context include_mod

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
