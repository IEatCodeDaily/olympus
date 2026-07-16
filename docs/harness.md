# Development QA harness

The live browser gate runs on `fxcompute-01` against Vite `127.0.0.1:5177`
and Hall `127.0.0.1:8799`. It reads
`~/.config/olympus-dev/admin-credentials` at runtime; credentials are never
stored in the repository.

Run on the dev host:

```bash
cd /home/rpw/olympus && ui/scripts/dev-e2e.sh
```

Replacement for the Terminus nightly command:

```bash
ssh fxcompute-01 'cd /home/rpw/olympus && ui/scripts/dev-e2e.sh'
```

The script fails before launching Playwright if either live service or the
credential file is unavailable and enforces a nine-minute timeout.
