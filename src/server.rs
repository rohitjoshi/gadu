/************************************************

   File Name: gadu:server
   Author: Rohit Joshi <rohit.c.joshi@gmail.com>
   Date: 2019-02-17:15:15
   License: Apache 2.0

**************************************************/

use mio::*;
use mio::net::TcpListener;

use mio_uds::UnixListener;
//#[cfg(feature = "tls")]
use openssl::error::ErrorStack;
//#[cfg(feature = "tls")]
use openssl::ssl::{
    HandshakeError, SslAcceptor, SslFiletype, SslMethod, SslMode, SslVerifyMode, SslRef
};
//#[cfg(feature = "tls")]


//use std::io::{Read, Write};
use std::io::{Error, ErrorKind};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use url::Url;

use crate::config::{GaduConfig, SSLConfig};
use crate::conn::{Conn, NetAddr, NetStream};


pub enum NetListener {
    UnsecuredTcpListener(TcpListener),
    //#[cfg(feature = "tls")]
    SslTcpListener(TcpListener),

    /// Unix domain socket stream
    UdsListener(UnixListener),

}

pub struct Server {
    pub id: Token,
    pub config: GaduConfig,
    pub url: Url,
    pub mail_poll: Poll,
    poll_timeout: Duration,
    pub server: NetListener,
    acceptor: Option<Arc<SslAcceptor>>,
    pub next_token_id: AtomicUsize,
}

impl Server {
    pub fn init(token_id: usize, config: &GaduConfig) -> Result<Server, String> {
        let res = Poll::new();
        if let Err(e) = res {
            return Err(e.to_string());
        }
        let poll = res.unwrap();

        //#[cfg(feature = "tls")]
        info!("Server initializing..");
        let url = match Url::parse(&config.url) {
            Ok(url) => url,
            Err(e) => {
                return Err(e.to_string());
            }
        };

        let net_server = match url.scheme() {
            // #[cfg(not(feature = "ssl"))]
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


                match config.ssl_config.enabled {
                    true => {
                        info!("Secure Server started on {}", addr);
                        poll.register(&server, Token(token_id), Ready::readable(), PollOpt::empty()).unwrap();
                        NetListener::SslTcpListener(server)
                    }
                    _ => {
                        info!("Tcp Server started on {}", addr);
                        poll.register(&server, Token(token_id), Ready::readable(), PollOpt::empty()).unwrap();
                        NetListener::UnsecuredTcpListener(server)
                    }
                }
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
                poll.register(&server, Token(token_id), Ready::readable(), PollOpt::empty()).unwrap();
                NetListener::UdsListener(server)
            }
            _ => {
                return Err("Unsupported scheme. Valid schemes are unix and tcp".to_owned());
            }
        };


        let acceptor = if config.ssl_config.enabled { Some(Arc::new(Server::init_ssl_acceptor(&config)?)) } else { None };


        Ok(Server {
            id: Token(token_id),
            config: config.clone(),
            url,
            mail_poll: poll,
            poll_timeout: Duration::from_millis(250),
            server: net_server,
            //#[cfg(feature = "tls")]
            acceptor,
            next_token_id: AtomicUsize::new(0),
        })
    }

    pub fn accept_connection(&self) -> Result<Vec<Conn>, Error> {
        let mut events = Events::with_capacity(1024);
        let total_events = self.mail_poll.poll(&mut events, Some(self.poll_timeout))?;

        let mut conns = Vec::with_capacity(10);

        if total_events == 0 {
            return Ok(conns);
        }

        debug!("Main Poll: Total events received:{}", total_events);


        for event in &events {
            debug!("event readiness:{:?}", event.readiness());
            if self.id == event.token() {
                let (net_stream, net_addr) = match self.server {
                    NetListener::UnsecuredTcpListener(ref listener) => {
                        self.accept_tcp_connection(&listener)?
                    }
                    // #[cfg(feature = "tls")]
                    NetListener::SslTcpListener(ref listener) => {
                        self.accept_ssl_connection(&listener)?
                    }
                    NetListener::UdsListener(ref listener) => {
                        self.accept_uds_connection(&listener)?
                    }
                };
                let mut conn = Conn::new(net_stream, net_addr);
                if self.config.ssl_config.enabled {
                    conn.server_ssl_config = Some(self.config.ssl_config.clone());
                }
                conns.push(conn);
            }
//            }else {
//                // these are uncompleted SSL handshake or others...
//                let entry_res = server.pending_streams.lock().remove(&event.token().0);
//                if entry_res.is_none() {
//                    continue;
//                }
//                let (ssl_net_stream, ssl_net_addr) = entry_res.unwrap();
//                match ssl_net_stream {
//                    NetStream::SslMidHandshakeStream(stream) => {
//                        server.mail_poll.deregister(stream.get_ref());
//                        let (net_stream, net_addr) = Server::ssl_handshake(server.clone(),stream,  ssl_net_addr)?;
//                        conns.push(Conn::new(net_stream, net_addr));
//
//                    }
//                    _ => {
//                        error!("Not supported pending stream");
//                        continue;
//                    }
//                }
//            }
            //} //match
        } //for events
        Ok(conns)
    }
    //#[cfg(feature = "tls")]
    fn init_ssl_acceptor(config: &GaduConfig) -> Result<SslAcceptor, String> {
        match Server::build_acceptor(&config) {
            Ok(acceptor) => Ok(acceptor),
            Err(e) => {
                error!("Failed to build SSL acceptor");
                Err(e.to_string())
            }
        }
    }
    //#[cfg(feature = "tls")]
    fn build_acceptor(config: &GaduConfig) -> Result<SslAcceptor, ErrorStack> {
        let mut ctx = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        {
            ctx.set_default_verify_paths()?;

            //ctx.clear_options(SslOptions::NO_TLSV1_3);

            //ctx.set_min_proto_version(Some(SslVersion::TLS1_2))?;

            ctx.set_mode(SslMode::AUTO_RETRY);

            // verify peer
            if config.ssl_config.verify.unwrap_or(false) {
                debug!("Set Verify : PEER");
                //ctx.set_verify(SslVerifyMode::PEER); //must be cert signed by CA
                ctx.set_verify_callback(openssl::ssl::SslVerifyMode::PEER, |_, _| true); //support self signed
            } else {
                debug!("Set Verify : NONE");
                ctx.set_verify(SslVerifyMode::NONE);
            }
            // verify depth
            if config.ssl_config.verify_depth.unwrap_or(0) > 0 {
                ctx.set_verify_depth(config.ssl_config.verify_depth.unwrap());
            }
            if config.ssl_config.certificate_file.is_some() {
                ctx.set_certificate_chain_file(
                    config.ssl_config.certificate_file.as_ref().unwrap()
                    // SslFiletype::PEM,
                )?;
                info!("Setting SSL certificate file: {:?}", config.ssl_config.certificate_file.as_ref().unwrap());
            }
            if config.ssl_config.private_key_file.is_some() {
                ctx.set_private_key_file(
                    config.ssl_config.private_key_file.as_ref().unwrap(),
                    SslFiletype::PEM,
                )?;
                info!("Setting SSL private key file: {:?}", config.ssl_config.private_key_file.as_ref().unwrap());
                ctx.check_private_key()?;
                info!("Checking private key file successful");
            }
            if config.ssl_config.ca_file.is_some() {
                ctx.set_ca_file(config.ssl_config.ca_file.as_ref().unwrap())?;
                info!("Setting SSL CA cert file: {:?}", config.ssl_config.ca_file.as_ref().unwrap());
            }
        }
        Ok(ctx.build())
    }
    pub fn accept_tcp_connection(&self,
                                 listener: &TcpListener,
    ) -> Result<(NetStream, NetAddr), Error> {
        let s = listener.accept()?;

        debug!("TCP:New peer connection received from: {:?}", s.1);
        //if let Err(e) = s.0.set_nonblocking(true) {
        //    error!("Failed to set nonblocking to true. Error:{:?}", e);
        //
        //}
        if let Err(e) = s.0.set_nodelay(true) {
            error!("Failed to set nodelay to true. Error:{:?}", e);
        }
        if let Err(e) =
        s.0.set_keepalive(Some(Duration::from_millis(self.config.keep_alive_time)))
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

    pub fn accept_uds_connection(&self, listener: &UnixListener) -> Result<(NetStream, NetAddr), Error> {
        debug!("accept_uds_connection()");
        let accept_results = listener.accept()?;

        if accept_results.is_none() {
            //error!("Failed to get uds connection");
            return Err(Error::new(ErrorKind::Other, "none retuned"));
        }

        let (stream, addr) = accept_results.unwrap();

        //let stream = mio_uds::UnixListener::from_listener(stream)?;

        debug!("UDS: New peer connection received from: {:?}", addr);
        //sock.set_nonblocking(true);

        Ok((NetStream::UdsStream(stream), NetAddr::UdsSocketAddress(addr)))
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


    pub fn validate_ssl_connection(ssl_config: &SSLConfig, ssl: &SslRef) -> Result<(),String> {
        debug!("validate_ssl_connection for CN");
        // if verify is falls, return
        /*if !ssl_config.verify.unwrap_or(false) {
            return Ok(());
        }*/

        fn get_friendly_name(peer: &openssl::x509::X509) -> String {
            peer.subject_name() // can't figure out how to get the real friendly name
                .entries_by_nid(openssl::nid::Nid::COMMONNAME)
                //.last()
                .map(|it| {
                    format!("{}={}", it.object().nid().short_name().unwrap_or(&"".to_string()), it.data()
                        .as_utf8()
                        .and_then(|s| Ok(s.to_string()))
                        .unwrap_or("".to_string()))
                }).collect()
                //.unwrap_or("".to_string())
        }

        match ssl.peer_certificate() {
            None => {
                return Err("ERR No certificate was provided\r\n".to_string());
            },
            Some(peer) => {
                let client_cn = get_friendly_name(&peer);
                if client_cn.is_empty() {
                    return Err("ERR No certificate was provided\r\n".to_string());
                }
                debug!("Connection with SSL CN :{} received", client_cn);
                // if CN not populated, return success
                if ssl_config.valid_cns.is_none() {
                    debug!("No list of CN defined");
                    return Ok(());
                }else if ssl_config.valid_cns.as_ref().unwrap().contains(&client_cn) {
                    info!("Client with SSL CN: {} is authenticated successfully", client_cn);
                } else {
                    warn!("Client with SSL CN: {} authentication failed.", client_cn);
                    return Err(format!("ERR Client SSL Authentication failed for CN: {} \r\n", client_cn));
                }

            }
        }
        Ok(())
    }


    // #[cfg(feature = "tls")]
    pub fn accept_ssl_connection(&self,
                                 listener: &TcpListener,
    ) -> Result<(NetStream, NetAddr), Error> {
//         if acceptor.is_none() {
//             return Err(Error::new(
//                 ErrorKind::Other,
//                 "An SSL error occurred. SslAcceptor not initialized".to_string(),
//             ));
//         }
        let (sock, addr) = listener.accept_std()?;
        debug!("SSL: New peer connection received from: {:?}", addr);
        let sock = mio::net::TcpStream::from_stream(sock)?;

        if let Err(e) = sock.set_nodelay(true) {
            error!("Failed to set nodelay to true for addr: {:?}. Error:{:?}", addr, e);
        }
        if let Err(e) = sock.set_keepalive(Some(Duration::from_millis(self.config.keep_alive_time))) {
            error!("Failed to set keepalive for addr: {:?}. Error:{:?}", addr, e);
        }

        match self.acceptor.as_ref().unwrap().accept(sock) {
            Ok(s) => {
                debug!("SSL Handshake successful. Validating connection..");
                if let Err(e) = Server::validate_ssl_connection(&self.config.ssl_config, s.ssl()) {
                    error!("Failed to accept SSL connection. Error:{:?}", e);
                    return Err(Error::new(
                        ErrorKind::Other,
                        format!("An SSL error occurred.{}", e.to_string()),
                    ));
                };
                Ok((
                    NetStream::SslTcpStream(s),
                    NetAddr::NetSocketAddress(addr)
                ))
            }
            Err(e) => {
                debug!("{:?}", e);
                let err_str = e.to_string();
                match e {
                    HandshakeError::WouldBlock(s) => {
                        debug!("Failed to handshake on SSL connection. Received error: HandshakeError::WouldBlock");
//                        if let Err(e) = self.validate_ssl_connection(s.ssl()) {
//                            error!("Failed to accept SSL connection. Error:{:?}", e);
//                            return Err(Error::new(
//                                ErrorKind::Other,
//                                format!("An SSL error occurred.{}", e.to_string()),
//                            ));
//                        };
                        Ok((
                            NetStream::SslMidHandshakeStream(s),
                            NetAddr::NetSocketAddress(addr)
                        ))
//                        server.mail_poll.register(s.get_ref(), Token(server.next_token_id.load(Ordering::Relaxed)), Ready::readable() , PollOpt::empty()  ).unwrap();
//                        server.pending_streams.lock().insert(server.next_token_id.load(Ordering::Relaxed), (NetStream::SslMidHandshakeStream(s), NetAddr::NetSocketAddress(addr)));
//                        server.next_token_id.fetch_add(1, Ordering::Relaxed);
//
//                        return Err(Error::new(
//                            ErrorKind::Other,
//                            format!("An SSL error occurred.{}", err_str),
//                        ));
                    }
                    _ => {
                        error!("Failed to accept SSL connection. Error:{}", err_str);
                        Err(Error::new(
                            ErrorKind::Other,
                            format!("An SSL error occurred.{}", err_str),
                        ))
                    }
                }
            }
        }
    }
}
