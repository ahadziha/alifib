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
type morphism_component = { morphism: Morphism.t; source: Complex.t }
type term = M_term of morphism_component | D_term of Diagram.t
type component = Term of term | Hole | Bd of Diagram.sign

type term_pair =
  | M_term_pair of { fst: Morphism.t; snd: Morphism.t; source: Complex.t }
  | D_term_pair of { fst: Diagram.t; snd: Diagram.t }

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

let combine ~previous ~next =
  let diagnostics = Report.append previous.diagnostics next.diagnostics in
  let status =
    match (previous.status, next.status) with
    | `Error, _ | _, `Error ->
        `Error
    | `Ok, `Ok ->
        `Ok
  in
  { context= next.context; diagnostics; status }

let has_errors { status; _ } = match status with `Error -> true | `Ok -> false

let interpreter_producer =
  { Error.Located.phase= `Interpreter; module_path= Some "interpreter" }

let unknown_span = Positions.point_span Positions.unknown_point
let span_or_unknown = function Some span -> span | None -> unknown_span

module Lang_ast = struct
  include Ast

  let span_of_node (node : _ node) = span_or_unknown node.span
end

let report_has_errors (report : Diagnostics.report) =
  List.exists
    (fun (diag : Diagnostics.t) -> diag.Diagnostics.severity = `Error)
    report

let identity_morphism context domain =
  let entries =
    Complex.generator_names domain
    |> List.map (fun name ->
           let generator_entry =
             match Complex.find_generator domain name with
             | Some entry ->
                 entry
             | None ->
                 assert false
           in
           let tag = generator_entry.tag in
           let dim = generator_entry.dim in
           let cell_data =
             match tag with
             | `Global global_id -> (
                 match State.find_cell context.state global_id with
                 | Some { data; _ } ->
                     data
                 | None -> (
                     match State.find_type context.state global_id with
                     | Some { data; _ } ->
                         data
                     | None ->
                         assert false))
             | `Local local_name -> (
                 match Complex.find_local_cell domain local_name with
                 | Some { data; _ } ->
                     data
                 | None ->
                     assert false)
           in
           let image =
             match Complex.classifier domain name with
             | Some classifier ->
                 classifier
             | None ->
                 assert false
           in
           (tag, dim, cell_data, image))
  in
  Morphism.of_entries entries ~cellular:true

let rec smart_extend context morphism ~source ~target ~tag ~dim ~diagram =
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
          | Some { data; _ } ->
              data
          | None -> (
              match State.find_type context.state global_id with
              | Some { data; _ } ->
                  data
              | None ->
                  assert false))
      | `Local name -> (
          match Complex.find_local_cell source name with
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
              | Diagram.Boundary { boundary_in; _ }, `Input -> (
                  match Diagram.boundary `Input dim_minus_one diagram with
                  | Error _ as err ->
                      err
                  | Ok boundary ->
                      Ok (boundary_in, boundary))
              | Diagram.Boundary { boundary_out; _ }, `Output -> (
                  match Diagram.boundary `Output dim_minus_one diagram with
                  | Error _ as err ->
                      err
                  | Ok boundary ->
                      Ok (boundary_out, boundary))
              | Diagram.Zero, _ ->
                  assert false
            in
            match boundary_sources with
            | Error _ as err ->
                err
            | Ok (boundary_diag, target_boundary) -> (
                if Diagram.is_cell boundary_diag then
                  match
                    smart_extend context partial ~source ~target ~tag:focus
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
                                  Complex.find_generator_by_tag target
                                    image_focus
                                with
                                | Some name ->
                                    name
                                | None ->
                                    assert false
                              in
                              let d_focus =
                                match
                                  Complex.classifier target generator_name
                                with
                                | Some diagram ->
                                    diagram
                                | None ->
                                    assert false
                              in
                              match
                                smart_extend context partial ~source ~target
                                  ~tag:focus ~dim:dim_minus_one ~diagram:d_focus
                              with
                              | Error _ as err ->
                                  err
                              | Ok updated ->
                                  extend_missing updated rest))))
    in
    extend_missing morphism missing_tags

let interpret_name (name : Lang_ast.name) = name.value
let interpret_nat (nat : Lang_ast.nat) = nat.value
let interpret_bd (bd : Lang_ast.bd) = bd.value

let interpret_address context address =
  let open Lang_ast in
  let string_or_empty name =
    let raw = Id.Local.to_string name in
    if String.length raw = 0 then "<empty>" else raw
  in
  let rec gather = function
    | [] ->
        []
    | name :: rest ->
        let value = interpret_name name in
        (name, value) :: gather rest
  in
  let pairs = gather address.value in
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

let interpret_include context include_stmt =
  let open Lang_ast in
  let include_desc = include_stmt.value in
  let address_node = include_desc.include_address in
  let address_output, address_result = interpret_address context address_node in
  match address_output with
  | None ->
      (None, address_result)
  | Some id -> (
      match include_desc.include_alias with
      | Some alias_node ->
          let alias = interpret_name alias_node in
          (Some (id, alias), address_result)
      | None -> (
          let module_id = context.current_module in
          match State.find_module context.state module_id with
          | None ->
              assert false
          | Some module_complex -> (
              let tag = Id.Tag.of_global id in
              match Complex.find_generator_by_tag module_complex tag with
              | None ->
                  assert false
              | Some generator_name ->
                  let name_str = Id.Local.to_string generator_name in
                  if String.contains name_str '.' then
                    let span = Lang_ast.span_of_node include_stmt in
                    let message =
                      "Inclusion of non-local types requires an alias"
                    in
                    let diagnostic =
                      Diagnostics.make `Error interpreter_producer span message
                    in
                    (None, add_diagnostic address_result diagnostic)
                  else (Some (id, generator_name), address_result))))

let interpret_include_module (include_mod : Lang_ast.include_module) =
  let include_desc = include_mod.value in
  let name = interpret_name include_desc.include_module_name in
  let alias =
    match include_desc.include_module_alias with
    | Some alias_node ->
        interpret_name alias_node
    | None ->
        name
  in
  (name, alias)

let validate_as_source morphism source =
  let tags = Morphism.domain_of_definition morphism in
  let cell_data_compatible lhs rhs =
    match (lhs, rhs) with
    | Diagram.Zero, Diagram.Zero ->
        true
    | ( Diagram.Boundary { boundary_in= in_lhs; boundary_out= out_lhs },
        Diagram.Boundary { boundary_in= in_rhs; boundary_out= out_rhs } ) ->
        Diagram.isomorphic in_lhs in_rhs && Diagram.isomorphic out_lhs out_rhs
    | _ ->
        false
  in
  let rec validate_tags = function
    | [] ->
        Ok ()
    | tag :: rest -> (
        if Option.is_none (Complex.find_generator_by_tag source tag) then
          Error "Map defined outside of the specified domain"
        else
          let check_local =
            match tag with
            | `Local local_name -> (
                match
                  ( Complex.find_local_cell source local_name,
                    Morphism.cell_data morphism tag )
                with
                | Some { data= source_data; _ }, Ok morphism_data ->
                    if cell_data_compatible source_data morphism_data then Ok ()
                    else Error "Local cell mismatch between map and complex"
                | _ ->
                    Error "Local cell mismatch between map and complex")
            | `Global _ ->
                Ok ()
          in
          match check_local with
          | Ok () ->
              validate_tags rest
          | Error _ as err ->
              err)
  in
  match validate_tags tags with
  | Ok () ->
      Ok ()
  | Error message ->
      Error message

let rec interpret_morphism context ~location morphism =
  let open Lang_ast in
  match morphism.Lang_ast.value with
  | Morphism_single m_comp ->
      interpret_m_comp context ~location m_comp
  | Morphism_concat { morphism_left; morphism_right } -> (
      let left_output, left_result =
        interpret_morphism context ~location morphism_left
      in
      match left_output with
      | None ->
          (None, left_result)
      | Some left_component -> (
          let right_output, right_result =
            interpret_m_comp left_result.context ~location:left_component.source
              morphism_right
          in
          let combined_result =
            combine ~previous:left_result ~next:right_result
          in
          match right_output with
          | None ->
              (None, combined_result)
          | Some right_component ->
              let composed =
                Morphism.compose left_component.morphism
                  right_component.morphism
              in
              ( Some { morphism= composed; source= right_component.source },
                combined_result )))

and interpret_m_comp context ~location m_comp =
  let open Lang_ast in
  match m_comp.value with
  | M_comp_term m_term ->
      interpret_m_term context ~location m_term
  | M_comp_name name_node -> (
      let name = interpret_name name_node in
      let base_result = empty_result context in
      match Complex.find_morphism location name with
      | None ->
          let span = Lang_ast.span_of_node name_node in
          let message =
            Format.asprintf "Map not found: %s" (Id.Local.to_string name)
          in
          let diagnostic =
            Diagnostics.make `Error interpreter_producer span message
          in
          (None, add_diagnostic base_result diagnostic)
      | Some entry ->
          let source =
            match entry.domain with
            | Complex.Type id -> (
                match State.find_type context.state id with
                | Some { complex; _ } ->
                    complex
                | None ->
                    assert false)
            | Complex.Module module_id -> (
                match State.find_module context.state module_id with
                | Some complex ->
                    complex
                | None ->
                    assert false)
          in
          (Some { morphism= entry.morphism; source }, base_result))

and interpret_m_term context ~location m_term =
  let Lang_ast.{ value= m_term_desc; _ } = m_term in
  let target_complex = m_term_desc.m_term_target in
  let namespace_opt, complex_result =
    interpret_complex context ~mode:Local target_complex
  in
  match namespace_opt with
  | None ->
      (None, complex_result)
  | Some namespace -> (
      let source = namespace.location in
      let ext_node = m_term_desc.m_term_ext in
      let ext_output, ext_result =
        interpret_m_ext complex_result.context ~location ~source ext_node
      in
      let combined_result = combine ~previous:complex_result ~next:ext_result in
      match ext_output with
      | None ->
          (None, combined_result)
      | Some morphism ->
          (Some { morphism; source }, combined_result))

and interpret_m_ext context ~location ~source m_ext =
  let open Lang_ast in
  let span = Lang_ast.span_of_node m_ext in
  let m_ext_desc = m_ext.value in
  let prefix_opt, block_opt =
    (m_ext_desc.m_ext_prefix, m_ext_desc.m_ext_block)
  in
  let component_opt, prefix_result =
    match prefix_opt with
    | None -> (
        match Morphism.init () with
        | Ok morphism ->
            (Some { morphism; source }, empty_result context)
        | Error _ ->
            assert false)
    | Some morphism_node ->
        interpret_morphism context ~location morphism_node
  in
  match component_opt with
  | None ->
      (None, prefix_result)
  | Some { morphism; source= morphism_source } -> (
      let ensure_source result =
        if morphism_source == source then Ok result
        else
          match validate_as_source morphism source with
          | Ok () ->
              Ok result
          | Error message ->
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span message
              in
              Error (add_diagnostic result diagnostic)
      in
      match ensure_source prefix_result with
      | Error result_with_error ->
          (None, result_with_error)
      | Ok validated_result -> (
          match block_opt with
          | None ->
              (Some morphism, validated_result)
          | Some block_node ->
              let block_output, block_result =
                interpret_m_block validated_result.context ~location ~source
                  ~morphism block_node
              in
              let combined_result =
                combine ~previous:validated_result ~next:block_result
              in
              (block_output, combined_result)))

and interpret_diagram context ~location diagram =
  let open Lang_ast in
  match diagram.value with
  | Diagram_single concat_node ->
      interpret_d_concat context ~location concat_node
  | Diagram_paste { diagram_left; diagram_nat; diagram_right } ->
      interpret_diagram_paste_case context ~location
        ~span:(Lang_ast.span_of_node diagram)
        ~diagram_left ~diagram_nat ~diagram_right

and interpret_diagram_paste_case context ~location ~span ~diagram_left
    ~diagram_nat ~diagram_right =
  let right_opt, right_result =
    interpret_d_concat context ~location diagram_right
  in
  match right_opt with
  | None ->
      (None, right_result)
  | Some d_right -> (
      let left_opt, left_result =
        interpret_diagram right_result.context ~location diagram_left
      in
      let combined_result = combine ~previous:right_result ~next:left_result in
      match left_opt with
      | None ->
          (None, combined_result)
      | Some d_left -> (
          let k = interpret_nat diagram_nat in
          match Diagram.paste k d_left d_right with
          | Ok diagram ->
              (Some diagram, combined_result)
          | Error err ->
              let message =
                Format.asprintf "Failed to paste diagrams: %a" Error.pp err
              in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span message
              in
              (None, add_diagnostic combined_result diagnostic)))

and interpret_d_concat context ~location d_concat =
  let open Lang_ast in
  match d_concat.value with
  | D_concat_single expr_node -> (
      let term_opt, expr_result =
        interpret_d_expr context ~location expr_node
      in
      match term_opt with
      | None ->
          (None, expr_result)
      | Some term -> (
          match term with
          | M_term _ ->
              let span = Lang_ast.span_of_node expr_node in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span
                  "Not a diagram"
              in
              (None, add_diagnostic expr_result diagnostic)
          | D_term diagram ->
              (Some diagram, expr_result)))
  | D_concat_concat { d_concat_left; d_concat_right } ->
      interpret_concat_concat_case context ~location
        ~span:(Lang_ast.span_of_node d_concat)
        ~left_concat:d_concat_left ~right_expr:d_concat_right

and interpret_concat_concat_case context ~location ~span ~left_concat
    ~right_expr =
  let term_opt, right_result = interpret_d_expr context ~location right_expr in
  match term_opt with
  | None ->
      (None, right_result)
  | Some term -> (
      match term with
      | M_term _ ->
          let diagnostic =
            Diagnostics.make `Error interpreter_producer span "Not a diagram"
          in
          (None, add_diagnostic right_result diagnostic)
      | D_term d_right -> (
          let left_opt, left_result =
            interpret_d_concat right_result.context ~location left_concat
          in
          let combined_result =
            combine ~previous:right_result ~next:left_result
          in
          match left_opt with
          | None ->
              (None, combined_result)
          | Some d_left -> (
              let k = min (Diagram.dim d_left) (Diagram.dim d_right) - 1 in
              match Diagram.paste k d_left d_right with
              | Ok diagram ->
                  (Some diagram, combined_result)
              | Error err ->
                  let message =
                    Format.asprintf "Failed to paste diagrams: %a" Error.pp err
                  in
                  let diagnostic =
                    Diagnostics.make `Error interpreter_producer span message
                  in
                  (None, add_diagnostic combined_result diagnostic))))

and interpret_d_expr context ~location d_expr =
  let open Lang_ast in
  match d_expr.value with
  | D_expr_single d_comp -> (
      let component_opt, component_result =
        interpret_d_comp context ~location d_comp
      in
      match component_opt with
      | None ->
          (None, component_result)
      | Some component -> (
          match component with
          | Hole | Bd _ ->
              let span = Lang_ast.span_of_node d_comp in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span
                  "Not a diagram or map"
              in
              (None, add_diagnostic component_result diagnostic)
          | Term term ->
              (Some term, component_result)))
  | D_expr_dot { d_expr_left; d_expr_right } -> (
      let left_opt, left_result =
        interpret_d_expr context ~location d_expr_left
      in
      match left_opt with
      | None ->
          (None, left_result)
      | Some term_left -> (
          match term_left with
          | D_term diagram -> (
              let component_opt, component_result =
                interpret_d_comp left_result.context ~location d_expr_right
              in
              let combined_result =
                combine ~previous:left_result ~next:component_result
              in
              match component_opt with
              | None ->
                  (None, combined_result)
              | Some component -> (
                  match component with
                  | Term _ | Hole ->
                      let span = Lang_ast.span_of_node d_expr_right in
                      let diagnostic =
                        Diagnostics.make `Error interpreter_producer span
                          "Not a well-formed diagram"
                      in
                      (None, add_diagnostic combined_result diagnostic)
                  | Bd sign -> (
                      let k = Diagram.dim diagram - 1 in
                      match Diagram.boundary sign k diagram with
                      | Error err ->
                          let span = Lang_ast.span_of_node d_expr_right in
                          let diagnostic =
                            Diagnostics.make `Error interpreter_producer span
                              err.Error.message
                          in
                          (None, add_diagnostic combined_result diagnostic)
                      | Ok d_boundary ->
                          (Some (D_term d_boundary), combined_result))))
          | M_term { morphism= m_left; source= c_left } -> (
              let component_opt, component_result =
                interpret_d_comp left_result.context ~location:c_left
                  d_expr_right
              in
              let combined_result =
                combine ~previous:left_result ~next:component_result
              in
              match component_opt with
              | None ->
                  (None, combined_result)
              | Some component -> (
                  match component with
                  | Hole | Bd _ ->
                      let span = Lang_ast.span_of_node d_expr_right in
                      let diagnostic =
                        Diagnostics.make `Error interpreter_producer span
                          "Not a diagram or map"
                      in
                      (None, add_diagnostic combined_result diagnostic)
                  | Term term_right -> (
                      match term_right with
                      | D_term diagram -> (
                          match Morphism.apply m_left diagram with
                          | Error err ->
                              let span = Lang_ast.span_of_node d_expr_right in
                              let diagnostic =
                                Diagnostics.make `Error interpreter_producer
                                  span err.Error.message
                              in
                              (None, add_diagnostic combined_result diagnostic)
                          | Ok d_image ->
                              (Some (D_term d_image), combined_result))
                      | M_term { morphism= m_right; source= c_right } ->
                          let morphism = Morphism.compose m_left m_right in
                          ( Some (M_term { morphism; source= c_right }),
                            combined_result ))))))

and interpret_d_comp context ~location d_comp =
  let open Lang_ast in
  match d_comp.value with
  | D_comp_mterm m_term -> (
      let component_opt, component_result =
        interpret_m_term context ~location m_term
      in
      match component_opt with
      | None ->
          (None, component_result)
      | Some { morphism; source } ->
          (Some (Term (M_term { morphism; source })), component_result))
  | D_comp_dterm d_term -> (
      let diagram_opt, diagram_result =
        interpret_d_term context ~location d_term
      in
      match diagram_opt with
      | None ->
          (None, diagram_result)
      | Some diagram ->
          (Some (Term (D_term diagram)), diagram_result))
  | D_comp_name name_node -> (
      let name = interpret_name name_node in
      let span = Lang_ast.span_of_node name_node in
      let base_result = empty_result context in
      let name_str = Id.Local.to_string name in
      match Complex.find_diagram location name with
      | Some diagram ->
          (Some (Term (D_term diagram)), base_result)
      | None -> (
          match Complex.find_morphism location name with
          | Some entry ->
              let source =
                match entry.domain with
                | Complex.Type id -> (
                    match State.find_type context.state id with
                    | Some { complex; _ } ->
                        complex
                    | None ->
                        assert false)
                | Complex.Module module_id -> (
                    match State.find_module context.state module_id with
                    | Some complex ->
                        complex
                    | None ->
                        assert false)
              in
              ( Some (Term (M_term { morphism= entry.morphism; source })),
                base_result )
          | None ->
              let message = Format.asprintf "Name `%s` not found" name_str in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span message
              in
              (None, add_diagnostic base_result diagnostic)))
  | D_comp_bd bd_node ->
      let sign = interpret_bd bd_node in
      (Some (Bd sign), empty_result context)
  | D_comp_hole ->
      (Some Hole, empty_result context)

and interpret_d_term context ~location d_term =
  let open Lang_ast in
  match d_term.value with
  | D_term_indexed { d_term_diagram; d_term_nat; d_term_tail } ->
      interpret_diagram_paste_case context ~location
        ~span:(Lang_ast.span_of_node d_term)
        ~diagram_left:d_term_diagram ~diagram_nat:d_term_nat
        ~diagram_right:d_term_tail
  | D_term_pair { d_term_concat; d_term_expr } ->
      interpret_concat_concat_case context ~location
        ~span:(Lang_ast.span_of_node d_term)
        ~left_concat:d_term_concat ~right_expr:d_term_expr

and interpret_concat context ~location concat =
  let open Lang_ast in
  match concat.value with
  | Concat_single d_expr ->
      interpret_d_expr context ~location d_expr
  | Concat_concat { concat_left; concat_right } -> (
      let right_term_opt, right_result =
        interpret_d_expr context ~location concat_right
      in
      match right_term_opt with
      | None ->
          (None, right_result)
      | Some term_right -> (
          match term_right with
          | M_term _ ->
              let span = Lang_ast.span_of_node concat_right in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span
                  "Not a diagram"
              in
              (None, add_diagnostic right_result diagnostic)
          | D_term d_right -> (
              let left_term_opt, left_result =
                interpret_concat right_result.context ~location concat_left
              in
              let combined_result =
                combine ~previous:right_result ~next:left_result
              in
              match left_term_opt with
              | None ->
                  (None, combined_result)
              | Some term_left -> (
                  match term_left with
                  | M_term _ ->
                      let span = Lang_ast.span_of_node concat_left in
                      let diagnostic =
                        Diagnostics.make `Error interpreter_producer span
                          "Not a diagram"
                      in
                      (None, add_diagnostic combined_result diagnostic)
                  | D_term d_left -> (
                      let k =
                        min (Diagram.dim d_left) (Diagram.dim d_right) - 1
                      in
                      match Diagram.paste k d_left d_right with
                      | Ok diagram ->
                          (Some (D_term diagram), combined_result)
                      | Error err ->
                          let span = Lang_ast.span_of_node concat in
                          let message =
                            Format.asprintf "Failed to paste diagrams: %a"
                              Error.pp err
                          in
                          let diagnostic =
                            Diagnostics.make `Error interpreter_producer span
                              message
                          in
                          (None, add_diagnostic combined_result diagnostic))))))

and interpret_boundaries context ~location boundaries =
  let open Lang_ast in
  let { value= boundaries_desc; _ } = boundaries in
  let source_node = boundaries_desc.boundaries_source in
  let target_node = boundaries_desc.boundaries_target in
  let boundary_in_opt, source_result =
    interpret_diagram context ~location source_node
  in
  match boundary_in_opt with
  | None ->
      (None, source_result)
  | Some boundary_in -> (
      let boundary_out_opt, target_result =
        interpret_diagram source_result.context ~location target_node
      in
      let combined_result =
        combine ~previous:source_result ~next:target_result
      in
      match boundary_out_opt with
      | None ->
          (None, combined_result)
      | Some boundary_out ->
          ( Some (Diagram.Boundary { boundary_in; boundary_out }),
            combined_result ))

and interpret_dnamer context ~location dnamer =
  let open Lang_ast in
  let dnamer_desc = dnamer.value in
  let diagram_opt, diagram_result =
    interpret_diagram context ~location dnamer_desc.dnamer_body
  in
  match diagram_opt with
  | None ->
      (None, diagram_result)
  | Some diagram -> (
      let name = interpret_name dnamer_desc.dnamer_name in
      let context_after = diagram_result.context in
      match dnamer_desc.dnamer_boundaries with
      | None ->
          (Some (name, diagram), diagram_result)
      | Some boundaries_node -> (
          let boundaries_opt, boundaries_result =
            interpret_boundaries context_after ~location boundaries_node
          in
          let combined_result =
            combine ~previous:diagram_result ~next:boundaries_result
          in
          match boundaries_opt with
          | None ->
              (None, combined_result)
          | Some boundaries -> (
              match boundaries with
              | Diagram.Zero ->
                  assert false
              | Diagram.Boundary { boundary_in; boundary_out } -> (
                  let boundary_span = Lang_ast.span_of_node boundaries_node in
                  let boundary_idx = Diagram.dim diagram - 1 in
                  let boundary_error kind =
                    let message =
                      match kind with
                      | `Input ->
                          "Diagram does not match input boundary annotation"
                      | `Output ->
                          "Diagram does not match output boundary annotation"
                    in
                    let diagnostic =
                      Diagnostics.make `Error interpreter_producer boundary_span
                        message
                    in
                    (None, add_diagnostic combined_result diagnostic)
                  in
                  let handle_boundary_error err =
                    let diagnostic =
                      Diagnostics.make `Error interpreter_producer boundary_span
                        err.Error.message
                    in
                    (None, add_diagnostic combined_result diagnostic)
                  in
                  match Diagram.boundary_normal `Input boundary_idx diagram with
                  | Error err ->
                      handle_boundary_error err
                  | Ok input_boundary -> (
                      if not (Diagram.isomorphic input_boundary boundary_in)
                      then boundary_error `Input
                      else
                        match
                          Diagram.boundary_normal `Output boundary_idx diagram
                        with
                        | Error err ->
                            handle_boundary_error err
                        | Ok output_boundary ->
                            if
                              not
                                (Diagram.isomorphic output_boundary boundary_out)
                            then boundary_error `Output
                            else (Some (name, diagram), combined_result))))))

and interpret_m_def context ~location ~source m_def =
  let open Lang_ast in
  match m_def.value with
  | M_def_morphism morph_node -> (
      let morph_output, morph_result =
        interpret_morphism context ~location morph_node
      in
      match morph_output with
      | None ->
          (None, morph_result)
      | Some { morphism; source= morph_source } -> (
          if morph_source == source then (Some morphism, morph_result)
          else
            match validate_as_source morphism source with
            | Ok () ->
                (Some morphism, morph_result)
            | Error message ->
                let span = Lang_ast.span_of_node m_def in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer span message
                in
                (None, add_diagnostic morph_result diagnostic)))
  | M_def_ext ext_node ->
      interpret_m_ext context ~location ~source ext_node

and interpret_mnamer context ~location mnamer =
  let open Lang_ast in
  let mnamer_desc = mnamer.value in
  let address_node = mnamer_desc.mnamer_address in
  let address_output, address_result = interpret_address context address_node in
  match address_output with
  | None ->
      (None, address_result)
  | Some id -> (
      let context_after = address_result.context in
      let state = context_after.state in
      match State.find_type state id with
      | None ->
          assert false
      | Some { complex= source; _ } -> (
          let m_def_node = mnamer_desc.mnamer_definition in
          let m_def_output, m_def_result =
            interpret_m_def context_after ~location ~source m_def_node
          in
          match m_def_output with
          | None ->
              (None, m_def_result)
          | Some morphism ->
              let name = interpret_name mnamer_desc.mnamer_name in
              (Some (name, morphism, Complex.Type id), m_def_result)))

and interpret_generator context ~location generator =
  let open Lang_ast in
  let generator_desc = generator.value in
  let name_node = generator_desc.generator_name in
  let name = interpret_name name_node in
  match generator_desc.generator_boundaries with
  | Some boundaries_node -> (
      let boundaries_output, boundaries_result =
        interpret_boundaries context ~location boundaries_node
      in
      match boundaries_output with
      | None ->
          (None, boundaries_result)
      | Some boundaries ->
          (Some (name, boundaries), boundaries_result))
  | None ->
      (Some (name, Diagram.Zero), empty_result context)

and interpret_attach context ~location attach =
  let open Lang_ast in
  let attach_desc = attach.value in
  let address_node = attach_desc.attach_address in
  let address_output, address_result = interpret_address context address_node in
  match address_output with
  | None ->
      (None, address_result)
  | Some id -> (
      let context_after = address_result.context in
      let name = interpret_name attach_desc.attach_name in
      match attach_desc.attach_along with
      | None -> (
          match Morphism.init () with
          | Error _ ->
              assert false
          | Ok morphism ->
              (Some (name, morphism, Complex.Type id), address_result))
      | Some m_def_node -> (
          let state = context_after.state in
          let source =
            match State.find_type state id with
            | Some { complex; _ } ->
                complex
            | None ->
                assert false
          in
          let m_def_output, m_def_result =
            interpret_m_def context_after ~location ~source m_def_node
          in
          match m_def_output with
          | None ->
              (None, m_def_result)
          | Some morphism ->
              (Some (name, morphism, Complex.Type id), m_def_result)))

and interpret_pasting context ~location pasting =
  let open Lang_ast in
  match pasting.value with
  | Pasting_single concat_node ->
      interpret_concat context ~location concat_node
  | Pasting_paste { pasting_left; pasting_nat; pasting_right } ->
      interpret_pasting_paste_case context ~location ~pasting_node:pasting
        pasting_left pasting_right pasting_nat

and interpret_pasting_paste_case context ~location ~pasting_node pasting_left
    pasting_right pasting_nat =
  let span_right = Lang_ast.span_of_node pasting_right in
  let span_left = Lang_ast.span_of_node pasting_left in
  let span_paste = Lang_ast.span_of_node pasting_node in
  let error_not_diagram result span =
    let diagnostic =
      Diagnostics.make `Error interpreter_producer span "Not a diagram"
    in
    (None, add_diagnostic result diagnostic)
  in
  let right_term_opt, right_result =
    interpret_concat context ~location pasting_right
  in
  match right_term_opt with
  | None ->
      (None, right_result)
  | Some term_right -> (
      match term_right with
      | M_term _ ->
          error_not_diagram right_result span_right
      | D_term d_right -> (
          let left_term_opt, left_result =
            interpret_pasting right_result.context ~location pasting_left
          in
          let combined_result =
            combine ~previous:right_result ~next:left_result
          in
          match left_term_opt with
          | None ->
              (None, combined_result)
          | Some term_left -> (
              match term_left with
              | M_term _ ->
                  error_not_diagram combined_result span_left
              | D_term d_left -> (
                  let k = interpret_nat pasting_nat in
                  match Diagram.paste k d_left d_right with
                  | Ok diagram ->
                      (Some (D_term diagram), combined_result)
                  | Error err ->
                      let message =
                        Format.asprintf "Failed to paste diagrams: %a" Error.pp
                          err
                      in
                      let diagnostic =
                        Diagnostics.make `Error interpreter_producer span_paste
                          message
                      in
                      (None, add_diagnostic combined_result diagnostic)))))

and interpret_assert context ~location assert_stmt =
  let Lang_ast.{ value= assert_desc; _ } = assert_stmt in
  let Lang_ast.{ assert_left= left_pasting; assert_right= right_pasting } =
    assert_desc
  in
  let left_term_opt, left_result =
    interpret_pasting context ~location left_pasting
  in
  match left_term_opt with
  | None ->
      (None, left_result)
  | Some term_left -> (
      let right_term_opt, right_result =
        interpret_pasting left_result.context ~location right_pasting
      in
      let combined_result = combine ~previous:left_result ~next:right_result in
      match right_term_opt with
      | None ->
          (None, combined_result)
      | Some term_right -> (
          match (term_left, term_right) with
          | D_term d_left, D_term d_right ->
              (Some (D_term_pair { fst= d_left; snd= d_right }), combined_result)
          | ( M_term { morphism= m_left; source= source_left },
              M_term { morphism= m_right; source= source_right } ) ->
              if source_left == source_right then
                ( Some
                    (M_term_pair
                       { fst= m_left; snd= m_right; source= source_left }),
                  combined_result )
              else
                let span = Lang_ast.span_of_node assert_stmt in
                let message =
                  "The two sides of the equation are incomparable"
                in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer span message
                in
                (None, add_diagnostic combined_result diagnostic)
          | _ ->
              let span = Lang_ast.span_of_node assert_stmt in
              let message = "The two sides of the equation are incomparable" in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span message
              in
              (None, add_diagnostic combined_result diagnostic)))

and interpret_c_instr_local context namespace c_instr_local =
  let { root; location } = namespace in
  let root_location =
    match State.find_type context.state root with
    | Some { complex; _ } ->
        complex
    | None ->
        assert false
  in
  let Lang_ast.{ value= instr_desc; _ } = c_instr_local in
  match instr_desc with
  | Lang_ast.C_instr_local_dnamer dnamer -> (
      let dnamer_output, dnamer_result =
        interpret_dnamer context ~location dnamer
      in
      let context_after = dnamer_result.context in
      match dnamer_output with
      | None ->
          (None, dnamer_result)
      | Some (name, diagram) ->
          if Complex.name_in_use location name then
            let name_span =
              let dnamer_desc = dnamer.value in
              Lang_ast.span_of_node dnamer_desc.dnamer_name
            in
            let message =
              Format.asprintf "Diagram name already in use: %s"
                (Id.Local.to_string name)
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer name_span message
            in
            (None, add_diagnostic dnamer_result diagnostic)
          else if Diagram.has_local_labels diagram then
            let diagram_span =
              let dnamer_desc = dnamer.value in
              Lang_ast.span_of_node dnamer_desc.dnamer_body
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer diagram_span
                "Named diagrams must contain only global cells"
            in
            (None, add_diagnostic dnamer_result diagnostic)
          else
            let updated_location = Complex.add_diagram location ~name diagram in
            let updated_root_location =
              Complex.add_diagram root_location ~name diagram
            in
            let state_with_root =
              State.update_type_complex context_after.state ~id:root
                updated_root_location
            in
            let context_updated = with_state context_after state_with_root in
            let updated_result =
              { dnamer_result with context= context_updated }
            in
            (Some updated_location, updated_result))
  | Lang_ast.C_instr_local_mnamer mnamer -> (
      let mnamer_output, mnamer_result =
        interpret_mnamer context ~location mnamer
      in
      let context_after = mnamer_result.context in
      match mnamer_output with
      | None ->
          (None, mnamer_result)
      | Some (name, morphism, domain) ->
          if Complex.name_in_use location name then
            let name_span =
              let mnamer_desc = mnamer.value in
              Lang_ast.span_of_node mnamer_desc.mnamer_name
            in
            let message =
              Format.asprintf "Map name already in use: %s"
                (Id.Local.to_string name)
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer name_span message
            in
            (None, add_diagnostic mnamer_result diagnostic)
          else
            let definition_span =
              let mnamer_desc = mnamer.value in
              Lang_ast.span_of_node mnamer_desc.mnamer_definition
            in
            if Morphism.has_local_labels morphism then
              let diagnostic =
                Diagnostics.make `Error interpreter_producer definition_span
                  "Named maps must only be valued in global cells"
              in
              (None, add_diagnostic mnamer_result diagnostic)
            else
              let updated_location =
                Complex.add_morphism location ~name ~domain morphism
              in
              let updated_root_location =
                Complex.add_morphism root_location ~name ~domain morphism
              in
              let state_with_root =
                State.update_type_complex context_after.state ~id:root
                  updated_root_location
              in
              let context_updated = with_state context_after state_with_root in
              let updated_result =
                { mnamer_result with context= context_updated }
              in
              (Some updated_location, updated_result))
  | Lang_ast.C_instr_local_assert assert_stmt -> (
      let term_pair_opt, assert_result =
        interpret_assert context ~location assert_stmt
      in
      let span = Lang_ast.span_of_node assert_stmt in
      match term_pair_opt with
      | None ->
          (None, assert_result)
      | Some term_pair -> (
          match term_pair with
          | M_term_pair record -> (
              let fst = record.fst in
              let snd = record.snd in
              let source = record.source in
              if fst == snd then (Some location, assert_result)
              else
                let generators =
                  Complex.generator_names source
                  |> List.filter_map (fun gen_name ->
                         match Complex.find_generator source gen_name with
                         | Some entry ->
                             Some (entry.dim, gen_name, entry.tag)
                         | None ->
                             None)
                  |> List.sort (fun (dim_a, _, _) (dim_b, _, _) ->
                         Int.compare dim_a dim_b)
                in
                let rec check_generators = function
                  | [] ->
                      None
                  | (_, gen_name, tag) :: rest ->
                      let in_first = Morphism.is_defined_at fst tag in
                      let in_second = Morphism.is_defined_at snd tag in
                      let name_str = Id.Local.to_string gen_name in
                      if in_first && not in_second then
                        Some
                          (Format.asprintf
                             "%s is in the domain of definition of the first \
                              map, but not the second map"
                             name_str)
                      else if in_second && not in_first then
                        Some
                          (Format.asprintf
                             "%s is in the domain of definition of the second \
                              map, but not the first map"
                             name_str)
                      else if in_first then
                        let image_first =
                          match Morphism.image fst tag with
                          | Ok diagram ->
                              diagram
                          | Error _ ->
                              assert false
                        in
                        let image_second =
                          match Morphism.image snd tag with
                          | Ok diagram ->
                              diagram
                          | Error _ ->
                              assert false
                        in
                        if Diagram.isomorphic image_first image_second then
                          check_generators rest
                        else
                          Some
                            (Format.asprintf "The maps differ on %s" name_str)
                      else check_generators rest
                in
                match check_generators generators with
                | None ->
                    (Some location, assert_result)
                | Some message ->
                    let diagnostic =
                      Diagnostics.make `Error interpreter_producer span message
                    in
                    (None, add_diagnostic assert_result diagnostic))
          | D_term_pair record ->
              let fst = record.fst in
              let snd = record.snd in
              if Diagram.isomorphic fst snd then (Some location, assert_result)
              else
                let message = "The diagrams are not equal" in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer span message
                in
                (None, add_diagnostic assert_result diagnostic)))

and interpret_c_block_local context namespace
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
        let combined = combine ~previous:acc_result ~next:instr_result in
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

and interpret_c_instr context ~mode ~location c_instr =
  let open Lang_ast in
  match c_instr.value with
  | C_instr_generator generator -> (
      let generator_output, generator_result =
        interpret_generator context ~location generator
      in
      let context_after = generator_result.context in
      match generator_output with
      | None ->
          (None, generator_result)
      | Some (name, boundaries) -> (
          if Complex.name_in_use location name then
            let name_span =
              let generator_desc = generator.value in
              Lang_ast.span_of_node generator_desc.generator_name
            in
            let message =
              Format.asprintf "Generator name already in use: %s"
                (Id.Local.to_string name)
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer name_span message
            in
            (None, add_diagnostic generator_result diagnostic)
          else
            let boundaries_span =
              let generator_desc = generator.value in
              match generator_desc.generator_boundaries with
              | Some boundaries_node ->
                  Lang_ast.span_of_node boundaries_node
              | None ->
                  Lang_ast.span_of_node generator
            in
            let dim =
              match boundaries with
              | Diagram.Zero ->
                  0
              | Diagram.Boundary { boundary_in; _ } ->
                  Diagram.dim boundary_in + 1
            in
            let tag, new_id_opt =
              match mode with
              | Global ->
                  let new_id = Id.Global.fresh () in
                  (Id.Tag.of_global new_id, Some new_id)
              | Local ->
                  (Id.Tag.of_local name, None)
            in
            match Diagram.cell tag boundaries with
            | Error err ->
                let message =
                  Format.asprintf "Failed to create generator cell: %a" Error.pp
                    err
                in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer boundaries_span
                    message
                in
                (None, add_diagnostic generator_result diagnostic)
            | Ok classifier ->
                let location_with_generator =
                  Complex.add_generator location ~name ~classifier
                in
                let location_with_diagram =
                  Complex.add_diagram location_with_generator ~name classifier
                in
                let location_final =
                  match mode with
                  | Local ->
                      Complex.add_local_cell location_with_diagram ~name ~dim
                        boundaries
                  | Global ->
                      location_with_diagram
                in
                let state_after =
                  match (mode, new_id_opt) with
                  | Global, Some new_id ->
                      State.set_cell context_after.state ~id:new_id ~dim
                        boundaries
                  | _ ->
                      context_after.state
                in
                let context_updated = with_state context_after state_after in
                let updated_result =
                  { generator_result with context= context_updated }
                in
                (Some location_final, updated_result)))
  | C_instr_dnamer dnamer -> (
      let dnamer_output, dnamer_result =
        interpret_dnamer context ~location dnamer
      in
      match dnamer_output with
      | None ->
          (None, dnamer_result)
      | Some (name, diagram) ->
          if Complex.name_in_use location name then
            let name_span =
              let dnamer_desc = dnamer.value in
              Lang_ast.span_of_node dnamer_desc.dnamer_name
            in
            let message =
              Format.asprintf "Diagram name already in use: %s"
                (Id.Local.to_string name)
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer name_span message
            in
            (None, add_diagnostic dnamer_result diagnostic)
          else
            let updated_location = Complex.add_diagram location ~name diagram in
            (Some updated_location, dnamer_result))
  | C_instr_mnamer mnamer -> (
      let mnamer_output, mnamer_result =
        interpret_mnamer context ~location mnamer
      in
      match mnamer_output with
      | None ->
          (None, mnamer_result)
      | Some (name, morphism, domain) ->
          if Complex.name_in_use location name then
            let name_span =
              let mnamer_desc = mnamer.value in
              Lang_ast.span_of_node mnamer_desc.mnamer_name
            in
            let message =
              Format.asprintf "Map name already in use: %s"
                (Id.Local.to_string name)
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer name_span message
            in
            (None, add_diagnostic mnamer_result diagnostic)
          else
            let updated_location =
              Complex.add_morphism location ~name ~domain morphism
            in
            (Some updated_location, mnamer_result))
  | C_instr_include include_stmt -> (
      let include_output, include_result =
        interpret_include context include_stmt
      in
      let context_after = include_result.context in
      match include_output with
      | None ->
          (None, include_result)
      | Some (id, name) ->
          if Complex.name_in_use location name then
            let include_desc = include_stmt.value in
            let name_span =
              match include_desc.include_alias with
              | Some alias_node ->
                  Lang_ast.span_of_node alias_node
              | None ->
                  Lang_ast.span_of_node include_stmt
            in
            let message =
              Format.asprintf "Map name already in use: %s"
                (Id.Local.to_string name)
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer name_span message
            in
            (None, add_diagnostic include_result diagnostic)
          else
            let include_resolution =
              match State.find_type context_after.state id with
              | None ->
                  let type_span =
                    match include_stmt.value.include_alias with
                    | Some alias_node ->
                        Lang_ast.span_of_node alias_node
                    | None ->
                        Lang_ast.span_of_node include_stmt
                  in
                  let message =
                    Format.asprintf "Type %a not found in global record"
                      Id.Global.pp id
                  in
                  let diagnostic =
                    Diagnostics.make `Error interpreter_producer type_span
                      message
                  in
                  (None, add_diagnostic include_result diagnostic)
              | Some { complex= subtype; _ } ->
                  let location_with_generators =
                    List.fold_left
                      (fun acc gen_name ->
                        let gen_entry =
                          match Complex.find_generator subtype gen_name with
                          | Some entry ->
                              entry
                          | None ->
                              assert false
                        in
                        if
                          Option.is_some
                            (Complex.find_generator_by_tag acc gen_entry.tag)
                        then acc
                        else
                          let classifier =
                            match Complex.classifier subtype gen_name with
                            | Some diagram ->
                                diagram
                            | None ->
                                assert false
                          in
                          let alias_prefix = Id.Local.to_string name in
                          let gen_suffix = Id.Local.to_string gen_name in
                          let combined =
                            if String.equal alias_prefix "" then gen_suffix
                            else if String.equal gen_suffix "" then alias_prefix
                            else alias_prefix ^ "." ^ gen_suffix
                          in
                          let combined_name = Id.Local.make combined in
                          Complex.add_generator acc ~name:combined_name
                            ~classifier)
                      location
                      (Complex.generator_names subtype)
                  in
                  let inclusion = identity_morphism context_after subtype in
                  let final_location =
                    Complex.add_morphism location_with_generators ~name
                      ~domain:(Complex.Type id) inclusion
                  in
                  let final_result =
                    { include_result with context= context_after }
                  in
                  (Some final_location, final_result)
            in
            include_resolution)
  | C_instr_attach attach_stmt -> (
      let attach_output, attach_result =
        interpret_attach context ~location attach_stmt
      in
      let context_after = attach_result.context in
      match attach_output with
      | None ->
          (None, attach_result)
      | Some (name, morphism, domain) ->
          if Complex.name_in_use location name then
            let name_span =
              let attach_desc = attach_stmt.value in
              Lang_ast.span_of_node attach_desc.attach_name
            in
            let message =
              Format.asprintf "Map name already in use: %s"
                (Id.Local.to_string name)
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer name_span message
            in
            (None, add_diagnostic attach_result diagnostic)
          else
            let attachment_id =
              match domain with
              | Complex.Type id ->
                  id
              | Complex.Module _ ->
                  assert false
            in
            let attach_resolution =
              match State.find_type context_after.state attachment_id with
              | None ->
                  let name_span =
                    Lang_ast.span_of_node attach_stmt.value.attach_name
                  in
                  let message =
                    Format.asprintf "Type %a not found in global record"
                      Id.Global.pp attachment_id
                  in
                  let diagnostic =
                    Diagnostics.make `Error interpreter_producer name_span
                      message
                  in
                  (None, add_diagnostic attach_result diagnostic)
              | Some { complex= attachment; _ } ->
                  let attachment_generators =
                    Complex.generator_names attachment
                    |> List.filter_map (fun gen_name ->
                           match Complex.find_generator attachment gen_name with
                           | Some entry ->
                               Some (entry.dim, gen_name, entry.tag)
                           | None ->
                               None)
                    |> List.sort (fun (dim_a, _, _) (dim_b, _, _) ->
                           Int.compare dim_a dim_b)
                  in
                  let initial_entries =
                    Morphism.domain_of_definition morphism
                    |> List.map (fun tag ->
                           let dim =
                             match Morphism.dim morphism tag with
                             | Ok d ->
                                 d
                             | Error _ ->
                                 assert false
                           in
                           let cell_data =
                             match Morphism.cell_data morphism tag with
                             | Ok data ->
                                 data
                             | Error _ ->
                                 assert false
                           in
                           let image =
                             match Morphism.image morphism tag with
                             | Ok diagram ->
                                 diagram
                             | Error _ ->
                                 assert false
                           in
                           (tag, dim, cell_data, image))
                  in
                  let initial_cellular = Morphism.is_cellular morphism in
                  let location_after, final_state, _, _, morphism_after =
                    List.fold_left
                      (fun (loc, state, entries, cellular, current_morphism)
                           (gen_dim, gen_name, gen_tag) ->
                        if Morphism.is_defined_at current_morphism gen_tag then
                          (loc, state, entries, cellular, current_morphism)
                        else
                          let gen_cell_data =
                            match gen_tag with
                            | `Global gen_id -> (
                                match State.find_cell state gen_id with
                                | Some { data; _ } ->
                                    data
                                | None ->
                                    assert false)
                            | `Local _ ->
                                assert false
                          in
                          let image_cell_data =
                            match gen_cell_data with
                            | Diagram.Zero ->
                                Diagram.Zero
                            | Diagram.Boundary { boundary_in; boundary_out } ->
                                let image_in =
                                  match
                                    Morphism.apply current_morphism boundary_in
                                  with
                                  | Ok diagram ->
                                      diagram
                                  | Error _ ->
                                      assert false
                                in
                                let image_out =
                                  match
                                    Morphism.apply current_morphism boundary_out
                                  with
                                  | Ok diagram ->
                                      diagram
                                  | Error _ ->
                                      assert false
                                in
                                Diagram.Boundary
                                  {
                                    boundary_in= image_in;
                                    boundary_out= image_out;
                                  }
                          in
                          let base_name = Id.Local.to_string name in
                          let gen_name_str = Id.Local.to_string gen_name in
                          let combined =
                            if String.equal base_name "" then gen_name_str
                            else if String.equal gen_name_str "" then base_name
                            else base_name ^ "." ^ gen_name_str
                          in
                          let image_name = Id.Local.make combined in
                          let image_tag, state_after_cells, loc_with_cells =
                            match mode with
                            | Global ->
                                let image_id = Id.Global.fresh () in
                                let state' =
                                  State.set_cell state ~id:image_id ~dim:gen_dim
                                    image_cell_data
                                in
                                (Id.Tag.of_global image_id, state', loc)
                            | Local ->
                                let loc' =
                                  Complex.add_local_cell loc ~name:image_name
                                    ~dim:gen_dim image_cell_data
                                in
                                (Id.Tag.of_local image_name, state, loc')
                          in
                          let image_classifier =
                            match Diagram.cell image_tag image_cell_data with
                            | Ok diagram ->
                                diagram
                            | Error _ ->
                                assert false
                          in
                          let loc_with_generator =
                            Complex.add_generator loc_with_cells
                              ~name:image_name ~classifier:image_classifier
                          in
                          let new_entry =
                            (gen_tag, gen_dim, gen_cell_data, image_classifier)
                          in
                          let entries' = new_entry :: entries in
                          let extended_morphism =
                            Morphism.of_entries entries' ~cellular
                          in
                          ( loc_with_generator,
                            state_after_cells,
                            entries',
                            cellular,
                            extended_morphism ))
                      ( location,
                        context_after.state,
                        initial_entries,
                        initial_cellular,
                        morphism )
                      attachment_generators
                  in
                  let final_location =
                    Complex.add_morphism location_after ~name ~domain
                      morphism_after
                  in
                  let final_context = with_state context_after final_state in
                  let final_result =
                    { attach_result with context= final_context }
                  in
                  (Some final_location, final_result)
            in
            attach_resolution)

and interpret_c_block context ~mode ~location (c_block : Lang_ast.c_block) =
  let instrs = c_block.value in
  let rec loop current_location acc_location acc_result = function
    | [] ->
        (acc_location, acc_result)
    | instr :: rest ->
        let ctx = acc_result.context in
        let location_opt, instr_result =
          interpret_c_instr ctx ~mode ~location:current_location instr
        in
        let combined = combine ~previous:acc_result ~next:instr_result in
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

and interpret_complex context ~mode complex =
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
                  let combined_result =
                    combine ~previous:root_result ~next:block_result
                  in
                  let namespace_opt =
                    match location_opt with
                    | Some location ->
                        Some { root; location }
                    | None ->
                        None
                  in
                  (namespace_opt, combined_result))))

and interpret_m_block context ~location ~source ~morphism m_block =
  let open Lang_ast in
  let instrs = m_block.value in
  let rec interpret_instrs latest_context latest_morphism acc_result = function
    | [] ->
        (Some latest_morphism, { acc_result with context= latest_context })
    | instr :: rest -> (
        let instr_output, instr_result =
          interpret_m_instr latest_context ~location ~source
            ~morphism:latest_morphism instr
        in
        let combined_result = combine ~previous:acc_result ~next:instr_result in
        match instr_output with
        | None ->
            (None, combined_result)
        | Some updated_morphism ->
            if has_errors instr_result then
              (Some updated_morphism, combined_result)
            else
              interpret_instrs instr_result.context updated_morphism
                combined_result rest)
  in
  interpret_instrs context morphism (empty_result context) instrs

and interpret_m_instr context ~location ~source ~morphism m_instr =
  let open Lang_ast in
  let span = Lang_ast.span_of_node m_instr in
  let instr_desc = m_instr.value in
  let interpret_assign term_left term_right combined_result target_location =
    let context_after = combined_result.context in
    match (term_left, term_right) with
    | D_term d_left, D_term d_right -> (
        if not (Diagram.is_cell d_left) then
          let message =
            "The left-hand side of an assignment must be a cell or a cellular \
             map"
          in
          let diagnostic =
            Diagnostics.make `Error interpreter_producer span message
          in
          (None, add_diagnostic combined_result diagnostic)
        else
          let dim = Diagram.dim d_left in
          let labels = Diagram.labels d_left in
          let tag = labels.(dim).(0) in
          match
            smart_extend context_after morphism ~source ~target:target_location
              ~tag ~dim ~diagram:d_right
          with
          | Ok extended ->
              (Some extended, combined_result)
          | Error err ->
              let message = err.Error.message in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span message
              in
              (None, add_diagnostic combined_result diagnostic))
    | ( M_term { morphism= m_left; source= src_left },
        M_term { morphism= m_right; source= src_right } ) -> (
        if src_left != src_right then
          let message = "Not a well-formed assignment" in
          let diagnostic =
            Diagnostics.make `Error interpreter_producer span message
          in
          (None, add_diagnostic combined_result diagnostic)
        else
          let generators =
            Complex.generator_names src_left
            |> List.filter_map (fun name ->
                   match Complex.find_generator src_left name with
                   | Some entry ->
                       Some (entry.dim, entry.tag, name)
                   | None ->
                       None)
            |> List.sort (fun (dim_a, _, _) (dim_b, _, _) ->
                   Int.compare dim_a dim_b)
          in
          let rec assign extended_morphism = function
            | [] ->
                Ok extended_morphism
            | (dim, tag, name) :: rest ->
                let defined_left = Morphism.is_defined_at m_left tag in
                let defined_right = Morphism.is_defined_at m_right tag in
                if defined_left && defined_right then
                  match Morphism.image m_left tag with
                  | Error err ->
                      Error err
                  | Ok left_image ->
                      if Diagram.is_cell left_image then
                        let left_labels = Diagram.labels left_image in
                        let tag_left = left_labels.(dim).(0) in
                        match Morphism.image m_right tag with
                        | Error err ->
                            Error err
                        | Ok image -> (
                            match
                              smart_extend context_after extended_morphism
                                ~source ~target:target_location ~tag:tag_left
                                ~dim ~diagram:image
                            with
                            | Ok updated ->
                                assign updated rest
                            | Error err ->
                                Error err)
                      else
                        let all_defined =
                          Diagram.label_set_of left_image
                          |> List.for_all (fun (tag_left, _) ->
                                 Morphism.is_defined_at extended_morphism
                                   tag_left)
                        in
                        if all_defined then assign extended_morphism rest
                        else
                          Error
                            (Error.make
                               "Failed to extend map (not enough information)")
                else if defined_left && not defined_right then
                  let message =
                    Format.asprintf
                      "%s is in the domain of definition of the first map, but \
                       not the second map"
                      (Id.Local.to_string name)
                  in
                  Error (Error.make message)
                else if defined_right && not defined_left then
                  let message =
                    Format.asprintf
                      "%s is in the domain of definition of the second map, \
                       but not the first map"
                      (Id.Local.to_string name)
                  in
                  Error (Error.make message)
                else assign extended_morphism rest
          in
          match assign morphism generators with
          | Ok updated ->
              (Some updated, combined_result)
          | Error err ->
              let message = err.Error.message in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer span message
              in
              (None, add_diagnostic combined_result diagnostic))
    | _ ->
        let message = "Not a well-formed assignment" in
        let diagnostic =
          Diagnostics.make `Error interpreter_producer span message
        in
        (None, add_diagnostic combined_result diagnostic)
  in
  let { m_instr_source= left; m_instr_target= right } = instr_desc in
  let left_term_opt, left_result =
    interpret_pasting context ~location:source left
  in
  match left_term_opt with
  | None ->
      (None, left_result)
  | Some term_left -> (
      let right_term_opt, right_result =
        interpret_pasting left_result.context ~location right
      in
      let combined_result = combine ~previous:left_result ~next:right_result in
      match right_term_opt with
      | None ->
          (None, combined_result)
      | Some term_right ->
          interpret_assign term_left term_right combined_result location)

let interpret_generator_type context generator_type =
  let open Lang_ast in
  let generator_desc = generator_type.value in
  let generator_node = generator_desc.generator_type_generator in
  let definition_node = generator_desc.generator_type_definition in
  let generator_name_node = generator_node.value.generator_name in
  let name_span = span_of_node generator_name_node in
  let module_location =
    let module_id = context.current_module in
    match State.find_module context.state module_id with
    | Some location ->
        location
    | None ->
        Complex.empty
  in
  let generator_output, generator_result =
    interpret_generator context ~location:module_location generator_node
  in
  match generator_output with
  | None ->
      (None, generator_result)
  | Some (name, boundaries) -> (
      match boundaries with
      | Diagram.Boundary _ ->
          let span = span_of_node generator_node in
          let diagnostic =
            Diagnostics.make `Error interpreter_producer span
              "Higher cells in modules are not implemented yet."
          in
          (None, add_diagnostic generator_result diagnostic)
      | Diagram.Zero -> (
          let namespace_opt, complex_result =
            interpret_complex generator_result.context ~mode:Global
              definition_node
          in
          let combined_result =
            combine ~previous:generator_result ~next:complex_result
          in
          match namespace_opt with
          | None ->
              (None, combined_result)
          | Some namespace ->
              let complex = namespace.location in
              if Complex.name_in_use complex name then
                let message =
                  Format.asprintf "Map name already in use: %s"
                    (Id.Local.to_string name)
                in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer name_span message
                in
                (None, add_diagnostic combined_result diagnostic)
              else (Some (name, boundaries, complex), combined_result)))

let rec interpret_program ~loader context program =
  let module_id = context.current_module in
  match State.find_module context.state module_id with
  | Some _ ->
      empty_result context
  | None ->
      let state = context.state in
      let empty_type_id = Id.Global.fresh () in
      let state =
        State.set_type state ~id:empty_type_id ~data:Diagram.Zero
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
      let state = State.set_module state ~id:module_id module_complex in
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

and interpret_c_instr_type ~loader context
    (c_instr_type : Lang_ast.c_instr_type) =
  let open Lang_ast in
  match c_instr_type.value with
  | C_instr_type_generator generator_type -> (
      let generator_output, generator_result =
        interpret_generator_type context generator_type
      in
      let generator_desc = generator_type.value in
      let generator_node = generator_desc.generator_type_generator in
      let generator_name_node = generator_node.value.generator_name in
      let definition_node = generator_desc.generator_type_definition in
      let module_span = span_of_node generator_type in
      let name_span = span_of_node generator_name_node in
      let definition_span = span_of_node definition_node in
      let context_after = generator_result.context in
      let module_id = context_after.current_module in
      match generator_output with
      | None ->
          (None, generator_result)
      | Some (name, boundaries, complex) -> (
          match State.find_module context_after.state module_id with
          | None ->
              let message =
                Format.asprintf "Module %s not found in module record"
                  (Id.Module.to_string module_id)
              in
              let diagnostic =
                Diagnostics.make `Error interpreter_producer module_span message
              in
              (None, add_diagnostic generator_result diagnostic)
          | Some location -> (
              if Complex.name_in_use location name then
                let message =
                  Format.asprintf "Generator name already in use: %s"
                    (Id.Local.to_string name)
                in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer name_span message
                in
                (None, add_diagnostic generator_result diagnostic)
              else
                let new_id = Id.Global.fresh () in
                let identity = identity_morphism context_after complex in
                let complex_with_identity =
                  Complex.add_morphism complex ~name
                    ~domain:(Complex.Type new_id) identity
                in
                let tag = Id.Tag.of_global new_id in
                match Diagram.cell tag boundaries with
                | Error err ->
                    let message =
                      Format.asprintf "Failed to create generator cell: %a"
                        Error.pp err
                    in
                    let diagnostic =
                      Diagnostics.make `Error interpreter_producer
                        definition_span message
                    in
                    (None, add_diagnostic generator_result diagnostic)
                | Ok classifier_diagram ->
                    let location_with_generator =
                      Complex.add_generator location ~name
                        ~classifier:classifier_diagram
                    in
                    let updated_location =
                      Complex.add_diagram location_with_generator ~name
                        classifier_diagram
                    in
                    let state =
                      State.set_type context_after.state ~id:new_id
                        ~data:boundaries ~complex:complex_with_identity
                    in
                    let state =
                      State.set_module state ~id:module_id updated_location
                    in
                    let context_updated = with_state context_after state in
                    let updated_result =
                      { generator_result with context= context_updated }
                    in
                    (Some updated_location, updated_result))))
  | C_instr_type_dnamer dnamer -> (
      let module_span = span_of_node dnamer in
      let module_id = context.current_module in
      let name_span =
        let dnamer_desc = dnamer.value in
        span_of_node dnamer_desc.dnamer_name
      in
      match State.find_module context.state module_id with
      | None ->
          let message =
            Format.asprintf "Module %s not found in module record"
              (Id.Module.to_string module_id)
          in
          let diagnostic =
            Diagnostics.make `Error interpreter_producer module_span message
          in
          (None, add_diagnostic (empty_result context) diagnostic)
      | Some location -> (
          let dnamer_output, dnamer_result =
            interpret_dnamer context ~location dnamer
          in
          let context_after = dnamer_result.context in
          match dnamer_output with
          | None ->
              (None, dnamer_result)
          | Some (name, diagram) -> (
              match State.find_module context_after.state module_id with
              | None ->
                  let message =
                    Format.asprintf "Module %s not found in module record"
                      (Id.Module.to_string module_id)
                  in
                  let diagnostic =
                    Diagnostics.make `Error interpreter_producer module_span
                      message
                  in
                  (None, add_diagnostic dnamer_result diagnostic)
              | Some current_location ->
                  if Complex.name_in_use current_location name then
                    let message =
                      Format.asprintf "Diagram name already in use: %s"
                        (Id.Local.to_string name)
                    in
                    let diagnostic =
                      Diagnostics.make `Error interpreter_producer name_span
                        message
                    in
                    (None, add_diagnostic dnamer_result diagnostic)
                  else
                    let updated_location =
                      Complex.add_diagram current_location ~name diagram
                    in
                    let state =
                      State.set_module context_after.state ~id:module_id
                        updated_location
                    in
                    let context_updated = with_state context_after state in
                    let updated_result =
                      { dnamer_result with context= context_updated }
                    in
                    (Some updated_location, updated_result))))
  | C_instr_type_mnamer mnamer -> (
      let module_span = span_of_node mnamer in
      let module_id = context.current_module in
      let name_span =
        let mnamer_desc = mnamer.value in
        span_of_node mnamer_desc.mnamer_name
      in
      match State.find_module context.state module_id with
      | None ->
          let message =
            Format.asprintf "Module %s not found in module record"
              (Id.Module.to_string module_id)
          in
          let diagnostic =
            Diagnostics.make `Error interpreter_producer module_span message
          in
          (None, add_diagnostic (empty_result context) diagnostic)
      | Some location -> (
          let mnamer_output, mnamer_result =
            interpret_mnamer context ~location mnamer
          in
          let context_after = mnamer_result.context in
          match mnamer_output with
          | None ->
              (None, mnamer_result)
          | Some (name, morphism, domain) -> (
              match State.find_module context_after.state module_id with
              | None ->
                  let message =
                    Format.asprintf "Module %s not found in module record"
                      (Id.Module.to_string module_id)
                  in
                  let diagnostic =
                    Diagnostics.make `Error interpreter_producer module_span
                      message
                  in
                  (None, add_diagnostic mnamer_result diagnostic)
              | Some current_location ->
                  if Complex.name_in_use current_location name then
                    let message =
                      Format.asprintf "Map name already in use: %s"
                        (Id.Local.to_string name)
                    in
                    let diagnostic =
                      Diagnostics.make `Error interpreter_producer name_span
                        message
                    in
                    (None, add_diagnostic mnamer_result diagnostic)
                  else
                    let updated_location =
                      Complex.add_morphism current_location ~name ~domain
                        morphism
                    in
                    let state =
                      State.set_module context_after.state ~id:module_id
                        updated_location
                    in
                    let context_updated = with_state context_after state in
                    let updated_result =
                      { mnamer_result with context= context_updated }
                    in
                    (Some updated_location, updated_result))))
  | C_instr_type_include_module include_module -> (
      let include_desc = include_module.value in
      let name_node = include_desc.include_module_name in
      let alias_node_opt = include_desc.include_module_alias in
      let name, alias = interpret_include_module include_module in
      let name_span = span_of_node name_node in
      let alias_span =
        match alias_node_opt with
        | Some alias_node ->
            span_of_node alias_node
        | None ->
            name_span
      in
      let module_span = span_of_node include_module in
      let module_id = context.current_module in
      let alias_str = Id.Local.to_string alias in
      let name_str = Id.Local.to_string name in
      match State.find_module context.state module_id with
      | None ->
          let message =
            Format.asprintf "Module %s not found in module record"
              (Id.Module.to_string module_id)
          in
          let diagnostic =
            Diagnostics.make `Error interpreter_producer module_span message
          in
          (None, add_diagnostic (empty_result context) diagnostic)
      | Some location -> (
          if Complex.name_in_use location alias then
            let message =
              Format.asprintf "Map name already in use: %s" alias_str
            in
            let diagnostic =
              Diagnostics.make `Error interpreter_producer alias_span message
            in
            (None, add_diagnostic (empty_result context) diagnostic)
          else
            let filename = name_str ^ ".ali" in
            let read_module () =
              if Filename.is_relative filename then
                let rec loop = function
                  | [] ->
                      Error `Not_found
                  | dir :: rest -> (
                      let candidate = Filename.concat dir filename in
                      let canonical = Path.canonicalize candidate in
                      match loader.read_file canonical with
                      | Ok contents ->
                          Ok (canonical, contents)
                      | Error `Not_found ->
                          loop rest
                      | Error (`Io_error reason) ->
                          Error (`Io_error (canonical, reason)))
                in
                loop loader.search_paths
              else
                let canonical = Path.canonicalize filename in
                match loader.read_file canonical with
                | Ok contents ->
                    Ok (canonical, contents)
                | Error `Not_found ->
                    Error `Not_found
                | Error (`Io_error reason) ->
                    Error (`Io_error (canonical, reason))
            in
            match read_module () with
            | Error `Not_found ->
                let message =
                  Format.asprintf "Module file %s not found in search paths"
                    filename
                in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer name_span message
                in
                (None, add_diagnostic (empty_result context) diagnostic)
            | Error (`Io_error (path, reason)) ->
                let message =
                  Format.asprintf "Failed to load %s: %s" path reason
                in
                let diagnostic =
                  Diagnostics.make `Error interpreter_producer name_span message
                in
                (None, add_diagnostic (empty_result context) diagnostic)
            | Ok (canonical_path, contents) ->
                let module_dir =
                  canonical_path |> Filename.dirname |> Path.canonicalize
                in
                let loader_paths =
                  Path.normalize_search_paths (module_dir :: loader.search_paths)
                in
                let loader_for_module =
                  { loader with search_paths= loader_paths }
                in
                let source = Positions.Source.of_path canonical_path in
                let stream = Token_stream.create ~source contents in
                let program, parse_diagnostics = Parser_driver.parse stream in
                if report_has_errors parse_diagnostics then
                  let parse_result =
                    List.fold_left add_diagnostic (empty_result context)
                      parse_diagnostics
                  in
                  (None, parse_result)
                else
                  let included_module_id = Id.Module.of_path canonical_path in
                  let include_context =
                    make_context ~module_id:included_module_id
                      ~state:context.state
                  in
                  let include_result =
                    interpret_program ~loader:loader_for_module include_context
                      program
                  in
                  let restored_context =
                    make_context ~module_id ~state:include_result.context.state
                  in
                  let include_result =
                    {
                      include_result with
                      context= restored_context;
                      diagnostics=
                        Report.append parse_diagnostics
                          include_result.diagnostics;
                    }
                  in
                  if has_errors include_result then (None, include_result)
                  else
                    let updated_state = include_result.context.state in
                    let module_location =
                      match
                        State.find_module updated_state included_module_id
                      with
                      | Some loc ->
                          loc
                      | None ->
                          assert false
                    in
                    let alias_prefix = alias_str in
                    let extend_generators acc gen_name =
                      if String.equal (Id.Local.to_string gen_name) "" then acc
                      else
                        let gen_entry =
                          match
                            Complex.find_generator module_location gen_name
                          with
                          | Some entry ->
                              entry
                          | None ->
                              assert false
                        in
                        if
                          Option.is_some
                            (Complex.find_generator_by_tag acc gen_entry.tag)
                        then acc
                        else
                          let classifier =
                            match
                              Complex.classifier module_location gen_name
                            with
                            | Some diagram ->
                                diagram
                            | None ->
                                assert false
                          in
                          let gen_name_str = Id.Local.to_string gen_name in
                          let combined_name =
                            if String.equal alias_prefix "" then gen_name_str
                            else if String.equal gen_name_str "" then
                              alias_prefix
                            else alias_prefix ^ "." ^ gen_name_str
                          in
                          let new_name = Id.Local.make combined_name in
                          Complex.add_generator acc ~name:new_name ~classifier
                    in
                    let location_with_generators =
                      List.fold_left extend_generators location
                        (Complex.generator_names module_location)
                    in
                    let inclusion =
                      identity_morphism include_result.context module_location
                    in
                    let final_location =
                      Complex.add_morphism location_with_generators ~name:alias
                        ~domain:(Complex.Module included_module_id) inclusion
                    in
                    let final_state =
                      State.set_module include_result.context.state
                        ~id:module_id final_location
                    in
                    let final_context =
                      with_state include_result.context final_state
                    in
                    let final_result =
                      { include_result with context= final_context }
                    in
                    (Some final_location, final_result)))

and interpret_c_block_type ~loader context
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
        let combined = combine ~previous:acc_result ~next:instr_result in
        let acc_location =
          match location_opt with Some loc -> Some loc | None -> acc_location
        in
        if has_errors instr_result then (acc_location, combined)
        else loop acc_location combined rest
  in
  loop None (empty_result context) instrs
