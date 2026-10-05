# Identity-specific preferences

This shared template intentionally contains no user's name, session ID, keys,
grants or personal preferences. Populate preferences in the identity's private
writable leaf from that user's instructions.

The provisioner supplies the verified identity and session-ID hostname. Do not
derive authentication or permissions from this file.

identity/ is reserved for this identity's sled record, for human and machine
identities alike. Provisioning must obtain that record from the existing identity
owner and write only the matching identity into the writable leaf. The shared
base contains no sled record. This directory does not replace central authority,
mint credentials, or authorize copying the all-identities projection into a guest.
