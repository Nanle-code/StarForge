/**
 * Mocha test-suite loader.
 *
 * @vscode/test-electron calls this module's exported `run` function after the
 * extension host has started. We discover all *.test.js files compiled into
 * dist/test/suite/ and feed them to Mocha.
 */

import * as path from "path";
import * as Mocha from "mocha";
import { glob } from "glob";

export async function run(): Promise<void> {
  const mocha = new Mocha({
    ui: "tdd",
    color: true,
    timeout: 10_000,
  });

  const testsRoot = path.resolve(__dirname);
  const files = await glob("**/*.test.js", { cwd: testsRoot });

  for (const f of files) {
    mocha.addFile(path.join(testsRoot, f));
  }

  return new Promise<void>((resolve, reject) => {
    mocha.run((failures) => {
      if (failures > 0) {
        reject(new Error(`${failures} test(s) failed.`));
      } else {
        resolve();
      }
    });
  });
}
