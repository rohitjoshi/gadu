use mio::*;
use std::collections::HashMap;
//use std::io::{Read, Write};
use std::io::Error;
use std::time::Duration;
use std::io;
use mio::*;
use mio::net::TcpListener;
use mio_uds::UnixListener;
use url::Url;

use crate::conn::{Conn,NetStream, NetAddr};


pub enum NetListener {
    UnsecuredTcpListener(TcpListener),
    #[cfg(feature = "ssl")]
    SslTcpListener(TcpListener),

    /// Unix domain socket stream
    UdsListener(UnixListener),

}

impl NetListener {
    pub fn accept_connection(&self) -> Result<(NetStream,NetAddr), Error>{


         match self {
            &NetListener::UnsecuredTcpListener(ref listener) => {
                let conn_res = listener.accept();
                match conn_res {
                    Ok(s) => {
                        info!("New peer connection received from: {:?}", s.1);
                        s.0.set_keepalive(Some(Server::KEEP_ALIVE_TIME)).unwrap();
                        Ok((NetStream::UnsecuredTcpStream(s.0), NetAddr::NetSocketAddress(s.1)))
                    },
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock =>
                        {
                             Err(Error::new(
                                io::ErrorKind::WouldBlock,
                                "Failed to accept new connection",
                            ))
                        },
                    Err(e) => panic!("encountered IO error: {}", e),
                }
            }
            #[cfg(feature = "ssl")]
            &NetListener::SslTcpListener(ref listener) => {
                let conn_res = listener.accept();
                match conn_res {
                    Ok(s) => {
                        info!("New peer connection received from: {:?}", s.1);
                        s.0.set_keepalive(Some(Server::KEEP_ALIVE_TIME)).unwrap();
                        Ok((NetStream::UnsecuredTcpStream(s.0), NetAddr::NetSocketAddress(s.1)))
                    },
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock =>
                        {
                             Err(Error::new(
                                io::ErrorKind::WouldBlock,
                                "Failed to accept new connection",
                            ))
                        },
                    Err(e) => panic!("encountered IO error: {}", e),
                }
            },
            &NetListener::UdsListener(ref listener) => {
                let conn_res = listener.accept();
                match conn_res {
                    Ok(s) => {
                        let (sock, addr) = s.unwrap();
                        //let addr = s.0.peer_addr().unwrap();
                        info!("New peer connection received from: {:?}", addr);

                        Ok((NetStream::UdsStream(sock), NetAddr::UdsSocketAddress(addr)))
                    },
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock =>
                        {
                             Err(Error::new(
                                io::ErrorKind::WouldBlock,
                                "Failed to accept new connection",
                            ))
                        },
                    Err(e) => panic!("encountered IO error: {}", e),
                }
            }
        }

    }
}
pub struct Server {
    pub id : Token,
    pub url: Url, //unix:/server/sock  or tcp://host:port, ssl://host:port
    pub server: NetListener,
}

impl Server {

    pub const KEEP_ALIVE_TIME: Duration = Duration::from_secs(600); //10 min

    pub fn init(token_id: usize, url_addr: &str) -> Result<Server, String> {

        let url = match Url::parse(url_addr) {
            Ok(url) => url,
            Err(e) => {
                return Err(e.to_string());
            }
        };
        let net_server = match url.scheme() {
            "tcp" => {
                if !url.has_host() {
                    return Err("Invalid Url.  It must have host defined. e.g. tcp://host:port".to_owned());
                }
                if !url.port().is_none() {
                    return Err("Invalid Url.  It must have port defined. e.g. tcp://host:port".to_owned());
                }
                let addr = format!("{}:{}", url.host_str().unwrap(), url.port().unwrap());

                debug!("Binding Server at {}", url_addr);

                let server = match TcpListener::bind(&addr.parse().unwrap()) {
                    Ok(sock) => sock,
                    Err(e) => {
                        error!("EventHandler: Couldn't bind at {}. Error: {:?}", addr, e);
                        return Err(e.to_string());
                    }
                };
                info!("Tcp Server started on {}", addr);
                NetListener::UnsecuredTcpListener(server)
            },
            "unix" => {
                let path= url.path();
                debug!("Binding Server at {}", path);
                let server = match UnixListener::bind(&path) {
                    Ok(sock) => sock,
                    Err(e) => {
                        error!(
                            "EventHandler: Couldn't bind at {}. Error: {:?}",
                            path, e
                        );
                        return Err(e.to_string());
                    }
                };
                info!("Unix Server started on {}", path);
                NetListener::UdsListener(server)
            },
            _ => {
                return Err("Unsupported scheme. Valid schemes are unix and tcp".to_owned());
            }
        };


        Ok(Server {
            id: Token(token_id),
            url,
            server: net_server,
        })

    }
    pub fn accept_connection(&self) -> Result<Conn, Error> {
        let (net_stream, net_addr) = self.server.accept_connection()?;
        Ok(Conn {
            url: self.url.clone(),
            stream: net_stream,
            addr:net_addr,
            close: false,
            reg_write: false,
            input: Vec::new(),
            output: Vec::new(),
            tags: HashMap::with_capacity(2),
        })
    }
}
