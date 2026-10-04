# Native protected fields and credentials

G0S 0.3 introduced field envelopes; the native store now writes 0.4 snapshots
with the same envelopes and a schema commitment, and continues reading 0.1..0.3.
Each record encodes a canonical sorted field map before relation metadata.
Public fields contain canonical typed G0V. Private and Secret fields contain
a random 96-bit nonce and an AES-256-GCM field envelope. HKDF-SHA256 derives
the field key from the store master key and a length-delimited context binding
the canonical store directory, tenant, resource, record ID, field name,
schema version, field tag, canonical semantic type and protection kind.
The complete snapshot remains encrypted and authenticated separately.
Moving a protected envelope between record identities fails authentication.

Secret fields require `Secret<T>` and restore the wrapper after decoding `T`.
Private fields permit the same public types as G0V. Nested protected values
are not accepted by that public inner codec. Store permission and per-field
policy checks occur before reads; Secret debug output is redacted. The native
engine wipes intermediate plaintext and derived-key buffers with `zeroize`.
Immutable application `Value` payloads do not promise zeroization on drop;
use secret regions for owned buffers requiring that lifetime behavior.

Credential fields accept `Credential<Text>` or `Credential<Bytes>`. Authorized
creation replaces the supplied original with an opaque verifier immediately.
Snapshots contain only a versioned PBKDF2-HMAC-SHA256 verifier with 600,000
iterations, a random 128-bit salt and a 256-bit derived value. The selected
work factor follows the [OWASP PBKDF2 guidance](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#pbkdf2);
this profile uses the existing ring implementation and makes no FIPS certification claim.
Inputs are 1..4096 bytes; no normalization or truncation occurs.

Ordinary reads cannot retrieve a credential or its verifier. Ordinary updates
cannot replace it. `verify_credential` and `set_credential` require explicit
resource policy actions `credential:verify:FIELD` and `credential:set:FIELD`.
Replacement also respects field mutability, managed-field restrictions and
the field's update policy. Every transaction permits at most 16 credential
derivations/verifications, including invalid authorized attempts. Hosts must
add their application-level admission/rate policy for externally offered login.

G0G 0.4 adds `StoreSetCredential` and `StoreVerifyCredential`, each with fixed
resource and field names. Inputs are record ID, typed credential and an optional
integer dependency ordering a preceding mutation. Replacement returns the
prospective transaction version; verification returns Bool. Exact host grants
match Storage/action/resource/tenant. Public transport codecs continue rejecting
credentials and secrets; these operations require explicit trusted host inputs.

Tests decrypt the outer test snapshot to verify that protected plaintext and
credential originals are still absent, then authenticate a changed record ID
with the test outer key to prove that independent field authentication rejects
the substitution. Persistence, owner isolation, denied ordinary access,
replacement policy, GIR grants and legacy format upgrades are exercised.
