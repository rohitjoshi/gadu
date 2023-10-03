/************************************************

   File Name: gadu:events
   Author: Rohit Joshi <rohit.c.joshi@gmail.com>
   Date: 2019-02-17:15:15
   License: Apache 2.0

**************************************************/

use crossbeam_channel as mpsc;
use hashbrown::HashMap;
use mio::{Events, Poll, PollOpt, Ready, Token};
use mio::event::Event;
use mio::unix::UnixReady;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use crate::config::GaduConfig;
use crate::conn::{Conn, NetAddr, NetStream};
use crate::net_events::NetEvents;
use crate::server::Server;

//use futures::future::Future;
//use futures::StreamExt;
//use futures::executor::{self, ThreadPool};
//use futures::io::AsyncWriteExt;
//use futures::task::{SpawnExt};


pub struct ServerEventHandler {
    pub server_id: usize,
    pub server: Arc<Server>,
    pub conn_handlers: Vec<Arc<ConnEventHandler>>,
    pub shutdown: bool,
}

impl ServerEventHandler {
    pub fn new(
        server_id: usize,
        num_threads: usize,
        config: &GaduConfig,
    ) -> Result<ServerEventHandler, String> {
        let server = Arc::new(Server::init(server_id, config)?);
        let mut conn_handlers = Vec::with_capacity(num_threads);
        for i in 0..num_threads {
            let handler = ConnEventHandler::new(i)?;
            conn_handlers.push(Arc::new(handler));
        }
        Ok(ServerEventHandler {
            server_id,
            server,
            conn_handlers,
            shutdown: false,
        })
    }
}

pub struct ConnEventHandler {
    id: usize,
    pub conns: Arc<Mutex<HashMap<usize, Conn>>>,
    pub poll: Poll,

}

//unsafe impl Send for ConnEventHandler {}
//unsafe impl Sync for ConnEventHandler {}


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnMsg {
    pub id: usize,
    pub output: Vec<u8>,
}

impl ConnEventHandler {
    pub fn new(id: usize) -> Result<ConnEventHandler, String> {
        let res = Poll::new();
        if let Err(e) = res {
            return Err(e.to_string()) ;
        }
        //let (sender, receiver) = mpsc::unbounded::<ConnMsg>();
        let conn_handler = ConnEventHandler {
            id,
            conns: Arc::new(Mutex::new(HashMap::new())),
            poll: res.unwrap(),
            // receiver,
            // sender
        };
        Ok(conn_handler)
    }
    pub fn add_connection(&self, conn_id: usize, conn: Conn) -> Result<(), String> {
        info!("ConnEventHandler: {} ::add_connection with conn id:{}", self.id, conn_id);
        if let Err(e) = self.register(conn_id, &conn) {
            // read only
            return Err(e.to_string());
        }

        self.conns.lock().insert(conn_id, conn);
        Ok(())
    }


    /*pub fn add_message(&self, id: usize, msg: &[u8])-> Result<(), String> {
        let mut output = Vec::with_capacity(msg.len());
        output.extend(msg);
        let msg = ConnMsg { id, output };
        self.sender.send(msg);
        Ok(())
    }*/
    #[inline]
    pub fn register(&self, id: usize, conn: &Conn) -> Result<(), String> {
        debug!("Register connection with id: {}", id);
        if !conn.output.is_empty() {
            if let Err(e) = self.reregister(&conn, id, false) {
                // read | write *
                return Err(e.to_string());
            }
        } else if !conn.close {
            if let Err(e) = self.reregister(&conn, id, true) {
                // read only
                return Err(e.to_string());
            }
        }

        Ok(())
    }
    #[inline]
    fn reregister(&self, conn: &Conn, id: usize, readable_only: bool) -> Result<(), String> {
        debug!("Reregister connection with id: {}", id);
        let flags = if !readable_only {
            Ready::writable() | Ready::readable()
        } else {
            Ready::readable()
        };

        let poll_opt = PollOpt::empty(); //PollOpt::edge() | PollOpt::oneshot();
        let res = match conn.get_stream() {
            NetStream::UnsecuredTcpStream(ref stream) => {
                self.poll
                    .register(stream, Token(id), flags, poll_opt)
            }
            NetStream::UdsStream(ref stream) => {
                self.poll
                    .register(stream, Token(id), flags, poll_opt)
            }
            // #[cfg(feature = "tls")]
            NetStream::SslTcpStream(ref stream) => {
                self.poll
                    .register(stream.get_ref(), Token(id), flags, poll_opt)
            }
            NetStream::SslMidHandshakeStream(ref stream) => {
                self.poll
                    .register(stream.get_ref(), Token(id), flags, poll_opt)
            }
            NetStream::Invalid => { Ok(()) }
        };
        if let Err(e) = res {
            error!("Failed to register connection with id:{} for flag:{:?}. Error:{:?}", id, flags, e);
            return Err(format!(
                "Failed to register connection with id:{}. Error:{:?}",
                id, e
            )
                .to_owned());
        }
        Ok(())
    }
    fn deregister(&self, id: usize, conn: &Conn) {
        debug!("Deregister connection with id: {}", id);
        let _res = match conn.get_stream() {
            NetStream::UnsecuredTcpStream(ref stream) => self.poll.deregister(stream),
            NetStream::UdsStream(ref stream) => self.poll.deregister(stream),
            //#[cfg(feature = "tls")]
            NetStream::SslTcpStream(ref stream) => self.poll.deregister(stream.get_ref()),
            NetStream::SslMidHandshakeStream(ref stream) => self.poll.deregister(stream.get_ref()),
            NetStream::Invalid => { Ok(()) }
        };
    }
    #[inline]
    pub fn check_error_event(addr: &NetAddr, event: &Event) -> bool {
        debug!("check_error_event:event:{:?}", event.readiness());
        let er = UnixReady::from(event.readiness());
        if er.is_hup() {
            info!(
                "UnixReady:Closing peer connection {} due to HUP signal received ",
                addr.to_string()
            );
            return true;
        } else if er.is_error() {
            warn!(
                "UnixReady:Closing peer connection {} due to error signal received ",
                addr.to_string()
            );
            return true;
        }
        false
    }

    pub fn child_loop_new<T>(
        &self,
        _event_handler: &T,
        receiver: Option<mpsc::Receiver<ConnMsg>>,
        shutdown: Arc<AtomicBool>,
    ) where
        T: NetEvents + ?Sized,
    {
        info!("Child Loop with receiver started for ConnectionHandler :{}", self.id);
        let mut streams: HashMap<usize, Conn> = HashMap::new();
        let mut events = Events::with_capacity(1024);
        //let mut read_buffer = [0; 32768];


        let timeout = if receiver.is_some() { Some(Duration::from_millis(100)) } else { Some(Duration::from_millis(250)) };

        loop {
            //check if shutdown signal received
            if shutdown.load(Ordering::SeqCst) {
                info!(
                    "Shutdown received. exiting child connection loop. thread_id: {:?}",
                    thread::current().id()
                );
                return;
            }

            // if any data in the receiver channel, assigned to connect to send it out
            if receiver.is_some() {
                //let r = receiver.unwrap();
                let data: Vec<ConnMsg> = receiver.as_ref().unwrap().try_iter().collect();
                for msg in data.iter() {
                    if let Some(conn) = streams.get_mut(&msg.id) {
                        conn.output.extend(&msg.output);
                        conn.reg_write = true;
                    }
                }
            }

            let total_events = match self.poll.poll(&mut events, timeout) {
                Ok(total_events) => total_events,
                Err(e) => {
                    error!("Error in child loop  poll. Error:{:?}", e);
                    return;
                }
            };

            if total_events == 0 {
                continue;
            }

            debug!("Child Poll id: {}: Total events received:{}, Number of connections:{}", self.id, total_events, streams.len());

            //self.process_events(&mut events, &event_handler, &mut streams, &mut read_buffer);

        }
    }

    pub fn child_loop<T>(&self, event_handler: Arc<T>, shutdown: Arc<AtomicBool>)
        where
            T: NetEvents + 'static + Sync + Send + Sized,
    {
        info!("Child Loop started for ConnectionHandler :{}", self.id);
        let mut streams: HashMap<usize, Conn> = HashMap::new();
        let mut events = Events::with_capacity(1024);
        let mut read_buffer = [0; 51200];

        let timeout = Some(Duration::from_millis(250));


        loop {
            //check if shutdown signal received
            if shutdown.load(Ordering::SeqCst) {
                info!(
                    "Shutdown received. exiting child connection loop. thread_id: {:?}",
                    thread::current().id()
                );
                return;
            }

            let total_events = match self.poll.poll(&mut events, timeout) {
                Ok(total_events) => total_events,
                Err(e) => {
                    error!("Error in child loop  poll. Error:{:?}", e);
                    return;
                }
            };

            if total_events == 0 {
                continue;
            }

            debug!("Child Poll id: {}: Total events received:{}, Number of connections:{}", self.id, total_events, streams.len());

            self.process_events(&mut events, &event_handler, &mut streams, &mut read_buffer);

        }
    }


    fn process_events<T>(&self, events : &mut Events, event_handler: &Arc<T>, streams: &mut HashMap<usize, Conn>, mut read_buffer: &mut[u8])
    where T: NetEvents + 'static + Sync + Send + Sized
    {
        for event in events.iter() {
            let token = event.token();

            debug!("Child loop event :{:?}", event);

            let id = token.0;
            let mut close = false;
            let mut found = false;

            if let Some(mut conn) = streams.get_mut(&id) {
                //check error/hup event received
                if conn.close {
                    debug!("Got connection from stream for id {}. Connection closed status:{}", id, conn.close);
                } else {
                    close = ConnEventHandler::check_error_event(&conn.get_address(), &event);
                }
                conn.close = close;
                if conn.close {
                    debug!("check_error_event:Connection closed status:{}", close);
                }
                found = true;

                if !conn.close {
                    // check if SSL handshake was still pending
                    if conn.is_ssl_handshake_pending() {
                        if let Err(e) = conn.ssl_handshake() {
                            warn!("SSL Handshake failed. Error:{:?}",e);
                        }
                    } else {

                        close = self.handle_event2(id, &mut conn, &mut read_buffer, &event_handler);

                    }
                }
                if !conn.output.is_empty() {
                    debug!("Reregistering child sock for read and write");
                    if !conn.reg_write {
                        conn.reg_write = true;
                        if let Err(e) = self.reregister(&conn, id, false) {
                            error!("Failed to reregister. Error:{:?}", e); //FIXME: should this be closed
                        }
                    }
                } else {
                    debug!("Reregistering child sock for read only");
                    if conn.reg_write {
                        conn.reg_write = false;
                        if let Err(e) = self.reregister(&conn, id, true) {
                            error!("Failed to reregister. Error:{:?}", e); //FIXME: should this be closed
                        }
                    }
                    if !close {
                        close = conn.close
                    }
                }
                if close {
                    close = self.close_connection2(id, conn, event_handler);
                }
            }
            if close {
                streams.remove(&id);
                debug!(
                    "Number of registered connections :{}, Number of new connection: {}",
                    streams.len(),
                    self.conns.lock().len()
                );
            } else if !found {
                if let Some(conn) = self.conns.lock().remove(&id) {
                    // if self.reregister( &conn, id, true).is_ok() {
                    streams.insert(id, conn);
                    //}
                }
            }
        }

    }

    ///
    /// close connection and deregister
    fn close_connection2<T>(&self, id: usize, mut conn: &mut Conn, event_handler: &Arc<T>) -> bool
        where T: NetEvents + 'static + Sync + Send + Sized{
        debug!("Socket is closed.. shutting down");
        let mut close = true;
        conn.shutdown();
        self.deregister(id, &conn);
        if let Ok(true) = event_handler.event_closed(id, &mut conn) {
            if self.register(id, &conn).is_ok() {
                info!("Auto reconnect successful. Reregistering socket");
                conn.reg_write = false;
                if let Err(e) = self.reregister(&conn, id, true) {
                    error!("Failed to reregister. Error:{:?}", e); //FIXME: should this be closed
                }
                close = false;
            }
        }
        if close {
            self.conns.lock().remove(&id);
        }
        close
    }

    fn close_connection<T>(&self, id: usize, mut conn: &mut Conn, event_handler: &T) -> bool
        where T: NetEvents + 'static + Sync + Send + Sized{
        debug!("Socket is closed.. shutting down");
        let mut close = true;
        conn.shutdown();
        self.deregister(id, &conn);
        if let Ok(true) = event_handler.event_closed(id, &mut conn) {
            if self.register(id, &conn).is_ok() {
                info!("Auto reconnect successful. Reregistering socket");
                conn.reg_write = false;
                if let Err(e) = self.reregister(&conn, id, true) {
                    error!("Failed to reregister. Error:{:?}", e); //FIXME: should this be closed
                }
                close = false;
            }
        }
        if close {
            self.conns.lock().remove(&id);
        }
        close
    }

    fn handle_event<T>(&self, id:usize, conn: &mut Conn, mut read_buffer: &mut [u8], event_handler: &T) -> bool
        where T: NetEvents + 'static + Sync + Send + Sized{
        let mut close = false;
        loop {
            debug!("output len:{}", conn.output.len());
            if !conn.output.is_empty() {
                close = conn.write();
            } else if !conn.close {
                close = conn.read(&mut read_buffer);
                let close_conn =
                    event_handler.event_data(id, &mut conn.tags, &mut conn.input, &mut conn.output);
                debug!("event_data output:{}", String::from_utf8_lossy(&conn.output));
                //conn.output.extend(&output);
                conn.close = close_conn;
            }
            if !conn.close && !conn.output.is_empty() {
                continue;
            }

            break;
        }
        close
    }

    fn handle_event2<T>(&self, id:usize, conn: &mut Conn, mut read_buffer: &mut [u8], event_handler: &Arc<T>) -> bool
        where T: NetEvents + 'static + Sync + Send + Sized{
        let mut close = false;
        loop {
            debug!("output len:{}", conn.output.len());
            if !conn.output.is_empty() {
                close = conn.write();
            } else if !conn.close {
                close = conn.read(&mut read_buffer);
                let close_conn =
                    event_handler.event_data(id, &mut conn.tags, &mut conn.input, &mut conn.output);
                debug!("event_data output:{}", String::from_utf8_lossy(&conn.output));
                //conn.output.extend(&output);
                conn.close = close_conn;
            }
            if !conn.close && !conn.output.is_empty() {
                continue;
            }

            break;
        }
        close
    }
}
