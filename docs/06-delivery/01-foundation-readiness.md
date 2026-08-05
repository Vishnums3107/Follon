# Foundation readiness

Complete this checklist before starting feature implementation. It turns the master plan into an executable engineering boundary.

## Product and legal decisions

- [ ] Select the initial commercial jurisdiction.
- [ ] Confirm the initial customer profile and interview plan.
- [ ] Record non-goals and risk philosophy in the product contract.
- [ ] Create the legal-question list: adviser/research classification, broker terms, market-data licensing, privacy, record retention, and cross-border restrictions.
- [ ] Create the initial threat model and data-classification policy.

## Architecture contracts

- [ ] Review and accept the glossary, event envelope, instrument model, order-intent model, and risk-decision model.
- [ ] Define identifiers, decimal/fixed-point conventions, and UTC/exchange-time handling.
- [ ] Accept ADR 0001 or record a superseding ADR.
- [ ] Define the versioning and compatibility policy for Protobuf and JSON schemas.

## Engineering setup

- [ ] Initialize source control and protect the default branch in the remote host.
- [ ] Create the Rust workspace, Python SDK package, TypeScript desktop shell, and contract directories only after the contracts above are reviewed.
- [ ] Configure CI for formatting, linting, tests, dependency pinning/scanning, secret scanning, and build reproducibility.
- [ ] Provision non-production PostgreSQL/object storage and a secrets interface; no broker credentials in code, fixtures, or CI logs.
- [ ] Establish an ADR template, test naming conventions, structured-log fields, and incident-ID convention.

## Exit criterion

Foundation is ready when a developer can implement the vertical slice without inventing a domain name, event field, ownership boundary, or safety rule while coding.
