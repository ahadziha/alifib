module StringSet = Set.Make (String)

let is_windows = Sys.win32

let normalize_separators path =
  if is_windows then (
    let buf = Bytes.of_string path in
    for i = 0 to Bytes.length buf - 1 do
      if Bytes.get buf i = '\\' then Bytes.set buf i '/'
    done
    ; Bytes.unsafe_to_string buf)
  else path

let to_absolute path =
  if Filename.is_relative path then Filename.concat (Sys.getcwd ()) path
  else path

let extract_root path =
  let path = normalize_separators path in
  let len = String.length path in
  if len = 0 then ("", "")
  else if is_windows && len >= 2 && path.[1] = ':' then
    let drive = String.sub path 0 2 in
    let rest_start = if len >= 3 && path.[2] = '/' then 3 else 2 in
    let rest =
      if rest_start >= len then ""
      else String.sub path rest_start (len - rest_start)
    in
    (drive ^ "/", rest)
  else if len >= 2 && path.[0] = '/' && path.[1] = '/' then
    let server_end =
      match String.index_from_opt path 2 '/' with
      | None ->
          len
      | Some idx ->
          idx
    in
    let share_end =
      if server_end >= len then len
      else
        match String.index_from_opt path (server_end + 1) '/' with
        | None ->
            len
        | Some idx ->
            idx
    in
    let root_end = share_end in
    let root = String.sub path 0 root_end in
    let rest =
      if root_end >= len then ""
      else String.sub path (root_end + 1) (len - root_end - 1)
    in
    (root, rest)
  else if path.[0] = '/' then
    let rec skip_slashes i =
      if i < len && path.[i] = '/' then skip_slashes (i + 1) else i
    in
    let rest_start = skip_slashes 1 in
    let rest =
      if rest_start >= len then ""
      else String.sub path rest_start (len - rest_start)
    in
    ("/", rest)
  else ("", path)

let split_components s =
  s |> String.split_on_char '/' |> List.filter (fun part -> part <> "")

let collapse_components components =
  let rec aux acc = function
    | [] ->
        List.rev acc
    | "." :: rest ->
        aux acc rest
    | ".." :: rest -> (
        match acc with [] -> aux acc rest | _ :: tail -> aux tail rest)
    | "" :: rest ->
        aux acc rest
    | part :: rest ->
        aux (part :: acc) rest
  in
  aux [] components

let join_path root components =
  match (root, components) with
  | "", [] ->
      "."
  | "", parts ->
      String.concat "/" parts
  | root, [] ->
      if root = "" then "." else if root = "/" then "/" else root
  | root, parts ->
      let prefix =
        if root = "/" || root = "" then "/"
        else if root.[String.length root - 1] = '/' then root
        else root ^ "/"
      in
      if prefix = "/" then "/" ^ String.concat "/" parts
      else prefix ^ String.concat "/" parts

let canonicalize path =
  let absolute = to_absolute path in
  let normalized = normalize_separators absolute in
  let root, rest = extract_root normalized in
  let components = split_components rest in
  let collapsed = collapse_components components in
  let joined = join_path root collapsed in
  normalize_separators joined

let normalize_search_paths paths =
  let rec aux (seen, acc) = function
    | [] ->
        List.rev acc
    | path :: rest ->
        let canonical = canonicalize path in
        if StringSet.mem canonical seen then aux (seen, acc) rest
        else aux (StringSet.add canonical seen, canonical :: acc) rest
  in
  aux (StringSet.empty, []) paths
