// Canonical production-UI gate, shared by local verification and CI.
const { spawnSync } = require("node:child_process");
const path = require("node:path");
const suites = ["browser_rewrite.cjs", "browser_rewrite_parameters.cjs", "browser_rewrite_plugins.cjs"];
let failed = false;
for (const suite of suites) {
  const result = spawnSync(process.execPath, [path.join(__dirname, suite)], {
    stdio: "inherit", windowsHide: true, env: process.env,
  });
  if (result.error || result.status !== 0) {
    failed = true;
    console.error("FAIL", suite, result.error || result.signal || result.status);
  }
}
process.exitCode = failed ? 1 : 0;
