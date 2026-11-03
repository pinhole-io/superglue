# Network privacy and telemetry shaping

## Status

The techniques on this page are not implemented.
They are not a shipping checklist.
The default path uses documented provider endpoints, TLS, and scrubbed logs.

## Warning

Padding, disguised endpoints, domain fronting, and steganography can break a provider terms of service.
They can also create compliance and trust problems.
Treat this page as background.

## TLS

TLS hides typical payload bytes from a passive observer on the path.
These facts can still be visible:

- A connection happened.
- Timing and size patterns.
- The destination name, unless you add another control.

## Payload shaping

Ideas from early design discussion:

- Pad ciphertext to a fixed size so length does not reveal the operation.
- Add jitter to heartbeats so the interval is not constant.

If you build either one, pair it with a written privacy policy and user consent.

## Destination hiding

Domain fronting can make traffic look like another site.
Many CDNs prohibit it. Get a legal and policy review before any experiment.

DNS over HTTPS and rotating endpoints hide queries from a local DNS observer.
They do not replace TLS or application redaction.

## Mimicry

Hiding control data in images, or copying browser telemetry, is a research topic.
It can violate policy and law. Do not add it to a default build.

## Default architecture

Use the documented endpoint.
Use mTLS when the peer must be authenticated.
Scrub observability. See [Observability](05-observability.md).
Any shaping feature needs an explicit flag and a review.

## See also

- [Security and threat model](06-security-and-threat-model.md)
