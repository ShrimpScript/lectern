# Security

Found something? Please use GitHub's private vulnerability reporting on this repo
("Security" tab → "Report a vulnerability") instead of a public issue.

Design notes relevant to review: Lectern is local-first — agent CLIs keep their own
credentials (Lectern never stores provider keys), MCP/channel configs are read from
each harness's own files, session export encryption is scrypt + XChaCha20-Poly1305
(crates/engine/src/securebundle.rs), and the cloud relay only ever sees counts and
ciphertext. The marketplace has a review-before-install gate plus an AI audit pass.
