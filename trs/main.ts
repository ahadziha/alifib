import * as fs from "fs";
import { parseARI } from "./parse";
import { generateAlifib } from "./generate";

function main() {
  const args = process.argv.slice(2);
  if (args.length < 1) {
    console.error("Usage: ari2ali <input.ari> [output.ali]");
    process.exit(1);
  }

  const inputFile = args[0];
  const outputFile = args[1] || inputFile.replace(/\.ari$/, ".ali");
  let moduleName = inputFile
    .replace(/.*\//, "")
    .replace(/\.ari$/, "")
    .replace(/[^a-zA-Z0-9]/g, "_");
  if (/^[0-9]/.test(moduleName)) moduleName = "TRS_" + moduleName;

  const input = fs.readFileSync(inputFile, "utf-8");
  const trs = parseARI(input);

  console.error(`Parsed: ${trs.funs.length} function symbols, ${trs.rules.length} rules`);
  for (const f of trs.funs) {
    console.error(`  ${f.name}/${f.arity}`);
  }

  const output = generateAlifib(trs, moduleName);

  if (outputFile === "-") {
    console.log(output);
  } else {
    fs.writeFileSync(outputFile, output + "\n");
    console.error(`Written to ${outputFile}`);
  }
}

main();
