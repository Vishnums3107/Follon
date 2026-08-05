# Desktop evidence shell

This first client renders a server-owned, immutable event trail and current
simulated P&L. It deliberately contains no action that can create an order,
approve risk, or transmit to a broker.

The eventual Tauri host will provide an authenticated WebSocket endpoint at
`/api/v1/evidence`; this TypeScript shell is kept framework-light until the
control-plane stream contract is exercised end to end.

```powershell
npm install
npm run typecheck
```
