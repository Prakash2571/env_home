# Test fixtures

- `access_test_rsa.pk8`: an RSA-2048 key in PKCS#8 DER, made for the tests with
  `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 | openssl pkcs8 -topk8 -nocrypt -outform der`
  (the PKCS#8 layout `ring` accepts).
  The tests sign Cloudflare Access style JWTs with it and serve its public half
  as the team's certs. It protects nothing and is never used outside `cargo test`.
