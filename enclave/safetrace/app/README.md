# Safetrace App
Code for the safetrace app. The code is adapted from the enigmampc/Safetrace.
The remote attestation is done with Intel Attestation Service instead of going
through Enigma's server, which requires authorization. See more on the
attestation under [./src/attestation/README.md](./src/attestation/README.md).

## Running the unit tests
To run the unit tests, two environment variables, used for the remote
attestation, must be set:

* `IAS_SGX_SPID` (Service Provider ID)
* `IAS_SGX_PRIMARY_KEY` (aka subscription & API key)

The SPID and API key can be obtained by signing up on the
[Development (DEV) attestation service portal][dev-ias-portal].

In the shell where unit tests or the `safetrace-app` binary are run,
the environment variable should be set, e.g.:

```shell
$ export IAS_SGX_SPID=39M83M77927C59MC038AMA3E26583340
$ export IAS_SGX_PRIMARY_KEY=c43d3253a9354f1984aee47e55c9bcaf
```

**IMPORTANT: KEEP the API key secret**

> Subscription Key is a credential to access the API. It is known only to the
  owner (i.e. Service Provider) and it is the responsibility of the owner to
  protect its confidentiality. API portal allows for an on-demand rotation of
  the keys to support custom key rotation policies.

For more details see
[./src/attestation/README.md](./src/attestation/README.md).

[dev-ias-portal]: https://api.portal.trustedservices.intel.com/EPID-attestation
