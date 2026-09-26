const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const spec = JSON.parse(fs.readFileSync(path.join(root, "openapi.json"), "utf8"));
const routeSources = [
  { file: "src/index.ts", prefix: "" },
  { file: "src/routes/auth.ts", prefix: "/api/auth" },
  { file: "src/routes/templates.ts", prefix: "/api/templates" },
  { file: "src/routes/reviews.ts", prefix: "/api/reviews" },
];
const sourceOperations = new Set();

for (const { file, prefix } of routeSources) {
  const source = fs.readFileSync(path.join(root, file), "utf8");
  const routePattern = /(?:app|router)\.(get|post|put|patch|delete)\s*\(\s*["']([^"']+)["']/g;

  for (const [, method, route] of source.matchAll(routePattern)) {
    const fullPath = `${prefix}${route}`.replace(/:([^/]+)/g, "{$1}");
    sourceOperations.add(`${method.toUpperCase()} ${fullPath}`);
  }
}

const specOperations = new Set();
for (const [route, methods] of Object.entries(spec.paths)) {
  for (const method of Object.keys(methods)) {
    if (["get", "post", "put", "patch", "delete"].includes(method)) {
      specOperations.add(`${method.toUpperCase()} ${route}`);
    }
  }
}

const missingFromSpec = [...sourceOperations].filter((operation) => !specOperations.has(operation));
const missingFromSource = [...specOperations].filter((operation) => !sourceOperations.has(operation));

if (missingFromSpec.length || missingFromSource.length) {
  if (missingFromSpec.length) {
    console.error("Routes missing from openapi.json:");
    missingFromSpec.forEach((operation) => console.error(`  ${operation}`));
  }
  if (missingFromSource.length) {
    console.error("OpenAPI operations missing from the Express routes:");
    missingFromSource.forEach((operation) => console.error(`  ${operation}`));
  }
  process.exit(1);
}

console.log(`OpenAPI route inventory matches ${sourceOperations.size} Express operations.`);