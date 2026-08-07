# Desktop evidence shell

This client renders a server-owned immutable event trail or the versioned
read-only PAPER operations dashboard, including kill switches, `UNKNOWN`
orders, reconciliation incidents, positions, and the measured 30-day gate.
It deliberately contains no action that can create, approve, cancel, or
transmit an order.

The eventual Tauri host will provide an authenticated WebSocket endpoint at
`/api/v1/evidence`; this TypeScript shell is kept framework-light until the
control-plane stream contract is exercised end to end.

```powershell
npm install
npm run typecheck
```
