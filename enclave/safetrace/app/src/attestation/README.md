# Remote attestation with Intel Attestation Service
Module to perform remote attestation using the Intel SGX Attestation Service
with EPID. The code allows to construct and send attestation verification
requests to the IAS development environment at
https://api.trustedservices.intel.com/sgx/dev//attestation/v4/report,
according to the
[Intel SGX EPID API Specification](https://api.trustedservices.intel.com/documents/sgx-attestation-api-spec.pdf)

## Requirements
In order to run the attestation code, the following two environment variables
must be set:

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

## Known Issues
The verification of the report via the `verify_report` function:

```rust
use openssl::sign::Verifier;
use openssl::x509::{X509VerifyResult, X509};

impl ASResult {
    /// This function verifies the report and the chain of trust.
    #[logfn(TRACE)]
    pub fn verify_report(&self) -> Result<bool, Error> {
        let ca = X509::from_pem(&self.ca.as_bytes())?;
        let cert = X509::from_pem(&self.certificate.as_bytes())?;
        println!("ca.issued(&cert): {:#?}", ca.issued(&cert));
        match ca.issued(&cert) {
            X509VerifyResult::OK => (),
            _ => return Ok(false),
        };
        let pubkey = cert.public_key()?;
        let sig: Vec<u8> = self.signature.from_hex()?;
        let mut verifier = Verifier::new(MessageDigest::sha256(), &pubkey)?;
        verifier.update(&self.report_string.as_bytes())?;
        println!("verify sig: {:#?}", verifier.verify(&sig)?);
        Ok(verifier.verify(&sig)?)
    }
}
```

returns `false` for the unit tests and end-to-end tests performed so far. The
failure seems to be caused by the signature validation.

The following three unit tests fail:

```shell
attestation::service::test::test_attestation_service_decode_and_verify
esgx::equote::test::test_produce_and_verify_qoute
esgx::equote::test::test_signing_key_against_quote
```

The `verify_report` function performs two checks:

* Did the CA issue the certificate?

```rust
        match ca.issued(&cert) {
            X509VerifyResult::OK => (),
            _ => return Ok(false),
        };
```

* Is the signature valid?

```rust
        let mut verifier = Verifier::new(MessageDigest::sha256(), &pubkey)?;
        verifier.update(&self.report_string.as_bytes())?;
        Ok(verifier.verify(&sig)?)
```

The first check, cert issuance verification, seems to pass, meanwhile the
signature verification appears the one to be failing.

Things to check:

* Is the signature properly extracted from the attestation verification
  response received from IAS?
* Is the report string (message) properly extracted from the response?
* Is the public key in the correct format?

Look at the unit test `test_verify_report`, which passes, and contains hard-
coded data (certs, signature, report string). It may help in providing
insights.

Write a test that fails with hard-coded data as it is done in
`test_verify_report`, as this may help to analyze what is incorrect.

Docs for the openssl X509 api are at
https://docs.rs/openssl/0.10.29/openssl/x509/struct.X509.html.

## Resources
* Entry point docs from Intel (see intro and section on remote attestation
  based on Intel EPID):
  https://software.intel.com/content/www/us/en/develop/topics/software-guard-extensions/attestation-services.html
* [Intel EPID Security Technology](https://software.intel.com/content/www/us/en/develop/articles/intel-enhanced-privacy-id-epid-security-technology.html)
* [IAS API Documentation](https://software.intel.com/content/dam/develop/public/us/en/documents/sgx-attestation-api-spec.pdf)
* [Development (DEV) attestation service portal](https://api.portal.trustedservices.intel.com/EPID-attestation)
* [Rust openssl X509](https://docs.rs/openssl/0.10.29/openssl/x509/struct.X509.html)


[dev-ias-portal]: https://api.portal.trustedservices.intel.com/EPID-attestation
