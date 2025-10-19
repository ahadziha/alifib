module GlobalOrd = struct
  type t = Id.Global.t

  let compare : t -> t -> int = Id.Global.compare
end

module ModuleOrd = struct
  type t = Id.Module.t

  let compare : t -> t -> int = Id.Module.compare
end

module GlobalMap = Map.Make (GlobalOrd)
module ModuleMap = Map.Make (ModuleOrd)

type type_entry = { data: Diagram.cell_data; complex: Complex.t }

type t = {
  cells: Diagram.cell_data GlobalMap.t;
  types: type_entry GlobalMap.t;
  modules: Complex.t ModuleMap.t;
}

let empty =
  { cells= GlobalMap.empty; types= GlobalMap.empty; modules= ModuleMap.empty }

let add_cell state ~id data =
  { state with cells= GlobalMap.add id data state.cells }

let add_type state ~id ~data ~complex =
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

let add_module state ~id complex =
  { state with modules= ModuleMap.add id complex state.modules }

let find_cell state id = GlobalMap.find_opt id state.cells
let find_type state id = GlobalMap.find_opt id state.types
let find_module state id = ModuleMap.find_opt id state.modules
