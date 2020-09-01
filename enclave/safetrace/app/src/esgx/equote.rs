use common_u::errors;
use failure::Error;
use sgx_types::*;
use std::str;
use crate::ocalls_u::ecall_get_signing_address;
// this struct is returned during the process registration back to the surface.
// quote: the base64 encoded quote
// address : the clear text public key for ecdsa signing and registration
#[derive(Serialize, Deserialize, Debug)]
pub struct GetRegisterResult {
    pub errored: bool,
    pub quote: String,
    pub address: String,
}

// wrapper function for getting the enclave public sign key (the one attached with produce_quote())
//#[logfn(TRACE)]
pub fn get_register_signing_address(eid: sgx_enclave_id_t) -> Result<[u8; 20], Error> {
    let mut address = [0u8; 20];
    let status = unsafe { ecall_get_signing_address(eid, &mut address) };
    if status == sgx_status_t::SGX_SUCCESS {
        Ok(address)
    } else {
        Err(errors::GetRegisterKeyErr { status, message: String::from("error in get_register_signing_key") }.into())
    }
}


#[cfg(test)]
mod test {
    use crate::esgx::general::init_enclave_wrapper;
    use crate::attestation::{self, service::*};
    use enigma_tools_u::esgx::equote::retry_quote;
    use std::env;

    fn get_spid() -> String {
        let spid = env::var("IAS_SGX_SPID")
            .expect("Environement variable 'IAS_SGX_SPID' is not set! \
                Set it with export IAS_SGX_SPID=...");
        spid
    }

    fn get_api_key() -> String {
        let api_key: String = env::var("IAS_SGX_PRIMARY_KEY")
            .expect("Environement variable 'IAS_SGX_PRIMARY_KEY' is not set! \
                Set it with export IAS_SGX_PRIMARY_KEY=...");
        api_key
    }

    #[test]
    fn test_produce_quote() {
        // initiate the enclave
        let enclave = init_enclave_wrapper().unwrap();
        // produce a quote

        let tested_encoded_quote = match retry_quote(enclave.geteid(), &get_spid(), 18) {
            Ok(encoded_quote) => encoded_quote,
            Err(e) => {
                println!("[-] Produce quote Err {}, {}", e.as_fail(), e.backtrace());
                assert_eq!(0, 1);
                return;
            }
        };
        println!("-------------------------");
        println!("{}", tested_encoded_quote);
        println!("-------------------------");
        enclave.destroy();
        assert!(!tested_encoded_quote.is_empty());
        //assert_eq!(real_encoded_quote, tested_encoded_quote);
    }

    #[test]
    fn test_produce_and_verify_qoute() {
        let enclave = init_enclave_wrapper().unwrap();
        let quote = retry_quote(enclave.geteid(), &get_spid(), 18).unwrap();
        let service = AttestationService::new(attestation::constants::ATTESTATION_SERVICE_URL);
        let as_response = service.get_report(quote, &get_api_key()).unwrap();

        assert!(as_response.result.verify_report().unwrap());
    }

    #[test]
    fn test_signing_key_against_quote() {
        let enclave = init_enclave_wrapper().unwrap();
        let quote = retry_quote(enclave.geteid(), &get_spid(), 18).unwrap();
        let service = AttestationService::new(attestation::constants::ATTESTATION_SERVICE_URL);
        let as_response = service.get_report(quote, &get_api_key()).unwrap();
        assert!(as_response.result.verify_report().unwrap());
        let key = super::get_register_signing_address(enclave.geteid()).unwrap();
        let quote = as_response.get_quote().unwrap();
        assert_eq!(key, &quote.report_body.report_data[..20]);
    }
}
