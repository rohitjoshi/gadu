/************************************************

   File Name: gadu:config
   Author: Rohit Joshi <rohit.c.joshi@gmail.com>
   Date: 2019-02-17:15:15
   License: Apache 2.0

**************************************************/

use std::fs::File;
use std::io::prelude::*;
use std::str;

#[cfg(feature = "tls")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SSLConfig {
    /// SSL Protocol
    //pub ssl_protocol : Option<>,
    /// Certificate File
    pub certificate_file: Option<PathBuf>,
    /// Private Key File
    pub private_key_file: Option<PathBuf>,
    /// CA File
    pub ca_file: Option<PathBuf>,
    /// Verify certificate
    pub verify: Option<bool>,
    /// Verify depth
    pub verify_depth: Option<u32>,
}
#[cfg(feature = "tls")]
impl Default for SSLConfig {
    fn default() -> SSLConfig {
        SSLConfig {
            certificate_file: None,
            private_key_file: None,
            ca_file: None,
            verify: None,
            verify_depth: None,
        }
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GaduConfig {
    /// The server to connect to.
    pub url: String,
    /// Read timeout.
    pub read_timeout: usize,
    /// Write timeout.
    pub write_timeout: usize,
    /// keep alive time
    pub keep_alive_time: u64,
    #[cfg(feature = "tls")]
    pub ssl_config: SSLConfig,
}

impl Default for GaduConfig {
    fn default() -> GaduConfig {
        GaduConfig {
            url: "tcp://127.0.0.1:10000".to_string(),
            read_timeout: 60_000,
            write_timeout: 60_000,
            keep_alive_time: 60_000,
            #[cfg(feature = "tls")]
            ssl_config: SSLConfig::default(),
        }
    }
}

impl GaduConfig {
    pub fn to_string(&self) -> String {
        serde_json::to_string(&self).unwrap()
    }
    pub fn from_file(file: &str) -> Result<GaduConfig, String> {
        println!("Config file: {}", file);
        println!("Opening config file: {}", file);
        let mut f = match File::open(file) {
            Err(e) => {
                println!("Failed to open file. Error:{:?}", e);
                return Err(e.to_string());
            }
            Ok(f) => f,
        };
        let mut buffer = String::new();
        if let Err(e) = f.read_to_string(&mut buffer) {
            println!("Failed to read file. Error:{:?}", e);
            return Err(e.to_string());
        }
        println!("Config file Str: {}", buffer.as_str());
        //Ok(serde_json::from_str(&buffer).unwrap())
        let conf: GaduConfig = match serde_json::from_str(&buffer) {
            Result::Err(e) => {
                println!("Failed to convert to Config object. Error:{:?}", e);
                return Err(e.to_string());
            }
            Result::Ok(conf) => conf,
        };
        Ok(conf)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkServerConfig {
    pub server_name: String,
    pub server_id: usize,
    pub num_threads: usize,
    pub server_config: GaduConfig,
}

impl Default for NetworkServerConfig {
    fn default() -> NetworkServerConfig {
        NetworkServerConfig {
            server_name: "".to_string(),
            server_id: 0,
            num_threads: 4,
            server_config: GaduConfig::default(),
        }
    }
}
