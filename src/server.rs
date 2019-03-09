/************************************************

   File Name: gadu:server
   Author: Rohit Joshi <rohit.c.joshi@gmail.com>
   Date: 2019-02-17:15:15
   License: Apache 2.0

**************************************************/
#[cfg(feature = "tls")]
use openssl::ssl::{
    SslAcceptor, SslConnectorBuilder, SslFiletype, SslMethod, SslStream, SslVerifyMode,
};

#[cfg(feature = "tls")]
use openssl::error::ErrorStack;
#[cfg(feature = "tls")]
use openssl::x509;

//use std::io::{Read, Write};
use std::io::Error;
use std::time::Duration;

use crate::config::GaduConfig;
use crate::conn::{Conn, NetAddr, NetStream};
use mio::net::TcpListener;
use mio::*;
use mio_uds::UnixListener;
use url::Url;

pub enum NetListener {
    UnsecuredTcpListener(TcpListener),
    #[cfg(feature = "tls")]
    SslTcpListener(TcpListener),

    /// Unix domain socket stream
    UdsListener(UnixListener),
}

impl NetListener {
    pub fn accept_tcp_connection(
        listener: &TcpListener,
        config: &GaduConfig,
    ) -> Result<(NetStream, NetAddr), Error> {
        let s = listener.accept()?;
        debug!("New peer connection received from: {:?}", s.1);
        if let Err(e) =
            s.0.set_keepalive(Some(Duration::from_millis(config.keep_alive_time)))
        {
            error!("Failed to set keepalive. Error:{:?}", e);
        }
        Ok((
            NetStream::UnsecuredTcpStream(s.0),
            NetAddr::NetSocketAddress(s.1),
        ))
        /*
        match conn_res {
            Ok(s) => {
                debug!("New peer connection received from: {:?}", s.1);
                s.0.set_keepalive(Some(Duration::from_millis(config.keep_alive_time)));
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
        }*/
    }

    pub fn accept_uds_connection(listener: &UnixListener) -> Result<(NetStream, NetAddr), Error> {
        let s = listener.accept()?;
        let (sock, addr) = s.unwrap();
        //let addr = s.0.peer_addr().unwrap();
        debug!("New peer connection received from: {:?}", addr);

        Ok((NetStream::UdsStream(sock), NetAddr::UdsSocketAddress(addr)))
        /*match conn_res {
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
                        format!("An UDS error occurred.({:?}",e)
                        )
                    )

                },
            Err(e) => panic!("encountered IO error: {}", e),
        }*/
    }
    #[cfg(feature = "tls")]
    pub fn accept_ssl_connection(
        listener: &TcpListener,
        config: &GaduConfig,
        acceptor: Arc<SslAcceptor>,
    ) -> Result<(NetStream, NetAddr), Error> {
        let s = listener.accept()?;
        debug!("New peer connection received from: {:?}", s.1);
        s.0.set_keepalive(Some(Duration::from_millis(config.keep_alive_time)));
        let stream = match acceptor.accept(s.0) {
            Ok(s) => s,
            Err(e) => {
                return Err(Error::new(
                    io::ErrorKind::Other,
                    format!("An SSL error occurred.({:?}", e),
                ));
            }
        };

        Ok((
            NetStream::SslTcpStream(stream),
            NetAddr::NetSocketAddress(s.1),
        ))
        /*
        match listener.accept() {
            Ok(s) => {
                info!("New peer connection received from: {:?}", s.1);
                s.0.set_keepalive(Some(Duration::from_millis(config.keep_alive_time)));
                let stream = match acceptor.accept(s.0) {
                    Ok(s) => s,
                    Err(e) => {
                        return Err(
                            Error::new(
                                io::ErrorKind::Other,
                                format!("An SSL error occurred.({:?}", e)
                            )
                        );
                    }
                };

                Ok((NetStream::SslTcpStream(stream), NetAddr::NetSocketAddress(s.1)))
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock =>
                {
                    Err(Error::new(
                        io::ErrorKind::WouldBlock,
                        "Failed to accept new connection",
                    ))
                },
            Err(e) => panic!("encountered IO error: {}", e),
        }*/
    }
    /*
    pub fn accept_connection(&self, config : &GaduConfig, acceptor: Arc<SslAcceptor>) -> Result<(NetStream,NetAddr), Error>{


         match self {
            &NetListener::UnsecuredTcpListener(ref listener) => {
                NetListener::accept_tcp_connection(&listener, &config)
            }
            #[cfg(feature = "tls")]
            &NetListener::SslTcpListener(ref listener) => {
                NetListener::accept_ssl_connection(&listener, &config, acceptor)
            },
            &NetListener::UdsListener(ref listener) => {
                NetListener::accept_uds_connection(&listener, &config)
            }
        }

    }*/
}
pub struct Server {
    pub id: Token,
    pub config: GaduConfig,
    pub url: Url,
    pub server: NetListener,
    #[cfg(feature = "tls")]
    acceptor: Arc<SslAcceptor>,
}

impl Server {
    pub fn init(token_id: usize, config: &GaduConfig) -> Result<Server, String> {
        #[cfg(feature = "tls")]
        info!("SSL Server initializing..");
        let url = match Url::parse(&config.url) {
            Ok(url) => url,
            Err(e) => {
                return Err(e.to_string());
            }
        };
        let net_server = match url.scheme() {
            #[cfg(not(feature = "ssl"))]
            "tcp" => {
                if !url.has_host() {
                    return Err(
                        "Invalid Url.  It must have host defined. e.g. tcp://host:port".to_owned(),
                    );
                }
                if url.port().is_none() {
                    return Err(
                        "Invalid Url.  It must have port defined. e.g. tcp://host:port".to_owned(),
                    );
                }
                let addr = format!("{}:{}", url.host_str().unwrap(), url.port().unwrap());

                debug!("Binding Server at {}", &config.url);

                let server = match TcpListener::bind(&addr.parse().unwrap()) {
                    Ok(sock) => sock,
                    Err(e) => {
                        error!("EventHandler: Couldn't bind at {}. Error: {:?}", addr, e);
                        return Err(e.to_string());
                    }
                };

                info!("Tcp Server started on {}", addr);
                NetListener::UnsecuredTcpListener(server)
            }
            #[cfg(feature = "tls")]
            "tcp" | "ssl" | "tls" => {
                info!("SSL enabled");
                if !url.has_host() {
                    return Err(
                        "Invalid Url.  It must have host defined. e.g. ssl://host:port".to_owned(),
                    );
                }
                if url.port().is_none() {
                    return Err(
                        "Invalid Url.  It must have port defined. e.g. ssl://host:port".to_owned(),
                    );
                }
                let addr = format!("{}:{}", url.host_str().unwrap(), url.port().unwrap());

                debug!("Binding Server at {}", &config.url);

                let server = match TcpListener::bind(&addr.parse().unwrap()) {
                    Ok(sock) => sock,
                    Err(e) => {
                        error!("EventHandler: Couldn't bind at {}. Error: {:?}", addr, e);
                        return Err(e.to_string());
                    }
                };

                info!("Secure Server started on {}", addr);

                NetListener::SslTcpListener(server)
            }
            "unix" => {
                let path = url.path();
                debug!("Binding Server at {}", path);
                let server = match UnixListener::bind(&path) {
                    Ok(sock) => sock,
                    Err(e) => {
                        error!("EventHandler: Couldn't bind at {}. Error: {:?}", path, e);
                        return Err(e.to_string());
                    }
                };
                info!("Unix Server started on {}", path);
                NetListener::UdsListener(server)
            }
            _ => {
                return Err("Unsupported scheme. Valid schemes are unix and tcp".to_owned());
            }
        };
        
        

        Ok(Server {
            id: Token(token_id),
            config: config.clone(),
            url,
            server: net_server,
            #[cfg(feature = "tls")]
            acceptor: Arc::new(Server::init_ssl_acceptor(&config)?),
        })
    }
    pub fn accept_connection(&self) -> Result<Conn, Error> {
        let (net_stream, net_addr) = match self.server {
            NetListener::UnsecuredTcpListener(ref listener) => {
                NetListener::accept_tcp_connection(&listener, &self.config)?
            }
            #[cfg(feature = "tls")]
            NetListener::SslTcpListener(ref listener) => {
                NetListener::accept_ssl_connection(&listener, &self.config, self.acceptor.clone())?
            }
            NetListener::UdsListener(ref listener) => {
                NetListener::accept_uds_connection(&listener)?
            }
        };

        Ok(Conn::new(net_stream, net_addr))
    }
    #[cfg(feature = "tls")]
    fn init_ssl_acceptor(config: &GaduConfig) -> Result<SslAcceptor, String> {
        match Server::build_acceptor(&config) {
            Ok(acceptor) => Ok(acceptor),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(feature = "tls")]
    fn build_acceptor(config: &GaduConfig) -> Result<SslAcceptor, ErrorStack> {
        let mut ctx = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        {
            ctx.set_default_verify_paths()?;

            // verify peer
            if config.ssl_config.verify.unwrap_or(false) {
                ctx.set_verify(SslVerifyMode::PEER);
            } else {
                ctx.set_verify(SslVerifyMode::NONE);
            }
            // verify depth
            if config.ssl_config.verify_depth.unwrap_or(0) > 0 {
                ctx.set_verify_depth(config.ssl_config.verify_depth.unwrap());
            }
            if config.ssl_config.certificate_file.is_some() {
                ctx.set_certificate_file(
                    config.ssl_config.certificate_file.as_ref().unwrap(),
                    SslFiletype::PEM,
                )?;
            }
            if config.ssl_config.private_key_file.is_some() {
                ctx.set_private_key_file(
                    config.ssl_config.private_key_file.as_ref().unwrap(),
                    SslFiletype::PEM,
                )?;
            }
            if config.ssl_config.ca_file.is_some() {
                let _ = ctx.set_ca_file(config.ssl_config.ca_file.as_ref().unwrap())?;
            }
        }
        Ok(ctx.build())
    }
}
