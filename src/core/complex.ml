type 'a checked = 'a Error.checked

module LocalNameOrd = struct
  type t = Id.Local.t

  let compare (a : t) (b : t) =
    String.compare (Id.Local.to_string a) (Id.Local.to_string b)
end

module SimpleNameOrd = struct
  type t = Id.Local.simple

  let compare (a : t) (b : t) =
    String.compare (Id.Local.simple_to_string a) (Id.Local.simple_to_string b)
end

module TagOrd = struct
  type t = Id.Tag.t

  let compare (a : t) (b : t) = Id.Tag.compare a b
end

module IntOrd = struct
  type t = int

  let compare (a : t) (b : t) = Int.compare a b
end

module NameMap = Map.Make (LocalNameOrd)
module NameSet = Set.Make (LocalNameOrd)
module SimpleMap = Map.Make (SimpleNameOrd)
module SimpleSet = Set.Make (SimpleNameOrd)
module TagMap = Map.Make (TagOrd)
module IntMap = Map.Make (IntOrd)

type generator_entry = { tag: Id.Tag.t; dim: int }
type morphism_entry = { morphism: Morphism.t; domain: Id.Tag.t }
type local_cell_entry = { data: Diagram.cell_data; dim: int }

type generators = {
  by_name: generator_entry NameMap.t;
  by_tag: Id.Local.t TagMap.t;
  by_dim: NameSet.t IntMap.t;
}

type local_cells = {
  by_name: local_cell_entry SimpleMap.t;
  by_dim: SimpleSet.t IntMap.t;
}

type t = {
  generators: generators;
  diagrams: Diagram.t SimpleMap.t;
  morphisms: morphism_entry SimpleMap.t;
  local_cells: local_cells;
  used_names: SimpleSet.t;
}

let empty_generators =
  { by_name= NameMap.empty; by_tag= TagMap.empty; by_dim= IntMap.empty }

let empty_local_cells = { by_name= SimpleMap.empty; by_dim= IntMap.empty }

let empty =
  {
    generators= empty_generators;
    diagrams= SimpleMap.empty;
    morphisms= SimpleMap.empty;
    local_cells= empty_local_cells;
    used_names= SimpleSet.empty;
  }

let ok x = Ok x
let error msg = Error (Error.make msg)
let option_default default = function Some value -> value | None -> default

let add_to_name_grade name dim grades =
  IntMap.update dim
    (fun current ->
      let set = option_default NameSet.empty current in
      Some (NameSet.add name set))
    grades

let add_to_simple_grade name dim grades =
  IntMap.update dim
    (fun current ->
      let set = option_default SimpleSet.empty current in
      Some (SimpleSet.add name set))
    grades

let add_generator t ~name ~dim ~tag =
  if NameMap.mem name t.generators.by_name then
    error "generator name already defined"
  else if TagMap.mem tag t.generators.by_tag then
    error "generator tag already defined"
  else
    let by_name = NameMap.add name { tag; dim } t.generators.by_name in
    let by_tag = TagMap.add tag name t.generators.by_tag in
    let by_dim = add_to_name_grade name dim t.generators.by_dim in
    ok { t with generators= { by_name; by_tag; by_dim } }

let ensure_name_available t name =
  if SimpleSet.mem name t.used_names then error "name already used" else Ok ()

let add_diagram t ~name diagram =
  match ensure_name_available t name with
  | Error _ as err ->
      err
  | Ok () ->
      let diagrams = SimpleMap.add name diagram t.diagrams in
      ok { t with diagrams; used_names= SimpleSet.add name t.used_names }

let add_morphism t ~name ~domain morphism =
  match ensure_name_available t name with
  | Error _ as err ->
      err
  | Ok () ->
      let entry = { morphism; domain } in
      let morphisms = SimpleMap.add name entry t.morphisms in
      ok { t with morphisms; used_names= SimpleSet.add name t.used_names }

let add_local_cell t ~name ~dim data =
  if SimpleMap.mem name t.local_cells.by_name then error "name already defined"
  else
    let by_name = SimpleMap.add name { data; dim } t.local_cells.by_name in
    let by_dim = add_to_simple_grade name dim t.local_cells.by_dim in
    ok { t with local_cells= { by_name; by_dim } }

let find_generator t name = NameMap.find_opt name t.generators.by_name
let find_generator_by_tag t tag = TagMap.find_opt tag t.generators.by_tag

let generator_dim t name =
  match NameMap.find_opt name t.generators.by_name with
  | Some entry ->
      Some entry.dim
  | None ->
      None

let generators_in_dim t dim =
  match IntMap.find_opt dim t.generators.by_dim with
  | Some names ->
      NameSet.elements names
  | None ->
      []

let find_diagram t name = SimpleMap.find_opt name t.diagrams
let find_morphism t name = SimpleMap.find_opt name t.morphisms
let find_local_cell t name = SimpleMap.find_opt name t.local_cells.by_name

let local_cell_dim t name =
  match SimpleMap.find_opt name t.local_cells.by_name with
  | Some entry ->
      Some entry.dim
  | None ->
      None

let local_cells_in_dim t dim =
  match IntMap.find_opt dim t.local_cells.by_dim with
  | Some names ->
      SimpleSet.elements names
  | None ->
      []

let name_in_use t name = SimpleSet.mem name t.used_names
let used_names t = SimpleSet.elements t.used_names
