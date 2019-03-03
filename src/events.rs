/************************************************

   File Name: gadu:events
   Author: Rohit Joshi <rohit.c.joshi@gmail.com>
   Date: 2019-02-17:15:15
   License: Apache 2.0

**************************************************/

use crate::conn::{Conn, NetAddr, NetStream};
use crate::server::Server;
use mio::event::Event;
use mio::unix::UnixReady;
use mio::{Events, Poll, PollOpt, Ready, Token};
use parking_lot::Mutex;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::config::GaduConfig;
use crossbeam_channel as mpsc;
use hashbrown::HashMap;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub trait NetEvents {
    fn event_opened(&self, id: usize, conn: &mut Conn) -> (Vec<u8>, bool);
    fn event_closed(&self, id: usize, conn: &mut Conn) -> Result<bool, String>;
    fn event_data(
        &self,
        id: usize,
        conn_tags: &mut HashMap<String, String>,
        buffer: &mut Vec<u8>,
    ) -> (Vec<u8>, bool);
}

pub struct ServerEventHandler {
    pub server_id: usize,
    pub server: Server,
    pub conn_handlers: Vec<Arc<ConnEventHandler>>,
    pub shutdown: bool,
}

impl ServerEventHandler {
    pub fn new(
        server_id: usize,
        num_threads: usize,
        config: &GaduConfig,
    ) -> Result<ServerEventHandler, String> {
        let server = Server::init(server_id, config)?;
        let mut conn_handlers = Vec::with_capacity(num_threads);
        for _i in 0..num_threads {
            let handler = ConnEventHandler::new()?;
            conn_handlers.push(Arc::new(handler));
        }
        Ok(ServerEventHandler {
            server_id,
            server,
            conn_handlers,
            shutdown: false,
        })
    }

    /*
    pub fn run_loop<T:NetEvents>(&mut self, event_handler: Arc<T>) where T: NetEvents + 'static + Sync + Send + Sized {

        let mut  id = self.server_id;

        crossbeam::scope(|scope| {
            for conn_handler in  self.conn_handlers.iter() {

                let ev = event_handler.clone();
                let c =conn_handler.clone();
                scope.spawn(move || c.child_loop(ev));
            }

            while !self.shutdown {
                id = id + 1;
                if let Ok(mut conn) = self.server.accept_connection() {
                    let (output, close) = event_handler.event_opened(id, &conn);
                    if close {
                        conn.close = close;
                    }else if !output.is_empty() {
                        conn.output = output;
                        conn.reg_write = true;

                    }
                    self.add_connection(id, conn);

                    continue;
                }
            }

        });
    }*/
}

pub struct ConnEventHandler {
    pub conns: Arc<Mutex<HashMap<usize, Conn>>>,
    pub poll: Poll,
    //receiver: mpsc::Receiver<ConnMsg>,
    //sender: mpsc::Sender<ConnMsg>
}

unsafe impl Send for ConnEventHandler {}
unsafe impl Sync for ConnEventHandler {}
/*
impl NetEvents for ConnEventHandler {
    ///
    /// event opened
    #[inline]
    fn event_opened(&self, id: usize, conn: &Conn) -> (Vec<u8>, bool) {
        // new connection, update write command
        debug!(
            "event_opened: New connection opend with id:{},  address: {:?}",
            id, conn.addr
        );

        (Vec::new(), false)
    }
    ///
    /// event close
    #[inline]
    fn event_closed(&self, id: usize) {
        // FUTURE: Adios connection.
        debug!("event_closed: Connection close for id:{}", id);
    }

    fn event_data(&self, id: usize, buffer: &mut Vec<u8>)-> (Vec<u8>, bool){
        return (vec![], false)
    }
}*/

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnMsg {
    pub id: usize,
    pub output: Vec<u8>,
}

impl ConnEventHandler {
    pub fn new() -> Result<ConnEventHandler, String> {
        let res = Poll::new();
        if let Err(e) = res {
            return { Err(e.to_string()) };
        }
        //let (sender, receiver) = mpsc::unbounded::<ConnMsg>();
        let conn_handler = ConnEventHandler {
            conns: Arc::new(Mutex::new(HashMap::new())),
            poll: res.unwrap(),
            // receiver,
            // sender
        };
        Ok(conn_handler)
    }
    pub fn add_connection(&self, id: usize, conn: Conn) -> Result<(), String> {
        debug!("ConnEventHandler::add_connection with id:{}", id);
        if let Err(e) = self.register(id, &conn) {
            // read only
            return Err(e.to_string());
        }
        self.conns.lock().insert(id, conn);
        Ok(())
    }

    /*pub fn add_message(&self, id: usize, msg: &[u8])-> Result<(), String> {
        let mut output = Vec::with_capacity(msg.len());
        output.extend(msg);
        let msg = ConnMsg { id, output };
        self.sender.send(msg);
        Ok(())
    }*/

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
    fn reregister(&self, conn: &Conn, id: usize, readable_only: bool) -> Result<(), String> {
        debug!("Reregister connection with id: {}", id);
        let flags = if !readable_only {
            Ready::writable() | Ready::readable()
        } else {
            Ready::readable()
        };
        let res = match conn.get_stream() {
            NetStream::UnsecuredTcpStream(ref stream) => {
                self.poll
                    .register(stream, Token(id), flags, PollOpt::empty())
            }
            NetStream::UdsStream(ref stream) => {
                self.poll
                    .register(stream, Token(id), flags, PollOpt::empty())
            }
            #[cfg(feature = "ssl")]
            NetStream::SslTcpStream(ref stream) => {
                self.poll
                    .register(stream.get_ref(), Token(id), flags, PollOpt::empty())
            }
        };
        if let Err(e) = res {
            error!("Failed to register connection with id:{}", id);
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
            #[cfg(feature = "ssl")]
            NetStream::SslTcpStream(ref stream) => self.poll.deregister(stream.get_ref()),
        };
    }

    pub fn check_error_event(addr: &NetAddr, event: &Event) -> bool {
        debug!("check_error_event:event:{:?}", event.readiness());
        let er = UnixReady::from(event.readiness());
        if er.is_hup() {
            debug!(
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
        event_handler: &T,
        receiver: Option<mpsc::Receiver<ConnMsg>>,
        shutdown: Arc<AtomicBool>,
    ) where
        T: NetEvents + ?Sized,
    {
        let mut streams: HashMap<usize, Conn> = HashMap::new();
        let mut events = Events::with_capacity(2048);
        let mut read_buffer = [0; 32768];

        let timeout = Some(Duration::from_millis(5000));

        loop {
            //check if shutdown signal received
            if shutdown.load(Ordering::SeqCst) {
                info!("Shutdown received. exiting child connection loop");
                return;
            }

            // if any data in the receiver channel, assigned to connect to send it out
            if receiver.is_some() {
                //let r = receiver.unwrap();
                let data: Vec<ConnMsg> = receiver.as_ref().unwrap().try_iter().collect();
                for msg in data.iter() {
                    if let Some(mut conn) = streams.get_mut(&msg.id) {
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

            debug!("Child Poll: Total events received:{}", total_events);

            for event in &events {
                let token = event.token();

                debug!("Child loop event :{:?}", event);

                let id = token.0;
                let mut close = false;
                let mut found = false;

                if let Some(mut conn) = streams.get_mut(&id) {
                    //check error/hup event received
                    debug!("Connection closed status:{}", conn.close);
                    close = ConnEventHandler::check_error_event(&conn.get_address(), &event);
                    conn.close = close;
                    debug!("Connection closed status:{}", close);
                    found = true;
                    if !conn.close {
                        loop {
                            debug!("output len:{}", conn.output.len());
                            if !conn.output.is_empty() {
                                close = conn.write();
                            } else if !conn.close {
                                close = conn.read(&mut read_buffer);
                                // PROFILER.lock().unwrap().start("/tmp/my-prof.profile").expect("Couldn't start");
                                let (output, close_conn) =
                                    event_handler.event_data(id, &mut conn.tags, &mut conn.input);
                                // PROFILER.lock().unwrap().stop().expect("Couldn't stop");
                                debug!("event_data output:{}", String::from_utf8_lossy(&output));
                                conn.output.extend(&output);
                                conn.close = close_conn;
                            }
                            if !conn.close && !conn.output.is_empty() {
                                continue;
                            }

                            break;
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
                        debug!("Socket is closed.. shutting down");
                        conn.shutdown();
                        self.deregister(id, &conn);
                        if let Ok(true) = event_handler.event_closed(id, &mut conn) {
                            if self.register(id, &conn).is_ok() {
                                debug!("Auto reconnect successful. Reregistering socket");
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
                        if self.register(id, &conn).is_ok() {
                            streams.insert(id, conn);
                        }
                    }
                }
            }
        }
    }

    pub fn child_loop<T>(&self, event_handler: Arc<T>, shutdown: Arc<AtomicBool>)
    where
        T: NetEvents + 'static + Sync + Send + Sized,
    {
        let mut streams: HashMap<usize, Conn> = HashMap::new();
        let mut events = Events::with_capacity(2048);
        let mut read_buffer = [0; 32768];

        let timeout = Some(Duration::from_millis(5000));

        loop {
            //check if shutdown signal received
            if shutdown.load(Ordering::SeqCst) {
                info!("Shutdown received. exiting child connection loop");
                return;
            }

            // if any data in the receiver channel, assigned to connect to send it out

            /*let mut data: Vec<ConnMsg> = self.receiver.try_iter().collect();
            for msg in data.iter() {
                if let Some(mut conn) = streams.get_mut(&msg.id) {
                     conn.output.extend(&msg.output);
                    conn.reg_write = true;
                }
            }*/

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

            debug!("Child Poll: Total events received:{}", total_events);

            for event in &events {
                let token = event.token();

                debug!("Child loop event :{:?}", event);

                let id = token.0;
                let mut close = false;
                let mut found = false;

                if let Some(mut conn) = streams.get_mut(&id) {
                    //check error/hup event received
                    debug!("Connection closed status:{}", conn.close);
                    close = ConnEventHandler::check_error_event(&conn.get_address(), &event);
                    conn.close = close;
                    debug!("Connection closed status:{}", close);
                    found = true;
                    if !conn.close {
                        loop {
                            debug!("output len:{}", conn.output.len());
                            if !conn.output.is_empty() {
                                close = conn.write();
                            } else if !conn.close {
                                close = conn.read(&mut read_buffer);
                                // PROFILER.lock().unwrap().start("/tmp/my-prof.profile").expect("Couldn't start");
                                let (output, close_conn) =
                                    event_handler.event_data(id, &mut conn.tags, &mut conn.input);
                                // PROFILER.lock().unwrap().stop().expect("Couldn't stop");
                                debug!("event_data output:{}", String::from_utf8_lossy(&output));
                                conn.output.extend(&output);
                                conn.close = close_conn;
                            }
                            if !conn.close && !conn.output.is_empty() {
                                continue;
                            }

                            break;
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
                        debug!("Socket is closed.. shutting down");
                        conn.shutdown();
                        self.deregister(id, &conn);
                        if let Ok(true) = event_handler.event_closed(id, &mut conn) {
                            if self.register(id, &conn).is_ok() {
                                debug!("Auto reconnect successful. Reregistering socket");
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
                        if self.register(id, &conn).is_ok() {
                            streams.insert(id, conn);
                        }
                    }
                }
            }
        }
    }
}
