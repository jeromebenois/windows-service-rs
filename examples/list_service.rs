#[cfg(windows)]
extern crate windows_service;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate flate2;

use std::io::prelude::*;
use flate2::Compression;
use flate2::write::ZlibEncoder;


use windows_service::ErrorKind;
use flate2::write::GzEncoder;

#[cfg(windows)]
fn main() -> windows_service::Result<()> {
    use std::ffi::OsString;
    use windows_service::service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType,
    };
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};


    let list = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::ENUMERATE_SERVICE)
        .and_then(|service_manager| {
            service_manager.list_services()
        }).map_err(|err| ErrorKind::InvalidAccountName)?;

    println!("{}", serde_json::to_string(&list).unwrap());
    println!("size : {}", serde_json::to_string(&list).unwrap().len());

    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(serde_json::to_string(&list).unwrap().as_bytes()).unwrap();
    println!("GzEncoder : {:?}", e.finish().unwrap().len());

    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(serde_json::to_string(&list).unwrap().as_bytes());
    let compressed_bytes = e.finish();
    println!("ZlibEncoder : {:?}", compressed_bytes.unwrap().len());

    Ok(())
}

#[cfg(not(windows))]
fn main() {
    panic!("This program is only intended to run on Windows.");
}
