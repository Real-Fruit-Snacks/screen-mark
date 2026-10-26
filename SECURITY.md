# Security

## Supported versions

Only the latest release on the `1.x` line gets fixes. Anything older: upgrade first, then report if it still reproduces.

## Reporting

Do not open a public issue. Use [GitHub private vulnerability reporting](https://github.com/Real-Fruit-Snacks/screen-mark/security/advisories/new).

Include what you found, how to reproduce it, and what you think it lets an attacker do. A suggested fix is welcome but not required.

I acknowledge reports within 48 hours and give a first assessment within a week. The fix ships as soon as it is ready; disclosure timing is agreed with you.

## In scope

- Command injection, in particular the PowerShell path used for OCR
- Memory safety in `unsafe` blocks (Win32 capture, X11 capture, clipboard)
- Path traversal in save, history, and pin temp-file handling
- Secret exposure, such as the Imgur client ID or anything written to logs
- Supply chain: dependencies, `Cargo.lock`, and the GitHub Actions workflows

## Previous fixes

- v1.1.0: `rustls-webpki` 0.103.10 → 0.103.13 (RUSTSEC-2026-0098, -0099, -0104)
- v0.5.3: removed the hardcoded Imgur client ID, fixed an OCR temp-file race, pinned GitHub Actions to commit SHAs
- v0.5.2: fixed command injection in the OCR PowerShell call and an integer overflow in capture buffer sizing
