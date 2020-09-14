//! # Attestation service.
//! Taken from enigmampc/enigma-core/enigma-tools-u/attestation_service/service.rs
//! and adapted to work with Intel's Attestation Service.

use base64;
use enigma_tools_u::common_u::errors;
use failure::Error;
use hex::{FromHex, ToHex};
use openssl::hash::MessageDigest;
use openssl::sign::Verifier;
use openssl::x509::{X509VerifyResult, X509};
use reqwest::{self, Client, header::HeaderMap};
use serde_json;
use serde_json::Value;
use std::io::Read;
use std::mem;
use std::string::ToString;

const ATTESTATION_SERVICE_DEFAULT_RETRIES: u32 = 10;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ASReport {
    pub id: String,
    pub timestamp: String,
    pub version: usize,
    #[serde(rename = "isvEnclaveQuoteStatus")]
    pub isv_enclave_quote_status: String,
    #[serde(rename = "isvEnclaveQuoteBody")]
    pub isv_enclave_quote_body: String,
    #[serde(rename = "revocationReason")]
    pub revocation_reason: Option<String>,
    #[serde(rename = "pseManifestStatus")]
    pub pse_manifest_satus: Option<String>,
    #[serde(rename = "pseManifestHash")]
    pub pse_manifest_hash: Option<String>,
    #[serde(rename = "platformInfoBlob")]
    pub platform_info_blob: Option<String>,
    pub nonce: Option<String>,
    #[serde(rename = "epidPseudonym")]
    pub epid_pseudonym: Option<String>,
    #[serde(rename = "advisoryIDs")]
    pub advisory_ids: Option<Vec<String>>,
    #[serde(rename = "advisoryURL")]
    pub advisory_url: Option<String>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct ASResult {
    pub ca: String,
    pub certificate: String,
    pub report: ASReport,
    pub report_string: String,
    pub signature: String,
    pub validate: bool,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct ASResponse {
    pub id: i64,
    pub jsonrpc: String,
    pub result: ASResult,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Params {
    pub quote: String,
    pub production: bool,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct QuoteRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Params,
    pub id: i32,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct IASRequest {
    #[serde(rename = "isvEnclaveQuote")]
    isv_enclave_quote: String,
}

#[derive(Default)]
pub struct Quote {
    pub body: QBody,
    pub report_body: QReportBody,
}

pub struct QBody {
    // size: 48
    pub version: [u8; 2],
    pub signature_type: [u8; 2],
    pub gid: [u8; 4],
    pub isv_svn_qe: [u8; 2],
    pub isv_svn_pce: [u8; 2],
    pub reserved: [u8; 4],
    pub base_name: [u8; 32],
}

pub struct QReportBody {
    // size: 384
    pub cpu_svn: [u8; 16],
    pub misc_select: [u8; 4],
    pub reserved: [u8; 28],
    pub attributes: [u8; 16],
    pub mr_enclave: [u8; 32],
    pub reserved2: [u8; 32],
    pub mr_signer: [u8; 32],
    pub reserved3: [u8; 96],
    pub isv_prod_id: [u8; 2],
    pub isv_svn: [u8; 2],
    pub reserved4: [u8; 60],
    pub report_data: [u8; 64],
}

pub struct AttestationService {
    connection_str: String,
    /// amount of attempts per network call
    retries: u32,
}

impl AttestationService {
    pub fn new(conn_str: &str) -> AttestationService {
        AttestationService { connection_str: conn_str.to_string(), retries: ATTESTATION_SERVICE_DEFAULT_RETRIES }
    }

    pub fn new_with_retries(conn_str: &str, retries: u32) -> AttestationService {
        AttestationService { connection_str: conn_str.to_string(), retries }
    }

    /* NOTE: Functions to interact with Intel's Attestation Service (IAS) for SGX.
     *
     * As opposed to sending requests to enigma's server, requests are sent to
     * https://api.trustedservices.intel.com/sgx/dev, and the request payload is
     * constructed according to the specification found at
     * https://api.trustedservices.intel.com/documents/sgx-attestation-api-spec.pdf.
     * The response is also processed according to the specification.
     */
    #[logfn(TRACE)]
    pub fn get_report(&self, quote: String, api_key: &str) -> Result<ASResponse, Error> {
        let request: IASRequest = IASRequest {
            isv_enclave_quote: quote,
        };
        println!("sending IAS request {:#?}: ", request);
        let response: ASResponse = self.send_request(&request, api_key)?;
        Ok(response)
    }


    // request the report object
    pub fn send_request(&self, quote_req: &IASRequest, api_key: &str) -> Result<ASResponse, Error> {
        let client = reqwest::Client::new();
        self.attempt_request(&client, quote_req, api_key).or_else(|mut res_err| {
            for _ in 0..self.retries {
                match self.attempt_request(&client, quote_req, api_key) {
                    Ok(response) => return Ok(response),
                    Err(e) => res_err = e,
                }
            }
            return Err(res_err)
        })
    }

    fn attempt_request(&self, client: &Client, quote_req: &IASRequest, api_key: &str) -> Result<ASResponse, Error> {
        let mut res = client.post(self.connection_str.as_str())
            .header("Content-type", "application/json")
            .header("Ocp-Apim-Subscription-Key", api_key)
            .json(&quote_req)
            .send()?;

        if res.status().is_success() {
            let json_response: Value = res.json()?;
            println!("json response: {:#?}", json_response);
            let headers: &HeaderMap = res.headers();
            println!("headers: {:#?}", headers);
            let response: ASResponse = self.unwrap_response(&headers, &json_response);
            Ok(response)
        }
        else {
            let message = format!("[-] AttestationService: Invalid quote. \
                                            Status code: {:?}\n", res.status());
            Err(errors::AttestationServiceErr { message }.into())
        }
    }

    #[logfn(TRACE)]
    fn unwrap_result(&self, headers: &HeaderMap, json_response: &Value) -> ASResult {
        let (ca, certificate) = self.get_signing_certs(headers).unwrap();
        let signature = self.get_signature(headers).unwrap();
        let validate = true;    // TODO see whether this is needed, or how it is used
        let report_string = json_response.to_string();
        let report: ASReport = serde_json::from_str(&report_string).unwrap();
        ASResult { ca, certificate, signature, validate, report, report_string }
    }

    fn unwrap_response(&self, headers: &HeaderMap, json_response: &Value) -> ASResponse {
        let result: ASResult = self.unwrap_result(headers, json_response);
        let id: i64 = 12345; // dummy id - not sure what this is supposed to be
        let jsonrpc = String::from("2.0"); // dummy - not sure what this is for
        ASResponse { id, jsonrpc, result }
    }

    fn get_signing_certs(&self, headers: &HeaderMap) -> Result<(String, String), Error> {
        let signing_cert_header = "X-IASReport-Signing-Certificate";
        let signature_cert = headers.get(signing_cert_header).unwrap().to_str().unwrap();
        let decoded_cert = percent_encoding::percent_decode_str(signature_cert).decode_utf8().unwrap();
        let certs = X509::stack_from_pem(decoded_cert.as_bytes())?;
        let cert_obj = &certs[0];
        let ca_obj = &certs[1];
        let certificate = String::from_utf8(cert_obj.to_pem().unwrap()).unwrap();
        let ca = String::from_utf8(ca_obj.to_pem().unwrap()).unwrap();
        Ok((ca, certificate))
    }

    fn get_signature(&self, headers: &HeaderMap) -> Result<String, Error> {
        let signature_header = "X-IASReport-Signature";
        // NOTE SIGNATURE (in hex)
        //let message = format!("[-] AttestationService: missing header {:?}", signature_header);
        let signature_b64 = headers.get(signature_header).unwrap();
            //.ok_or_else(|| errors::AttestationServiceErr { message }.into())?;
        //println!("signature: {:#?}", signature_b64);
        let signature_bytes = base64::decode(signature_b64)?;
        let signature = signature_bytes.to_hex();
        //println!("signature base64 decoded in hex fmt: {:#?}", signature);
        Ok(signature)
    }
}

impl ASResponse {
    pub fn get_quote(&self) -> Result<Quote, Error> { Quote::from_base64(&self.result.report.isv_enclave_quote_body) }
}

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

impl Quote {
    pub fn from_base64(encoded_quote: &str) -> Result<Quote, Error> {
        let quote_bytes = base64::decode(encoded_quote)?;

        Ok(Quote {
            body: QBody::from_bytes_read(&mut &quote_bytes[..48])?,
            report_body: QReportBody::from_bytes_read(&mut &quote_bytes[48..432])?,
        })
    }
}

impl QBody {

    /// This will read the data given to it and parse it byte by byte just like the API says
    /// The exact sizes of the field in `QBody` are extremley important.
    /// also the order in which `read_exact` is executed (filed by field just like the API) is also important
    /// because it reads the bytes sequentially.
    /// if the Reader is shorter or longer then the size of QBody it will return an error.
    pub fn from_bytes_read<R: Read>(body: &mut R) -> Result<QBody, Error> {
        let mut result: QBody = Default::default();

        body.read_exact(&mut result.version)?;
        body.read_exact(&mut result.signature_type)?;
        body.read_exact(&mut result.gid)?;
        body.read_exact(&mut result.isv_svn_qe)?;
        body.read_exact(&mut result.isv_svn_pce)?;
        body.read_exact(&mut result.reserved)?;
        body.read_exact(&mut result.base_name)?;

        if body.read(&mut [0u8])? != 0 {
            return Err(errors::QuoteErr { message: "String passed to QBody is too big".to_string() }.into());
        }
        Ok(result)
    }
}

impl Default for QBody {
    // Using `mem::zeroed()` here should be safe because all the fields are [u8]
    // *But* this isn't good practice. because if you add a Box/Vec or any other complex type this *will* become UB(Undefined Behavior).
    fn default() -> QBody { unsafe { mem::zeroed() } }
}

impl QReportBody {
    /// This will read the data given to it and parse it byte by byte just like the API says
    /// The exact sizes of the field in `QBody` are extremley important.
    /// also the order in which `read_exact` is executed (filed by field just like the API) is also important
    /// because it reads the bytes sequentially.
    /// if the Reader is shorter or longer then the size of QBody it will return an error.
    /// Overall Size: 384
    pub fn from_bytes_read<R: Read>(body: &mut R) -> Result<QReportBody, Error> {
        let mut result: QReportBody = Default::default();

        body.read_exact(&mut result.cpu_svn)?;
        body.read_exact(&mut result.misc_select)?;
        body.read_exact(&mut result.reserved)?;
        body.read_exact(&mut result.attributes)?;
        body.read_exact(&mut result.mr_enclave)?;
        body.read_exact(&mut result.reserved2)?;
        body.read_exact(&mut result.mr_signer)?;
        body.read_exact(&mut result.reserved3)?;
        body.read_exact(&mut result.isv_prod_id)?;
        body.read_exact(&mut result.isv_svn)?;
        body.read_exact(&mut result.reserved4)?;
        body.read_exact(&mut result.report_data)?;

        if body.read(&mut [0u8])? != 0 {
            return Err(errors::QuoteErr { message: "String passed to QReportBody is too big".to_string() }.into());
        }
        Ok(result)
    }
}

impl Default for QReportBody {
    // Using `mem::zeroed()` here should be safe because all the fields are [u8]
    // *But* this isn't good practice. because if you add a Box/Vec or any other complex type this *will* become UB(Undefined Behavior).
    fn default() -> QReportBody { unsafe { mem::zeroed() } }
}

#[cfg(test)]
mod test {
    use crate::attestation::{self, service::*};
    use std::env;
    use std::str::from_utf8;
    use hex::FromHex;
    use enigma_tools_u::common_u::errors::AttestationServiceErr;

    fn get_api_key() -> String {
        let api_key: String = env::var("IAS_SGX_PRIMARY_KEY")
            .expect("Environement variable 'IAS_SGX_PRIMARY_KEY' is not set! \
                Set it with export IAS_SGX_PRIMARY_KEY=...");
        api_key
    }

    // this unit-test is for the attestation service
    // it uses a hardcoded quote that is validated
    // the test requests a report from the attestation service construct an object with the response
    // for signing the report there's additional field that can be accessed via ASResponse.result.report_string
    #[test]
    fn test_get_response_attestation_service() {
        // build a request
        let service: AttestationService = AttestationService::new(attestation::constants::ATTESTATION_SERVICE_URL);
        let quote = String::from("AgAAAFsLAAALAAoAAAAAAGSNbR/rEqR4eYf3LM8K2cd8sdcwHQX1eJnLpKpgjG8uCRD//wECAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABwAAAAAAAAAHAAAAAAAAAIlox4uTl6KzEQsrHb0d9FtwycmY7eKPyJgUNhOSTM0MAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACD1xnnferKFHD2uvYqTXdDA8iZ22kCD5xw7h38CMfOngAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADmCQibI7Rr/gg54tJ49aVH0fFhyQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAqAIAAJzTIYbi2wIitvQEZ0uQ1i6IAl3wrSvjXCUwxHDSLaRaYRB3JwQil8huhGwkx2NB30WgdjMnPtVkGI/6LSgJOLoWpR9fiAuhNYNmm6NQVBD+VXG4vwrhRuKD9nqonSg5+v/aAWgMLdjVieG+erXlySXTahf0EaSmWtB6PwN+Ks0meM9TGvBsJp1QSSb8/sRwC6/380MNd5cfRmhV+OzswFI6qcR/7XYcsefwzfTHQnG4KFr0SfYaT0ZL4s4mYQosWflAQi1o1DI46EjhR87prnKbElA5BJbpMRHaEdcbCf/BZKINkB3/Mhpb9+B/k2/TEmDC8MlzqKWyq+tuD/uqdmzWLr25ra5aMvXt2BHNQd76K6BbRy1xDGlmgUgW6+zGkX7HOaOmKL23rOBuE2gBAAB/f7hJDJb/p/Uzd/XeoLNI5KxkFYYq1FcYEMRwYJTc9JpS9hBMDjVeARpBGuj3MKjEpQXdepLX1YCjBW0x9xwFYPG4e5ZNAWzxyRMLiaOxA7RBNmt3flypUfmWKaS1zTSzNZhxZbJeQw7En0LudaIbBGB0nbWo0Y6usNW2rXSTEh3DZZGdxGoQoUk2LagQcPwesWl5WuIBqESsYgDErubwBUq/XZ9Nrpepf5Hg7xbnLtYJMrF+FBFLg5FU/19cAY5ZokeYwIRheULn8w4q6E9ownlIrXZyV/o5ykAm2GC1a900MzpGOyc7Cb6ujNo+YQajO1RsipB/ahr8DaSBmL2ao01+tRW3izlH67Kx82CdeM3+f7KObK0AjF5lW8mOEf0kpeyiCgHkDMkzWToPUg3S6sgzAefI5PpmG9VzBwJ+7R3kCIumO0VklaVeuD2GwKwfEMlDbEjF7P95AgCaPoqNLG0dxTl4ee10z9OBFdtv4QVFSL2vpcWK");
        let as_response = service.get_report(quote, &get_api_key()).unwrap();
        // THE report as a string ready for signing
        //println!("report to be signed string => {}",as_response.result.report_string );
        // example on how to access some param inside ASResponse
        //println!("report isv enclave quote status  => {}",as_response.result.report.isvEnclaveQuoteStatus );
        assert_eq!(true, as_response.result.validate);
        assert_eq!("2.0", as_response.jsonrpc);
    }

    // Run the same test but with no option of retries
    #[test]
    fn test_get_response_attestation_service_no_retries() {
        // build a request with an initialized amount of 0 retries
        let service: AttestationService = AttestationService::new_with_retries(attestation::constants::ATTESTATION_SERVICE_URL, 0);
        let quote = String::from("AgAAAFsLAAALAAoAAAAAAGSNbR/rEqR4eYf3LM8K2cd8sdcwHQX1eJnLpKpgjG8uCRD//wECAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABwAAAAAAAAAHAAAAAAAAAIlox4uTl6KzEQsrHb0d9FtwycmY7eKPyJgUNhOSTM0MAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACD1xnnferKFHD2uvYqTXdDA8iZ22kCD5xw7h38CMfOngAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADmCQibI7Rr/gg54tJ49aVH0fFhyQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAqAIAAJzTIYbi2wIitvQEZ0uQ1i6IAl3wrSvjXCUwxHDSLaRaYRB3JwQil8huhGwkx2NB30WgdjMnPtVkGI/6LSgJOLoWpR9fiAuhNYNmm6NQVBD+VXG4vwrhRuKD9nqonSg5+v/aAWgMLdjVieG+erXlySXTahf0EaSmWtB6PwN+Ks0meM9TGvBsJp1QSSb8/sRwC6/380MNd5cfRmhV+OzswFI6qcR/7XYcsefwzfTHQnG4KFr0SfYaT0ZL4s4mYQosWflAQi1o1DI46EjhR87prnKbElA5BJbpMRHaEdcbCf/BZKINkB3/Mhpb9+B/k2/TEmDC8MlzqKWyq+tuD/uqdmzWLr25ra5aMvXt2BHNQd76K6BbRy1xDGlmgUgW6+zGkX7HOaOmKL23rOBuE2gBAAB/f7hJDJb/p/Uzd/XeoLNI5KxkFYYq1FcYEMRwYJTc9JpS9hBMDjVeARpBGuj3MKjEpQXdepLX1YCjBW0x9xwFYPG4e5ZNAWzxyRMLiaOxA7RBNmt3flypUfmWKaS1zTSzNZhxZbJeQw7En0LudaIbBGB0nbWo0Y6usNW2rXSTEh3DZZGdxGoQoUk2LagQcPwesWl5WuIBqESsYgDErubwBUq/XZ9Nrpepf5Hg7xbnLtYJMrF+FBFLg5FU/19cAY5ZokeYwIRheULn8w4q6E9ownlIrXZyV/o5ykAm2GC1a900MzpGOyc7Cb6ujNo+YQajO1RsipB/ahr8DaSBmL2ao01+tRW3izlH67Kx82CdeM3+f7KObK0AjF5lW8mOEf0kpeyiCgHkDMkzWToPUg3S6sgzAefI5PpmG9VzBwJ+7R3kCIumO0VklaVeuD2GwKwfEMlDbEjF7P95AgCaPoqNLG0dxTl4ee10z9OBFdtv4QVFSL2vpcWK");
        let as_response = service.get_report(quote, &get_api_key()).unwrap();

        assert_eq!(true, as_response.result.validate);
        assert_eq!("2.0", as_response.jsonrpc);
    }

    #[test]
    fn test_response_attestation_service_failure_no_retries() {
        // build a faulty request
        let service: AttestationService = AttestationService::new_with_retries(attestation::constants::ATTESTATION_SERVICE_URL, 0);
        let quote = String::from("Wrong quote");
        let as_response = service.get_report(quote.clone(), &get_api_key());
        // if it's able to do the downcast, we got the correct error
        println!("error: {:#?}", as_response);
        assert!(as_response.unwrap_err().downcast::<AttestationServiceErr>().is_ok());
    }

    #[test]
    fn test_verify_report() {
        let report = ASResult {
             ca: "-----BEGIN CERTIFICATE-----\nMIIFSzCCA7OgAwIBAgIJANEHdl0yo7CUMA0GCSqGSIb3DQEBCwUAMH4xCzAJBgNV\nBAYTAlVTMQswCQYDVQQIDAJDQTEUMBIGA1UEBwwLU2FudGEgQ2xhcmExGjAYBgNV\nBAoMEUludGVsIENvcnBvcmF0aW9uMTAwLgYDVQQDDCdJbnRlbCBTR1ggQXR0ZXN0\nYXRpb24gUmVwb3J0IFNpZ25pbmcgQ0EwIBcNMTYxMTE0MTUzNzMxWhgPMjA0OTEy\nMzEyMzU5NTlaMH4xCzAJBgNVBAYTAlVTMQswCQYDVQQIDAJDQTEUMBIGA1UEBwwL\nU2FudGEgQ2xhcmExGjAYBgNVBAoMEUludGVsIENvcnBvcmF0aW9uMTAwLgYDVQQD\nDCdJbnRlbCBTR1ggQXR0ZXN0YXRpb24gUmVwb3J0IFNpZ25pbmcgQ0EwggGiMA0G\nCSqGSIb3DQEBAQUAA4IBjwAwggGKAoIBgQCfPGR+tXc8u1EtJzLA10Feu1Wg+p7e\nLmSRmeaCHbkQ1TF3Nwl3RmpqXkeGzNLd69QUnWovYyVSndEMyYc3sHecGgfinEeh\nrgBJSEdsSJ9FpaFdesjsxqzGRa20PYdnnfWcCTvFoulpbFR4VBuXnnVLVzkUvlXT\nL/TAnd8nIZk0zZkFJ7P5LtePvykkar7LcSQO85wtcQe0R1Raf/sQ6wYKaKmFgCGe\nNpEJUmg4ktal4qgIAxk+QHUxQE42sxViN5mqglB0QJdUot/o9a/V/mMeH8KvOAiQ\nbyinkNndn+Bgk5sSV5DFgF0DffVqmVMblt5p3jPtImzBIH0QQrXJq39AT8cRwP5H\nafuVeLHcDsRp6hol4P+ZFIhu8mmbI1u0hH3W/0C2BuYXB5PC+5izFFh/nP0lc2Lf\n6rELO9LZdnOhpL1ExFOq9H/B8tPQ84T3Sgb4nAifDabNt/zu6MmCGo5U8lwEFtGM\nRoOaX4AS+909x00lYnmtwsDVWv9vBiJCXRsCAwEAAaOByTCBxjBgBgNVHR8EWTBX\nMFWgU6BRhk9odHRwOi8vdHJ1c3RlZHNlcnZpY2VzLmludGVsLmNvbS9jb250ZW50\nL0NSTC9TR1gvQXR0ZXN0YXRpb25SZXBvcnRTaWduaW5nQ0EuY3JsMB0GA1UdDgQW\nBBR4Q3t2pn680K9+QjfrNXw7hwFRPDAfBgNVHSMEGDAWgBR4Q3t2pn680K9+Qjfr\nNXw7hwFRPDAOBgNVHQ8BAf8EBAMCAQYwEgYDVR0TAQH/BAgwBgEB/wIBADANBgkq\nhkiG9w0BAQsFAAOCAYEAeF8tYMXICvQqeXYQITkV2oLJsp6J4JAqJabHWxYJHGir\nIEqucRiJSSx+HjIJEUVaj8E0QjEud6Y5lNmXlcjqRXaCPOqK0eGRz6hi+ripMtPZ\nsFNaBwLQVV905SDjAzDzNIDnrcnXyB4gcDFCvwDFKKgLRjOB/WAqgscDUoGq5ZVi\nzLUzTqiQPmULAQaB9c6Oti6snEFJiCQ67JLyW/E83/frzCmO5Ru6WjU4tmsmy8Ra\nUd4APK0wZTGtfPXU7w+IBdG5Ez0kE1qzxGQaL4gINJ1zMyleDnbuS8UicjJijvqA\n152Sq049ESDz+1rRGc2NVEqh1KaGXmtXvqxXcTB+Ljy5Bw2ke0v8iGngFBPqCTVB\n3op5KBG3RjbF6RRSzwzuWfL7QErNC8WEy5yDVARzTA5+xmBc388v9Dm21HGfcC8O\nDD+gT9sSpssq0ascmvH49MOgjt1yoysLtdCtJW/9FZpoOypaHx0R+mJTLwPXVMrv\nDaVzWh5aiEx+idkSGMnX\n-----END CERTIFICATE-----".to_string(),
             certificate: "-----BEGIN CERTIFICATE-----\nMIIEoTCCAwmgAwIBAgIJANEHdl0yo7CWMA0GCSqGSIb3DQEBCwUAMH4xCzAJBgNV\nBAYTAlVTMQswCQYDVQQIDAJDQTEUMBIGA1UEBwwLU2FudGEgQ2xhcmExGjAYBgNV\nBAoMEUludGVsIENvcnBvcmF0aW9uMTAwLgYDVQQDDCdJbnRlbCBTR1ggQXR0ZXN0\nYXRpb24gUmVwb3J0IFNpZ25pbmcgQ0EwHhcNMTYxMTIyMDkzNjU4WhcNMjYxMTIw\nMDkzNjU4WjB7MQswCQYDVQQGEwJVUzELMAkGA1UECAwCQ0ExFDASBgNVBAcMC1Nh\nbnRhIENsYXJhMRowGAYDVQQKDBFJbnRlbCBDb3Jwb3JhdGlvbjEtMCsGA1UEAwwk\nSW50ZWwgU0dYIEF0dGVzdGF0aW9uIFJlcG9ydCBTaWduaW5nMIIBIjANBgkqhkiG\n9w0BAQEFAAOCAQ8AMIIBCgKCAQEAqXot4OZuphR8nudFrAFiaGxxkgma/Es/BA+t\nbeCTUR106AL1ENcWA4FX3K+E9BBL0/7X5rj5nIgX/R/1ubhkKWw9gfqPG3KeAtId\ncv/uTO1yXv50vqaPvE1CRChvzdS/ZEBqQ5oVvLTPZ3VEicQjlytKgN9cLnxbwtuv\nLUK7eyRPfJW/ksddOzP8VBBniolYnRCD2jrMRZ8nBM2ZWYwnXnwYeOAHV+W9tOhA\nImwRwKF/95yAsVwd21ryHMJBcGH70qLagZ7Ttyt++qO/6+KAXJuKwZqjRlEtSEz8\ngZQeFfVYgcwSfo96oSMAzVr7V0L6HSDLRnpb6xxmbPdqNol4tQIDAQABo4GkMIGh\nMB8GA1UdIwQYMBaAFHhDe3amfrzQr35CN+s1fDuHAVE8MA4GA1UdDwEB/wQEAwIG\nwDAMBgNVHRMBAf8EAjAAMGAGA1UdHwRZMFcwVaBToFGGT2h0dHA6Ly90cnVzdGVk\nc2VydmljZXMuaW50ZWwuY29tL2NvbnRlbnQvQ1JML1NHWC9BdHRlc3RhdGlvblJl\ncG9ydFNpZ25pbmdDQS5jcmwwDQYJKoZIhvcNAQELBQADggGBAGcIthtcK9IVRz4r\nRq+ZKE+7k50/OxUsmW8aavOzKb0iCx07YQ9rzi5nU73tME2yGRLzhSViFs/LpFa9\nlpQL6JL1aQwmDR74TxYGBAIi5f4I5TJoCCEqRHz91kpG6Uvyn2tLmnIdJbPE4vYv\nWLrtXXfFBSSPD4Afn7+3/XUggAlc7oCTizOfbbtOFlYA4g5KcYgS1J2ZAeMQqbUd\nZseZCcaZZZn65tdqee8UXZlDvx0+NdO0LR+5pFy+juM0wWbu59MvzcmTXbjsi7HY\n6zd53Yq5K244fwFHRQ8eOB0IWB+4PfM7FeAApZvlfqlKOlLcZL2uyVmzRkyR5yW7\n2uo9mehX44CiPJ2fse9Y6eQtcfEhMPkmHXI01sN+KwPbpA39+xOsStjhP9N1Y1a2\ntQAVo+yVgLgV2Hws73Fc0o3wC78qPEA+v2aRs/Be3ZFDgDyghc/1fgU+7C+P6kbq\nd4poyb6IW8KCJbxfMJvkordNOgOUUxndPHEi/tb/U7uLjLOgPA==\n-----END CERTIFICATE-----".to_string(),
             report: Default::default(),
             report_string: "{\"id\":\"100342731086430570647295023189732744265\",\"timestamp\":\"2018-07-15T16:06:47.993263\",\"isvEnclaveQuoteStatus\":\"GROUP_OUT_OF_DATE\",\"platformInfoBlob\":\"1502006504000100000505020401010000000000000000000007000006000000020000000000000ADAD85ADE5C84743B9E8ABF2638808A7597A6EEBCEAA6A041429083B3CF232D6F746C7B19C832166D8ABB60F90BCE917270555115B0050F7E65B81253F794F665AA\",\"isvEnclaveQuoteBody\":\"AgAAANoKAAAHAAYAAAAAABYB+Vw5ueowf+qruQGtw+5gbJslhOX9eWDNazWpHhBVBAT/////AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABwAAAAAAAAAHAAAAAAAAABIhP23bLUNSZ1yvFIrZa0pu/zt6/n3X8qNjMVbWgOGDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACD1xnnferKFHD2uvYqTXdDA8iZ22kCD5xw7h38CMfOngAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAweDRlNmRkMjg0NzdkM2NkY2QzMTA3NTA3YjYxNzM3YWFhMTU5MTYwNzAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"}".to_string(),
             signature: "9e6a05bf42a627e3066b0067dc98bc22670df0061e42eed6a5af51ffa2e3b41949b6b177980b68c43855d4df71b2817b30f54bc40566225e6b721eb21fc0aba9b58e043bfaaae320e8d9613d514c0694b36b3fe41588b15480a6f7a4d025c244af531c7145d37f8b28c223bfb46c157470246e3dbd4aa15681103df2c8fd47bb59f7b827de559992fd24260e1113912bd98ba5cd769504bb5f21471ecd4f7713f600ae5169761c9047c09d186ad91f5ff89893c13be15d11bb663099192bcf2ce81f3cbbc28c9db93ce1a4df1141372d0d738fd9d0924d1e4fe58a6e2d12a5d2f723e498b783a6355ca737c4b0feeae3285340171cbe96ade8d8b926b23a8c90".to_string(),
             validate: true,
         };
        assert!(report.verify_report().unwrap());
    }

    #[test]
    fn test_attestation_service_decode_and_verify() {
        let service: AttestationService = AttestationService::new(attestation::constants::ATTESTATION_SERVICE_URL);
        let encrypted_quote = String::from("AgAAAFsLAAALAAoAAAAAAGSNbR/rEqR4eYf3LM8K2cd8sdcwHQX1eJnLpKpgjG8uCRD//wECAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABwAAAAAAAAAHAAAAAAAAAIlox4uTl6KzEQsrHb0d9FtwycmY7eKPyJgUNhOSTM0MAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACD1xnnferKFHD2uvYqTXdDA8iZ22kCD5xw7h38CMfOngAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADmCQibI7Rr/gg54tJ49aVH0fFhyQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAqAIAAJzTIYbi2wIitvQEZ0uQ1i6IAl3wrSvjXCUwxHDSLaRaYRB3JwQil8huhGwkx2NB30WgdjMnPtVkGI/6LSgJOLoWpR9fiAuhNYNmm6NQVBD+VXG4vwrhRuKD9nqonSg5+v/aAWgMLdjVieG+erXlySXTahf0EaSmWtB6PwN+Ks0meM9TGvBsJp1QSSb8/sRwC6/380MNd5cfRmhV+OzswFI6qcR/7XYcsefwzfTHQnG4KFr0SfYaT0ZL4s4mYQosWflAQi1o1DI46EjhR87prnKbElA5BJbpMRHaEdcbCf/BZKINkB3/Mhpb9+B/k2/TEmDC8MlzqKWyq+tuD/uqdmzWLr25ra5aMvXt2BHNQd76K6BbRy1xDGlmgUgW6+zGkX7HOaOmKL23rOBuE2gBAAB/f7hJDJb/p/Uzd/XeoLNI5KxkFYYq1FcYEMRwYJTc9JpS9hBMDjVeARpBGuj3MKjEpQXdepLX1YCjBW0x9xwFYPG4e5ZNAWzxyRMLiaOxA7RBNmt3flypUfmWKaS1zTSzNZhxZbJeQw7En0LudaIbBGB0nbWo0Y6usNW2rXSTEh3DZZGdxGoQoUk2LagQcPwesWl5WuIBqESsYgDErubwBUq/XZ9Nrpepf5Hg7xbnLtYJMrF+FBFLg5FU/19cAY5ZokeYwIRheULn8w4q6E9ownlIrXZyV/o5ykAm2GC1a900MzpGOyc7Cb6ujNo+YQajO1RsipB/ahr8DaSBmL2ao01+tRW3izlH67Kx82CdeM3+f7KObK0AjF5lW8mOEf0kpeyiCgHkDMkzWToPUg3S6sgzAefI5PpmG9VzBwJ+7R3kCIumO0VklaVeuD2GwKwfEMlDbEjF7P95AgCaPoqNLG0dxTl4ee10z9OBFdtv4QVFSL2vpcWK");
        let response = service.get_report(encrypted_quote, &get_api_key()).unwrap();
        let quote = response.get_quote().unwrap();
        let address = "fdb14b52d7f567e65be4dccc61f9e5f400e8dda0".from_hex().unwrap();
        assert_eq!(&quote.report_body.report_data[..20], &address[..]);
        assert!(response.result.verify_report().unwrap());
    }
}
