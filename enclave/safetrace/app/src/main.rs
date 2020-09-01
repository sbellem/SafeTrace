// Licensed to the Apache Software Foundation (ASF) under one
// or more contributor license agreements.  See the NOTICE file
// distributed with this work for additional information
// regarding copyright ownership.  The ASF licenses this file
// to you under the Apache License, Version 2.0 (the
// "License"); you may not use this file except in compliance
// with the License.  You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing,
// software distributed under the License is distributed on an
// "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.  See the License for the
// specific language governing permissions and limitations
// under the License..


extern crate sgx_types;
extern crate sgx_urts;

pub extern crate futures;
extern crate tokio_zmq;
extern crate zmq;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate serde;
extern crate serde_repr;
pub extern crate serde_json;
extern crate rmp_serde;
extern crate rustc_hex as hex;
#[macro_use]
pub extern crate log;
#[macro_use]
extern crate log_derive;

use std::env;
use sgx_types::*;
use sgx_urts::SgxEnclave;

extern crate enigma_types;
pub extern crate enigma_tools_u;
extern crate enigma_tools_m;
extern crate enigma_crypto;

extern crate base64;
extern crate openssl;
extern crate reqwest;

pub mod attestation;
pub mod common_u;
pub mod keys_u;
pub mod networking;
pub mod ocalls_u;
pub mod esgx;

use futures::Future;
use networking::{ipc_listener, IpcListener};

static ENCLAVE_FILE: &'static str = "enclave.signed.so";


fn init_enclave() -> SgxResult<SgxEnclave> {
    let mut launch_token: sgx_launch_token_t = [0; 1024];
    let mut launch_token_updated: i32 = 0;
    // call sgx_create_enclave to initialize an enclave instance
    // Debug Support: set 2nd parameter to 1
    let debug = 1;
    let mut misc_attr = sgx_misc_attribute_t {secs_attr: sgx_attributes_t { flags:0, xfrm:0}, misc_select:0};
    SgxEnclave::create(ENCLAVE_FILE,
                       debug,
                       &mut launch_token,
                       &mut launch_token_updated,
                       &mut misc_attr)
}

fn main() {
    let enclave= match init_enclave() {
        Ok(r) => {
            println!("[+] Init Enclave Successfully {}!", r.geteid());
            r
        },
        Err(x) => {
            println!("[-] Init Enclave Failed {}!", x.as_str());
            return;
        },
    };

    let server = IpcListener::new(&format!("tcp://*:5552"));

    // NOTE get env var for SPID and Primary Key
    let spid = env::var("IAS_SGX_SPID")
        .expect("Environement variable 'IAS_SGX_SPID' is not set! Set it with export IAS_SGX_SPID=...");
    let api_key = env::var("IAS_SGX_PRIMARY_KEY")
        .expect("Environement variable 'IAS_SGX_PRIMARY_KEY' is not set! Set it with export IAS_SGX_PRIMARY_KEY=...");

    server
        .run(move |multi| ipc_listener::handle_message(multi, &spid, &api_key, enclave.geteid(), 1))

        //.run(move |multi| ipc_listener::handle_message(multi, &opt.spid, eid, opt.retries))
        // .run(|mul| {
        //     println!("{:?}", mul);
        //     mul
        // })
        .wait()
        .unwrap();

    // enclave.destroy();
}
