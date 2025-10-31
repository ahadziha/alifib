open Format

let usage = "Usage: alifib <input-file> [-o|--output <output-file>]"

let read_file path =
  In_channel.with_open_bin path In_channel.input_all

let write_file path contents =
  Out_channel.with_open_bin path (fun oc ->
      Out_channel.output_string oc contents;
      Out_channel.output_char oc '\n')

let set_output output ref_output =
  match !ref_output with
  | None ->
      ref_output := Some output
  | Some _ ->
      invalid_arg "Output file specified multiple times"

let parse_args () =
  let input = ref None in
  let output = ref None in
  let speclist =
    [
      ("-o", Arg.String (fun s -> set_output s output), "Write AST to file");
      ("--output", Arg.String (fun s -> set_output s output), "Write AST to file");
    ]
  in
  let anon_fun arg =
    match !input with
    | None ->
        input := Some arg
    | Some _ ->
        raise (Arg.Bad "Multiple input files specified")
  in
  Arg.parse speclist anon_fun usage;
  match !input with
  | None ->
      Arg.usage speclist usage;
      exit 1
  | Some path ->
      (path, !output)

let has_error diagnostics =
  List.exists
    (fun (diag : Diagnostics.t) ->
      match diag.severity with
      | `Error ->
          true
      | `Warning | `Info ->
          false)
    diagnostics

let () =
  let input_path, output_path = parse_args () in
  let source = Positions.Source.of_path input_path in
  let contents = read_file input_path in
  let stream = Token_stream.create ~source contents in
  let ast, diagnostics = Parser_driver.parse stream in
  if diagnostics <> [] then
    eprintf "%a@." Diagnostics.Report.pp diagnostics;
  let rendered = Ast_pp.to_string ast in
  (match output_path with
   | None ->
       print_endline rendered
   | Some path ->
       write_file path rendered);
  if has_error diagnostics then exit 1
