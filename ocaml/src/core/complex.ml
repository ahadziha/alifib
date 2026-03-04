module LocalOrd = struct
  type t = Id.Local.t

  let compare (a : t) (b : t) = Id.Local.compare a b
end

module TagOrd = struct
  type t = Id.Tag.t

  let compare (a : t) (b : t) = Id.Tag.compare a b
end

module IntOrd = struct
  type t = int

  let compare (a : t) (b : t) = Int.compare a b
end

module LocalMap = Map.Make (LocalOrd)
module LocalSet = Set.Make (LocalOrd)
module TagMap = Map.Make (TagOrd)
module IntMap = Map.Make (IntOrd)

type generator_entry = { tag: Id.Tag.t; dim: int }
type morphism_domain = Type of Id.Global.t | Module of Id.Module.t
type morphism_entry = { morphism: Morphism.t; domain: morphism_domain }
type local_cell_entry = { data: Diagram.cell_data; dim: int }

type generators = {
  by_name: generator_entry LocalMap.t;
  by_tag: Id.Local.t TagMap.t;
  by_dim: LocalSet.t IntMap.t;
  classifiers: Diagram.t LocalMap.t;
}

type local_cells = {
  by_id: local_cell_entry LocalMap.t;
  by_dim: LocalSet.t IntMap.t;
}

type t = {
  generators: generators;
  diagrams: Diagram.t LocalMap.t;
  morphisms: morphism_entry LocalMap.t;
  local_cells: local_cells;
  used_names: LocalSet.t;
}

let empty_generators =
  {
    by_name= LocalMap.empty;
    by_tag= TagMap.empty;
    by_dim= IntMap.empty;
    classifiers= LocalMap.empty;
  }

let empty_local_cells = { by_id= LocalMap.empty; by_dim= IntMap.empty }

let empty =
  {
    generators= empty_generators;
    diagrams= LocalMap.empty;
    morphisms= LocalMap.empty;
    local_cells= empty_local_cells;
    used_names= LocalSet.empty;
  }

let option_default default = function Some value -> value | None -> default

let add_to_name_grade name dim grades =
  IntMap.update dim
    (fun current ->
      let set = option_default LocalSet.empty current in
      Some (LocalSet.add name set))
    grades

let add_to_local_grade name dim grades =
  IntMap.update dim
    (fun current ->
      let set = option_default LocalSet.empty current in
      Some (LocalSet.add name set))
    grades

let add_generator t ~name ~classifier =
  let dim = Diagram.dim classifier in
  let labels = Diagram.labels classifier in
  let top_labels = labels.(dim) in
  assert (Array.length top_labels > 0)
  ; let tag = top_labels.(0) in
    let by_name = LocalMap.add name { tag; dim } t.generators.by_name in
    let by_tag = TagMap.add tag name t.generators.by_tag in
    let by_dim = add_to_name_grade name dim t.generators.by_dim in
    let classifiers = LocalMap.add name classifier t.generators.classifiers in
    { t with generators= { by_name; by_tag; by_dim; classifiers } }

let add_diagram t ~name diagram =
  let diagrams = LocalMap.add name diagram t.diagrams in
  { t with diagrams; used_names= LocalSet.add name t.used_names }

let add_morphism t ~name ~domain morphism =
  let entry = { morphism; domain } in
  let morphisms = LocalMap.add name entry t.morphisms in
  { t with morphisms; used_names= LocalSet.add name t.used_names }

let add_local_cell t ~name ~dim data =
  let by_id = LocalMap.add name { data; dim } t.local_cells.by_id in
  let by_dim = add_to_local_grade name dim t.local_cells.by_dim in
  { t with local_cells= { by_id; by_dim } }

let find_generator t name = LocalMap.find_opt name t.generators.by_name
let find_generator_by_tag t tag = TagMap.find_opt tag t.generators.by_tag
let classifier t name = LocalMap.find_opt name t.generators.classifiers

let generator_dim t name =
  match LocalMap.find_opt name t.generators.by_name with
  | Some entry ->
      Some entry.dim
  | None ->
      None

let generators_in_dim t dim =
  match IntMap.find_opt dim t.generators.by_dim with
  | Some names ->
      LocalSet.elements names
  | None ->
      []

let find_diagram t name = LocalMap.find_opt name t.diagrams
let find_morphism t name = LocalMap.find_opt name t.morphisms
let find_local_cell t name = LocalMap.find_opt name t.local_cells.by_id

let local_cell_dim t name =
  match LocalMap.find_opt name t.local_cells.by_id with
  | Some entry ->
      Some entry.dim
  | None ->
      None

let local_cells_in_dim t dim =
  match IntMap.find_opt dim t.local_cells.by_dim with
  | Some names ->
      LocalSet.elements names
  | None ->
      []

let generator_names t =
  t.generators.by_name |> LocalMap.bindings |> List.map fst

let diagram_names t = t.diagrams |> LocalMap.bindings |> List.map fst
let morphism_names t = t.morphisms |> LocalMap.bindings |> List.map fst
let name_in_use t name = LocalSet.mem name t.used_names
let used_names t = LocalSet.elements t.used_names
