# Contract compatibility policy

The event envelope is the replay and audit boundary. Published v1 payloads are
additive-only: fields may be added only when consumers can ignore them, and no
field may be renamed, retyped, or silently change meaning. Breaking changes
require a new event type version, for example `risk.decision.v2`.

Decimal quantities and monetary values are JSON strings with up to eight
fractional digits. Floating-point JSON numbers are not permitted for accounting
or order quantities.

Protobuf fields are never reused. New fields receive new tag numbers; a removed
field is reserved before the next compatibility release. Worker output is not
trusted merely because it is schema-valid: the Rust core always validates the
canonical intent and applies risk before creating an OMS order.

## Verification

- Validate JSON ingress against the schemas in `json-schema/v1`.
- Ensure canonical event serialization remains stable with deterministic replay tests.
- Treat new required fields or changed enum meaning as a major contract change.
- Document a producer/consumer migration before publishing an incompatible version.

## Pre-release contract status

The checked-in operations, options, commercial-control, and storage-receipt
JSON schemas are pre-release repository contracts until a release explicitly
publishes them. Their required provenance fields must be frozen before external
adoption; after publication, changes such as a new required per-book identity,
predecessor fingerprint, retention confirmation field, dataset identity/hash,
or release-signature binding require a v2 schema plus a documented
producer/consumer migration.
