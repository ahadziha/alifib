module Report = Diagnostics.Report

type status = Load_error | Parser_error | Interpreter_error | Success

type result = {
  context: Interpreter.context;
  diagnostics: Report.t;
  status: status;
}

module Loader = struct
  type t = Interpreter.file_loader

  let path_separator = if Sys.win32 then ';' else ':'

  let split_paths value =
    value
    |> String.split_on_char path_separator
    |> List.filter (fun path -> not (String.equal "" path))

  let env_search_paths () =
    match Sys.getenv_opt "ALIFIB_PATH" with
    | None | Some "" ->
        []
    | Some value ->
        split_paths value

  let normalize = Path.normalize_search_paths

  let default_read_file path =
    if not (Sys.file_exists path) then Error `Not_found
    else
      try
        let contents = In_channel.with_open_bin path In_channel.input_all in
        Ok contents
      with Sys_error message -> Error (`Io_error message)

  let make ?(search_paths = []) ?read_file () =
    let read_file = Option.value ~default:default_read_file read_file in
    let search_paths = normalize search_paths in
    Interpreter.{ search_paths; read_file }

  let default ?search_paths ?read_file () =
    let cwd = Sys.getcwd () |> Path.canonicalize in
    let env_paths = env_search_paths () in
    let extra_paths = Option.value ~default:[] search_paths in
    let combined = (cwd :: env_paths) @ extra_paths in
    make ?read_file ~search_paths:combined ()

  let with_search_paths loader paths =
    let search_paths = normalize paths in
    Interpreter.{ loader with search_paths }

  let prepend_search_paths loader paths =
    let combined = paths @ loader.Interpreter.search_paths in
    with_search_paths loader combined

  let append_search_paths loader paths =
    let combined = loader.Interpreter.search_paths @ paths in
    with_search_paths loader combined
end

let session_producer =
  { Error.Located.phase= `Driver; module_path= Some "interpreter.session" }

let unknown_span = Positions.point_span Positions.unknown_point

let diagnostic ?(details = []) headline =
  Diagnostics.make ~details `Error session_producer unknown_span headline

let has_errors diagnostics =
  List.exists
    (fun (d : Diagnostics.t) -> d.Diagnostics.severity = `Error)
    diagnostics

let ensure_root_in_loader loader canonical_path =
  let root = Filename.dirname canonical_path |> Path.canonicalize in
  let Interpreter.{ search_paths; _ } = loader in
  let desired_paths = Path.normalize_search_paths (root :: search_paths) in
  if desired_paths = search_paths then loader
  else Interpreter.{ loader with search_paths= desired_paths }

let run ?(loader = Loader.default ()) ~path () =
  let canonical_path = Path.canonicalize path in
  let module_id = Id.Module.of_path canonical_path in
  let base_context = Interpreter.make_context ~module_id ~state:State.empty in
  let Interpreter.{ read_file; _ } = loader in
  match read_file canonical_path with
  | Error `Not_found ->
      let diagnostics =
        [
          diagnostic (Printf.sprintf "Could not load `%s`: file not found" path);
        ]
      in
      { context= base_context; diagnostics; status= Load_error }
  | Error (`Io_error reason) ->
      let diagnostics =
        [ diagnostic (Printf.sprintf "Could not load `%s`: %s" path reason) ]
      in
      { context= base_context; diagnostics; status= Load_error }
  | Ok contents ->
      let loader = ensure_root_in_loader loader canonical_path in
      let source = Positions.Source.of_path canonical_path in
      let stream = Token_stream.create ~source contents in
      let program, parse_diagnostics = Parser_driver.parse stream in
      if has_errors parse_diagnostics then
        {
          context= base_context;
          diagnostics= parse_diagnostics;
          status= Parser_error;
        }
      else
        let context_result =
          Interpreter.interpret_program ~loader base_context program
        in
        let diagnostics =
          Report.append parse_diagnostics context_result.diagnostics
        in
        let status =
          match context_result.status with
          | `Ok ->
              Success
          | `Error ->
              Interpreter_error
        in
        { context= context_result.context; diagnostics; status }
