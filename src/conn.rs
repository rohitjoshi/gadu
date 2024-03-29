/************************************************

   File Name: gadu:conn
   Author: Rohit Joshi <rohit.c.joshi@gmail.com>
   Date: 2019-02-17:15:15
   License: Apache 2.0

**************************************************/
use hashbrown::HashMap;
use mio::net::TcpStream;
use mio_uds::UnixStream;
use openssl::ssl::{HandshakeError, MidHandshakeSslStream};
//#[cfg(feature = "tls")]
use openssl::ssl::SslStream;
use std::io::{Error, ErrorKind};
use std::io::{Read, Write};
use std::io::Result as IoResult;
use std::net::Shutdown;
use std::net::SocketAddr;
use std::os::unix::net::SocketAddr as UnixSocketAddr;
use std::time::Duration;
use url::Url;
use crate::config::SSLConfig;
use crate::server::Server;

pub struct Conn {
    url: String,
    stream: NetStream,
    addr: NetAddr,
    pub input: Vec<u8>,
    pub output: Vec<u8>,
    pub close: bool,
    pub reg_write: bool,
    pub tags: HashMap<String, String>,
    pub server_ssl_config: Option<SSLConfig>
}

impl Conn {
    pub const KEEP_ALIVE_TIME: Duration = Duration::from_secs(600); //10 min

    pub fn new(net_conn: NetStream, net_addr: NetAddr) -> Conn {
        Conn {
            url: "".to_string(),
            stream: net_conn,
            addr: net_addr,
            close: false,
            reg_write: false,
            input: Vec::with_capacity(32768),
            output: Vec::with_capacity(32768),
            tags: HashMap::with_capacity(2),
            server_ssl_config: None
        }
    }


    #[inline]
    pub fn is_remote_connection(&self) -> bool {
        self.url.is_empty()
    }

    #[inline]
    pub fn get_address(&self) -> &NetAddr {
        &self.addr
    }

    #[inline]
    pub fn get_stream(&self) -> &NetStream {
        &self.stream
    }

    #[inline]
    pub fn set_output_buffer(&mut self, output: Vec<u8>) {
        self.output = output;
    }

    #[inline]
    pub fn add_tag(&mut self, tag: &str, tag_val: &str) {
        self.tags.insert(tag.to_string(), tag_val.to_string());
    }

    ///
    /// connect internal function
    ///
    fn connect_internal(addr_url: &str) -> Result<(NetStream, NetAddr), String> {
        let url = match Url::parse(addr_url) {
            Ok(url) => url,
            Err(e) => {
                return Err(e.to_string());
            }
        };

        let (net_conn, net_addr) = match url.scheme() {
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
                let conn_addr: SocketAddr = addr.parse().unwrap();
                debug!("Connecting to Server at {}", addr_url);

                let sock = match TcpStream::connect(&conn_addr) {
                    Ok(sock) => sock,
                    Err(e) => {
                        error!("EventHandler: Couldn't connect at {}. Error: {:?}", addr, e);
                        return Err(e.to_string());
                    }
                };

                //set keep alive
                if let Err(e) = sock.set_keepalive(Some(Conn::KEEP_ALIVE_TIME)) {
                    error!("Faile to set keep alive : {}.", e.to_string());
                }
                debug!("Tcp client connected with server at {}", addr);
                (
                    NetStream::UnsecuredTcpStream(sock),
                    NetAddr::NetSocketAddress(conn_addr),
                )
            }
            "unix" => {
                let path = url.path();
                debug!("Connecting to Server at {}", path);
                let sock = match UnixStream::connect(&path) {
                    Ok(sock) => sock,
                    Err(e) => {
                        error!("EventHandler: Couldn't connect at {}. Error: {:?}", path, e);
                        return Err(e.to_string());
                    }
                };
                debug!("Unix Socket connected with server at {}", path);
                let addr = sock.peer_addr().unwrap();
                (NetStream::UdsStream(sock), NetAddr::UdsSocketAddress(addr))
            }
            _ => {
                return Err("Unsupported scheme. Valid schemes are unix and tcp".to_owned());
            }
        };

        Ok((net_conn, net_addr))
    }
    pub fn connect(addr_url: &str) -> Result<Conn, String> {
        debug!("Connecting to {}", addr_url);

        let (net_conn, net_addr) = Conn::connect_internal(&addr_url)?;

        let mut conn = Conn::new(net_conn, net_addr);
        conn.url = addr_url.to_string();
        Ok(conn)
    }

    pub fn reconnect(&mut self) -> Result<(), String> {
        debug!("Reconnect to server: {}", self.url);

        let (net_conn, net_addr) = Conn::connect_internal(&self.url)?;

        self.close = false;
        self.input.clear();
        self.output.clear();
        self.reg_write = false;
        self.stream = net_conn;
        self.addr = net_addr;
        self.tags.clear();
        Ok(())
    }

    pub fn write(&mut self) -> bool {
        // info!("Write: Thread_id: {:?}", thread::current().id());
        trace!(
            "Write invoked. Sending data: {}",
            String::from_utf8_lossy(&self.output)
        );
        match self.stream.write(self.output.as_slice()) {
            Ok(n) => {
                if n < self.output.len() {
                    // let mut output = Vec::new();
                    // output.extend_from_slice(&self.output[n..self.output.len()]);
                    // self.output = output
                    self.output.drain(0..n);
                } else {
                    self.output.clear();
                }
            }
            Err(ref e) => if e.kind() == ErrorKind::WouldBlock {
                debug!(
                    "Write: ErrorKind::WouldBlock on connection :{}",
                    self.addr.to_string()
                );
            } else if e.kind() == ErrorKind::ConnectionReset {
                info!("Write: Connection reset by peer:{}", self.addr.to_string());
                self.close = true;
            } else {
                error!(
                    "Write: Peer Connection:{}, Write Error: {:?}",
                    self.addr.to_string(),
                    e
                );
                self.close = true;
            },
        }
        self.close // close is false
    }

    pub fn read(&mut self, mut packet: &mut [u8]) -> bool {
        // packet.clear();
        // let mut packet = [0; 4096];
        //let mut buffer = Vec::with_capacity(4096);
        //match (&self.stream).read_to_end(&mut buffer) {
        //info!("Read: Thread_id: {:?}", thread::current().id());

        match self.stream.read(&mut packet) {
            Ok(n) => {
                debug!("Conn bytes read: {}", n);
                if n == 0 {
                    self.close = true;
                    //break;
                } else {
                    self.input.extend_from_slice(&packet[0..n]);
                    //self.input.extend(&buffer);
                    debug!(
                        "Received Length:{}, data: {}. ",
                        n,
                        String::from_utf8_lossy(&self.input)
                    );
                }
            }
            Err(ref e) => if e.kind() == ErrorKind::WouldBlock {
                debug!(
                    "Read: ErrorKind::WouldBlock on connection :{}",
                    self.addr.to_string()
                );
                //break;
            } else if e.kind() == ErrorKind::ConnectionReset {
                info!("Read: Connection reset by peer:{}", self.addr.to_string());
                self.close = true;
                //break;
            } else {
                error!(
                    "Read: Peer Connection:{}, Read Error: {:?}",
                    self.addr.to_string(),
                    e
                );
                self.close = true;
                //break;
            }, /* Err(_) => {
                  error!("Peer Connection:{}, Read Error: Unknown", self.addr);
                  self.close = true;
                  //break;
              }*/
        }

        self.close
    }

    pub fn shutdown(&mut self) {
        let _ = self.stream.shutdown();
    }

    pub fn is_ssl_handshake_pending(&self) -> bool {
        match self.stream {
            NetStream::SslMidHandshakeStream(_) => {
                true
            }
            _ => { false }
        }
    }


    pub fn ssl_handshake(&mut self) -> Result<(), Error> {
        use std::mem;
        let old = mem::replace(&mut self.stream, NetStream::Invalid);
        match old {
            NetStream::SslMidHandshakeStream(mid_stream) => {
                match mid_stream.handshake() {
                    Ok(s) => {
                        debug!("ssl_handshake:SSL Handshake successful");
                        if self.server_ssl_config.is_some() {
                            debug!("ssl_handshake:Validate client CN");
                            if let Err(e) = Server::validate_ssl_connection(self.server_ssl_config.as_ref().unwrap(), s.ssl()) {
                                error!("Failed to accept SSL connection. Error:{:?}", e);
                                return Err(Error::new(
                                    ErrorKind::Other,
                                    format!("An SSL error occurred.{}", e.to_string()),
                                ));
                            };
                        }
                        self.stream = NetStream::SslTcpStream(s);
                        Ok(())
                    }
                    Err(e) => {
                        debug!("{:?}", e);
                        let err_str = e.to_string();
                        match e {
                            HandshakeError::WouldBlock(s) => {
                                debug!("Failed to handshake on SSL connection. Error:{}", err_str);
                                self.stream = NetStream::SslMidHandshakeStream(s);
                                Ok(())
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
            _ => { Ok(()) }
        }
    }
    /* if let Ok(_) = NetStream::ssl_handshake(&mut self.stream) {
                //self.stream = stream;
            }else {
                self.close = true;
            }*/
}

#[derive(Debug)]
pub enum NetStream {
    /// An unsecured TcpStream.
    UnsecuredTcpStream(TcpStream),
    /// Unix domain socket stream
    UdsStream(UnixStream),
    /// An SSL-secured TcpStream.
    /// This is only available when compiled with SSL support.
    //#[cfg(feature = "tls")]
    SslTcpStream(SslStream<TcpStream>),
    //SSL mid handshake stream
    SslMidHandshakeStream(MidHandshakeSslStream<TcpStream>),

    Invalid,
}

impl NetStream {
    pub fn shutdown(&mut self) -> Result<(), Error> {
        match *self {
            NetStream::UnsecuredTcpStream(ref stream) => stream.shutdown(Shutdown::Both),
            NetStream::UdsStream(ref stream) => stream.shutdown(Shutdown::Both),
            //#[cfg(feature = "tls")]
            NetStream::SslTcpStream(ref mut stream) => {
                if let Err(e) = stream.shutdown() {
                    warn!("Failed to shutdown SSL stream. Error:{:?}",e);
                }
                Ok(())
            }
            NetStream::SslMidHandshakeStream(ref mut mid_stream) => {
                mid_stream.get_mut().shutdown(Shutdown::Both)
            }
            NetStream::Invalid => { Ok(()) }
        }
    }
    #[inline]
    pub fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        match *self {
            NetStream::UnsecuredTcpStream(ref mut stream) => stream.read(buf),
            NetStream::UdsStream(ref mut stream) => stream.read(buf),
            //#[cfg(feature = "tls")]
            NetStream::SslTcpStream(ref mut stream) => stream.read(buf),
            NetStream::SslMidHandshakeStream(ref mut mid_stream) => {
                mid_stream.get_mut().read(buf)
            }
            NetStream::Invalid => { Ok(0) }
        }
    }
    #[inline]
    pub fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        match *self {
            NetStream::UnsecuredTcpStream(ref mut stream) => stream.write(buf),
            NetStream::UdsStream(ref mut stream) => stream.write(buf),
            //#[cfg(feature = "tls")]
            NetStream::SslTcpStream(ref mut stream) => {
                // Arc::get_mut(stream).unwrap().write(buf)
                stream.write(buf)
            }
            NetStream::SslMidHandshakeStream(ref mut mid_stream) => {
                mid_stream.get_mut().write(buf)
            }
            NetStream::Invalid => { Ok(0) }
        }
    }
    #[inline]
    pub fn write_all(&mut self, buf: &[u8]) -> IoResult<()> {
        match *self {
            NetStream::UnsecuredTcpStream(ref mut stream) => stream.write_all(buf),
            NetStream::UdsStream(ref mut stream) => stream.write_all(buf),
            //#[cfg(feature = "tls")]
            NetStream::SslTcpStream(ref mut stream) => stream.write_all(buf),
            NetStream::SslMidHandshakeStream(ref mut mid_stream) => {
                mid_stream.get_mut().write_all(buf)
            }
            NetStream::Invalid => { Ok(()) }
        }
    }
    pub fn flush(&mut self) -> IoResult<()> {
        match *self {
            NetStream::UnsecuredTcpStream(ref mut stream) => stream.flush(),
            NetStream::UdsStream(ref mut stream) => stream.flush(),
            //#[cfg(feature = "tls")]
            NetStream::SslTcpStream(ref mut stream) => stream.flush(),
            NetStream::SslMidHandshakeStream(ref mut mid_stream) => {
                mid_stream.get_mut().flush()
            }
            NetStream::Invalid => { Ok(()) }
        }
    }

    /*pub fn ssl_handshake(stream : &mut NetStream)  -> Result<(), Error> {
        match *stream {
            NetStream::SslMidHandshakeStream(mut mid_stream) => {
                match mid_stream.handshake() {
                    Ok(s) => {
                        info!("ssl_handshake:SSL Handshake successful");
                        *stream = NetStream::SslTcpStream(s);
                        Ok(())
                    },
                    Err(e) => {
                        info!("{:?}", e);
                        let err_str = e.to_string();
                        match e {
                            HandshakeError::WouldBlock(s) => {
                                info!("Failed to handshake on SSL connection. Error:{}", err_str);
                                *stream = NetStream::SslMidHandshakeStream(s);
                                Ok(())
                            },
                            _ => {
                                error!("Failed to accept SSL connection. Error:{}", err_str);
                                return Err(Error::new(
                                    ErrorKind::Other,
                                    format!("An SSL error occurred.{}", err_str),
                                ));
                            }
                        }
                    }
                }
            },
            _ => {
                error!("Invalid operation. SSL Handshake is only supported on SslMidHandshakeStream");
                return Err(Error::new(
                    ErrorKind::Other,
                    "An SSL error occurred.Invalid operation. SSL Handshake is only supported on SslMidHandshakeStream",
                ));
            }

        }
    }*/
}

#[derive(Debug)]
pub enum NetAddr {
    /// std net socket address.
    NetSocketAddress(SocketAddr),

    /// This is only available when compiled with SSL support.
    UdsSocketAddress(UnixSocketAddr),
}

impl ToString for NetAddr {
    fn to_string(&self) -> String {
        match *self {
            NetAddr::NetSocketAddress(ref addr) => format!("{:?}", addr).to_owned(),
            NetAddr::UdsSocketAddress(ref addr) => format!("{:?}", addr).to_owned(),
        }
    }
}
