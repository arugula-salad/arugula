// #682: an agent block exports ARUGULA_* (and ILLOGICAL_*) variables
// (ARUGULA_KEEP_PANES, ARUGULA_SOCK, …) that change what every daemon the
// specs start does. playwright.config.ts imports this first, so they go
// before any other module of the run sets its own (local-token.ts sets
// ARUGULA_LOCAL_TOKEN_FILE as it loads). Workers load the config again and
// find the marker. The opt-ins specs read (testnet, VS Code) stay; a
// sandbox provider named in the environment (LOAM_API_URL) goes too.
if (!process.env.E2E_ENV_SCRUBBED) {
  for (const k of Object.keys(process.env)) {
    if (/^(ARUGULA|ILLOGICAL)_/.test(k) && !/^ARUGULA_(TESTNET|VSCODE)/.test(k)) delete process.env[k];
  }
  delete process.env.LOAM_API_URL;
  process.env.E2E_ENV_SCRUBBED = "1";
}
