/**
 * Test runner entry-point for the StarForge VS Code extension.
 *
 * Uses @vscode/test-electron to launch a sandboxed VS Code instance, load
 * the compiled extension, and run the Mocha suite in src/test/suite/.
 */

import * as path from "path";
import { runTests } from "@vscode/test-electron";

async function main(): Promise<void> {
  // The folder that contains the extension's package.json
  const extensionDevelopmentPath = path.resolve(__dirname, "../../");

  // The compiled test-suite entry file
  const extensionTestsPath = path.resolve(__dirname, "./suite/index");

  await runTests({
    extensionDevelopmentPath,
    extensionTestsPath,
    // Open an empty workspace so the extension activates cleanly
    launchArgs: ["--disable-extensions"],
  });
}

main().catch((err: unknown) => {
  console.error("Test runner failed:", err);
  process.exit(1);
});
