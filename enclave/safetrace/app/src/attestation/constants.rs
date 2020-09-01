// the attestation service end-point (enigma server)
// pub const ATTESTATION_SERVICE_URL: &str = "https://sgx.enigma.co/api";

/*
 * attestation service end-point (intel, development environment)
 *
 * see section 2.2 Supported Environments in
 * https://api.trustedservices.intel.com/documents/sgx-attestation-api-spec.pdf
 */
pub const ATTESTATION_SERVICE_URL: &str = "https://api.trustedservices.intel.com/sgx/dev/attestation/v4/report";
