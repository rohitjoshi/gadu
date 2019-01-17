use mio::*;
use std::collections::HashMap;
use std::net::SocketAddr;
use mio::net::TcpStream;
use std::net::Shutdown;
use mio_uds::UnixStream;
use std::os::unix::net::SocketAddr as UnixSocketAddr;

use std::io::Result as IoResult;
use std::io::{Read, Write};
use std::time::Duration;
use url::Url;

pub struct Conn {
    pub url: Url,
    pub stream: NetStream,
    pub addr: NetAddr,
    pub input: Vec<u8>,
    pub output: Vec<u8>,
    pub close: bool,
    pub reg_write: bool,
    pub tags: HashMap<String, String>,
}

impl Conn {
    pub const KEEP_ALIVE_TIME: Duration = Duration::from_secs(600); //10 min

     fn new(url: Url, net_conn: NetStream, net_addr:NetAddr) -> Conn {
        Conn {
            url,
            stream: net_conn,
            addr: net_addr,
            close: false,
            reg_write: false,
            input: Vec::new(),
            output: Vec::new(),
            tags: HashMap::with_capacity(2),
        }

    }

    pub fn connect(addr_url: &str) -> Result<Conn, String>{
        let url = match Url::parse(addr_url) {
            Ok(url) => url,
            Err(e) => {
                return Err(e.to_string());
            }
        };

        let (net_conn, net_addr) = match url.scheme() {
            "tcp" => {
                if !url.has_host() {
                    return Err("Invalid Url.  It must have host defined. e.g. tcp://host:port".to_owned());
                }
                if !url.port().is_none() {
                    return Err("Invalid Url.  It must have port defined. e.g. tcp://host:port".to_owned());
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
                sock.set_keepalive(Some(Conn::KEEP_ALIVE_TIME)).unwrap();

                //set keep alive
                if let Err(e) = sock.set_keepalive(Some(Conn::KEEP_ALIVE_TIME)) {
                    error!("Faile to set keep alive : {}.", e.to_string());

                }
                info!("Tcp client connected with server at {}", addr);
                (NetStream::UnsecuredTcpStream(sock), NetAddr::NetSocketAddress(conn_addr))
            },
            "unix" => {
                let path= url.path();
                debug!("Connecting to Server at {}", path);
                let sock = match UnixStream::connect(&path) {
                    Ok(sock) => sock,
                    Err(e) => {
                        error!(
                            "EventHandler: Couldn't connect at {}. Error: {:?}",
                            path, e
                        );
                        return Err(e.to_string());
                    }
                };
                info!("Unix Socket connected with server at {}", path);
                let addr = sock.peer_addr().unwrap();
                (NetStream::UdsStream(sock), NetAddr::UdsSocketAddress(addr))
            },
            _ => {
                return Err("Unsupported scheme. Valid schemes are unix and tcp".to_owned());
            }
        };


        Ok(Conn::new(url, net_conn,net_addr))

    }

    pub fn shutdown(&self) {
        self.stream.shutdown();
    }
}


pub enum NetStream {
    /// An unsecured TcpStream.
    UnsecuredTcpStream(TcpStream),
    /// An SSL-secured TcpStream.
    /// This is only available when compiled with SSL support.
    #[cfg(feature = "ssl")]
    SslTcpStream(TcpStream),

    /// Unix domain socket stream
    UdsStream(UnixStream),
}

impl NetStream {
    pub fn shutdown(&self) ->Result<(), std::io::Error>{
        match self {
            &NetStream::UnsecuredTcpStream(ref stream) => stream.shutdown(Shutdown::Both),
            #[cfg(feature = "ssl")]
            &NetStream::SslTcpStream(ref stream) => stream.shutdown(Shutdown::Both),
            &NetStream::UdsStream(ref stream) => stream.shutdown(Shutdown::Both),
        }

    }
    pub fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        match self {
            &mut NetStream::UnsecuredTcpStream(ref mut stream) => stream.read(buf),
            #[cfg(feature = "ssl")]
            &mut NetStream::SslTcpStream(ref mut stream) => stream.read(buf),
            &mut NetStream::UdsStream(ref mut stream) => stream.read(buf),
        }
    }
    pub fn write(&mut self, buf: &[u8]) -> IoResult<(usize)> {
        match self {
            &mut NetStream::UnsecuredTcpStream(ref mut stream) => stream.write(buf),
            #[cfg(feature = "ssl")]
            &mut NetStream::SslTcpStream(ref mut stream) => {
                // Arc::get_mut(stream).unwrap().write(buf)
                stream.lock().unwrap().write(buf)
            },
            &mut NetStream::UdsStream(ref mut stream) => stream.write(buf),
        }

    }
    pub fn write_all(&mut self, buf: &[u8]) -> IoResult<()> {
        match self {
            &mut NetStream::UnsecuredTcpStream(ref mut stream) => stream.write_all(buf),
            #[cfg(feature = "ssl")]
            &mut NetStream::SslTcpStream(ref mut stream) => stream.write_all(buf),
            &mut NetStream::UdsStream(ref mut stream) => stream.write_all(buf),
        }
    }
    pub fn flush(&mut self) -> IoResult<()> {
        match self {
            &mut NetStream::UnsecuredTcpStream(ref mut stream) => stream.flush(),
            #[cfg(feature = "ssl")]
            &mut NetStream::SslTcpStream(ref mut stream) => stream.flush(),
            &mut NetStream::UdsStream(ref mut stream) => stream.flush(),
        }
    }
}

pub enum NetAddr {
    /// std net socket address.
    NetSocketAddress(SocketAddr),

    /// This is only available when compiled with SSL support.
    UdsSocketAddress(UnixSocketAddr)
}

