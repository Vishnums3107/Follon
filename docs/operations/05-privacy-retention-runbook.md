# Privacy and retention runbook

This procedure works from a data inventory containing only pseudonymous subject
hashes. Do not place a name, email address, provider payload, account secret,
or raw customer content in an inventory, plan, receipt, terminal capture, or
incident ticket.

## Preconditions

1. Verify the requester's identity using the organization-approved process
   outside this repository; create a SHA-256 subject reference only after that
   verification.
2. Verify tenant scope and the applicable retention policy with counsel or the
   designated privacy owner.
3. Update the versioned data inventory with every affected regular file,
   classification, owner tenant, explicit UTC retention deadline, subject hash,
   and legal-hold state. Legal holds and `AUDIT_EVIDENCE` are not privacy-erasure
   candidates.
4. Take a protected backup according to the approved retention policy. A backup
   is a governed data asset too; do not quietly preserve data past its deadline.
5. Run from an exclusive maintenance window. The tool rehashes immediately
   before deletion, but the storage administrator must prevent concurrent data
   writers for the selected root.

## Plan before deleting

For retention expiry:

```powershell
cargo run -p follon-cli --bin follon-admin -- retention-plan tests/fixtures/config/commercial-data-inventory-v1.json --data-root C:\tenant-data --tenant-id tenant.acme --as-of 2026-08-12T10:00:00Z --output C:\evidence\retention-plan.json
```

For a verified erasure request:

```powershell
cargo run -p follon-cli --bin follon-admin -- privacy-plan tests/fixtures/config/commercial-data-inventory-v1.json tests/fixtures/config/commercial-privacy-erasure-v1.json --data-root C:\tenant-data --as-of 2026-08-12T10:00:00Z --output C:\evidence\privacy-plan.json
```

Review the immutable plan with the privacy owner. Confirm tenant ID, request ID,
candidate IDs, classification, reason, withheld IDs, expected size, and expected
file hash. The command prints the SHA-256 confirmation token. A plan never
deletes a file by itself.

## Execute one reviewed candidate

Supply the exact printed plan hash, one asset ID, explicit UTC time, and
operator identity. The command refuses an altered plan, changed file, missing
file, symlink, traversal path, unexpected candidate, or time before the plan.

```powershell
cargo run -p follon-cli --bin follon-admin -- retention-execute C:\evidence\privacy-plan.json --data-root C:\tenant-data --asset-id asset.privacy.customer --confirm-plan-hash <printed-plan-sha256> --executed-at 2026-08-12T10:01:00Z --actor privacy.operator --receipt C:\evidence\privacy-receipt.json
```

Archive the immutable receipt with the request record and policy decision. It
contains the plan hash, asset ID, deleted file hash, explicit time, and actor;
it intentionally contains no deleted content.

## Access requests and exceptions

`ACCESS` requests are intentionally not a content-export command. Use the
inventory to locate records, then produce an approved, access-controlled export
through the owning service. Record only the export's immutable evidence hash in
the authorized privacy case system.

If a legal hold, financial-record obligation, security investigation, or audit
retention requirement applies, mark the governed inventory asset `legal_hold:
true`, retain the reason in the organization case system, and do not execute a
plan until the hold is released. Never change a previously published plan or
receipt—issue a new plan with a new explicit time and fingerprint.

## Failure handling

Treat any execution failure as a no-delete result unless the filesystem state
proves otherwise. Inspect the named file and preservation boundary, retain the
failed plan and command error, correct inventory or storage under change
control, and generate a new plan. Do not retry with a modified hash or bypass
the confirmation argument.
