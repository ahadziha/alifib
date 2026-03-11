open Sexplib.Std
open Sexp

type type_entry = { data: Diagram.cell_data; complex: Complex.t } [@@deriving sexp_of]
type cell_entry = { data: Diagram.cell_data; dim: int } [@@deriving sexp_of]
type cells = { by_id: cell_entry GlobalMap.t; by_dim: GlobalSet.t IntMap.t } [@@deriving sexp_of]

type t = {
  cells: cells;
  types: type_entry GlobalMap.t;
  modules: Complex.t ModuleMap.t;
} [@@deriving sexp_of]

let empty =
  {
    cells= { by_id= GlobalMap.empty; by_dim= IntMap.empty };
    types= GlobalMap.empty;
    modules= ModuleMap.empty;
  }

let set_cell state ~id ~dim data =
  let by_id = GlobalMap.add id { data; dim } state.cells.by_id in
  let by_dim =
    IntMap.update dim
      (fun current ->
        let set = match current with Some s -> s | None -> GlobalSet.empty in
        Some (GlobalSet.add id set))
      state.cells.by_dim
  in
  { state with cells= { by_id; by_dim } }

let set_type state ~id ~data ~complex =
  { state with types= GlobalMap.add id { data; complex } state.types }

let update_type_complex state ~id complex =
  let types =
    match GlobalMap.find_opt id state.types with
    | Some entry ->
        GlobalMap.add id { entry with complex } state.types
    | None ->
        state.types
  in
  { state with types }

let set_module state ~id complex =
  { state with modules= ModuleMap.add id complex state.modules }

let find_cell state id = GlobalMap.find_opt id state.cells.by_id
let find_type state id = GlobalMap.find_opt id state.types
let find_module state id = ModuleMap.find_opt id state.modules

let pp fmt state =
  let open Format in
  let cells_count = GlobalMap.cardinal state.cells.by_id in
  let types_count = GlobalMap.cardinal state.types in
  let modules_count = ModuleMap.cardinal state.modules in
  fprintf fmt "@[<v>%d cells, %d types, %d modules@,@," cells_count types_count
    modules_count
  ; let module_entries = ModuleMap.bindings state.modules in
    let render_list items =
      match items with [] -> "(none)" | _ -> String.concat ", " items
    in
    let string_or_empty name =
      let s = Id.Local.to_string name in
      if String.length s = 0 then "<empty>" else s
    in
    let render_cells_by_dim type_complex =
      let dims =
        Complex.generator_names type_complex
        |> List.filter_map (fun name -> Complex.generator_dim type_complex name)
        |> List.sort_uniq Int.compare
      in
      match dims with
      | [] ->
          "(none)"
      | _ ->
          dims
          |> List.map (fun dim ->
                 let rendered =
                   Complex.generators_in_dim type_complex dim
                   |> List.map string_or_empty |> List.sort String.compare
                   |> String.concat ", "
                 in
                 if String.length rendered = 0 then Printf.sprintf "[%d]" dim
                 else Printf.sprintf "[%d] %s" dim rendered)
          |> String.concat ", "
    in
    let pp_module fmt (module_id, module_complex) =
      fprintf fmt "@[<v>* Module %s" (Id.Module.to_string module_id)
      ; let generator_names = Complex.generator_names module_complex in
        let string_of_domain = function
          | Complex.Type global_id -> (
              match
                Complex.find_generator_by_tag module_complex
                  (Id.Tag.of_global global_id)
              with
              | Some domain_name ->
                  string_or_empty domain_name
              | None ->
                  asprintf "%a" Id.Global.pp global_id)
          | Complex.Module module_id ->
              Id.Module.to_string module_id
        in
        let pp_type fmt generator_name =
          let type_label = string_or_empty generator_name in
          let print_details ~cells ~diagrams ~morphisms =
            fprintf fmt
              "@[<v 2>Type %s@,- Cells: %s@,- Diagrams: %s@,- Maps: %s@]"
              type_label cells diagrams morphisms
          in
          match Complex.find_generator module_complex generator_name with
          | None ->
              let missing = "(missing generator)" in
              print_details ~cells:missing ~diagrams:missing ~morphisms:missing
          | Some { tag; _ } -> (
              match tag with
              | `Local _ ->
                  let local = "(local tag)" in
                  print_details ~cells:local ~diagrams:local ~morphisms:local
              | `Global global_id -> (
                  match find_type state global_id with
                  | None ->
                      let missing = "(type not found)" in
                      print_details ~cells:missing ~diagrams:missing
                        ~morphisms:missing
                  | Some { complex= type_complex; _ } ->
                      let cells = render_cells_by_dim type_complex in
                      let diagrams =
                        Complex.diagram_names type_complex
                        |> List.map string_or_empty |> render_list
                      in
                      let morphisms =
                        Complex.morphism_names type_complex
                        |> List.map (fun morph_name ->
                               let domain_label =
                                 match
                                   Complex.find_morphism type_complex morph_name
                                 with
                                 | Some { domain; _ } ->
                                     string_of_domain domain
                                 | None ->
                                     "?"
                               in
                               Printf.sprintf "%s :: %s"
                                 (string_or_empty morph_name)
                                 domain_label)
                        |> render_list
                      in
                      print_details ~cells ~diagrams ~morphisms))
        in
        if generator_names <> [] then (
          fprintf fmt "@,"
          ; pp_print_list
              ~pp_sep:(fun fmt () -> fprintf fmt "@,@,")
              (fun fmt name -> pp_type fmt name)
              fmt generator_names)
        ; fprintf fmt "@]"
    in
    pp_print_list
      ~pp_sep:(fun fmt () -> fprintf fmt "@,@,")
      pp_module fmt module_entries
    ; fprintf fmt "@]"
